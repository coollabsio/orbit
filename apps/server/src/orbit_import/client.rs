//! HTTP client for the public API of another Orbit instance (`/api/public/v1`), used with an
//! export API token. The token goes only to the origin the member typed: the URL must be
//! `https` (or `http` to a loopback address under the development flag), without credentials,
//! and not a local address (an IP literal is checked here, a host name when it resolves through
//! [`PublicResolver`]); redirects are not followed.

use std::net::IpAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use orbit_platform::{PublicResolver, is_public_ip};
use reqwest::header::{self, HeaderMap, HeaderValue};
use reqwest::{StatusCode, Url};
use serde::Deserialize;
use serde_json::json;
use thiserror::Error;
use tokio::io::AsyncWriteExt;

use crate::workspace_export::{ExportMode, WorkspaceExportStatus};

const MAX_URL_BYTES: usize = 2_048;
const MAX_TOKEN_CHARS: usize = 1_024;
/// Largest JSON answer read into memory.
const MAX_JSON_BYTES: usize = 1024 * 1024;

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("the instance URL is invalid")]
    UrlInvalid,
    #[error("the API token is invalid")]
    TokenInvalid,
    /// The token exists but is not an export token (`insufficient_scope`).
    #[error("the API token has no export scope")]
    ScopeMissing,
    /// The token's creator may no longer run this export (`workspace_action_forbidden`).
    #[error("the token's creator may not run this export")]
    Forbidden,
    #[error("the export was not found on the other instance")]
    NotFound,
    #[error("the export is not ready")]
    NotReady,
    #[error("the archive is larger than {limit} bytes")]
    TooLarge { limit: u64 },
    /// Connection, timeout or a 5xx: try again later.
    #[error("the other Orbit instance could not be reached: {0}")]
    Unreachable(String),
    #[error("the other Orbit instance answered {status}: {message}")]
    Api { status: u16, message: String },
    #[error("the answer of the other Orbit instance could not be read: {0}")]
    Decode(String),
}

/// `GET /api/public/v1/me`, the parts the importer uses.
#[derive(Debug, Deserialize)]
pub struct RemoteMe {
    pub workspace: RemoteNamed,
    #[serde(default)]
    pub workspaces: Vec<RemoteGrant>,
    pub user: Option<RemoteUser>,
    pub service_account: Option<RemoteNamed>,
}

#[derive(Debug, Deserialize)]
pub struct RemoteNamed {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct RemoteGrant {
    #[serde(default)]
    pub scopes: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct RemoteUser {
    pub email: String,
    pub display_name: String,
}

impl RemoteMe {
    /// The export mode the token's scope allows; `None` for any other token.
    #[must_use]
    pub fn export_mode(&self) -> Option<ExportMode> {
        let scopes: Vec<&str> = self
            .workspaces
            .iter()
            .flat_map(|grant| grant.scopes.iter().map(String::as_str))
            .collect();
        match scopes.as_slice() {
            ["workspace:export"] => Some(ExportMode::Full),
            ["workspace:export:personal"] => Some(ExportMode::Personal),
            _ => None,
        }
    }
}

/// `POST /exports` and `GET /exports/{id}`, the parts the importer uses.
#[derive(Debug, Deserialize)]
pub struct RemoteExport {
    pub id: String,
    pub status: WorkspaceExportStatus,
    pub mode: ExportMode,
}

#[derive(Deserialize)]
struct ProblemBody {
    #[serde(default)]
    code: String,
    #[serde(default)]
    detail: String,
}

/// The origin of another Orbit instance, checked as [`OrbitClient`] requires. The path, query
/// and fragment of what the member typed are dropped.
pub fn parse_origin(raw: &str, allow_local: bool) -> Result<Url, ClientError> {
    let raw = raw.trim();
    if raw.is_empty() || raw.len() > MAX_URL_BYTES {
        return Err(ClientError::UrlInvalid);
    }
    let url = Url::parse(raw).map_err(|_| ClientError::UrlInvalid)?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(ClientError::UrlInvalid);
    }
    let host = url.host().ok_or(ClientError::UrlInvalid)?;
    let loopback = match host {
        url::Host::Ipv4(ip) => ip.is_loopback(),
        url::Host::Ipv6(ip) => ip.is_loopback(),
        url::Host::Domain(name) => name.eq_ignore_ascii_case("localhost"),
    };
    let public = match host {
        url::Host::Ipv4(ip) => is_public_ip(IpAddr::V4(ip)),
        url::Host::Ipv6(ip) => is_public_ip(IpAddr::V6(ip)),
        url::Host::Domain(name) => !name.eq_ignore_ascii_case("localhost"),
    };
    let allowed = match url.scheme() {
        "https" => public || (allow_local && loopback),
        "http" => allow_local && loopback,
        _ => false,
    };
    if !allowed {
        return Err(ClientError::UrlInvalid);
    }
    let mut origin = url;
    origin.set_path("/");
    origin.set_query(None);
    origin.set_fragment(None);
    Ok(origin)
}

/// Client for one origin and one token. Cheap to clone.
#[derive(Clone)]
pub struct OrbitClient {
    inner: Arc<Inner>,
}

struct Inner {
    http: reqwest::Client,
    origin: Url,
    authorization: HeaderValue,
    allow_local: bool,
}

impl OrbitClient {
    /// `origin` comes from [`parse_origin`]; the token's characters are checked here.
    pub fn new(origin: Url, token: &str, allow_local: bool) -> Result<Self, ClientError> {
        let token = token.trim();
        if token.is_empty()
            || token.chars().count() > MAX_TOKEN_CHARS
            || !token.chars().all(|c| c.is_ascii_graphic())
        {
            return Err(ClientError::TokenInvalid);
        }
        let mut authorization = HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| ClientError::TokenInvalid)?;
        authorization.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert(header::ACCEPT, HeaderValue::from_static("application/json"));
        let mut builder = reqwest::Client::builder()
            .user_agent(concat!("Orbit/", env!("CARGO_PKG_VERSION"), " (import)"))
            .default_headers(headers)
            .redirect(reqwest::redirect::Policy::none())
            // A proxy from the environment would resolve the host itself, past the checks here.
            .no_proxy()
            .connect_timeout(Duration::from_secs(10));
        if !allow_local {
            builder = builder.dns_resolver(Arc::new(PublicResolver));
        }
        let http = builder
            .build()
            .map_err(|error| ClientError::Unreachable(error.to_string()))?;
        Ok(Self {
            inner: Arc::new(Inner {
                http,
                origin,
                authorization,
                allow_local,
            }),
        })
    }

    #[must_use]
    pub fn origin(&self) -> &Url {
        &self.inner.origin
    }

    fn endpoint(&self, path: &str) -> Url {
        let mut url = self.inner.origin.clone();
        url.set_path(&format!("/api/public/v1/{path}"));
        url
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.inner
            .http
            .request(method, self.endpoint(path))
            .header(header::AUTHORIZATION, self.inner.authorization.clone())
    }

    /// Validates the token and names its workspace and creator.
    pub async fn me(&self) -> Result<RemoteMe, ClientError> {
        let response = self
            .request(reqwest::Method::GET, "me")
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .map_err(transport)?;
        self.json(response).await
    }

    /// Starts an export of `mode`, or returns the one already running.
    pub async fn start_export(&self, mode: ExportMode) -> Result<RemoteExport, ClientError> {
        let response = self
            .request(reqwest::Method::POST, "exports")
            .timeout(Duration::from_secs(30))
            .json(&json!({ "mode": mode }))
            .send()
            .await
            .map_err(transport)?;
        self.json(response).await
    }

    pub async fn export(&self, export_id: &str) -> Result<RemoteExport, ClientError> {
        let response = self
            .request(reqwest::Method::GET, &format!("exports/{export_id}"))
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(transport)?;
        self.json(response).await
    }

    /// Streams the ready export's ZIP to `path`; returns its size. More than `max_bytes`
    /// (also announced by `Content-Length`) fails before the file is complete.
    pub async fn download_export(
        &self,
        export_id: &str,
        path: &Path,
        max_bytes: u64,
    ) -> Result<u64, ClientError> {
        let response = self
            .request(
                reqwest::Method::GET,
                &format!("exports/{export_id}/download"),
            )
            .header(header::ACCEPT, HeaderValue::from_static("application/zip"))
            .send()
            .await
            .map_err(transport)?;
        if !response.status().is_success() {
            return Err(self.problem(response).await);
        }
        if response
            .content_length()
            .is_some_and(|length| length > max_bytes)
        {
            return Err(ClientError::TooLarge { limit: max_bytes });
        }
        let io = |error: std::io::Error| ClientError::Unreachable(error.to_string());
        let mut file = tokio::fs::File::create(path).await.map_err(io)?;
        let mut size = 0u64;
        let mut response = response;
        while let Some(chunk) = response.chunk().await.map_err(transport)? {
            size = size.saturating_add(chunk.len() as u64);
            if size > max_bytes {
                return Err(ClientError::TooLarge { limit: max_bytes });
            }
            file.write_all(&chunk).await.map_err(io)?;
        }
        file.flush().await.map_err(io)?;
        Ok(size)
    }

    async fn json<T: for<'de> Deserialize<'de>>(
        &self,
        response: reqwest::Response,
    ) -> Result<T, ClientError> {
        if !response.status().is_success() {
            return Err(self.problem(response).await);
        }
        let bytes = limited_body(response).await?;
        serde_json::from_slice(&bytes).map_err(|error| ClientError::Decode(error.to_string()))
    }

    async fn problem(&self, response: reqwest::Response) -> ClientError {
        let status = response.status();
        let body = limited_body(response)
            .await
            .ok()
            .and_then(|bytes| serde_json::from_slice::<ProblemBody>(&bytes).ok())
            .unwrap_or(ProblemBody {
                code: String::new(),
                detail: String::new(),
            });
        match (status, body.code.as_str()) {
            (StatusCode::UNAUTHORIZED, _) => ClientError::TokenInvalid,
            (StatusCode::FORBIDDEN, "insufficient_scope") => ClientError::ScopeMissing,
            (StatusCode::FORBIDDEN, _) => ClientError::Forbidden,
            (StatusCode::NOT_FOUND, _) => ClientError::NotFound,
            (StatusCode::CONFLICT, "export_not_ready") => ClientError::NotReady,
            (status, _) if status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS => {
                ClientError::Unreachable(format!("HTTP {status}"))
            }
            (status, code) => ClientError::Api {
                status: status.as_u16(),
                message: if body.detail.is_empty() {
                    code.to_owned()
                } else {
                    body.detail.chars().take(300).collect()
                },
            },
        }
    }

    /// Whether this client was built under the development flag.
    #[must_use]
    pub fn allows_local(&self) -> bool {
        self.inner.allow_local
    }
}

async fn limited_body(response: reqwest::Response) -> Result<Vec<u8>, ClientError> {
    let mut response = response;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(transport)? {
        if bytes.len() + chunk.len() > MAX_JSON_BYTES {
            return Err(ClientError::Decode("the answer is too large".to_owned()));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn transport(error: reqwest::Error) -> ClientError {
    ClientError::Unreachable(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_public_https_origins() {
        assert_eq!(
            parse_origin("https://orbit.example.com/settings?x=1#y", false)
                .unwrap()
                .as_str(),
            "https://orbit.example.com/"
        );
        assert!(parse_origin("http://orbit.example.com", false).is_err());
        assert!(parse_origin("https://user:pw@orbit.example.com", false).is_err());
        assert!(parse_origin("https://10.0.0.1", false).is_err());
        assert!(parse_origin("https://127.0.0.1:8080", false).is_err());
        assert!(parse_origin("http://127.0.0.1:8080", false).is_err());
        assert!(parse_origin("https://localhost", false).is_err());
        assert!(parse_origin("ftp://orbit.example.com", true).is_err());
        assert_eq!(
            parse_origin("http://127.0.0.1:8080/", true)
                .unwrap()
                .as_str(),
            "http://127.0.0.1:8080/"
        );
        assert!(parse_origin("http://10.0.0.1:8080/", true).is_err());
    }
}
