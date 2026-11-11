//! GitHub for PR review: where GitHub is, one HTTP client, and its errors. Everything here acts with a person's
//! GitHub App user token (see [`user_tokens`]); GitHub then applies that person's own permissions.

pub mod user_tokens;

use std::time::Duration;

use axum::http::StatusCode;
use orbit_platform::RequestId;
use reqwest::Method;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use url::Url;

use crate::integration_routes::ApiError;

/// The REST API version Orbit is written against.
const API_VERSION: &str = "2026-03-10";

/// Where GitHub is. Production is github.com; tests point both URLs at one local fake.
#[derive(Clone, Debug)]
pub struct GithubEndpoints {
    /// `https://github.com/`: OAuth authorize and token exchange.
    pub web: Url,
    /// `https://api.github.com/`: REST and GraphQL.
    pub api: Url,
}

impl Default for GithubEndpoints {
    fn default() -> Self {
        Self::production()
    }
}

impl GithubEndpoints {
    #[must_use]
    pub fn production() -> Self {
        Self {
            web: Url::parse("https://github.com/").expect("static URL"),
            api: Url::parse("https://api.github.com/").expect("static URL"),
        }
    }

    /// `{base}github/web/` and `{base}github/api/`.
    #[must_use]
    pub fn local(base: &Url) -> Self {
        Self {
            web: base.join("github/web/").expect("valid base URL"),
            api: base.join("github/api/").expect("valid base URL"),
        }
    }
}

/// What went wrong talking to GitHub, by what the caller should do about it.
#[derive(Debug)]
pub enum GithubError {
    /// The token is invalid or revoked (401), or GitHub refused an OAuth code or refresh token.
    Unauthorized,
    /// 404, or 403 without a reason: GitHub hides what the person may not see.
    NotFound,
    Forbidden(String),
    Unprocessable(String),
    RateLimited { retry_after_secs: u64 },
    Conflict(String),
    MethodNotAllowed(String),
    Network,
    Unexpected(u16),
}

impl GithubError {
    pub(crate) fn into_api(self, request_id: Option<&RequestId>) -> ApiError {
        let says = |message: &str| format!("GitHub says: {message}");
        match self {
            Self::NotFound => ApiError::new(
                StatusCode::NOT_FOUND,
                "pull_not_found",
                "Not found",
                "The pull request was not found, or your GitHub account cannot see it.",
                request_id,
            ),
            Self::Forbidden(message) if message.is_empty() => {
                Self::NotFound.into_api(request_id)
            }
            Self::Unauthorized => ApiError::new(
                StatusCode::CONFLICT,
                "github_reconnect",
                "Reconnect GitHub",
                "GitHub no longer accepts your connection. Connect GitHub again.",
                request_id,
            ),
            Self::RateLimited { retry_after_secs } => ApiError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "github_rate_limited",
                "GitHub rate limit",
                &format!("GitHub asks to wait {retry_after_secs} seconds. Try again later."),
                request_id,
            ),
            Self::Forbidden(message) => ApiError::new(
                StatusCode::FORBIDDEN,
                "github_forbidden",
                "GitHub refused",
                &says(&message),
                request_id,
            ),
            Self::Unprocessable(message) | Self::MethodNotAllowed(message) => ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "github_refused",
                "GitHub refused",
                &says(&message),
                request_id,
            ),
            Self::Conflict(message) => ApiError::new(
                StatusCode::CONFLICT,
                "github_conflict",
                "GitHub conflict",
                &says(&message),
                request_id,
            ),
            Self::Network | Self::Unexpected(_) => ApiError::new(
                StatusCode::BAD_GATEWAY,
                "github_unavailable",
                "GitHub unavailable",
                "Orbit could not get an answer from GitHub. Try again.",
                request_id,
            ),
        }
    }
}

/// Tokens from GitHub's OAuth token endpoint. Expiry fields are absent when the app does not expire tokens.
#[derive(Debug, Deserialize)]
pub struct OauthTokens {
    pub access_token: String,
    pub expires_in: Option<i64>,
    pub refresh_token: Option<String>,
    pub refresh_token_expires_in: Option<i64>,
}

#[derive(Deserialize)]
struct OauthReply {
    error: Option<String>,
    access_token: Option<String>,
    expires_in: Option<i64>,
    refresh_token: Option<String>,
    refresh_token_expires_in: Option<i64>,
}

/// One HTTP client for GitHub: no redirects, short timeouts, rustls.
#[derive(Clone)]
pub struct GithubClient {
    http: reqwest::Client,
    endpoints: GithubEndpoints,
}

impl Default for GithubClient {
    fn default() -> Self {
        Self::new(GithubEndpoints::default())
    }
}

impl GithubClient {
    #[must_use]
    pub fn new(endpoints: GithubEndpoints) -> Self {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(20))
            .user_agent("Orbit")
            .build()
            .unwrap_or_default();
        Self { http, endpoints }
    }

    #[must_use]
    pub fn endpoints(&self) -> &GithubEndpoints {
        &self.endpoints
    }

    /// `https://github.com/login/oauth/authorize` with PKCE (`S256`).
    #[must_use]
    pub fn authorize_url(
        &self,
        client_id: &str,
        redirect_uri: &str,
        state: &str,
        code_challenge: &str,
    ) -> Url {
        let mut url = self
            .endpoints
            .web
            .join("login/oauth/authorize")
            .expect("static path");
        url.query_pairs_mut()
            .append_pair("client_id", client_id)
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("state", state)
            .append_pair("code_challenge", code_challenge)
            .append_pair("code_challenge_method", "S256");
        url
    }

    /// The OAuth token endpoint: a code exchange or a refresh. GitHub answers 200 with `error` for a bad code or
    /// refresh token; that is [`GithubError::Unauthorized`].
    pub async fn oauth_token(&self, form: &[(&str, &str)]) -> Result<OauthTokens, GithubError> {
        let url = self
            .endpoints
            .web
            .join("login/oauth/access_token")
            .expect("static path");
        let response = self
            .http
            .post(url)
            .header("Accept", "application/json")
            .form(form)
            .send()
            .await
            .map_err(|_| GithubError::Network)?;
        let response = check(response).await?;
        let reply: OauthReply = response.json().await.map_err(|_| GithubError::Network)?;
        if let Some(error) = reply.error.as_deref() {
            tracing::warn!(error, "GitHub refused an OAuth token request");
            // The app's own client is wrong (secret rotated on GitHub): not the member's fault.
            if error == "incorrect_client_credentials" {
                return Err(GithubError::Forbidden(
                    "GitHub rejected this workspace's GitHub App credentials. Ask an admin to re-create the GitHub App."
                        .to_owned(),
                ));
            }
        }
        let access_token = reply
            .access_token
            .filter(|token| !token.is_empty())
            .ok_or(GithubError::Unauthorized)?;
        Ok(OauthTokens {
            access_token,
            expires_in: reply.expires_in,
            refresh_token: reply.refresh_token.filter(|token| !token.is_empty()),
            refresh_token_expires_in: reply.refresh_token_expires_in,
        })
    }

    /// A REST call relative to the API root (`repos/o/r/pulls/1`). An empty `token` sends no `Authorization`.
    pub async fn rest<T: DeserializeOwned>(
        &self,
        token: &str,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<T, GithubError> {
        let response = self.send(token, method, path, body).await?;
        response.json().await.map_err(|_| GithubError::Network)
    }

    /// A REST call whose answer has no body Orbit needs.
    pub async fn rest_empty(
        &self,
        token: &str,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<(), GithubError> {
        self.send(token, method, path, body).await.map(|_| ())
    }

    /// A GraphQL query. GraphQL `errors` become [`GithubError::NotFound`] (`NOT_FOUND`), [`GithubError::Forbidden`]
    /// (`FORBIDDEN`) or [`GithubError::Unprocessable`] with the first message.
    pub async fn graphql<T: DeserializeOwned>(
        &self,
        token: &str,
        query: &str,
        variables: Value,
    ) -> Result<T, GithubError> {
        #[derive(Deserialize)]
        struct Reply<T> {
            data: Option<T>,
            #[serde(default)]
            errors: Vec<GraphqlError>,
        }
        #[derive(Deserialize)]
        struct GraphqlError {
            #[serde(default, rename = "type")]
            kind: String,
            #[serde(default)]
            message: String,
        }
        let body = serde_json::json!({ "query": query, "variables": variables });
        let response = self.send(token, Method::POST, "graphql", Some(&body)).await?;
        let reply: Reply<T> = response.json().await.map_err(|_| GithubError::Network)?;
        if let Some(error) = reply.errors.into_iter().next() {
            return Err(match error.kind.as_str() {
                "NOT_FOUND" => GithubError::NotFound,
                "FORBIDDEN" => GithubError::Forbidden(error.message),
                "RATE_LIMITED" => GithubError::RateLimited {
                    retry_after_secs: 60,
                },
                _ => GithubError::Unprocessable(error.message),
            });
        }
        reply.data.ok_or(GithubError::Network)
    }

    async fn send(
        &self,
        token: &str,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<reqwest::Response, GithubError> {
        let url = self
            .endpoints
            .api
            .join(path.trim_start_matches('/'))
            .map_err(|_| GithubError::NotFound)?;
        // Paths carry names from users (owner, repo, file): never let one leave the API root.
        if !url.as_str().starts_with(self.endpoints.api.as_str()) {
            return Err(GithubError::NotFound);
        }
        let mut request = self
            .http
            .request(method, url)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", API_VERSION);
        if !token.is_empty() {
            request = request.bearer_auth(token);
        }
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request.send().await.map_err(|_| GithubError::Network)?;
        check(response).await
    }
}

/// Maps a non-2xx answer to a [`GithubError`].
async fn check(response: reqwest::Response) -> Result<reqwest::Response, GithubError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    };
    let retry_after = header("retry-after").and_then(|value| value.parse::<u64>().ok());
    let exhausted = header("x-ratelimit-remaining").as_deref() == Some("0");
    let reset = header("x-ratelimit-reset").and_then(|value| value.parse::<u64>().ok());
    let message = response
        .json::<Value>()
        .await
        .ok()
        .and_then(|body| body.get("message").and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_default();
    Err(match status.as_u16() {
        401 => GithubError::Unauthorized,
        403 | 429 if exhausted || retry_after.is_some() || status.as_u16() == 429 => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_secs());
            GithubError::RateLimited {
                retry_after_secs: retry_after
                    .or_else(|| reset.map(|reset| reset.saturating_sub(now)))
                    .unwrap_or(60),
            }
        }
        403 => GithubError::Forbidden(message),
        404 => GithubError::NotFound,
        405 => GithubError::MethodNotAllowed(message),
        409 => GithubError::Conflict(message),
        422 => GithubError::Unprocessable(message),
        other => GithubError::Unexpected(other),
    })
}
