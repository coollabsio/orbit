//! Each member's GitHub App user token: stored encrypted, renewed shortly before it expires. A refresh token works
//! once, so two servers that renew at the same time race: the loser finds the winner's new token in the row.

use std::time::Duration;

use axum::http::StatusCode;
use orbit_platform::{Id, RequestId, Row, TimestampMillis};

use super::GithubError;
use crate::integration_routes::{ApiError, IntegrationState};
use crate::secret_box::{decrypt_secret, encrypt_secret};

/// Renew this long before the access token expires.
const RENEW_BEFORE_MS: i64 = 5 * 60 * 1000;
/// How long a refresh loser waits for the winner's row (polls × interval).
const RACE_POLLS: u32 = 15;
const RACE_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// A usable token and the GitHub account it belongs to.
#[derive(Clone, Debug)]
pub struct UserToken {
    pub token: String,
    pub github_user_id: i64,
    pub login: String,
}

#[derive(Debug)]
pub enum TokenError {
    /// The member never connected GitHub (or disconnected).
    NotConnected,
    /// GitHub no longer accepts the connection; the member must connect again.
    Broken,
    /// The workspace has no GitHub App.
    AppMissing,
    /// The workspace's app is from before PR review: it has no OAuth client.
    NeedsRecreate,
    /// GitHub did not answer as it should (outage, rate limit, rejected app credentials).
    Github(GithubError),
    Database,
}

impl TokenError {
    pub(crate) fn into_api(self, request_id: Option<&RequestId>) -> ApiError {
        let (code, title, detail) = match self {
            Self::NotConnected => (
                "github_not_connected",
                "Connect GitHub",
                "Connect your GitHub account to use pull requests in Orbit.",
            ),
            Self::Broken => (
                "github_reconnect",
                "Reconnect GitHub",
                "GitHub no longer accepts your connection. Connect GitHub again.",
            ),
            Self::AppMissing => (
                "github_app_missing",
                "No GitHub App",
                "This workspace has no GitHub App. Ask an admin to add one.",
            ),
            Self::NeedsRecreate => (
                "github_app_recreate",
                "Re-create the GitHub App",
                "This workspace's GitHub App is from an older Orbit version. Ask an admin to re-create it.",
            ),
            Self::Github(error) => return error.into_api(request_id),
            Self::Database => return ApiError::internal(request_id),
        };
        ApiError::new(StatusCode::CONFLICT, code, title, detail, request_id)
    }
}

/// The workspace app's OAuth client, decrypted.
pub(crate) struct AppClient {
    pub(crate) client_id: String,
    pub(crate) client_secret: String,
    /// Where the app's OAuth callback URL lives.
    pub(crate) callback_origin: Option<String>,
}

pub(crate) async fn app_client(
    state: &IntegrationState,
    workspace: &Id,
) -> Result<AppClient, TokenError> {
    let row = sqlx::query(
        "SELECT client_id, client_secret_encrypted, callback_origin FROM github_apps WHERE workspace_id = $1",
    )
    .bind(workspace.to_string())
    .fetch_optional(state.database().pool())
    .await
    .map_err(|_| TokenError::Database)?
    .ok_or(TokenError::AppMissing)?;
    let client_id: Option<String> = row.get("client_id");
    let secret: Option<Vec<u8>> = row.get("client_secret_encrypted");
    let (Some(client_id), Some(secret)) = (client_id, secret) else {
        return Err(TokenError::NeedsRecreate);
    };
    let key = state.encryption_key().ok_or(TokenError::Database)?;
    Ok(AppClient {
        client_id,
        client_secret: decrypt_secret(&key, &secret).map_err(|_| TokenError::NeedsRecreate)?,
        callback_origin: row.get("callback_origin"),
    })
}

struct Stored {
    token: UserToken,
    access_expires_at: Option<i64>,
    refresh: Option<Vec<u8>>,
    refresh_expires_at: Option<i64>,
    broken: bool,
}

impl Stored {
    fn fresh(&self, now: i64) -> bool {
        self.access_expires_at
            .is_none_or(|expires| expires - RENEW_BEFORE_MS > now)
    }
}

async fn load(
    state: &IntegrationState,
    key: &[u8; 32],
    workspace: &Id,
    user: &Id,
) -> Result<Option<Stored>, TokenError> {
    let row = sqlx::query(
        "SELECT github_user_id, login, access_token_encrypted, access_expires_at, refresh_token_encrypted, \
         refresh_expires_at, broken FROM github_user_connections WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(workspace.to_string())
    .bind(user.to_string())
    .fetch_optional(state.database().pool())
    .await
    .map_err(|_| TokenError::Database)?;
    let Some(row) = row else { return Ok(None) };
    let access: Vec<u8> = row.get("access_token_encrypted");
    Ok(Some(Stored {
        token: UserToken {
            token: decrypt_secret(key, &access).map_err(|_| TokenError::Broken)?,
            github_user_id: row.get("github_user_id"),
            login: row.get("login"),
        },
        access_expires_at: row.get("access_expires_at"),
        refresh: row.get("refresh_token_encrypted"),
        refresh_expires_at: row.get("refresh_expires_at"),
        broken: row.get::<i64, _>("broken") != 0,
    }))
}

/// The member's GitHub token, renewed when it expires within five minutes.
pub async fn user_token(
    state: &IntegrationState,
    workspace: &Id,
    user: &Id,
) -> Result<UserToken, TokenError> {
    let client = app_client(state, workspace).await?;
    let key = state.encryption_key().ok_or(TokenError::Database)?;
    let stored = load(state, &key, workspace, user)
        .await?
        .ok_or(TokenError::NotConnected)?;
    if stored.broken {
        return Err(TokenError::Broken);
    }
    if stored.fresh(TimestampMillis::now().as_millis()) {
        return Ok(stored.token);
    }
    // One refresh at a time in this process; another server may still race (handled below).
    let lock = state.refresh_lock(workspace, user);
    let _guard = lock.lock().await;
    let stored = load(state, &key, workspace, user)
        .await?
        .ok_or(TokenError::NotConnected)?;
    if stored.broken {
        return Err(TokenError::Broken);
    }
    let now = TimestampMillis::now().as_millis();
    if stored.fresh(now) {
        return Ok(stored.token);
    }
    let old = stored.refresh.clone();
    let refresh = match (&stored.refresh, stored.refresh_expires_at) {
        (Some(refresh), expires) if expires.is_none_or(|expires| expires > now) => {
            decrypt_secret(&key, refresh).map_err(|_| TokenError::Broken)?
        }
        _ => {
            mark_broken_if(state, workspace, user, old.as_deref()).await?;
            return Err(TokenError::Broken);
        }
    };
    let renewed = state
        .github_client()
        .ok_or(TokenError::Database)?
        .oauth_token(&[
            ("grant_type", "refresh_token"),
            ("client_id", &client.client_id),
            ("client_secret", &client.client_secret),
            ("refresh_token", &refresh),
        ])
        .await;
    match renewed {
        Ok(tokens) => {
            let now = TimestampMillis::now().as_millis();
            let access = encrypt_secret(&key, &tokens.access_token).map_err(|_| TokenError::Database)?;
            let refresh = tokens
                .refresh_token
                .as_deref()
                .map(|token| encrypt_secret(&key, token))
                .transpose()
                .map_err(|_| TokenError::Database)?;
            let mut tx = state
                .database()
                .immediate_transaction()
                .await
                .map_err(|_| TokenError::Database)?;
            let updated = sqlx::query(
                "UPDATE github_user_connections SET access_token_encrypted = $1, access_expires_at = $2, \
                 refresh_token_encrypted = $3, refresh_expires_at = $4, broken = 0, updated_at = $5 \
                 WHERE workspace_id = $6 AND user_id = $7 AND refresh_token_encrypted IS NOT DISTINCT FROM $8",
            )
            .bind(access)
            .bind(tokens.expires_in.map(|seconds| now + seconds * 1000))
            .bind(refresh)
            .bind(tokens.refresh_token_expires_in.map(|seconds| now + seconds * 1000))
            .bind(now)
            .bind(workspace.to_string())
            .bind(user.to_string())
            .bind(old.clone())
            .execute(&mut *tx)
            .await
            .map_err(|_| TokenError::Database)?;
            tx.commit().await.map_err(|_| TokenError::Database)?;
            if updated.rows_affected() == 1 {
                return Ok(UserToken {
                    token: tokens.access_token,
                    ..stored.token
                });
            }
            // The row changed under us: someone else renewed or disconnected. Use what is there now.
            winner(state, &key, workspace, user, old.as_deref()).await
        }
        Err(GithubError::Unauthorized) => {
            // GitHub refused our refresh token: another server may have used it a moment ago.
            match winner(state, &key, workspace, user, old.as_deref()).await {
                Ok(token) => Ok(token),
                Err(TokenError::Broken) => {
                    mark_broken_if(state, workspace, user, old.as_deref()).await?;
                    Err(TokenError::Broken)
                }
                Err(error) => Err(error),
            }
        }
        // GitHub unreachable or refusing the app: keep the connection; the caller fails this once.
        Err(error) => Err(TokenError::Github(error)),
    }
}

/// Waits briefly for another server's renewal to appear (a changed refresh token) and returns its token.
async fn winner(
    state: &IntegrationState,
    key: &[u8; 32],
    workspace: &Id,
    user: &Id,
    old: Option<&[u8]>,
) -> Result<UserToken, TokenError> {
    for _ in 0..RACE_POLLS {
        let Some(stored) = load(state, key, workspace, user).await? else {
            return Err(TokenError::NotConnected);
        };
        if stored.refresh.as_deref() != old && !stored.broken {
            return Ok(stored.token);
        }
        tokio::time::sleep(RACE_POLL_INTERVAL).await;
    }
    Err(TokenError::Broken)
}

async fn mark_broken_if(
    state: &IntegrationState,
    workspace: &Id,
    user: &Id,
    old: Option<&[u8]>,
) -> Result<(), TokenError> {
    let mut tx = state
        .database()
        .immediate_transaction()
        .await
        .map_err(|_| TokenError::Database)?;
    sqlx::query(
        "UPDATE github_user_connections SET broken = 1, updated_at = $1 WHERE workspace_id = $2 AND user_id = $3 \
         AND refresh_token_encrypted IS NOT DISTINCT FROM $4",
    )
    .bind(TimestampMillis::now().as_millis())
    .bind(workspace.to_string())
    .bind(user.to_string())
    .bind(old.map(<[u8]>::to_vec))
    .execute(&mut *tx)
    .await
    .map_err(|_| TokenError::Database)?;
    tx.commit().await.map_err(|_| TokenError::Database)
}

/// Marks the connection broken after GitHub answered 401 to `token`. Does nothing when the connection holds another
/// token by now (renewed meanwhile): GitHub revokes the old access token on renewal.
pub async fn mark_broken(
    state: &IntegrationState,
    workspace: &Id,
    user: &Id,
    token: &str,
) -> Result<(), TokenError> {
    let key = state.encryption_key().ok_or(TokenError::Database)?;
    let mut tx = state
        .database()
        .immediate_transaction()
        .await
        .map_err(|_| TokenError::Database)?;
    let stored: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT access_token_encrypted FROM github_user_connections WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(workspace.to_string())
    .bind(user.to_string())
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| TokenError::Database)?;
    let Some(stored) = stored else { return Ok(()) };
    if decrypt_secret(&key, &stored).ok().as_deref() != Some(token) {
        return Ok(());
    }
    sqlx::query(
        "UPDATE github_user_connections SET broken = 1, updated_at = $1 WHERE workspace_id = $2 AND user_id = $3 \
         AND access_token_encrypted = $4",
    )
    .bind(TimestampMillis::now().as_millis())
    .bind(workspace.to_string())
    .bind(user.to_string())
    .bind(stored)
    .execute(&mut *tx)
    .await
    .map_err(|_| TokenError::Database)?;
    tx.commit().await.map_err(|_| TokenError::Database)
}
