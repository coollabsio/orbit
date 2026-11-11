//! A small typed Trello REST client for the import
//! (<https://developer.atlassian.com/cloud/trello/rest/>).
//!
//! - Every request carries the API key and token in
//!   `Authorization: OAuth oauth_consumer_key="KEY", oauth_token="TOKEN"`, never in the URL, so
//!   neither appears in logs (<https://developer.atlassian.com/cloud/trello/guides/rest-api/authorization/>).
//! - Requests are sequential per import and spaced by a rate limiter (default 8 per second,
//!   under the 100 requests per 10 seconds a token may make and the 300 of a key). After each
//!   response the `x-rate-limit-api-{token,key}-remaining` headers are read: when a window is
//!   nearly spent, the client waits for the window
//!   (<https://developer.atlassian.com/cloud/trello/guides/rest-api/rate-limits/>).
//! - A `429` waits for `Retry-After` (or one window) and retries; network errors and `5xx`
//!   retry with bounded exponential backoff. Every call is a read, so retrying is safe.
//! - Uploaded attachments are downloaded from their `…/download/…` URL with the same header.
//!   The credentials go only to HTTPS `trello.com` and `api.trello.com`; the redirect to the file
//!   store is followed without them.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use reqwest::header::{self, HeaderMap, HeaderValue};
use reqwest::{StatusCode, Url};
use serde::de::DeserializeOwned;
use thiserror::Error;

use super::model::{
    ACTION_FIELDS, ATTACHMENT_FIELDS, BOARD_FIELDS, CARD_FIELDS, CHECK_ITEM_FIELDS,
    CHECKLIST_FIELDS, CommentAction, CustomField, IdRef, LABEL_FIELDS, LIST_FIELDS, MEMBER_FIELDS,
    ORGANIZATION_FIELDS, TrelloBoard, TrelloCard, TrelloChecklist, TrelloLabel, TrelloList,
    TrelloMember, TrelloOrganization,
};
use crate::notion::files::sanitize_file_name;
use crate::task_import::{
    RateLimiter, backoff, disposition_file_name, is_transient, parse_retry_after,
};

/// The production REST API.
pub const DEFAULT_BASE_URL: &str = "https://api.trello.com/1";
/// Page size of the comment list (the most Trello returns per page).
pub const COMMENT_PAGE: usize = 1_000;
/// Below this many requests left in a rate-limit window, wait for the window.
const LOW_REMAINING: i64 = 3;
/// A rate-limit window, when the headers do not say.
const WINDOW: Duration = Duration::from_secs(10);

/// Client settings. `Default` is right for production; tests change the base URL and speed.
#[derive(Clone, Debug)]
pub struct TrelloClientConfig {
    /// The REST base URL, e.g. `https://api.trello.com/1`.
    pub base_url: String,
    /// Average request rate; requests are spaced `1 / requests_per_second` apart.
    pub requests_per_second: f64,
    /// Attempts per request, including the first (minimum 1).
    pub max_attempts: u32,
    /// First backoff for network/5xx errors; doubles per attempt.
    pub initial_backoff: Duration,
    /// Upper bound for one backoff sleep.
    pub max_backoff: Duration,
    /// A rate-limit wait longer than this fails with [`TrelloError::RateLimited`] instead of
    /// sleeping, so the job can reschedule itself.
    pub max_retry_after: Duration,
    pub request_timeout: Duration,
    pub download_timeout: Duration,
    /// Treat `http://127.0.0.1:<port>` as the Trello file host (it gets the credentials) and
    /// follow redirects to other loopback ports (without them). For tests only: never set from
    /// user configuration.
    pub loopback_file_port: Option<u16>,
}

impl Default for TrelloClientConfig {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_owned(),
            requests_per_second: 8.0,
            max_attempts: 5,
            initial_backoff: Duration::from_millis(500),
            max_backoff: Duration::from_secs(30),
            max_retry_after: Duration::from_secs(5 * 60),
            request_timeout: Duration::from_secs(60),
            download_timeout: Duration::from_secs(120),
            loopback_file_port: None,
        }
    }
}

/// Errors from the Trello client. Messages never contain the key or token.
#[derive(Debug, Error)]
pub enum TrelloError {
    /// `401`: Trello's plain-text reason, e.g. `invalid key`, `invalid token`, `expired token`.
    #[error("Trello rejected the credentials: {message}")]
    Unauthorized { message: String },
    #[error("the Trello token has no access to this resource: {message}")]
    Forbidden { message: String },
    #[error("the Trello object was not found: {message}")]
    NotFound { message: String },
    /// Still rate limited after the allowed attempts, or the wait exceeded `max_retry_after`.
    #[error("Trello kept rate limiting the requests")]
    RateLimited { retry_after: Option<Duration> },
    /// Any other error (after retries for 5xx).
    #[error("Trello API error {status}: {message}")]
    Api { status: u16, message: String },
    #[error("the request to Trello failed: {0}")]
    Http(String),
    #[error("could not decode the Trello response: {0}")]
    Decode(String),
    /// The file URL is not a Trello download, or a redirect left HTTPS.
    #[error("the file URL is not a Trello download")]
    FileNotAllowed,
    /// Trello refused the download (`401`/`403`): some Enterprise workspaces block it.
    #[error("Trello refused the file download ({status})")]
    FileRefused { status: u16 },
    #[error("the file is larger than {limit} bytes")]
    FileTooLarge { limit: u64 },
    #[error("invalid Trello client configuration: {0}")]
    Config(String),
}

/// A downloaded Trello attachment.
#[derive(Clone, Debug, PartialEq)]
pub struct DownloadedFile {
    pub bytes: Vec<u8>,
    /// A sanitized file name from `Content-Disposition`, else the URL's last path segment.
    pub file_name: String,
}

/// Trello API client. Cheap to clone; clones share the rate limiter.
#[derive(Clone)]
pub struct TrelloClient {
    inner: Arc<Inner>,
}

struct Inner {
    api: reqwest::Client,
    files: reqwest::Client,
    authorization: HeaderValue,
    base_url: Url,
    config: TrelloClientConfig,
    limiter: RateLimiter,
    file_policy: FilePolicy,
}

impl fmt::Debug for TrelloClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TrelloClient")
            .field("base_url", &self.inner.base_url.as_str())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug)]
enum FilePolicy {
    Trello,
    Loopback(u16),
}

impl FilePolicy {
    /// Whether a request to `url` may carry the credentials.
    fn sends_credentials(self, url: &Url) -> bool {
        match self {
            Self::Trello => is_trello_host(url),
            Self::Loopback(port) => is_loopback(url) && url.port() == Some(port),
        }
    }

    /// Whether `url` is an uploaded attachment the import downloads.
    fn is_download(self, url: &Url) -> bool {
        self.sends_credentials(url)
            && url
                .path_segments()
                .is_some_and(|mut segments| segments.any(|segment| segment == "download"))
    }

    /// Whether a redirect may lead to `url` (the credentials go along only if
    /// [`Self::sends_credentials`]).
    fn may_follow(self, url: &Url) -> bool {
        match self {
            Self::Trello => {
                url.scheme() == "https" && url.username().is_empty() && url.password().is_none()
            }
            Self::Loopback(_) => is_loopback(url),
        }
    }
}

fn is_loopback(url: &Url) -> bool {
    url.scheme() == "http" && url.host_str() == Some("127.0.0.1")
}

/// True for HTTPS URLs on the default port, without credentials, on `trello.com` or
/// `api.trello.com`: the only hosts that receive the key and token.
#[must_use]
pub fn is_trello_host(url: &Url) -> bool {
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some_and(|port| port != 443)
    {
        return false;
    }
    url.host_str()
        .map(str::to_ascii_lowercase)
        .is_some_and(|host| host == "trello.com" || host == "api.trello.com")
}

impl TrelloClient {
    /// Builds a client for an API key and a user token.
    pub fn new(key: &str, token: &str, config: TrelloClientConfig) -> Result<Self, TrelloError> {
        let (key, token) = (key.trim(), token.trim());
        if key.is_empty() || token.is_empty() {
            return Err(TrelloError::Config("the key or token is empty".to_owned()));
        }
        // Both are alphanumeric; anything else could break out of the quoted header value.
        if !key.chars().all(|c| c.is_ascii_alphanumeric())
            || !token.chars().all(|c| c.is_ascii_alphanumeric())
        {
            return Err(TrelloError::Config(
                "the key or token contains invalid characters".to_owned(),
            ));
        }
        let mut authorization = HeaderValue::from_str(&format!(
            "OAuth oauth_consumer_key=\"{key}\", oauth_token=\"{token}\""
        ))
        .map_err(|_| TrelloError::Config("the key or token is invalid".to_owned()))?;
        authorization.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, authorization.clone());
        headers.insert(header::ACCEPT, HeaderValue::from_static("application/json"));

        let mut base_url = Url::parse(config.base_url.trim())
            .map_err(|error| TrelloError::Config(format!("base URL: {error}")))?;
        if !matches!(base_url.scheme(), "https" | "http") {
            return Err(TrelloError::Config("base URL must be http(s)".to_owned()));
        }
        if !base_url.path().ends_with('/') {
            base_url.set_path(&format!("{}/", base_url.path()));
        }
        let file_policy = config
            .loopback_file_port
            .map_or(FilePolicy::Trello, FilePolicy::Loopback);
        let api = reqwest::Client::builder()
            .default_headers(headers)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(config.request_timeout)
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|error| TrelloError::Config(error.to_string()))?;
        // Redirects are followed by hand, so the credentials go only where
        // `sends_credentials` allows.
        let files = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(config.download_timeout)
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|error| TrelloError::Config(error.to_string()))?;
        Ok(Self {
            inner: Arc::new(Inner {
                api,
                files,
                authorization,
                base_url,
                limiter: RateLimiter::per_second(config.requests_per_second),
                config,
                file_policy,
            }),
        })
    }

    /// Whether `url` is an uploaded Trello attachment (a file the import downloads).
    #[must_use]
    pub fn is_download_url(&self, url: &str) -> bool {
        Url::parse(url).is_ok_and(|url| self.inner.file_policy.is_download(&url))
    }

    /// The token's member: validates the key and token.
    pub async fn me(&self) -> Result<TrelloMember, TrelloError> {
        self.get("members/me", &[("fields", MEMBER_FIELDS)]).await
    }

    /// The Trello Workspaces the member belongs to.
    pub async fn organizations(&self) -> Result<Vec<TrelloOrganization>, TrelloError> {
        self.get(
            "members/me/organizations",
            &[("fields", ORGANIZATION_FIELDS)],
        )
        .await
    }

    /// Every board the member can see, closed ones included.
    pub async fn boards(&self) -> Result<Vec<TrelloBoard>, TrelloError> {
        self.get(
            "members/me/boards",
            &[("filter", "all"), ("fields", BOARD_FIELDS)],
        )
        .await
    }

    /// The board's open lists, each with the ids of its open cards (for the scan's counts).
    pub async fn open_lists_with_cards(
        &self,
        board_id: &str,
    ) -> Result<Vec<TrelloList>, TrelloError> {
        self.get(
            &format!("boards/{}/lists", checked_id(board_id)?),
            &[
                ("filter", "open"),
                ("fields", "id"),
                ("cards", "open"),
                ("card_fields", "id"),
            ],
        )
        .await
    }

    /// Every list of the board, archived ones included.
    pub async fn lists(&self, board_id: &str) -> Result<Vec<TrelloList>, TrelloError> {
        self.get(
            &format!("boards/{}/lists", checked_id(board_id)?),
            &[("filter", "all"), ("fields", LIST_FIELDS)],
        )
        .await
    }

    pub async fn labels(&self, board_id: &str) -> Result<Vec<TrelloLabel>, TrelloError> {
        self.get(
            &format!("boards/{}/labels", checked_id(board_id)?),
            &[("fields", LABEL_FIELDS), ("limit", "1000")],
        )
        .await
    }

    pub async fn members(&self, board_id: &str) -> Result<Vec<TrelloMember>, TrelloError> {
        self.get(
            &format!("boards/{}/members", checked_id(board_id)?),
            &[("fields", MEMBER_FIELDS)],
        )
        .await
    }

    /// The board's custom field definitions (the Custom Fields Power-Up).
    pub async fn custom_fields(&self, board_id: &str) -> Result<Vec<CustomField>, TrelloError> {
        self.get(
            &format!("boards/{}/customFields", checked_id(board_id)?),
            &[],
        )
        .await
    }

    /// The board's open cards with their attachments and custom field values.
    pub async fn open_cards(&self, board_id: &str) -> Result<Vec<TrelloCard>, TrelloError> {
        self.get(
            &format!("boards/{}/cards/open", checked_id(board_id)?),
            &[
                ("fields", CARD_FIELDS),
                ("attachments", "true"),
                ("attachment_fields", ATTACHMENT_FIELDS),
                ("customFieldItems", "true"),
            ],
        )
        .await
    }

    /// How many archived cards the board has.
    pub async fn archived_card_count(&self, board_id: &str) -> Result<usize, TrelloError> {
        let cards: Vec<IdRef> = self
            .get(
                &format!("boards/{}/cards/closed", checked_id(board_id)?),
                &[("fields", "id")],
            )
            .await?;
        Ok(cards.len())
    }

    /// Every checklist on the board's cards, with its items.
    pub async fn checklists(&self, board_id: &str) -> Result<Vec<TrelloChecklist>, TrelloError> {
        self.get(
            &format!("boards/{}/checklists", checked_id(board_id)?),
            &[
                ("fields", CHECKLIST_FIELDS),
                ("checkItems", "all"),
                ("checkItem_fields", CHECK_ITEM_FIELDS),
            ],
        )
        .await
    }

    /// Every comment on the board's cards, newest first: `commentCard` actions in pages of
    /// [`COMMENT_PAGE`], each page `before` the oldest one so far.
    pub async fn comments(&self, board_id: &str) -> Result<Vec<CommentAction>, TrelloError> {
        let path = format!("boards/{}/actions", checked_id(board_id)?);
        let limit = COMMENT_PAGE.to_string();
        let mut actions: Vec<CommentAction> = Vec::new();
        loop {
            let before = actions.last().map(|action| action.id.clone());
            let mut query = vec![
                ("filter", "commentCard"),
                ("fields", ACTION_FIELDS),
                ("limit", limit.as_str()),
                ("memberCreator", "true"),
                ("memberCreator_fields", MEMBER_FIELDS),
            ];
            if let Some(before) = before.as_deref() {
                query.push(("before", before));
            }
            let page: Vec<CommentAction> = self.get(&path, &query).await?;
            let full = page.len() >= COMMENT_PAGE;
            let progressed = page.last().map(|action| &action.id) != before.as_ref();
            actions.extend(page);
            if !full || !progressed {
                break;
            }
        }
        Ok(actions)
    }

    /// Downloads an uploaded attachment with the credentials. Fails with
    /// [`TrelloError::FileTooLarge`] once more than `max_bytes` arrive, and with
    /// [`TrelloError::FileRefused`] when Trello answers `401` or `403`.
    pub async fn download_file(
        &self,
        url: &str,
        max_bytes: u64,
    ) -> Result<DownloadedFile, TrelloError> {
        let original = Url::parse(url).map_err(|_| TrelloError::FileNotAllowed)?;
        let policy = self.inner.file_policy;
        if !policy.is_download(&original) {
            return Err(TrelloError::FileNotAllowed);
        }
        let max_attempts = self.inner.config.max_attempts.max(1);
        let mut target = original.clone();
        let mut redirects = 0;
        let mut attempt = 0;
        let mut response = loop {
            attempt += 1;
            let mut request = self.inner.files.get(target.clone());
            if policy.sends_credentials(&target) {
                self.inner.limiter.acquire().await;
                request = request.header(header::AUTHORIZATION, self.inner.authorization.clone());
            }
            match request.send().await {
                Ok(response) if response.status().is_redirection() => {
                    let next = response
                        .headers()
                        .get(header::LOCATION)
                        .and_then(|value| value.to_str().ok())
                        .and_then(|location| target.join(location).ok())
                        .ok_or(TrelloError::FileNotAllowed)?;
                    redirects += 1;
                    if redirects > 5 || !policy.may_follow(&next) {
                        return Err(TrelloError::FileNotAllowed);
                    }
                    target = next;
                    attempt = 0;
                }
                Ok(response) => {
                    let status = response.status();
                    if status == StatusCode::TOO_MANY_REQUESTS && attempt < max_attempts {
                        let wait = RateLimits::from_headers(response.headers())
                            .retry_after
                            .unwrap_or(WINDOW);
                        self.inner.limiter.pause(wait);
                        continue;
                    }
                    if status.is_server_error() && attempt < max_attempts {
                        tokio::time::sleep(self.backoff(attempt)).await;
                        continue;
                    }
                    break response;
                }
                Err(error) if is_transient(&error) && attempt < max_attempts => {
                    tokio::time::sleep(self.backoff(attempt)).await;
                }
                Err(error) => return Err(http_error(error)),
            }
        };
        let status = response.status();
        if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
            return Err(TrelloError::FileRefused {
                status: status.as_u16(),
            });
        }
        if status == StatusCode::TOO_MANY_REQUESTS {
            return Err(TrelloError::RateLimited { retry_after: None });
        }
        if !status.is_success() {
            return Err(TrelloError::Api {
                status: status.as_u16(),
                message: format!("the file host answered {status}"),
            });
        }
        if response
            .content_length()
            .is_some_and(|length| length > max_bytes)
        {
            return Err(TrelloError::FileTooLarge { limit: max_bytes });
        }
        let disposition_name = response
            .headers()
            .get(header::CONTENT_DISPOSITION)
            .and_then(|value| value.to_str().ok())
            .and_then(disposition_file_name);
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(http_error)? {
            if bytes.len() as u64 + chunk.len() as u64 > max_bytes {
                return Err(TrelloError::FileTooLarge { limit: max_bytes });
            }
            bytes.extend_from_slice(&chunk);
        }
        let file_name = disposition_name.unwrap_or_else(|| {
            original
                .path_segments()
                .and_then(|mut segments| segments.next_back())
                .map(|segment| {
                    percent_encoding::percent_decode_str(segment)
                        .decode_utf8_lossy()
                        .into_owned()
                })
                .unwrap_or_default()
        });
        Ok(DownloadedFile {
            bytes,
            file_name: sanitize_file_name(&file_name),
        })
    }

    async fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<T, TrelloError> {
        let mut url = self
            .inner
            .base_url
            .join(path)
            .map_err(|error| TrelloError::Config(format!("path: {error}")))?;
        if !query.is_empty() {
            url.query_pairs_mut().extend_pairs(query);
        }
        let config = &self.inner.config;
        let max_attempts = config.max_attempts.max(1);
        let mut attempt = 0;
        loop {
            attempt += 1;
            self.inner.limiter.acquire().await;
            let response = match self.inner.api.get(url.clone()).send().await {
                Ok(response) => response,
                Err(error) if is_transient(&error) && attempt < max_attempts => {
                    tokio::time::sleep(self.backoff(attempt)).await;
                    continue;
                }
                Err(error) => return Err(http_error(error)),
            };
            let status = response.status();
            let limits = RateLimits::from_headers(response.headers());
            let bytes = response.bytes().await.map_err(http_error)?;
            if status == StatusCode::TOO_MANY_REQUESTS {
                let wait = limits
                    .retry_after
                    .or_else(|| limits.window())
                    .unwrap_or(WINDOW)
                    .max(self.backoff(attempt).min(WINDOW));
                if attempt >= max_attempts || wait > config.max_retry_after {
                    return Err(TrelloError::RateLimited {
                        retry_after: Some(wait),
                    });
                }
                tracing::debug!(wait_ms = wait.as_millis() as u64, "Trello rate limited");
                self.inner.limiter.pause(wait);
                continue;
            }
            let message = error_message(&bytes);
            if status.is_server_error() {
                if attempt < max_attempts {
                    tokio::time::sleep(self.backoff(attempt)).await;
                    continue;
                }
                return Err(TrelloError::Api {
                    status: status.as_u16(),
                    message,
                });
            }
            if let Some(wait) = limits.nearly_spent() {
                self.inner.limiter.pause(wait.min(config.max_retry_after));
            }
            match status {
                StatusCode::UNAUTHORIZED => return Err(TrelloError::Unauthorized { message }),
                StatusCode::FORBIDDEN => return Err(TrelloError::Forbidden { message }),
                StatusCode::NOT_FOUND => return Err(TrelloError::NotFound { message }),
                _ if !status.is_success() => {
                    return Err(TrelloError::Api {
                        status: status.as_u16(),
                        message,
                    });
                }
                _ => {}
            }
            return serde_json::from_slice(&bytes)
                .map_err(|error| TrelloError::Decode(error.to_string()));
        }
    }

    fn backoff(&self, attempt: u32) -> Duration {
        let config = &self.inner.config;
        backoff(config.initial_backoff, config.max_backoff, attempt)
    }
}

/// Trello answers errors in plain text (`invalid key`) or as JSON with `message`; at most 300
/// characters of it.
fn error_message(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let message = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|json| json.get("message")?.as_str().map(str::to_owned))
        .unwrap_or_else(|| text.trim().to_owned());
    message.chars().take(300).collect()
}

/// A Trello object id: 24 hex digits. Anything else could change the request path.
fn checked_id(raw: &str) -> Result<&str, TrelloError> {
    if raw.len() == 24 && raw.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(raw)
    } else {
        Err(TrelloError::NotFound {
            message: "invalid Trello id".to_owned(),
        })
    }
}

/// What the rate-limit headers of one response say.
#[derive(Debug, Default, PartialEq)]
struct RateLimits {
    retry_after: Option<Duration>,
    token_remaining: Option<i64>,
    token_interval: Option<Duration>,
    key_remaining: Option<i64>,
    key_interval: Option<Duration>,
}

impl RateLimits {
    fn from_headers(headers: &HeaderMap) -> Self {
        let number = |name: &str| {
            headers
                .get(name)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.trim().parse::<i64>().ok())
        };
        let millis = |name: &str| {
            number(name)
                .and_then(|value| u64::try_from(value).ok())
                .map(Duration::from_millis)
        };
        Self {
            retry_after: headers
                .get(header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(parse_retry_after),
            token_remaining: number("x-rate-limit-api-token-remaining"),
            token_interval: millis("x-rate-limit-api-token-interval-ms"),
            key_remaining: number("x-rate-limit-api-key-remaining"),
            key_interval: millis("x-rate-limit-api-key-interval-ms"),
        }
    }

    /// The longer of the two windows.
    fn window(&self) -> Option<Duration> {
        self.token_interval.max(self.key_interval)
    }

    /// The wait before the next request when a window is (nearly) spent.
    fn nearly_spent(&self) -> Option<Duration> {
        let low = |remaining: Option<i64>, interval: Option<Duration>| {
            remaining
                .filter(|remaining| *remaining < LOW_REMAINING)
                .map(|_| interval.unwrap_or(WINDOW))
        };
        low(self.token_remaining, self.token_interval)
            .max(low(self.key_remaining, self.key_interval))
    }
}

fn http_error(error: reqwest::Error) -> TrelloError {
    TrelloError::Http(error.without_url().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sends_the_credentials_only_to_trello_hosts() {
        let sends = |url: &str| FilePolicy::Trello.sends_credentials(&Url::parse(url).unwrap());
        assert!(sends(
            "https://trello.com/1/cards/a/attachments/b/download/x.png"
        ));
        assert!(sends("https://api.trello.com/1/members/me"));
        for url in [
            "http://trello.com/1/cards/a/attachments/b/download/x.png",
            "https://trello.com:8443/a",
            "https://user:pw@trello.com/a",
            "https://trello.com.evil.com/a",
            "https://eviltrello.com/a",
            "https://trello-attachments.s3.amazonaws.com/a/b/x.png",
        ] {
            assert!(!sends(url), "{url}");
        }
        let download = |url: &str| FilePolicy::Trello.is_download(&Url::parse(url).unwrap());
        assert!(download(
            "https://trello.com/1/cards/a/attachments/b/download/x.png"
        ));
        assert!(!download("https://trello.com/c/abc/1-card"));
    }

    #[test]
    fn reads_rate_limit_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-rate-limit-api-token-remaining",
            HeaderValue::from_static("1"),
        );
        headers.insert(
            "x-rate-limit-api-token-interval-ms",
            HeaderValue::from_static("10000"),
        );
        headers.insert(
            "x-rate-limit-api-key-remaining",
            HeaderValue::from_static("250"),
        );
        let limits = RateLimits::from_headers(&headers);
        assert_eq!(
            limits,
            RateLimits {
                retry_after: None,
                token_remaining: Some(1),
                token_interval: Some(Duration::from_secs(10)),
                key_remaining: Some(250),
                key_interval: None,
            }
        );
        assert_eq!(limits.nearly_spent(), Some(Duration::from_secs(10)));
        assert_eq!(RateLimits::default().nearly_spent(), None);
        assert_eq!(error_message(b"invalid key\n"), "invalid key");
        assert_eq!(
            error_message(br#"{"message": "unauthorized permission requested"}"#),
            "unauthorized permission requested"
        );
    }

    #[test]
    fn checks_ids_and_hides_the_credentials() {
        assert!(checked_id("5f0c7c1d2e3f4a5b6c7d8e9f").is_ok());
        assert!(checked_id("../members/me").is_err());
        let client =
            TrelloClient::new("abc123", "secrettoken", TrelloClientConfig::default()).unwrap();
        assert!(!format!("{client:?}").contains("secret"));
        assert!(matches!(
            TrelloClient::new("abc\"", "token", TrelloClientConfig::default()),
            Err(TrelloError::Config(_))
        ));
    }
}
