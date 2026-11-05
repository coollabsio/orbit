//! A small typed Linear GraphQL client for the import (<https://linear.app/developers/graphql>).
//!
//! - Every API request is `POST /graphql` with `Authorization: <personal API key>` (no `Bearer`:
//!   that prefix is for OAuth tokens).
//! - Requests are sequential per import and spaced by a rate limiter (default 0.6 per second,
//!   under the 2,500 requests per hour a key may make). After each response the
//!   `X-RateLimit-*-Remaining` headers are read: when the hourly request or complexity budget
//!   (3,000,000 points) is nearly spent, the client waits for its reset
//!   (<https://linear.app/developers/rate-limiting>).
//! - A rate-limited answer (`429`, or any status with an error whose `extensions.code` is
//!   `RATELIMITED`) waits for `Retry-After` or the reset header, then retries; network errors and
//!   `5xx` retry with bounded exponential backoff. Every call is a read, so retrying is safe.
//! - Lists use Relay paging with an explicit `first` and follow `pageInfo.endCursor`.
//! - Files on `uploads.linear.app` need the same `Authorization` header
//!   (<https://linear.app/developers/file-storage-authentication>). The key is sent only to
//!   HTTPS hosts under `linear.app`; a redirect elsewhere is followed without it.

use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::header::{self, HeaderMap, HeaderValue};
use reqwest::{StatusCode, Url};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use thiserror::Error;

use super::model::{
    ATTACHMENT_FIELDS, COMMENT_FIELDS, Connection, FOLLOW_UP_PAGE, ISSUE_PAGE, LABEL_FIELDS,
    LinearIssue, LinearTeam, LinearUser, Organization, RELATION_FIELDS, STATE_FIELDS, TEAM_FIELDS,
    USER_FIELDS, WorkflowState, issue_fields,
};
use crate::notion::files::sanitize_file_name;
use crate::task_import::{RateLimiter, backoff, is_transient, parse_retry_after};

/// The production GraphQL endpoint.
pub const DEFAULT_ENDPOINT: &str = "https://api.linear.app/graphql";
/// Where Linear serves uploaded files.
pub const UPLOAD_HOST: &str = "uploads.linear.app";
/// Below this many complexity points left in the hour, wait for the reset before the next query
/// (an issue page costs about 5,000).
const LOW_COMPLEXITY: i64 = 20_000;

/// Client settings. `Default` is right for production; tests change the endpoint and speed.
#[derive(Clone, Debug)]
pub struct LinearClientConfig {
    /// The GraphQL endpoint, e.g. `https://api.linear.app/graphql`.
    pub endpoint: String,
    /// Average request rate; requests are spaced `1 / requests_per_second` apart.
    pub requests_per_second: f64,
    /// Attempts per request, including the first (minimum 1).
    pub max_attempts: u32,
    /// First backoff for network/5xx errors; doubles per attempt.
    pub initial_backoff: Duration,
    /// Upper bound for one backoff sleep.
    pub max_backoff: Duration,
    /// A rate-limit wait longer than this fails with [`LinearError::RateLimited`] instead of
    /// sleeping, so the job can reschedule itself.
    pub max_retry_after: Duration,
    pub request_timeout: Duration,
    pub download_timeout: Duration,
    /// Treat `http://127.0.0.1:<port>` as the Linear file host (it gets the key) and follow
    /// redirects to other loopback ports (without it). For tests only: never set from user
    /// configuration.
    pub loopback_file_port: Option<u16>,
}

impl Default for LinearClientConfig {
    fn default() -> Self {
        Self {
            endpoint: DEFAULT_ENDPOINT.to_owned(),
            requests_per_second: 0.6,
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

/// Errors from the Linear client. Messages never contain the key.
#[derive(Debug, Error)]
pub enum LinearError {
    /// The key is invalid, expired or revoked.
    #[error("the Linear API key is invalid or was revoked: {message}")]
    Unauthorized { message: String },
    #[error("the Linear API key has no access to this resource: {message}")]
    Forbidden { message: String },
    #[error("the Linear object was not found: {message}")]
    NotFound { message: String },
    /// Still rate limited after the allowed attempts, or the wait exceeded `max_retry_after`.
    #[error("Linear kept rate limiting the requests")]
    RateLimited { retry_after: Option<Duration> },
    /// Any other error (after retries for 5xx).
    #[error("Linear API error {status}: {message}")]
    Api { status: u16, message: String },
    #[error("the request to Linear failed: {0}")]
    Http(String),
    #[error("could not decode the Linear response: {0}")]
    Decode(String),
    /// The file URL is not a Linear upload, or a redirect left HTTPS.
    #[error("the file URL is not a Linear upload")]
    FileNotAllowed,
    #[error("the file is larger than {limit} bytes")]
    FileTooLarge { limit: u64 },
    #[error("invalid Linear client configuration: {0}")]
    Config(String),
}

/// A downloaded Linear upload.
#[derive(Clone, Debug, PartialEq)]
pub struct DownloadedFile {
    pub bytes: Vec<u8>,
    /// A sanitized file name from `Content-Disposition`, else the URL's last path segment.
    pub file_name: String,
}

/// Linear API client. Cheap to clone; clones share the rate limiter.
#[derive(Clone)]
pub struct LinearClient {
    inner: Arc<Inner>,
}

struct Inner {
    api: reqwest::Client,
    files: reqwest::Client,
    authorization: HeaderValue,
    endpoint: Url,
    config: LinearClientConfig,
    limiter: RateLimiter,
    file_policy: FilePolicy,
}

impl fmt::Debug for LinearClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LinearClient")
            .field("endpoint", &self.inner.endpoint.as_str())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug)]
enum FilePolicy {
    Linear,
    Loopback(u16),
}

impl FilePolicy {
    /// Whether a request to `url` may carry the key.
    fn sends_key(self, url: &Url) -> bool {
        match self {
            Self::Linear => is_linear_host(url),
            Self::Loopback(port) => is_loopback(url) && url.port() == Some(port),
        }
    }

    /// Whether `url` is a Linear upload the import downloads.
    fn is_upload(self, url: &Url) -> bool {
        match self {
            Self::Linear => {
                is_linear_host(url)
                    && url.host_str().map(str::to_ascii_lowercase).as_deref() == Some(UPLOAD_HOST)
            }
            Self::Loopback(_) => self.sends_key(url),
        }
    }

    /// Whether a redirect may lead to `url` (the key goes along only if [`Self::sends_key`]).
    fn may_follow(self, url: &Url) -> bool {
        match self {
            Self::Linear => {
                url.scheme() == "https" && url.username().is_empty() && url.password().is_none()
            }
            Self::Loopback(_) => is_loopback(url),
        }
    }
}

fn is_loopback(url: &Url) -> bool {
    url.scheme() == "http" && url.host_str() == Some("127.0.0.1")
}

/// True for HTTPS URLs on the default port, without credentials, on `linear.app` or one of its
/// subdomains: the only hosts that receive the API key.
#[must_use]
pub fn is_linear_host(url: &Url) -> bool {
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some_and(|port| port != 443)
    {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    host == "linear.app" || host.ends_with(".linear.app")
}

impl LinearClient {
    /// Builds a client for a personal API key.
    pub fn new(key: &str, config: LinearClientConfig) -> Result<Self, LinearError> {
        let key = key.trim();
        if key.is_empty() {
            return Err(LinearError::Config("the key is empty".to_owned()));
        }
        let mut authorization = HeaderValue::from_str(key)
            .map_err(|_| LinearError::Config("the key contains invalid characters".to_owned()))?;
        authorization.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, authorization.clone());
        headers.insert(header::ACCEPT, HeaderValue::from_static("application/json"));

        let endpoint = Url::parse(config.endpoint.trim())
            .map_err(|error| LinearError::Config(format!("endpoint: {error}")))?;
        if !matches!(endpoint.scheme(), "https" | "http") {
            return Err(LinearError::Config("endpoint must be http(s)".to_owned()));
        }
        let file_policy = config
            .loopback_file_port
            .map_or(FilePolicy::Linear, FilePolicy::Loopback);
        let api = reqwest::Client::builder()
            .default_headers(headers)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(config.request_timeout)
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|error| LinearError::Config(error.to_string()))?;
        // Redirects are followed by hand, so the key goes only where `sends_key` allows.
        let files = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(config.download_timeout)
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|error| LinearError::Config(error.to_string()))?;
        Ok(Self {
            inner: Arc::new(Inner {
                api,
                files,
                authorization,
                endpoint,
                limiter: RateLimiter::per_second(config.requests_per_second),
                config,
                file_policy,
            }),
        })
    }

    /// Whether `url` is a Linear upload (a file the import downloads and attaches).
    #[must_use]
    pub fn is_upload_url(&self, url: &str) -> bool {
        Url::parse(url).is_ok_and(|url| self.inner.file_policy.is_upload(&url))
    }

    /// The key's user: validates the key.
    pub async fn viewer(&self) -> Result<LinearUser, LinearError> {
        #[derive(Deserialize)]
        struct Data {
            viewer: LinearUser,
        }
        let data: Data = self
            .query(
                &format!("query {{ viewer {{ {USER_FIELDS} }} }}"),
                json!({}),
            )
            .await?;
        Ok(data.viewer)
    }

    pub async fn organization(&self) -> Result<Organization, LinearError> {
        #[derive(Deserialize)]
        struct Data {
            organization: Organization,
        }
        let data: Data = self
            .query("query { organization { name urlKey } }", json!({}))
            .await?;
        Ok(data.organization)
    }

    /// The teams the key's user can see, archived ones included. Stops after `max`.
    pub async fn teams(&self, max: usize) -> Result<Vec<LinearTeam>, LinearError> {
        self.paginate(
            &format!(
                "query($after: String) {{ teams(first: 100, after: $after, includeArchived: true) \
                 {{ nodes {{ {TEAM_FIELDS} }} pageInfo {{ hasNextPage endCursor }} }} }}"
            ),
            json!({}),
            "/teams",
            max,
        )
        .await
    }

    /// The team's workflow states.
    pub async fn states(&self, team_id: &str) -> Result<Vec<WorkflowState>, LinearError> {
        self.paginate(
            &format!(
                "query($id: String!, $after: String) {{ team(id: $id) {{ states(first: 100, \
                 after: $after) {{ nodes {{ {STATE_FIELDS} }} pageInfo {{ hasNextPage endCursor }} }} }} }}"
            ),
            json!({"id": team_id}),
            "/team/states",
            usize::MAX,
        )
        .await
    }

    /// Every issue of the team, archived ones too, oldest first, stopping after `max`. Each
    /// issue's labels, relations, attachments and comments are complete.
    pub async fn team_issues(
        &self,
        team_id: &str,
        max: usize,
    ) -> Result<Vec<LinearIssue>, LinearError> {
        let mut issues: Vec<LinearIssue> = self
            .paginate(
                &format!(
                    "query($id: String!, $after: String) {{ team(id: $id) {{ issues(first: {ISSUE_PAGE}, \
                     after: $after, includeArchived: true, orderBy: createdAt) {{ nodes {{ {} }} \
                     pageInfo {{ hasNextPage endCursor }} }} }} }}",
                    issue_fields()
                ),
                json!({"id": team_id}),
                "/team/issues",
                max,
            )
            .await?;
        for issue in &mut issues {
            self.complete(issue).await?;
        }
        Ok(issues)
    }

    /// Fetches the rest of each nested connection the issue page cut off.
    async fn complete(&self, issue: &mut LinearIssue) -> Result<(), LinearError> {
        self.rest(&issue.id, "labels", LABEL_FIELDS, &mut issue.labels)
            .await?;
        self.rest(
            &issue.id,
            "relations",
            RELATION_FIELDS,
            &mut issue.relations,
        )
        .await?;
        self.rest(
            &issue.id,
            "attachments",
            ATTACHMENT_FIELDS,
            &mut issue.attachments,
        )
        .await?;
        self.rest(&issue.id, "comments", COMMENT_FIELDS, &mut issue.comments)
            .await
    }

    async fn rest<T: DeserializeOwned>(
        &self,
        issue_id: &str,
        name: &str,
        fields: &str,
        connection: &mut Connection<T>,
    ) -> Result<(), LinearError> {
        if !connection.page_info.has_next_page {
            return Ok(());
        }
        let Some(after) = connection.page_info.end_cursor.take() else {
            connection.page_info.has_next_page = false;
            return Ok(());
        };
        let more: Vec<T> = self
            .paginate(
                &format!(
                    "query($id: String!, $after: String) {{ issue(id: $id) {{ {name}(first: \
                     {FOLLOW_UP_PAGE}, after: $after) {{ nodes {{ {fields} }} pageInfo {{ hasNextPage endCursor }} }} }} }}"
                ),
                json!({"id": issue_id, "after": after}),
                &format!("/issue/{name}"),
                usize::MAX,
            )
            .await?;
        connection.nodes.extend(more);
        connection.page_info = Default::default();
        Ok(())
    }

    /// Downloads a Linear upload with the key. Fails with [`LinearError::FileTooLarge`] once
    /// more than `max_bytes` arrive.
    pub async fn download_file(
        &self,
        url: &str,
        max_bytes: u64,
    ) -> Result<DownloadedFile, LinearError> {
        let original = Url::parse(url).map_err(|_| LinearError::FileNotAllowed)?;
        let policy = self.inner.file_policy;
        if !policy.is_upload(&original) {
            return Err(LinearError::FileNotAllowed);
        }
        let max_attempts = self.inner.config.max_attempts.max(1);
        let mut target = original.clone();
        let mut redirects = 0;
        let mut attempt = 0;
        let mut response = loop {
            attempt += 1;
            let mut request = self.inner.files.get(target.clone());
            if policy.sends_key(&target) {
                request = request.header(header::AUTHORIZATION, self.inner.authorization.clone());
            }
            match request.send().await {
                Ok(response) if response.status().is_redirection() => {
                    let next = response
                        .headers()
                        .get(header::LOCATION)
                        .and_then(|value| value.to_str().ok())
                        .and_then(|location| target.join(location).ok())
                        .ok_or(LinearError::FileNotAllowed)?;
                    redirects += 1;
                    if redirects > 5 || !policy.may_follow(&next) {
                        return Err(LinearError::FileNotAllowed);
                    }
                    target = next;
                    attempt = 0;
                }
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
                Err(error) if is_transient(&error) && attempt < max_attempts => {
                    tokio::time::sleep(self.backoff(attempt)).await;
                }
                Err(error) => return Err(http_error(error)),
            }
        };
        let status = response.status();
        if !status.is_success() {
            return Err(LinearError::Api {
                status: status.as_u16(),
                message: format!("the file host answered {status}"),
            });
        }
        if response
            .content_length()
            .is_some_and(|length| length > max_bytes)
        {
            return Err(LinearError::FileTooLarge { limit: max_bytes });
        }
        let disposition_name = response
            .headers()
            .get(header::CONTENT_DISPOSITION)
            .and_then(|value| value.to_str().ok())
            .and_then(disposition_file_name);
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(http_error)? {
            if bytes.len() as u64 + chunk.len() as u64 > max_bytes {
                return Err(LinearError::FileTooLarge { limit: max_bytes });
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

    /// Follows `pageInfo.endCursor` of the connection at `pointer` (a JSON pointer into `data`)
    /// until the last page, stopping once `max` nodes arrived. The query takes `$after`.
    async fn paginate<T: DeserializeOwned>(
        &self,
        query: &str,
        mut variables: Value,
        pointer: &str,
        max: usize,
    ) -> Result<Vec<T>, LinearError> {
        let mut nodes = Vec::new();
        let mut seen = HashSet::new();
        loop {
            let data: Value = self.query(query, variables.clone()).await?;
            let connection = data
                .pointer(pointer)
                .cloned()
                .ok_or_else(|| LinearError::Decode(format!("missing {pointer}")))?;
            let page: Connection<T> = serde_json::from_value(connection)
                .map_err(|error| LinearError::Decode(error.to_string()))?;
            nodes.extend(page.nodes);
            if nodes.len() >= max {
                nodes.truncate(max);
                break;
            }
            match page.page_info.end_cursor {
                Some(cursor) if page.page_info.has_next_page && seen.insert(cursor.clone()) => {
                    variables["after"] = Value::String(cursor);
                }
                _ => break,
            }
        }
        Ok(nodes)
    }

    async fn query<T: DeserializeOwned>(
        &self,
        query: &str,
        variables: Value,
    ) -> Result<T, LinearError> {
        let body = json!({"query": query, "variables": variables});
        let config = &self.inner.config;
        let max_attempts = config.max_attempts.max(1);
        let mut attempt = 0;
        loop {
            attempt += 1;
            self.inner.limiter.acquire().await;
            let response = match self
                .inner
                .api
                .post(self.inner.endpoint.clone())
                .json(&body)
                .send()
                .await
            {
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
            let parsed: Option<GraphqlResponse> = serde_json::from_slice(&bytes).ok();
            let errors = parsed
                .as_ref()
                .map(|parsed| parsed.errors.as_slice())
                .unwrap_or_default();
            let code = |wanted: &str| errors.iter().any(|error| error.code() == Some(wanted));
            let message = errors
                .iter()
                .filter_map(|error| error.message.clone())
                .collect::<Vec<_>>()
                .join("; ");

            if status == StatusCode::TOO_MANY_REQUESTS || code("RATELIMITED") {
                let wait = limits
                    .retry_after
                    .or_else(|| limits.until_reset())
                    .unwrap_or_else(|| self.backoff(attempt));
                if attempt >= max_attempts || wait > config.max_retry_after {
                    return Err(LinearError::RateLimited {
                        retry_after: Some(wait),
                    });
                }
                tracing::debug!(wait_ms = wait.as_millis() as u64, "Linear rate limited");
                self.inner.limiter.pause(wait);
                continue;
            }
            if status.is_server_error() {
                if attempt < max_attempts {
                    tokio::time::sleep(self.backoff(attempt)).await;
                    continue;
                }
                return Err(LinearError::Api {
                    status: status.as_u16(),
                    message,
                });
            }
            if let Some(wait) = limits.nearly_spent() {
                self.inner.limiter.pause(wait.min(config.max_retry_after));
            }
            if status == StatusCode::UNAUTHORIZED || code("AUTHENTICATION_ERROR") {
                return Err(LinearError::Unauthorized { message });
            }
            if status == StatusCode::FORBIDDEN || code("FORBIDDEN") {
                return Err(LinearError::Forbidden { message });
            }
            if errors.iter().any(GraphqlError::is_not_found) {
                return Err(LinearError::NotFound { message });
            }
            if !errors.is_empty() || !status.is_success() {
                return Err(LinearError::Api {
                    status: status.as_u16(),
                    message,
                });
            }
            let data = parsed
                .and_then(|parsed| parsed.data)
                .ok_or_else(|| LinearError::Decode("the response has no data".to_owned()))?;
            return serde_json::from_value(data)
                .map_err(|error| LinearError::Decode(error.to_string()));
        }
    }

    fn backoff(&self, attempt: u32) -> Duration {
        let config = &self.inner.config;
        backoff(config.initial_backoff, config.max_backoff, attempt)
    }
}

#[derive(Deserialize)]
struct GraphqlResponse {
    #[serde(default)]
    data: Option<Value>,
    #[serde(default)]
    errors: Vec<GraphqlError>,
}

#[derive(Deserialize)]
struct GraphqlError {
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    extensions: Option<Value>,
}

impl GraphqlError {
    fn code(&self) -> Option<&str> {
        self.extensions.as_ref()?.get("code")?.as_str()
    }

    fn is_not_found(&self) -> bool {
        self.message
            .as_deref()
            .is_some_and(|message| message.to_ascii_lowercase().contains("not found"))
    }
}

/// What the rate-limit headers of one response say. Resets are epoch milliseconds.
#[derive(Debug, Default, PartialEq)]
struct RateLimits {
    retry_after: Option<Duration>,
    requests_remaining: Option<i64>,
    requests_reset: Option<i64>,
    complexity_remaining: Option<i64>,
    complexity_reset: Option<i64>,
}

impl RateLimits {
    fn from_headers(headers: &HeaderMap) -> Self {
        let number = |name: &str| {
            headers
                .get(name)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.trim().parse::<f64>().ok())
                .filter(|value| value.is_finite())
                .map(|value| value as i64)
        };
        Self {
            retry_after: headers
                .get(header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(parse_retry_after),
            requests_remaining: number("x-ratelimit-requests-remaining"),
            requests_reset: number("x-ratelimit-requests-reset"),
            complexity_remaining: number("x-ratelimit-complexity-remaining"),
            complexity_reset: number("x-ratelimit-complexity-reset"),
        }
    }

    /// The wait until the later of the two resets.
    fn until_reset(&self) -> Option<Duration> {
        let reset = self.requests_reset.max(self.complexity_reset)?;
        Some(wait_until(reset))
    }

    /// The wait before the next request when an hourly budget is (nearly) spent.
    fn nearly_spent(&self) -> Option<Duration> {
        let requests = self
            .requests_remaining
            .filter(|remaining| *remaining <= 1)
            .and(self.requests_reset);
        let complexity = self
            .complexity_remaining
            .filter(|remaining| *remaining < LOW_COMPLEXITY)
            .and(self.complexity_reset);
        requests.max(complexity).map(wait_until)
    }
}

fn wait_until(reset_millis: i64) -> Duration {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as i64);
    Duration::from_millis(u64::try_from(reset_millis - now).unwrap_or_default())
}

/// `attachment; filename="a.png"` → `a.png` (the quoted or bare form; not RFC 5987 `filename*`).
fn disposition_file_name(value: &str) -> Option<String> {
    value.split(';').find_map(|part| {
        let name = part.trim().strip_prefix("filename=")?;
        let name = name.trim().trim_matches('"').trim();
        (!name.is_empty()).then(|| name.to_owned())
    })
}

fn http_error(error: reqwest::Error) -> LinearError {
    LinearError::Http(error.without_url().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sends_the_key_only_to_linear_hosts() {
        let sends = |url: &str| FilePolicy::Linear.sends_key(&Url::parse(url).unwrap());
        assert!(sends("https://uploads.linear.app/org/a/b"));
        assert!(sends("https://api.linear.app/graphql"));
        for url in [
            "http://uploads.linear.app/org/a/b",
            "https://uploads.linear.app:8443/a",
            "https://user:pw@uploads.linear.app/a",
            "https://uploads.linear.app.evil.com/a",
            "https://evillinear.app/a",
            "https://storage.googleapis.com/linear-uploads/a",
        ] {
            assert!(!sends(url), "{url}");
        }
        let upload = |url: &str| FilePolicy::Linear.is_upload(&Url::parse(url).unwrap());
        assert!(upload("https://uploads.linear.app/org/a/b"));
        assert!(!upload("https://linear.app/acme/issue/ENG-1"));
    }

    #[test]
    fn reads_rate_limit_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("Retry-After", HeaderValue::from_static("3"));
        headers.insert(
            "X-RateLimit-Requests-Remaining",
            HeaderValue::from_static("1"),
        );
        headers.insert(
            "X-RateLimit-Requests-Reset",
            HeaderValue::from_static("1000"),
        );
        headers.insert(
            "X-RateLimit-Complexity-Remaining",
            HeaderValue::from_static("500000"),
        );
        let limits = RateLimits::from_headers(&headers);
        assert_eq!(
            limits,
            RateLimits {
                retry_after: Some(Duration::from_secs(3)),
                requests_remaining: Some(1),
                requests_reset: Some(1000),
                complexity_remaining: Some(500_000),
                complexity_reset: None,
            }
        );
        assert_eq!(
            limits.nearly_spent(),
            Some(Duration::ZERO),
            "reset already passed"
        );
        assert_eq!(RateLimits::default().nearly_spent(), None);
        assert_eq!(
            disposition_file_name("attachment; filename=\"spec v2.pdf\""),
            Some("spec v2.pdf".to_owned())
        );
    }

    #[test]
    fn debug_output_hides_the_key() {
        let client = LinearClient::new("lin_api_secret", LinearClientConfig::default()).unwrap();
        assert!(!format!("{client:?}").contains("secret"));
        assert!(matches!(
            LinearClient::new("bad\nkey", LinearClientConfig::default()),
            Err(LinearError::Config(_))
        ));
    }
}
