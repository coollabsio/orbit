//! Connecting a member's GitHub account to the workspace's GitHub App (OAuth web flow with PKCE). The callback has
//! no session: the single-use state row says who started the flow, so the mobile app can use the system browser.

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

/// Where the browser goes after the flow, with `github=connected|failed|taken`.
struct Return {
    client: String,
    return_url: Option<String>,
    origin: String,
}

impl Return {
    fn to(&self, outcome: &str) -> Redirect {
        let url = match (self.client.as_str(), &self.return_url) {
            ("mobile", Some(url)) => {
                let separator = if url.contains('?') { '&' } else { '?' };
                format!("{url}{separator}github={outcome}")
            }
            _ => format!(
                "{}/profile/github?github={outcome}",
                self.origin.trim_end_matches('/')
            ),
        };
        Redirect::to(&url)
    }
}

#[utoipa::path(get, path = "/api/v1/integrations/github/user/callback", params(("code" = Option<String>, Query), ("state" = Option<String>, Query)), responses((status = 303, description = "Back to Orbit with github=connected, failed or taken")))]
pub(crate) async fn github_connection_callback(
    State(state): State<IntegrationState>,
    Query(query): Query<CallbackQuery>,
) -> Redirect {
    let fallback = Return {
        client: "web".to_owned(),
        return_url: None,
        origin: state.public_origin().to_owned(),
    };
    let Some(pending) = take_state(&state, query.state.as_deref()).await else {
        return fallback.to("failed");
    };
    let (workspace, user, verifier, back) = pending;
    let code = query.code.filter(|code| !code.is_empty() && code.len() <= 200);
    let Some(code) = code else { return back.to("failed") };
    match finish(&state, &workspace, &user, &verifier, &code, &back.origin).await {
        Ok(()) => back.to("connected"),
        Err(Outcome::Taken) => back.to("taken"),
        Err(Outcome::Failed) => back.to("failed"),
    }
}

/// Consumes the state row; `None` when it is unknown, expired or already used.
async fn take_state(
    state: &IntegrationState,
    token: Option<&str>,
) -> Option<(Id, Id, String, Return)> {
    let token = token.filter(|token| !token.is_empty() && token.len() <= 200)?;
    let key = state.encryption_key()?;
    let hash: [u8; 32] = Sha256::digest(token.as_bytes()).into();
    let mut tx = state.database().immediate_transaction().await.ok()?;
    let row = sqlx::query(
        "SELECT a.workspace_id, a.user_id, a.code_verifier_encrypted, a.client, a.return_url, a.expires_at, \
         g.callback_origin FROM github_user_authorizations a JOIN github_apps g ON g.workspace_id = a.workspace_id \
         WHERE a.state_hash = $1",
    )
    .bind(hash.to_vec())
    .fetch_optional(&mut *tx)
    .await
    .ok()??;
    sqlx::query("DELETE FROM github_user_authorizations WHERE state_hash = $1")
        .bind(hash.to_vec())
        .execute(&mut *tx)
        .await
        .ok()?;
    tx.commit().await.ok()?;
    if row.get::<i64, _>("expires_at") <= TimestampMillis::now().as_millis() {
        return None;
    }
    let verifier: Vec<u8> = row.get("code_verifier_encrypted");
    let origin: Option<String> = row.get("callback_origin");
    Some((
        row.get::<String, _>("workspace_id").parse().ok()?,
        row.get::<String, _>("user_id").parse().ok()?,
        decrypt_secret(&key, &verifier).ok()?,
        Return {
            client: row.get("client"),
            return_url: row.get("return_url"),
            origin: origin.unwrap_or_else(|| state.public_origin().to_owned()),
        },
    ))
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
) -> Result<(), Outcome> {
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
    Ok(())
}
