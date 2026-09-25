//! A small typed Notion API client for the import.
//!
//! - Every request carries `Authorization: Bearer <token>` and `Notion-Version: 2026-03-11`.
//! - A rate limiter spaces requests (default 3 per second, Notion's average for non-Business
//!   plans); it is shared by all clones of a client.
//! - `429`/`529` wait for `Retry-After` (header, else `additional_data.retry_after`), and then
//!   pause every request of this client; network errors and `500/502/503/504` retry with
//!   bounded exponential backoff. Every call here is a read, so retrying is safe.
//! - File downloads use a separate HTTP client without the token and with a redirect policy that
//!   only follows allowlisted Notion file hosts (see [`super::files`]).

use std::collections::HashSet;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::header::{self, HeaderMap, HeaderValue};
use reqwest::{Method, StatusCode, Url};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};
use thiserror::Error;
use tokio::time::Instant;

use super::files::{is_allowed_notion_file, suggested_file_name};
use super::model::{
    Block, BlockNode, BlockTree, NotionDataSource, NotionDatabase, NotionPage, NotionUser,
    QueryResults, SearchKind, SearchPage, SearchResult, SearchResults, normalize_id,
};

/// The Notion API version this client speaks.
pub const NOTION_VERSION: &str = "2026-03-11";
/// The production API origin.
pub const DEFAULT_BASE_URL: &str = "https://api.notion.com";

/// Client settings. `Default` is right for production; tests change `base_url` and speed.
#[derive(Clone, Debug)]
pub struct NotionClientConfig {
    /// API origin without a trailing path, e.g. `https://api.notion.com`.
    pub base_url: String,
    /// Average request rate; requests are spaced `1 / requests_per_second` apart.
    pub requests_per_second: f64,
    /// Attempts per request, including the first (minimum 1).
    pub max_attempts: u32,
    /// First backoff for network/5xx errors; doubles per attempt.
    pub initial_backoff: Duration,
    /// Upper bound for one backoff sleep.
    pub max_backoff: Duration,
    /// A `Retry-After` longer than this fails with [`NotionError::RateLimited`] instead of
    /// sleeping, so a job can reschedule itself.
    pub max_retry_after: Duration,
    /// Timeout of one API request.
    pub request_timeout: Duration,
    /// Timeout of one file download.
    pub download_timeout: Duration,
    /// `page_size` for paginated endpoints (Notion's maximum is 100).
    pub page_size: u32,
    /// `fetch_block_tree` stops descending below this depth.
    pub max_tree_depth: usize,
    /// `fetch_block_tree` stops after this many blocks.
    pub max_tree_blocks: usize,
    /// Also download files from `http://127.0.0.1:<port>` (a local fake Notion server). For
    /// tests only: never set from user configuration, since it lifts the file host allowlist
    /// for that loopback port.
    pub loopback_file_port: Option<u16>,
}

impl Default for NotionClientConfig {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_owned(),
            requests_per_second: 3.0,
            max_attempts: 5,
            initial_backoff: Duration::from_millis(500),
            max_backoff: Duration::from_secs(30),
            max_retry_after: Duration::from_secs(120),
            request_timeout: Duration::from_secs(60),
            download_timeout: Duration::from_secs(120),
            page_size: 100,
            max_tree_depth: 24,
            max_tree_blocks: 20_000,
            loopback_file_port: None,
        }
    }
}

/// Errors from the Notion client. Messages never contain the token or signed file URLs.
#[derive(Debug, Error)]
pub enum NotionError {
    /// `401 unauthorized`: the token is invalid, expired or revoked.
    #[error("the Notion token is invalid or was revoked: {message}")]
    Unauthorized { message: String },
    /// `403 restricted_resource`: the token cannot read this object.
    #[error("the Notion token has no access to this resource: {message}")]
    RestrictedResource { message: String },
    /// `404 object_not_found`: missing, or not shared with the token.
    #[error("the Notion object was not found or is not shared: {message}")]
    ObjectNotFound { message: String },
    /// Still rate limited after the allowed attempts, or `Retry-After` exceeded
    /// `max_retry_after`. `retry_after` is Notion's last requested wait.
    #[error("Notion kept rate limiting the requests")]
    RateLimited { retry_after: Option<Duration> },
    /// `429` with `public_api_request_blocked`: retrying does not help.
    #[error("Notion blocked API access for this connection: {message}")]
    Blocked { message: String },
    /// Any other error status (after retries for 5xx).
    #[error("Notion API error {status} ({code}): {message}")]
    Api {
        status: u16,
        code: String,
        message: String,
    },
    /// Network or protocol failure after retries.
    #[error("the request to Notion failed: {0}")]
    Http(String),
    /// A successful response did not match the expected shape.
    #[error("could not decode the Notion response: {0}")]
    Decode(String),
    /// An id that is not a Notion UUID (never sent, to keep paths intact).
    #[error("invalid Notion id")]
    InvalidId,
    /// The file URL (or a redirect) is not an allowlisted Notion file host.
    #[error("the file URL is not an allowed Notion file host")]
    FileNotAllowed,
    /// The file is larger than the caller's limit.
    #[error("the file is larger than {limit} bytes")]
    FileTooLarge { limit: u64 },
    /// The client could not be built (bad token characters or base URL).
    #[error("invalid Notion client configuration: {0}")]
    Config(String),
}

/// A downloaded Notion-hosted file.
#[derive(Clone, Debug, PartialEq)]
pub struct DownloadedFile {
    pub bytes: Vec<u8>,
    /// `Content-Type` without parameters, when the host sent one.
    pub content_type: Option<String>,
    /// A sanitized file name derived from the URL (`downloadName` or last path segment).
    pub file_name: String,
}

/// Notion API client. Cheap to clone; clones share the rate limiter.
#[derive(Clone)]
pub struct NotionClient {
    inner: Arc<Inner>,
}

struct Inner {
    api: reqwest::Client,
    files: reqwest::Client,
    base_url: String,
    config: NotionClientConfig,
    limiter: RateLimiter,
    file_policy: FilePolicy,
}

impl fmt::Debug for NotionClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NotionClient")
            .field("base_url", &self.inner.base_url)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug)]
enum FilePolicy {
    Notion,
    Loopback(u16),
}

impl FilePolicy {
    fn allows(self, url: &Url) -> bool {
        match self {
            Self::Notion => is_allowed_notion_file(url),
            Self::Loopback(port) => {
                url.scheme() == "http"
                    && url.host_str() == Some("127.0.0.1")
                    && url.port() == Some(port)
            }
        }
    }
}

impl NotionClient {
    /// Builds a client for `token` (a personal access token or connection token).
    pub fn new(token: &str, config: NotionClientConfig) -> Result<Self, NotionError> {
        let policy = config
            .loopback_file_port
            .map_or(FilePolicy::Notion, FilePolicy::Loopback);
        Self::build(token, config, policy)
    }

    fn build(
        token: &str,
        config: NotionClientConfig,
        file_policy: FilePolicy,
    ) -> Result<Self, NotionError> {
        let token = token.trim();
        if token.is_empty() {
            return Err(NotionError::Config("the token is empty".to_owned()));
        }
        let mut authorization = HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| NotionError::Config("the token contains invalid characters".to_owned()))?;
        authorization.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, authorization);
        headers.insert("Notion-Version", HeaderValue::from_static(NOTION_VERSION));

        let base_url = config.base_url.trim_end_matches('/').to_owned();
        let parsed = Url::parse(&base_url)
            .map_err(|error| NotionError::Config(format!("base URL: {error}")))?;
        if !matches!(parsed.scheme(), "https" | "http") {
            return Err(NotionError::Config("base URL must be http(s)".to_owned()));
        }

        let api = reqwest::Client::builder()
            .default_headers(headers)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(config.request_timeout)
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|error| NotionError::Config(error.to_string()))?;
        let files = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::custom(move |attempt| {
                if attempt.previous().len() >= 5 {
                    attempt.error("too many redirects")
                } else if file_policy.allows(attempt.url()) {
                    attempt.follow()
                } else {
                    attempt.error("redirect outside the Notion file host allowlist")
                }
            }))
            .timeout(config.download_timeout)
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|error| NotionError::Config(error.to_string()))?;

        let interval = if config.requests_per_second.is_finite() && config.requests_per_second > 0.0
        {
            Duration::from_secs_f64(1.0 / config.requests_per_second)
        } else {
            Duration::ZERO
        };
        Ok(Self {
            inner: Arc::new(Inner {
                api,
                files,
                base_url,
                limiter: RateLimiter::new(interval),
                config,
                file_policy,
            }),
        })
    }

    /// `GET /v1/users/me`: validates the token and returns its user (bot for connection
    /// tokens, with `workspace_name`; the creating person for personal access tokens).
    pub async fn me(&self) -> Result<NotionUser, NotionError> {
        self.request(Method::GET, "/v1/users/me", &[], None).await
    }

    /// `POST /v1/search` over all pages: every page and/or data source the token can see.
    pub async fn search_all(&self, kind: SearchKind) -> Result<SearchResults, NotionError> {
        let (results, incomplete) = self
            .collect_list::<SearchResult>(Method::POST, "/v1/search", Some(search_body(kind)))
            .await?;
        Ok(SearchResults {
            results: results
                .into_iter()
                .filter(|result| !matches!(result, SearchResult::Other))
                .collect(),
            incomplete,
        })
    }

    /// One page of `POST /v1/search` after `start_cursor`, for callers that stream a large
    /// workspace (and can stop early) instead of collecting every result.
    pub async fn search_page(
        &self,
        kind: SearchKind,
        start_cursor: Option<&str>,
    ) -> Result<SearchPage, NotionError> {
        let mut body = search_body(kind);
        body.insert(
            "page_size".into(),
            json!(self.inner.config.page_size.clamp(1, 100)),
        );
        if let Some(cursor) = start_cursor {
            body.insert("start_cursor".into(), json!(cursor));
        }
        let page: ListResponse<SearchResult> = self
            .request(Method::POST, "/v1/search", &[], Some(&Value::Object(body)))
            .await?;
        Ok(SearchPage {
            incomplete: page
                .request_status
                .as_ref()
                .is_some_and(|status| status.kind == "incomplete"),
            next_cursor: page
                .next_cursor
                .filter(|cursor| page.has_more && Some(cursor.as_str()) != start_cursor),
            results: page
                .results
                .into_iter()
                .filter(|result| !matches!(result, SearchResult::Other))
                .collect(),
        })
    }

    /// `GET /v1/pages/{id}`.
    pub async fn retrieve_page(&self, page_id: &str) -> Result<NotionPage, NotionError> {
        let id = checked_id(page_id)?;
        self.request(Method::GET, &format!("/v1/pages/{id}"), &[], None)
            .await
    }

    /// `GET /v1/blocks/{id}/children`, all pages of results (one level).
    pub async fn block_children_all(&self, block_id: &str) -> Result<Vec<Block>, NotionError> {
        let id = checked_id(block_id)?;
        let (blocks, _) = self
            .collect_list::<Block>(Method::GET, &format!("/v1/blocks/{id}/children"), None)
            .await?;
        Ok(blocks)
    }

    /// Fetches a page's (or block's) content recursively. Children of `child_page` and
    /// `child_database` are not fetched (they are separate pages). A child list the token cannot
    /// read (404/403, e.g. a synced block whose original is not shared) is left empty. Stops at
    /// `max_tree_depth` / `max_tree_blocks` and sets `truncated`.
    pub async fn fetch_block_tree(&self, block_id: &str) -> Result<BlockTree, NotionError> {
        let mut state = TreeState::default();
        let blocks = self
            .fetch_subtree(block_id.to_owned(), 0, &mut state)
            .await?;
        Ok(BlockTree {
            blocks,
            block_count: state.count,
            truncated: state.truncated,
        })
    }

    fn fetch_subtree<'a>(
        &'a self,
        block_id: String,
        depth: usize,
        state: &'a mut TreeState,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<BlockNode>, NotionError>> + Send + 'a>> {
        Box::pin(async move {
            let blocks = match self.block_children_all(&block_id).await {
                Ok(blocks) => blocks,
                Err(
                    NotionError::ObjectNotFound { .. } | NotionError::RestrictedResource { .. },
                ) if depth > 0 => {
                    return Ok(Vec::new());
                }
                Err(error) => return Err(error),
            };
            let mut nodes = Vec::with_capacity(blocks.len());
            for block in blocks {
                if state.count >= self.inner.config.max_tree_blocks {
                    state.truncated = true;
                    break;
                }
                state.count += 1;
                let children = if block.has_children && !block.is_page_boundary() {
                    if depth + 1 >= self.inner.config.max_tree_depth {
                        state.truncated = true;
                        Vec::new()
                    } else {
                        self.fetch_subtree(block.id.clone(), depth + 1, state)
                            .await?
                    }
                } else {
                    Vec::new()
                };
                nodes.push(BlockNode { block, children });
            }
            Ok(nodes)
        })
    }

    /// `GET /v1/blocks/{id}`: one block; its `parent` is in `fields["parent"]`.
    pub async fn retrieve_block(&self, block_id: &str) -> Result<Block, NotionError> {
        let id = checked_id(block_id)?;
        self.request(Method::GET, &format!("/v1/blocks/{id}"), &[], None)
            .await
    }

    /// `GET /v1/databases/{id}`: the database with its data source ids.
    pub async fn retrieve_database(
        &self,
        database_id: &str,
    ) -> Result<NotionDatabase, NotionError> {
        let id = checked_id(database_id)?;
        self.request(Method::GET, &format!("/v1/databases/{id}"), &[], None)
            .await
    }

    /// `GET /v1/data_sources/{id}`: title and property schema.
    pub async fn retrieve_data_source(
        &self,
        data_source_id: &str,
    ) -> Result<NotionDataSource, NotionError> {
        let id = checked_id(data_source_id)?;
        self.request(Method::GET, &format!("/v1/data_sources/{id}"), &[], None)
            .await
    }

    /// `POST /v1/data_sources/{id}/query`, all rows (oldest first). Non-page results (wiki
    /// data sources can list data sources) are dropped.
    pub async fn query_data_source_all(
        &self,
        data_source_id: &str,
    ) -> Result<QueryResults, NotionError> {
        let id = checked_id(data_source_id)?;
        let mut body = Map::new();
        body.insert(
            "sorts".into(),
            json!([{"timestamp": "created_time", "direction": "ascending"}]),
        );
        let (results, incomplete) = self
            .collect_list::<SearchResult>(
                Method::POST,
                &format!("/v1/data_sources/{id}/query"),
                Some(body),
            )
            .await?;
        Ok(QueryResults {
            pages: results
                .into_iter()
                .filter_map(|result| match result {
                    SearchResult::Page(page) => Some(page),
                    _ => None,
                })
                .collect(),
            incomplete,
        })
    }

    /// Downloads a Notion-hosted file. Only HTTPS URLs on the Notion file host allowlist are
    /// fetched (redirects included); the token is never sent. Fails with
    /// [`NotionError::FileTooLarge`] once more than `max_bytes` arrive.
    pub async fn download_file(
        &self,
        url: &str,
        max_bytes: u64,
    ) -> Result<DownloadedFile, NotionError> {
        let parsed = Url::parse(url).map_err(|_| NotionError::FileNotAllowed)?;
        let policy = self.inner.file_policy;
        if !policy.allows(&parsed) {
            return Err(NotionError::FileNotAllowed);
        }
        let max_attempts = self.inner.config.max_attempts.max(1);
        let mut attempt = 0;
        let mut response = loop {
            attempt += 1;
            match self.inner.files.get(parsed.clone()).send().await {
                Ok(response) => {
                    let status = response.status();
                    if (status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS)
                        && attempt < max_attempts
                    {
                        tokio::time::sleep(self.backoff(attempt)).await;
                        continue;
                    }
                    break response;
                }
                Err(error) if error.is_redirect() => return Err(NotionError::FileNotAllowed),
                Err(error) if is_transient(&error) && attempt < max_attempts => {
                    tokio::time::sleep(self.backoff(attempt)).await;
                }
                Err(error) => return Err(http_error(error)),
            }
        };
        if !policy.allows(response.url()) {
            return Err(NotionError::FileNotAllowed);
        }
        let status = response.status();
        if !status.is_success() {
            return Err(NotionError::Api {
                status: status.as_u16(),
                code: "file_download_failed".to_owned(),
                message: format!("the file host answered {status}"),
            });
        }
        if response
            .content_length()
            .is_some_and(|length| length > max_bytes)
        {
            return Err(NotionError::FileTooLarge { limit: max_bytes });
        }
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty());
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(http_error)? {
            if bytes.len() as u64 + chunk.len() as u64 > max_bytes {
                return Err(NotionError::FileTooLarge { limit: max_bytes });
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(DownloadedFile {
            bytes,
            content_type,
            file_name: suggested_file_name(&parsed),
        })
    }

    /// Follows `next_cursor` until `has_more` is false. GET endpoints take the cursor as a query
    /// parameter, POST endpoints in the body.
    async fn collect_list<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<Map<String, Value>>,
    ) -> Result<(Vec<T>, bool), NotionError> {
        let page_size = self.inner.config.page_size.clamp(1, 100).to_string();
        let mut items = Vec::new();
        let mut incomplete = false;
        let mut cursor: Option<String> = None;
        let mut seen = HashSet::new();
        loop {
            let page: ListResponse<T> = if method == Method::GET {
                let mut query = vec![("page_size", page_size.as_str())];
                if let Some(cursor) = cursor.as_deref() {
                    query.push(("start_cursor", cursor));
                }
                self.request(Method::GET, path, &query, None).await?
            } else {
                let mut body = body.clone().unwrap_or_default();
                body.insert(
                    "page_size".into(),
                    json!(self.inner.config.page_size.clamp(1, 100)),
                );
                if let Some(cursor) = cursor.as_deref() {
                    body.insert("start_cursor".into(), json!(cursor));
                }
                self.request(method.clone(), path, &[], Some(&Value::Object(body)))
                    .await?
            };
            items.extend(page.results);
            incomplete |= page
                .request_status
                .as_ref()
                .is_some_and(|status| status.kind == "incomplete");
            match page.next_cursor {
                Some(next) if page.has_more && seen.insert(next.clone()) => cursor = Some(next),
                _ => break,
            }
        }
        Ok((items, incomplete))
    }

    async fn request<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, &str)],
        body: Option<&Value>,
    ) -> Result<T, NotionError> {
        let mut url = Url::parse(&format!("{}{path}", self.inner.base_url))
            .map_err(|error| NotionError::Config(format!("request URL: {error}")))?;
        if !query.is_empty() {
            url.query_pairs_mut().extend_pairs(query);
        }
        let config = &self.inner.config;
        let max_attempts = config.max_attempts.max(1);
        let mut attempt = 0;
        loop {
            attempt += 1;
            self.inner.limiter.acquire().await;
            let mut request = self.inner.api.request(method.clone(), url.clone());
            if let Some(body) = body {
                request = request.json(body);
            }
            let response = match request.send().await {
                Ok(response) => response,
                Err(error) if is_transient(&error) && attempt < max_attempts => {
                    tokio::time::sleep(self.backoff(attempt)).await;
                    continue;
                }
                Err(error) => return Err(http_error(error)),
            };
            let status = response.status();
            if status.is_success() {
                let bytes = response.bytes().await.map_err(http_error)?;
                return serde_json::from_slice(&bytes)
                    .map_err(|error| NotionError::Decode(error.to_string()));
            }
            let header_retry_after = response
                .headers()
                .get(header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(parse_seconds);
            let bytes = response.bytes().await.unwrap_or_default();
            let error: ErrorBody = serde_json::from_slice(&bytes).unwrap_or_default();
            match status.as_u16() {
                429 | 529 => {
                    if error.additional("rate_limit_reason") == Some("public_api_request_blocked") {
                        return Err(NotionError::Blocked {
                            message: error.message(),
                        });
                    }
                    let retry_after = header_retry_after
                        .or_else(|| error.additional("retry_after").and_then(parse_seconds));
                    let wait = retry_after.unwrap_or_else(|| self.backoff(attempt));
                    if attempt >= max_attempts || wait > config.max_retry_after {
                        return Err(NotionError::RateLimited { retry_after });
                    }
                    tracing::debug!(wait_ms = wait.as_millis() as u64, "Notion rate limited");
                    self.inner.limiter.pause(wait);
                }
                500 | 502 | 503 | 504 if attempt < max_attempts => {
                    tokio::time::sleep(self.backoff(attempt)).await;
                }
                401 => {
                    return Err(NotionError::Unauthorized {
                        message: error.message(),
                    });
                }
                403 => {
                    return Err(NotionError::RestrictedResource {
                        message: error.message(),
                    });
                }
                404 => {
                    return Err(NotionError::ObjectNotFound {
                        message: error.message(),
                    });
                }
                code => {
                    return Err(NotionError::Api {
                        status: code,
                        code: error.code.clone().unwrap_or_default(),
                        message: error.message(),
                    });
                }
            }
        }
    }

    fn backoff(&self, attempt: u32) -> Duration {
        let config = &self.inner.config;
        let factor = 2u32.saturating_pow(attempt.saturating_sub(1).min(16));
        config
            .initial_backoff
            .saturating_mul(factor)
            .min(config.max_backoff)
    }

    #[cfg(test)]
    fn with_loopback_files(token: &str, config: NotionClientConfig, port: u16) -> Self {
        Self::build(token, config, FilePolicy::Loopback(port)).expect("client")
    }
}

#[derive(Default)]
struct TreeState {
    count: usize,
    truncated: bool,
}

#[derive(Deserialize)]
struct ListResponse<T> {
    results: Vec<T>,
    #[serde(default)]
    next_cursor: Option<String>,
    #[serde(default)]
    has_more: bool,
    #[serde(default)]
    request_status: Option<RequestStatus>,
}

#[derive(Deserialize)]
struct RequestStatus {
    #[serde(rename = "type", default)]
    kind: String,
}

#[derive(Default, Deserialize)]
struct ErrorBody {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    additional_data: Option<Map<String, Value>>,
}

impl ErrorBody {
    fn message(&self) -> String {
        self.message.clone().unwrap_or_default()
    }

    fn additional(&self, key: &str) -> Option<&str> {
        self.additional_data.as_ref()?.get(key)?.as_str()
    }
}

/// Spaces requests `interval` apart; `pause` pushes the next slot out for every caller.
struct RateLimiter {
    interval: Duration,
    next: Mutex<Instant>,
}

impl RateLimiter {
    fn new(interval: Duration) -> Self {
        Self {
            interval,
            next: Mutex::new(Instant::now()),
        }
    }

    async fn acquire(&self) {
        let slot = {
            let mut next = self
                .next
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            let slot = (*next).max(Instant::now());
            *next = slot + self.interval;
            slot
        };
        tokio::time::sleep_until(slot).await;
    }

    fn pause(&self, wait: Duration) {
        let mut next = self
            .next
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let resume = Instant::now() + wait;
        if *next < resume {
            *next = resume;
        }
    }
}

fn search_body(kind: SearchKind) -> Map<String, Value> {
    let mut body = Map::new();
    let object = match kind {
        SearchKind::Pages => Some("page"),
        SearchKind::DataSources => Some("data_source"),
        SearchKind::All => None,
    };
    if let Some(object) = object {
        body.insert(
            "filter".into(),
            json!({"property": "object", "value": object}),
        );
    }
    body
}

fn checked_id(raw: &str) -> Result<String, NotionError> {
    normalize_id(raw).ok_or(NotionError::InvalidId)
}

/// Parses a `Retry-After` number of seconds (fractions allowed, HTTP dates ignored).
fn parse_seconds(raw: &str) -> Option<Duration> {
    let seconds: f64 = raw.trim().parse().ok()?;
    (seconds.is_finite() && seconds >= 0.0).then(|| Duration::from_secs_f64(seconds.min(86_400.0)))
}

fn is_transient(error: &reqwest::Error) -> bool {
    error.is_timeout() || error.is_connect() || error.is_request()
}

fn http_error(error: reqwest::Error) -> NotionError {
    NotionError::Http(error.without_url().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::http::StatusCode as AxumStatus;
    use axum::response::{IntoResponse, Redirect};
    use axum::routing::get;

    async fn serve(router: Router) -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        port
    }

    fn client(port: u16) -> NotionClient {
        NotionClient::with_loopback_files(
            "secret_token",
            NotionClientConfig {
                base_url: format!("http://127.0.0.1:{port}"),
                requests_per_second: 1000.0,
                initial_backoff: Duration::from_millis(5),
                ..NotionClientConfig::default()
            },
            port,
        )
    }

    fn file_router() -> Router {
        Router::new()
            .route(
                "/files/small.png",
                get(|| async {
                    (
                        [(header::CONTENT_TYPE, "image/png; charset=binary")],
                        vec![7u8; 10],
                    )
                }),
            )
            .route("/files/big.bin", get(|| async { vec![0u8; 4096] }))
            .route(
                "/files/stream.bin",
                get(|| async {
                    let chunks = (0..8).map(|_| Ok::<_, std::io::Error>(vec![1u8; 512]));
                    axum::body::Body::from_stream(futures_util::stream::iter(chunks))
                }),
            )
            .route(
                "/files/redirect-out",
                get(|| async { Redirect::temporary("https://example.com/evil.png") }),
            )
            .route(
                "/files/redirect-in",
                get(|| async { Redirect::temporary("/files/small.png") }),
            )
            .route(
                "/files/flaky",
                get(|| async { AxumStatus::SERVICE_UNAVAILABLE.into_response() }),
            )
    }

    #[tokio::test]
    async fn downloads_allowed_file_with_type_and_name() {
        let port = serve(file_router()).await;
        let file = client(port)
            .download_file(&format!("http://127.0.0.1:{port}/files/small.png"), 100)
            .await
            .unwrap();
        assert_eq!(file.bytes, vec![7u8; 10]);
        assert_eq!(file.content_type.as_deref(), Some("image/png"));
        assert_eq!(file.file_name, "small.png");
    }

    #[tokio::test]
    async fn enforces_size_cap_by_length_and_stream() {
        let port = serve(file_router()).await;
        let client = client(port);
        let error = client
            .download_file(&format!("http://127.0.0.1:{port}/files/big.bin"), 1000)
            .await
            .unwrap_err();
        assert!(matches!(error, NotionError::FileTooLarge { limit: 1000 }));
        let error = client
            .download_file(&format!("http://127.0.0.1:{port}/files/stream.bin"), 1000)
            .await
            .unwrap_err();
        assert!(matches!(error, NotionError::FileTooLarge { limit: 1000 }));
    }

    #[tokio::test]
    async fn follows_only_allowlisted_redirects() {
        let port = serve(file_router()).await;
        let client = client(port);
        let file = client
            .download_file(&format!("http://127.0.0.1:{port}/files/redirect-in"), 100)
            .await
            .unwrap();
        assert_eq!(file.bytes.len(), 10);
        let error = client
            .download_file(&format!("http://127.0.0.1:{port}/files/redirect-out"), 100)
            .await
            .unwrap_err();
        assert!(matches!(error, NotionError::FileNotAllowed), "{error:?}");
    }

    #[tokio::test]
    async fn rejects_non_allowlisted_urls_before_fetching() {
        let port = serve(file_router()).await;
        let client = client(port);
        for url in [
            "https://example.com/a.png".to_owned(),
            format!("http://localhost:{port}/files/small.png"),
            format!("http://127.0.0.1:{}/files/small.png", port.wrapping_add(1)),
        ] {
            let error = client.download_file(&url, 100).await.unwrap_err();
            assert!(matches!(error, NotionError::FileNotAllowed), "{url}");
        }
        // The production policy rejects plain HTTP and non-Notion hosts too.
        let production = NotionClient::new("t", NotionClientConfig::default()).unwrap();
        let error = production
            .download_file(&format!("http://127.0.0.1:{port}/files/small.png"), 100)
            .await
            .unwrap_err();
        assert!(matches!(error, NotionError::FileNotAllowed));
    }

    #[tokio::test]
    async fn gives_up_on_persistent_5xx_downloads() {
        let port = serve(file_router()).await;
        let error = client(port)
            .download_file(&format!("http://127.0.0.1:{port}/files/flaky"), 100)
            .await
            .unwrap_err();
        assert!(
            matches!(error, NotionError::Api { status: 503, .. }),
            "{error:?}"
        );
    }

    #[test]
    fn parses_retry_after_and_backoff() {
        assert_eq!(parse_seconds("2"), Some(Duration::from_secs(2)));
        assert_eq!(parse_seconds("0.05"), Some(Duration::from_millis(50)));
        assert_eq!(parse_seconds("Wed, 21 Oct 2015 07:28:00 GMT"), None);
        assert_eq!(parse_seconds("-1"), None);
        let client = NotionClient::new(
            "t",
            NotionClientConfig {
                initial_backoff: Duration::from_millis(100),
                max_backoff: Duration::from_millis(350),
                ..NotionClientConfig::default()
            },
        )
        .unwrap();
        assert_eq!(client.backoff(1), Duration::from_millis(100));
        assert_eq!(client.backoff(2), Duration::from_millis(200));
        assert_eq!(client.backoff(3), Duration::from_millis(350));
        assert_eq!(client.backoff(40), Duration::from_millis(350));
    }

    #[test]
    fn debug_output_hides_the_token() {
        let client = NotionClient::new("secret_abc", NotionClientConfig::default()).unwrap();
        assert!(!format!("{client:?}").contains("secret_abc"));
        assert!(matches!(
            NotionClient::new("  ", NotionClientConfig::default()),
            Err(NotionError::Config(_))
        ));
        assert!(matches!(
            NotionClient::new("bad\ntoken", NotionClientConfig::default()),
            Err(NotionError::Config(_))
        ));
    }
}
