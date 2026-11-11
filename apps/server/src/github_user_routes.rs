//! Connecting a member's GitHub account to the workspace's GitHub App (OAuth web flow with PKCE). GitHub's callback
//! has no session (the mobile app uses the system browser), so it only keeps the code; the member who started the
//! flow finishes it with their own session.

use axum::Json;
use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Redirect;
use orbit_platform::{Id, RequestId, Row, TimestampMillis, generate_opaque_token};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use utoipa::ToSchema;

use crate::github::user_tokens::{TokenError, app_client};
use crate::integration_routes::{ApiError, ApiJson, IntegrationState, github_workspace_access};
use crate::secret_box::{decrypt_secret, encrypt_secret};
use crate::sign_in_providers::{pkce_challenge, pkce_verifier};

const CALLBACK_PATH: &str = "/api/v1/integrations/github/user/callback";
const STATE_TTL_MS: i64 = 10 * 60 * 1000;

#[derive(Serialize, ToSchema)]
#[schema(as = GithubConnection)]
pub struct GithubConnection {
    /// `connected`, `disconnected`, `broken` (connect again), `unavailable` (no app) or `needs_recreate`.
    status: &'static str,
    login: Option<String>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = GithubConnectStart)]
pub struct GithubConnectStart {
    /// `web` or `mobile`.
    client: String,
    /// Mobile only: where the browser returns (the app's sign-in redirect URI).
    return_url: Option<String>,
}

#[derive(Serialize, ToSchema)]
#[schema(as = GithubConnectUrl)]
pub struct GithubConnectUrl {
    authorize_url: String,
}

#[derive(Deserialize)]
pub(crate) struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
}

pub(crate) fn callback_url(origin: &str) -> String {
    format!("{}{CALLBACK_PATH}", origin.trim_end_matches('/'))
}

fn request_id(id: Option<&Extension<RequestId>>) -> Option<&RequestId> {
    id.map(|Extension(id)| id)
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/github/me", params(("workspace_id" = String, Path)), responses((status = 200, body = GithubConnection)))]
pub(crate) async fn github_connection(
    State(state): State<IntegrationState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request: Option<Extension<RequestId>>,
) -> Result<Json<GithubConnection>, ApiError> {
    let id = request_id(request.as_ref());
    let (workspace_id, _) = github_workspace_access(&state, &headers, &workspace, id).await?;
    let user = state.session_user(&headers, id).await?;
    match app_client(&state, &workspace_id).await {
        Ok(_) => {}
        Err(TokenError::AppMissing) => {
            return Ok(Json(GithubConnection { status: "unavailable", login: None }));
        }
        Err(TokenError::NeedsRecreate) => {
            return Ok(Json(GithubConnection { status: "needs_recreate", login: None }));
        }
        Err(error) => return Err(error.into_api(id)),
    }
    let row = sqlx::query(
        "SELECT login, broken FROM github_user_connections WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(&workspace)
    .bind(user.to_string())
    .fetch_optional(state.database().pool())
    .await
    .map_err(|_| ApiError::internal(id))?;
    Ok(Json(match row {
        None => GithubConnection { status: "disconnected", login: None },
        Some(row) => GithubConnection {
            status: if row.get::<i64, _>("broken") == 0 { "connected" } else { "broken" },
            login: Some(row.get("login")),
        },
    }))
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/github/me", params(("workspace_id" = String, Path)), request_body = GithubConnectStart, responses((status = 200, body = GithubConnectUrl)))]
pub(crate) async fn start_github_connection(
    State(state): State<IntegrationState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<GithubConnectStart>,
) -> Result<Json<GithubConnectUrl>, ApiError> {
    let id = request_id(request.as_ref());
    let (workspace_id, _) = github_workspace_access(&state, &headers, &workspace, id).await?;
    let user = state.session_user(&headers, id).await?;
    let key = state.app_key(id)?;
    let return_url = match (body.client.as_str(), body.return_url) {
        ("web", None) => None,
        ("mobile", Some(url)) if state.native_redirect_allowed(&url) => Some(url),
        _ => {
            return Err(ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid_redirect",
                "Invalid redirect",
                "This client or return URL is not allowed.",
                id,
            ));
        }
    };
    let client = app_client(&state, &workspace_id)
        .await
        .map_err(|error| error.into_api(id))?;
    let token = generate_opaque_token();
    let hash: [u8; 32] = Sha256::digest(token.as_bytes()).into();
    let verifier = pkce_verifier();
    let verifier_encrypted = encrypt_secret(&key, &verifier).map_err(|_| ApiError::internal(id))?;
    let now = TimestampMillis::now().as_millis();
    let mut tx = state
        .database()
        .immediate_transaction()
        .await
        .map_err(|_| ApiError::internal(id))?;
    sqlx::query("DELETE FROM github_user_authorizations WHERE expires_at <= $1")
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(|_| ApiError::internal(id))?;
    sqlx::query("INSERT INTO github_user_authorizations (state_hash, workspace_id, user_id, code_verifier_encrypted, client, return_url, expires_at) VALUES ($1, $2, $3, $4, $5, $6, $7)")
        .bind(hash.to_vec())
        .bind(&workspace)
        .bind(user.to_string())
        .bind(verifier_encrypted)
        .bind(&body.client)
        .bind(return_url)
        .bind(now + STATE_TTL_MS)
        .execute(&mut *tx)
        .await
        .map_err(|_| ApiError::internal(id))?;
    tx.commit().await.map_err(|_| ApiError::internal(id))?;
    let origin = client
        .callback_origin
        .unwrap_or_else(|| state.public_origin().to_owned());
    let url = state.github_client().ok_or_else(|| ApiError::internal(id))?.authorize_url(
        &client.client_id,
        &callback_url(&origin),
        &token,
        &pkce_challenge(&verifier),
    );
    Ok(Json(GithubConnectUrl { authorize_url: url.to_string() }))
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/github/me", params(("workspace_id" = String, Path)), responses((status = 204, description = "Disconnected")))]
pub(crate) async fn delete_github_connection(
    State(state): State<IntegrationState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request: Option<Extension<RequestId>>,
) -> Result<StatusCode, ApiError> {
    let id = request_id(request.as_ref());
    github_workspace_access(&state, &headers, &workspace, id).await?;
    let user = state.session_user(&headers, id).await?;
    let mut tx = state
        .database()
        .immediate_transaction()
        .await
        .map_err(|_| ApiError::internal(id))?;
    sqlx::query("DELETE FROM github_user_connections WHERE workspace_id = $1 AND user_id = $2")
        .bind(&workspace)
        .bind(user.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|_| ApiError::internal(id))?;
    tx.commit().await.map_err(|_| ApiError::internal(id))?;
    Ok(StatusCode::NO_CONTENT)
}

/// Where the browser goes after GitHub's callback: `github=finish&token=…`, or `github=failed`.
struct Return {
    client: String,
    return_url: Option<String>,
    origin: String,
}

impl Return {
    fn to(&self, query: &str) -> Redirect {
        let url = match (self.client.as_str(), &self.return_url) {
            ("mobile", Some(url)) => {
                let separator = if url.contains('?') { '&' } else { '?' };
                format!("{url}{separator}{query}")
            }
            _ => format!("{}/profile/github?{query}", self.origin.trim_end_matches('/')),
        };
        Redirect::to(&url)
    }
}

/// How long the member has to finish after GitHub sent the browser back.
const FINISH_TTL_MS: i64 = 5 * 60 * 1000;

/// GitHub's callback. It connects nothing: it keeps the code and sends the browser to Orbit with a one-time token.
/// The signed-in member then finishes (`POST /github/me/finish`), so a link someone else started cannot connect the
/// account of whoever opens it.
#[utoipa::path(get, path = "/api/v1/integrations/github/user/callback", params(("code" = Option<String>, Query), ("state" = Option<String>, Query)), responses((status = 303, description = "Back to Orbit with github=finish and a token, or github=failed")))]
pub(crate) async fn github_connection_callback(
    State(state): State<IntegrationState>,
    Query(query): Query<CallbackQuery>,
) -> Redirect {
    let fallback = Return {
        client: "web".to_owned(),
        return_url: None,
        origin: state.public_origin().to_owned(),
    };
    let code = query.code.filter(|code| !code.is_empty() && code.len() <= 200);
    let token = query.state.filter(|token| !token.is_empty() && token.len() <= 200);
    let (Some(code), Some(token)) = (code, token) else {
        return fallback.to("github=failed");
    };
    match keep_code(&state, &token, &code).await {
        Some((back, finish)) => back.to(&format!("github=finish&token={finish}")),
        None => fallback.to("github=failed"),
    }
}

/// Stores GitHub's code on the state row (once) and returns where to go with the new finish token.
async fn keep_code(state: &IntegrationState, token: &str, code: &str) -> Option<(Return, String)> {
    let key = state.encryption_key()?;
    let hash: [u8; 32] = Sha256::digest(token.as_bytes()).into();
    let now = TimestampMillis::now().as_millis();
    let finish = generate_opaque_token();
    let finish_hash: [u8; 32] = Sha256::digest(finish.as_bytes()).into();
    let mut tx = state.database().immediate_transaction().await.ok()?;
    let row = sqlx::query(
        "SELECT a.client, a.return_url, g.callback_origin FROM github_user_authorizations a \
         JOIN github_apps g ON g.workspace_id = a.workspace_id \
         WHERE a.state_hash = $1 AND a.code_encrypted IS NULL AND a.expires_at > $2",
    )
    .bind(hash.to_vec())
    .bind(now)
    .fetch_optional(&mut *tx)
    .await
    .ok()??;
    let updated = sqlx::query(
        "UPDATE github_user_authorizations SET code_encrypted = $1, finish_hash = $2, expires_at = $3 \
         WHERE state_hash = $4 AND code_encrypted IS NULL",
    )
    .bind(encrypt_secret(&key, code).ok()?)
    .bind(finish_hash.to_vec())
    .bind(now + FINISH_TTL_MS)
    .bind(hash.to_vec())
    .execute(&mut *tx)
    .await
    .ok()?;
    if updated.rows_affected() != 1 {
        return None;
    }
    tx.commit().await.ok()?;
    let origin: Option<String> = row.get("callback_origin");
    Some((
        Return {
            client: row.get("client"),
            return_url: row.get("return_url"),
            origin: origin.unwrap_or_else(|| state.public_origin().to_owned()),
        },
        finish,
    ))
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = GithubConnectFinish)]
pub struct GithubConnectFinish {
    /// The `token` GitHub's callback put in the return URL.
    token: String,
}

/// Finishes a connect flow as the signed-in member. Problems: 403 `github_connect_mismatch` (another member started
/// this flow), 409 `github_account_taken`, 422 `github_connect_failed` (unknown, used or expired token; GitHub refused).
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/github/me/finish", params(("workspace_id" = String, Path)), request_body = GithubConnectFinish, responses((status = 200, body = GithubConnection)))]
pub(crate) async fn finish_github_connection(
    State(state): State<IntegrationState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<GithubConnectFinish>,
) -> Result<Json<GithubConnection>, ApiError> {
    let id = request_id(request.as_ref());
    let (workspace_id, _) = github_workspace_access(&state, &headers, &workspace, id).await?;
    let user = state.session_user(&headers, id).await?;
    let key = state.app_key(id)?;
    let failed = || {
        ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "github_connect_failed",
            "GitHub not connected",
            "Orbit could not connect GitHub. Start again.",
            id,
        )
    };
    if body.token.is_empty() || body.token.len() > 200 {
        return Err(failed());
    }
    let hash: [u8; 32] = Sha256::digest(body.token.as_bytes()).into();
    // Single use: the row goes whether this finish works or not.
    let mut tx = state
        .database()
        .immediate_transaction()
        .await
        .map_err(|_| ApiError::internal(id))?;
    let row = sqlx::query(
        "SELECT a.user_id, a.code_encrypted, a.code_verifier_encrypted, a.expires_at, g.callback_origin \
         FROM github_user_authorizations a JOIN github_apps g ON g.workspace_id = a.workspace_id \
         WHERE a.finish_hash = $1 AND a.workspace_id = $2",
    )
    .bind(hash.to_vec())
    .bind(&workspace)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| ApiError::internal(id))?;
    if row.is_some() {
        sqlx::query("DELETE FROM github_user_authorizations WHERE finish_hash = $1")
            .bind(hash.to_vec())
            .execute(&mut *tx)
            .await
            .map_err(|_| ApiError::internal(id))?;
    }
    tx.commit().await.map_err(|_| ApiError::internal(id))?;
    let row = row.ok_or_else(failed)?;
    if row.get::<i64, _>("expires_at") <= TimestampMillis::now().as_millis() {
        return Err(failed());
    }
    if row.get::<String, _>("user_id") != user.to_string() {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "github_connect_mismatch",
            "Not your connection",
            "Another member started this GitHub connection. Start your own from Account settings.",
            id,
        ));
    }
    let code: Option<Vec<u8>> = row.get("code_encrypted");
    let verifier: Vec<u8> = row.get("code_verifier_encrypted");
    let code = code
        .and_then(|code| decrypt_secret(&key, &code).ok())
        .ok_or_else(failed)?;
    let verifier = decrypt_secret(&key, &verifier).map_err(|_| failed())?;
    let origin: Option<String> = row.get("callback_origin");
    let origin = origin.unwrap_or_else(|| state.public_origin().to_owned());
    match finish(&state, &workspace_id, &user, &verifier, &code, &origin).await {
        Ok(login) => Ok(Json(GithubConnection { status: "connected", login: Some(login) })),
        Err(Outcome::Taken) => Err(ApiError::new(
            StatusCode::CONFLICT,
            "github_account_taken",
            "GitHub account in use",
            "This GitHub account is already connected to another Orbit user.",
            id,
        )),
        Err(Outcome::Failed) => Err(failed()),
    }
}

enum Outcome {
    Taken,
    Failed,
}

#[derive(Deserialize)]
struct GithubUser {
    id: i64,
    login: String,
}

async fn finish(
    state: &IntegrationState,
    workspace: &Id,
    user: &Id,
    verifier: &str,
    code: &str,
    origin: &str,
) -> Result<String, Outcome> {
    let key = state.encryption_key().ok_or(Outcome::Failed)?;
    let client = app_client(state, workspace).await.map_err(|_| Outcome::Failed)?;
    let github = state.github_client().ok_or(Outcome::Failed)?;
    let redirect = callback_url(origin);
    let tokens = github
        .oauth_token(&[
            ("client_id", &client.client_id),
            ("client_secret", &client.client_secret),
            ("code", code),
            ("redirect_uri", &redirect),
            ("code_verifier", verifier),
        ])
        .await
        .map_err(|_| Outcome::Failed)?;
    let profile: GithubUser = github
        .rest(&tokens.access_token, reqwest::Method::GET, "user", None)
        .await
        .map_err(|_| Outcome::Failed)?;
    let now = TimestampMillis::now().as_millis();
    let access = encrypt_secret(&key, &tokens.access_token).map_err(|_| Outcome::Failed)?;
    let refresh = tokens
        .refresh_token
        .as_deref()
        .map(|token| encrypt_secret(&key, token))
        .transpose()
        .map_err(|_| Outcome::Failed)?;
    let mut tx = state
        .database()
        .immediate_transaction()
        .await
        .map_err(|_| Outcome::Failed)?;
    let member: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM memberships WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(workspace.to_string())
    .bind(user.to_string())
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| Outcome::Failed)?;
    if member.is_none() {
        return Err(Outcome::Failed);
    }
    let taken: Option<String> = sqlx::query_scalar(
        "SELECT user_id FROM github_user_connections WHERE workspace_id = $1 AND github_user_id = $2 AND user_id <> $3",
    )
    .bind(workspace.to_string())
    .bind(profile.id)
    .bind(user.to_string())
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| Outcome::Failed)?;
    if taken.is_some() {
        return Err(Outcome::Taken);
    }
    sqlx::query(
        "INSERT INTO github_user_connections (workspace_id, user_id, github_user_id, login, access_token_encrypted, \
         access_expires_at, refresh_token_encrypted, refresh_expires_at, broken, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 0, $9, $10) \
         ON CONFLICT (workspace_id, user_id) DO UPDATE SET github_user_id = excluded.github_user_id, \
         login = excluded.login, access_token_encrypted = excluded.access_token_encrypted, \
         access_expires_at = excluded.access_expires_at, refresh_token_encrypted = excluded.refresh_token_encrypted, \
         refresh_expires_at = excluded.refresh_expires_at, broken = 0, updated_at = excluded.updated_at",
    )
    .bind(workspace.to_string())
    .bind(user.to_string())
    .bind(profile.id)
    .bind(&profile.login)
    .bind(access)
    .bind(tokens.expires_in.map(|seconds| now + seconds * 1000))
    .bind(refresh)
    .bind(tokens.refresh_token_expires_in.map(|seconds| now + seconds * 1000))
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await
    .map_err(|_| Outcome::Failed)?;
    tx.commit().await.map_err(|_| Outcome::Failed)?;
    Ok(profile.login)
}
