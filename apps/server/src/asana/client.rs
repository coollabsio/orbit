//! A small typed Asana API client for the import (<https://developers.asana.com/reference>).
//!
//! - Every request carries `Authorization: Bearer <personal access token>`.
//! - Requests are sequential per import and spaced by a rate limiter (default 2 per second).
//!   Asana allows 150 requests per minute per token on free domains and 1,500 on paid ones
//!   (<https://developers.asana.com/docs/rate-limits>), so 2/s fits every plan.
//! - `429` waits for `Retry-After` (seconds) and then pauses every request of this client;
//!   network errors and `500/502/503/504` retry with bounded exponential backoff. Every call
//!   here is a read, so retrying is safe.
//! - Lists use `limit=100` and follow `next_page.offset` until it is null
//!   (<https://developers.asana.com/docs/pagination>).
//! - File downloads use a separate HTTP client without the token whose redirect policy only
//!   follows allowlisted Asana file hosts ([`is_allowed_asana_file`]).

use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use reqwest::header::{self, HeaderMap, HeaderValue};
use reqwest::{StatusCode, Url};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use thiserror::Error;

use super::model::{
    ATTACHMENT_FIELDS, AsanaAttachment, AsanaProject, AsanaStory, AsanaTask, AsanaUser,
    AsanaWorkspace, Named, STORY_FIELDS, TASK_FIELDS,
};
use crate::notion::files::sanitize_file_name;
use crate::task_import::{RateLimiter, backoff, is_transient, parse_retry_after};

/// The production API root.
pub const DEFAULT_BASE_URL: &str = "https://app.asana.com/api/1.0";

/// Client settings. `Default` is right for production; tests change `base_url` and speed.
#[derive(Clone, Debug)]
pub struct AsanaClientConfig {
    /// API root without a trailing slash, e.g. `https://app.asana.com/api/1.0`.
    pub base_url: String,
    /// Average request rate; requests are spaced `1 / requests_per_second` apart.
    pub requests_per_second: f64,
    /// Attempts per request, including the first (minimum 1).
    pub max_attempts: u32,
    /// First backoff for network/5xx errors; doubles per attempt.
    pub initial_backoff: Duration,
    /// Upper bound for one backoff sleep.
    pub max_backoff: Duration,
    /// A `Retry-After` longer than this fails with [`AsanaError::RateLimited`] instead of
    /// sleeping, so a job can reschedule itself.
    pub max_retry_after: Duration,
    pub request_timeout: Duration,
    pub download_timeout: Duration,
    /// Also download files from `http://127.0.0.1:<port>` (a local fake Asana server). For
    /// tests only: never set from user configuration, since it lifts the file host allowlist
    /// for that loopback port.
    pub loopback_file_port: Option<u16>,
}

impl Default for AsanaClientConfig {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_owned(),
            requests_per_second: 2.0,
            max_attempts: 5,
            initial_backoff: Duration::from_millis(500),
            max_backoff: Duration::from_secs(30),
            max_retry_after: Duration::from_secs(120),
            request_timeout: Duration::from_secs(60),
            download_timeout: Duration::from_secs(120),
            loopback_file_port: None,
        }
    }
}

/// Errors from the Asana client. Messages never contain the token or signed file URLs.
#[derive(Debug, Error)]
pub enum AsanaError {
    /// `401`: the token is invalid, expired or revoked.
    #[error("the Asana token is invalid or was revoked: {message}")]
    Unauthorized { message: String },
    /// `403`: the token cannot read this object.
    #[error("the Asana token has no access to this resource: {message}")]
    Forbidden { message: String },
    /// `402`: a feature of a paid Asana plan (e.g. dependencies on a free domain).
    #[error("this needs a paid Asana plan: {message}")]
    PaymentRequired { message: String },
    #[error("the Asana object was not found: {message}")]
    NotFound { message: String },
    /// Still rate limited after the allowed attempts, or `Retry-After` exceeded
    /// `max_retry_after`.
    #[error("Asana kept rate limiting the requests")]
    RateLimited { retry_after: Option<Duration> },
    /// Any other error status (after retries for 5xx).
    #[error("Asana API error {status}: {message}")]
    Api { status: u16, message: String },
    #[error("the request to Asana failed: {0}")]
    Http(String),
    #[error("could not decode the Asana response: {0}")]
    Decode(String),
    /// A gid that is not numeric (never sent, to keep paths intact).
    #[error("invalid Asana id")]
    InvalidGid,
    /// The file URL (or a redirect) is not an allowlisted Asana file host.
    #[error("the file URL is not an allowed Asana file host")]
    FileNotAllowed,
    #[error("the file is larger than {limit} bytes")]
    FileTooLarge { limit: u64 },
    #[error("invalid Asana client configuration: {0}")]
    Config(String),
}

/// A downloaded Asana-hosted file.
#[derive(Clone, Debug, PartialEq)]
pub struct DownloadedFile {
    pub bytes: Vec<u8>,
    /// A sanitized file name from the URL's last path segment.
    pub file_name: String,
}

/// Asana API client. Cheap to clone; clones share the rate limiter.
#[derive(Clone)]
pub struct AsanaClient {
    inner: Arc<Inner>,
}

struct Inner {
    api: reqwest::Client,
    files: reqwest::Client,
    base_url: String,
    config: AsanaClientConfig,
    limiter: RateLimiter,
    file_policy: FilePolicy,
}

impl fmt::Debug for AsanaClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AsanaClient")
            .field("base_url", &self.inner.base_url)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug)]
enum FilePolicy {
    Asana,
    Loopback(u16),
}

impl FilePolicy {
    fn allows(self, url: &Url) -> bool {
        match self {
            Self::Asana => is_allowed_asana_file(url),
            Self::Loopback(port) => {
                url.scheme() == "http"
                    && url.host_str() == Some("127.0.0.1")
                    && url.port() == Some(port)
            }
        }
    }
}

/// True when `url` is an Asana-hosted file the import may download: HTTPS on the default port,
/// no credentials, on one of Asana's private S3 buckets (`asana-user-private-<region>`),
/// virtual-hosted (`<bucket>.s3.amazonaws.com`, `<bucket>.s3.<region>.amazonaws.com` or
/// `<bucket>.s3-<region>.amazonaws.com`). An attachment's `download_url` points there; other
/// attachment kinds (Google Drive, Dropbox, links) are never fetched, so a task cannot make
/// Orbit request an arbitrary host. Sources: the attachment reference
/// (<https://developers.asana.com/reference/getattachment>) and download URLs reported on the
/// Asana forum (<https://forum.asana.com/t/api-attachments-error/806570>).
#[must_use]
pub fn is_allowed_asana_file(url: &Url) -> bool {
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
    let Some(rest) = host.strip_suffix(".amazonaws.com") else {
        return false;
    };
    let Some((bucket, endpoint)) = rest.split_once('.') else {
        return false;
    };
    let region = |value: &str| {
        !value.is_empty()
            && value
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    };
    let endpoint_ok = endpoint == "s3"
        || endpoint.strip_prefix("s3.").is_some_and(region)
        || endpoint.strip_prefix("s3-").is_some_and(region);
    endpoint_ok
        && bucket
            .strip_prefix("asana-user-private-")
            .is_some_and(region)
}

impl AsanaClient {
    /// Builds a client for a personal access token.
    pub fn new(token: &str, config: AsanaClientConfig) -> Result<Self, AsanaError> {
        let token = token.trim();
        if token.is_empty() {
            return Err(AsanaError::Config("the token is empty".to_owned()));
        }
        let mut authorization = HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| AsanaError::Config("the token contains invalid characters".to_owned()))?;
        authorization.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, authorization);
        headers.insert(header::ACCEPT, HeaderValue::from_static("application/json"));

        let base_url = config.base_url.trim_end_matches('/').to_owned();
        let parsed = Url::parse(&base_url)
            .map_err(|error| AsanaError::Config(format!("base URL: {error}")))?;
        if !matches!(parsed.scheme(), "https" | "http") {
            return Err(AsanaError::Config("base URL must be http(s)".to_owned()));
        }
        let file_policy = config
            .loopback_file_port
            .map_or(FilePolicy::Asana, FilePolicy::Loopback);

        let api = reqwest::Client::builder()
            .default_headers(headers)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(config.request_timeout)
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|error| AsanaError::Config(error.to_string()))?;
        let files = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::custom(move |attempt| {
                if attempt.previous().len() >= 5 {
                    attempt.error("too many redirects")
                } else if file_policy.allows(attempt.url()) {
                    attempt.follow()
                } else {
                    attempt.error("redirect outside the Asana file host allowlist")
                }
            }))
            .timeout(config.download_timeout)
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|error| AsanaError::Config(error.to_string()))?;
        Ok(Self {
            inner: Arc::new(Inner {
                api,
                files,
                base_url,
                limiter: RateLimiter::per_second(config.requests_per_second),
                config,
                file_policy,
            }),
        })
    }

    /// `GET /users/me`: validates the token and returns its user.
    pub async fn me(&self) -> Result<AsanaUser, AsanaError> {
        self.get_one("/users/me", &[("opt_fields", "name,email")])
            .await
    }

    /// `GET /workspaces`: the workspaces and organizations of the token's user.
    pub async fn workspaces(&self) -> Result<Vec<AsanaWorkspace>, AsanaError> {
        self.get_all("/workspaces", &[("opt_fields", "name")], usize::MAX)
            .await
    }

    /// `GET /projects?workspace=`: the workspace's projects the user can see, archived ones
    /// included, with their team. Stops after `max` projects.
    pub async fn projects(
        &self,
        workspace_gid: &str,
        max: usize,
    ) -> Result<Vec<AsanaProject>, AsanaError> {
        let workspace = checked_gid(workspace_gid)?;
        self.get_all(
            "/projects",
            &[
                ("workspace", workspace),
                ("opt_fields", "name,archived,team.name"),
            ],
            max,
        )
        .await
    }

    /// `GET /projects/{gid}/sections`, in board order.
    pub async fn sections(&self, project_gid: &str) -> Result<Vec<Named>, AsanaError> {
        let gid = checked_gid(project_gid)?;
        self.get_all(
            &format!("/projects/{gid}/sections"),
            &[("opt_fields", "name")],
            usize::MAX,
        )
        .await
    }

    /// `GET /projects/{gid}/tasks`: every top-level task of the project, completed ones too
    /// (no `completed_since`), stopping after `max`.
    pub async fn project_tasks(
        &self,
        project_gid: &str,
        max: usize,
    ) -> Result<Vec<AsanaTask>, AsanaError> {
        let gid = checked_gid(project_gid)?;
        self.get_all(
            &format!("/projects/{gid}/tasks"),
            &[("opt_fields", TASK_FIELDS)],
            max,
        )
        .await
    }

    /// `GET /tasks/{gid}/subtasks` (one level).
    pub async fn subtasks(&self, task_gid: &str, max: usize) -> Result<Vec<AsanaTask>, AsanaError> {
        let gid = checked_gid(task_gid)?;
        self.get_all(
            &format!("/tasks/{gid}/subtasks"),
            &[("opt_fields", TASK_FIELDS)],
            max,
        )
        .await
    }

    /// `GET /tasks/{gid}/stories`, oldest first (comments and system stories).
    pub async fn stories(&self, task_gid: &str) -> Result<Vec<AsanaStory>, AsanaError> {
        let gid = checked_gid(task_gid)?;
        self.get_all(
            &format!("/tasks/{gid}/stories"),
            &[("opt_fields", STORY_FIELDS)],
            usize::MAX,
        )
        .await
    }

    /// `GET /attachments?parent=`: the task's attachments without download URLs.
    pub async fn attachments(&self, task_gid: &str) -> Result<Vec<AsanaAttachment>, AsanaError> {
        let gid = checked_gid(task_gid)?;
        self.get_all(
            "/attachments",
            &[("parent", gid), ("opt_fields", ATTACHMENT_FIELDS)],
            usize::MAX,
        )
        .await
    }

    /// `GET /attachments/{gid}` with a fresh `download_url` (valid for about two minutes, so
    /// call it right before [`Self::download_file`]).
    pub async fn attachment(&self, attachment_gid: &str) -> Result<AsanaAttachment, AsanaError> {
        let gid = checked_gid(attachment_gid)?;
        self.get_one(
            &format!("/attachments/{gid}"),
            &[("opt_fields", "name,resource_subtype,size,download_url")],
        )
        .await
    }

    /// `GET /tasks/{gid}/dependencies`: the tasks this task waits for (it is blocked by them).
    pub async fn dependencies(&self, task_gid: &str) -> Result<Vec<Named>, AsanaError> {
        let gid = checked_gid(task_gid)?;
        self.get_all(
            &format!("/tasks/{gid}/dependencies"),
            &[("opt_fields", "name")],
            usize::MAX,
        )
        .await
    }

    /// Downloads an Asana-hosted file. Only allowlisted HTTPS URLs are fetched (redirects
    /// included); the token is never sent. Fails with [`AsanaError::FileTooLarge`] once more
    /// than `max_bytes` arrive.
    pub async fn download_file(
        &self,
        url: &str,
        max_bytes: u64,
    ) -> Result<DownloadedFile, AsanaError> {
        let parsed = Url::parse(url).map_err(|_| AsanaError::FileNotAllowed)?;
        let policy = self.inner.file_policy;
        if !policy.allows(&parsed) {
            return Err(AsanaError::FileNotAllowed);
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
                Err(error) if error.is_redirect() => return Err(AsanaError::FileNotAllowed),
                Err(error) if is_transient(&error) && attempt < max_attempts => {
                    tokio::time::sleep(self.backoff(attempt)).await;
                }
                Err(error) => return Err(http_error(error)),
            }
        };
        if !policy.allows(response.url()) {
            return Err(AsanaError::FileNotAllowed);
        }
        let status = response.status();
        if !status.is_success() {
            return Err(AsanaError::Api {
                status: status.as_u16(),
                message: format!("the file host answered {status}"),
            });
        }
        if response
            .content_length()
            .is_some_and(|length| length > max_bytes)
        {
            return Err(AsanaError::FileTooLarge { limit: max_bytes });
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(http_error)? {
            if bytes.len() as u64 + chunk.len() as u64 > max_bytes {
                return Err(AsanaError::FileTooLarge { limit: max_bytes });
            }
            bytes.extend_from_slice(&chunk);
        }
        let file_name = parsed
            .path_segments()
            .and_then(|mut segments| segments.next_back())
            .map(|segment| {
                percent_encoding::percent_decode_str(segment)
                    .decode_utf8_lossy()
                    .into_owned()
            })
            .unwrap_or_default();
        Ok(DownloadedFile {
            bytes,
            file_name: sanitize_file_name(&file_name),
        })
    }

    async fn get_one<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<T, AsanaError> {
        let envelope: Envelope<T> = self.request(path, query).await?;
        Ok(envelope.data)
    }

    /// Follows `next_page.offset` until it is null, stopping once `max` items arrived.
    async fn get_all<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, &str)],
        max: usize,
    ) -> Result<Vec<T>, AsanaError> {
        let mut items = Vec::new();
        let mut offset: Option<String> = None;
        let mut seen = HashSet::new();
        loop {
            let mut params: Vec<(&str, &str)> = query.to_vec();
            params.push(("limit", "100"));
            if let Some(offset) = offset.as_deref() {
                params.push(("offset", offset));
            }
            let page: ListEnvelope<T> = self.request(path, &params).await?;
            items.extend(page.data);
            if items.len() >= max {
                items.truncate(max);
                break;
            }
            match page.next_page.and_then(|next| next.offset) {
                Some(next) if seen.insert(next.clone()) => offset = Some(next),
                _ => break,
            }
        }
        Ok(items)
    }

    async fn request<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<T, AsanaError> {
        let mut url = Url::parse(&format!("{}{path}", self.inner.base_url))
            .map_err(|error| AsanaError::Config(format!("request URL: {error}")))?;
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
            if status.is_success() {
                let bytes = response.bytes().await.map_err(http_error)?;
                return serde_json::from_slice(&bytes)
                    .map_err(|error| AsanaError::Decode(error.to_string()));
            }
            let retry_after = response
                .headers()
                .get(header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(parse_retry_after);
            let bytes = response.bytes().await.unwrap_or_default();
            let message = serde_json::from_slice::<ErrorBody>(&bytes)
                .ok()
                .and_then(|body| body.errors.into_iter().find_map(|error| error.message))
                .unwrap_or_default();
            match status.as_u16() {
                429 => {
                    let wait = retry_after.unwrap_or_else(|| self.backoff(attempt));
                    if attempt >= max_attempts || wait > config.max_retry_after {
                        return Err(AsanaError::RateLimited { retry_after });
                    }
                    tracing::debug!(wait_ms = wait.as_millis() as u64, "Asana rate limited");
                    self.inner.limiter.pause(wait);
                }
                500 | 502 | 503 | 504 if attempt < max_attempts => {
                    tokio::time::sleep(self.backoff(attempt)).await;
                }
                401 => return Err(AsanaError::Unauthorized { message }),
                402 => return Err(AsanaError::PaymentRequired { message }),
                403 => return Err(AsanaError::Forbidden { message }),
                404 => return Err(AsanaError::NotFound { message }),
                code => {
                    return Err(AsanaError::Api {
                        status: code,
                        message,
                    });
                }
            }
        }
    }

    fn backoff(&self, attempt: u32) -> Duration {
        let config = &self.inner.config;
        backoff(config.initial_backoff, config.max_backoff, attempt)
    }
}

#[derive(Deserialize)]
struct Envelope<T> {
    data: T,
}

#[derive(Deserialize)]
struct ListEnvelope<T> {
    data: Vec<T>,
    #[serde(default)]
    next_page: Option<NextPage>,
}

#[derive(Deserialize)]
struct NextPage {
    #[serde(default)]
    offset: Option<String>,
}

#[derive(Default, Deserialize)]
struct ErrorBody {
    #[serde(default)]
    errors: Vec<ErrorItem>,
}

#[derive(Deserialize)]
struct ErrorItem {
    #[serde(default)]
    message: Option<String>,
}

/// Asana gids are decimal strings.
fn checked_gid(raw: &str) -> Result<&str, AsanaError> {
    if !raw.is_empty() && raw.len() <= 32 && raw.bytes().all(|byte| byte.is_ascii_digit()) {
        Ok(raw)
    } else {
        Err(AsanaError::InvalidGid)
    }
}

fn http_error(error: reqwest::Error) -> AsanaError {
    AsanaError::Http(error.without_url().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlists_only_asana_private_buckets() {
        let allowed = |url: &str| is_allowed_asana_file(&Url::parse(url).unwrap());
        assert!(allowed(
            "https://asana-user-private-us-east-1.s3.amazonaws.com/assets/1/2/a.png?X-Amz-Expires=120"
        ));
        assert!(allowed(
            "https://asana-user-private-eu-west-1.s3.eu-west-1.amazonaws.com/a.pdf"
        ));
        assert!(allowed(
            "https://asana-user-private-us-east-1.s3-us-east-1.amazonaws.com/a"
        ));
        for url in [
            "http://asana-user-private-us-east-1.s3.amazonaws.com/a.png",
            "https://asana-user-private-us-east-1.s3.amazonaws.com:8443/a.png",
            "https://user:pw@asana-user-private-us-east-1.s3.amazonaws.com/a.png",
            "https://evil-bucket.s3.amazonaws.com/a.png",
            "https://s3.amazonaws.com/asana-user-private-us-east-1/a.png",
            "https://asana-user-private-x.evil.amazonaws.com/a.png",
            "https://asana-user-private-x.s3.amazonaws.com.evil.com/a.png",
            "https://app.asana.com/app/asana/-/get_asset?asset_id=1",
            "https://drive.google.com/file/d/1",
        ] {
            assert!(!allowed(url), "{url}");
        }
    }

    #[test]
    fn checks_gids() {
        assert!(checked_gid("1204").is_ok());
        assert!(checked_gid("12/../users").is_err());
        assert!(checked_gid("").is_err());
    }

    #[test]
    fn debug_output_hides_the_token() {
        let client = AsanaClient::new("2/secret", AsanaClientConfig::default()).unwrap();
        assert!(!format!("{client:?}").contains("secret"));
        assert!(matches!(
            AsanaClient::new("bad\ntoken", AsanaClientConfig::default()),
            Err(AsanaError::Config(_))
        ));
    }
}
