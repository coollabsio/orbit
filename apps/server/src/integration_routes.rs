use std::sync::Arc;

use axum::body::to_bytes;
use axum::extract::{Extension, FromRequest, Path, Query, Request, State};
use axum::http::header::{AUTHORIZATION, COOKIE, ORIGIN};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use hmac::{Hmac, Mac};
use orbit_platform::{Id, Problem, RequestId, TimestampMillis, generate_opaque_token};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::Row;
use sqlx::{Sqlite, Transaction};
use utoipa::ToSchema;

use crate::audit::{self, AuditOutcome};
use crate::auth_routes::CookieMode;
use crate::repositories::api_tokens::{ApiTokenError, ApiTokenRepository, ApiTokenScope};
use crate::repositories::identity::IdentityRepository;
use crate::repositories::tasks::{
    DiscordTask, GithubWorkItem, TaskError, TaskRecord, TaskRepository,
};

const INSTANCE: &str = "/api/v1/integrations/discord/events";
const MAX_EVENT_ID_CHARS: usize = 200;
const MAX_DESCRIPTION_CHARS: usize = 100_000;
const MAX_URL_CHARS: usize = 2_048;

async fn record_github_change(
    tx: &mut Transaction<'_, Sqlite>,
    workspace: &str,
    action: &str,
    request_id: Option<&RequestId>,
) -> Result<(), ApiError> {
    let workspace_id: Id = workspace
        .parse()
        .map_err(|_| ApiError::internal(request_id))?;
    audit::record(
        tx,
        workspace_id,
        None,
        action,
        AuditOutcome::Success,
        "github",
        None,
        request_id.map_or("github-webhook", RequestId::as_str),
        serde_json::json!({}),
        TimestampMillis::now(),
    )
    .await
    .map_err(|_| ApiError::internal(request_id))
}

#[derive(Clone)]
pub struct IntegrationState {
    tokens: Arc<ApiTokenRepository>,
    tasks: Arc<TaskRepository>,
    github_settings: Option<GithubSettings>,
}

#[derive(Clone)]
struct GithubSettings {
    identity: Arc<IdentityRepository>,
    cookie_mode: CookieMode,
    public_origin: String,
    allow_request_origin: bool,
    encryption_key: Option<[u8; 32]>,
    client: reqwest::Client,
}

impl IntegrationState {
    #[must_use]
    pub fn new(tokens: Arc<ApiTokenRepository>, tasks: Arc<TaskRepository>) -> Self {
        Self {
            tokens,
            tasks,
            github_settings: None,
        }
    }

    #[must_use]
    pub fn with_github_settings(
        mut self,
        identity: Arc<IdentityRepository>,
        cookie_mode: CookieMode,
        public_origin: String,
        allow_request_origin: bool,
        encryption_key: Option<[u8; 32]>,
    ) -> Self {
        self.github_settings = Some(GithubSettings {
            identity,
            cookie_mode,
            public_origin,
            allow_request_origin,
            encryption_key,
            client: reqwest::Client::new(),
        });
        self
    }
}

pub fn integration_router(state: IntegrationState) -> Router {
    Router::new()
        .route(INSTANCE, post(create_discord_event))
        .route("/api/v1/integrations/github/webhook", post(github_webhook))
        .route(
            "/api/v1/integrations/github/manifest/callback",
            get(github_manifest_callback),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/projects/{project_id}/github",
            get(github_project_settings)
                .put(save_github_project_connection)
                .delete(delete_github_project_connection),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/github",
            get(github_workspace_settings),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/github/manifest",
            post(start_github_manifest),
        )
        .with_state(state)
}

#[derive(Serialize, ToSchema)]
pub struct GithubRepositoryOption {
    installation_id: i64,
    repository: String,
}

#[derive(Serialize, ToSchema)]
pub struct GithubProjectSettings {
    app_slug: Option<String>,
    install_url: Option<String>,
    repository: Option<String>,
    label: Option<String>,
    repositories: Vec<GithubRepositoryOption>,
    can_manage: bool,
    key_configured: bool,
}

#[derive(Serialize, ToSchema)]
pub struct GithubWorkspaceSettings {
    app_slug: Option<String>,
    install_url: Option<String>,
    repositories: Vec<GithubRepositoryOption>,
    can_manage: bool,
    key_configured: bool,
}

#[derive(Serialize, ToSchema)]
pub struct GithubManifestStart {
    action: String,
    manifest: Value,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GithubManifestBody {
    organization: Option<String>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GithubProjectConnectionBody {
    installation_id: i64,
    repository: String,
    label: String,
}

#[derive(Deserialize)]
pub(crate) struct GithubManifestCallbackQuery {
    code: String,
    state: String,
}

fn github_settings_context<'a>(
    state: &'a IntegrationState,
    request_id: Option<&RequestId>,
) -> Result<&'a GithubSettings, ApiError> {
    state.github_settings.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "github_not_configured",
            "GitHub is unavailable",
            "GitHub settings are not available.",
            request_id,
        )
    })
}

fn app_key(
    settings: &GithubSettings,
    request_id: Option<&RequestId>,
) -> Result<[u8; 32], ApiError> {
    settings.encryption_key.ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "app_key_missing",
            "App key missing",
            "Set the server App key before you register an App.",
            request_id,
        )
    })
}

fn encrypt_github_secret(key: &[u8; 32], value: &str) -> Result<Vec<u8>, ()> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
    let encrypted = cipher.encrypt(&nonce, value.as_bytes()).map_err(|_| ())?;
    let mut result = nonce.to_vec();
    result.extend_from_slice(&encrypted);
    Ok(result)
}

fn decrypt_github_secret(key: &[u8; 32], value: &[u8]) -> Result<String, ()> {
    if value.len() < 28 {
        return Err(());
    }
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&value[..12]), &value[12..])
        .map_err(|_| ())?;
    String::from_utf8(plaintext).map_err(|_| ())
}

async fn github_session_user(
    state: &IntegrationState,
    headers: &HeaderMap,
    request_id: Option<&RequestId>,
) -> Result<Id, ApiError> {
    let settings = github_settings_context(state, request_id)?;
    let token = headers
        .get(COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|cookies| {
            cookies.split(';').map(str::trim).find_map(|cookie| {
                cookie.strip_prefix(&format!("{}=", settings.cookie_mode.session_cookie_name()))
            })
        })
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::UNAUTHORIZED,
                "authentication_required",
                "Authentication required",
                "A valid session is required.",
                request_id,
            )
        })?;
    let session = settings
        .identity
        .authenticate_session(token, TimestampMillis::now())
        .await
        .map_err(|_| {
            ApiError::new(
                StatusCode::UNAUTHORIZED,
                "authentication_required",
                "Authentication required",
                "A valid session is required.",
                request_id,
            )
        })?;
    Ok(session.user.id)
}

async fn github_workspace_access(
    state: &IntegrationState,
    headers: &HeaderMap,
    workspace: &str,
    request_id: Option<&RequestId>,
) -> Result<(Id, bool), ApiError> {
    let user_id = github_session_user(state, headers, request_id).await?;
    let workspace_id: Id = workspace.parse().map_err(|_| {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "github_workspace_not_found",
            "Workspace not found",
            "The workspace was not found.",
            request_id,
        )
    })?;
    let role: Option<String> = sqlx::query_scalar("SELECT memberships.role FROM memberships JOIN workspaces ON workspaces.id = memberships.workspace_id WHERE memberships.workspace_id = ? AND memberships.user_id = ? AND workspaces.deleted_at IS NULL")
        .bind(workspace).bind(user_id.to_string()).fetch_optional(state.tasks.database().pool()).await
        .map_err(|_| ApiError::internal(request_id))?;
    let role = role.ok_or_else(|| {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "github_workspace_not_found",
            "Workspace not found",
            "The workspace was not found.",
            request_id,
        )
    })?;
    Ok((workspace_id, role != "member"))
}

async fn github_project_access(
    state: &IntegrationState,
    headers: &HeaderMap,
    workspace: &str,
    project: &str,
    request_id: Option<&RequestId>,
) -> Result<(Id, Id, bool), ApiError> {
    let user_id = github_session_user(state, headers, request_id).await?;
    let workspace_id: Id = workspace.parse().map_err(|_| {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "github_project_not_found",
            "Project not found",
            "The project was not found.",
            request_id,
        )
    })?;
    let project_id: Id = project.parse().map_err(|_| {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "github_project_not_found",
            "Project not found",
            "The project was not found.",
            request_id,
        )
    })?;
    let role: Option<String> = sqlx::query_scalar("SELECT memberships.role FROM memberships JOIN workspaces ON workspaces.id = memberships.workspace_id JOIN projects ON projects.workspace_id = memberships.workspace_id WHERE memberships.workspace_id = ? AND memberships.user_id = ? AND projects.id = ? AND projects.deleted_at IS NULL AND workspaces.deleted_at IS NULL")
        .bind(workspace).bind(user_id.to_string()).bind(project).fetch_optional(state.tasks.database().pool()).await
        .map_err(|_| ApiError::internal(request_id))?;
    let role = role.ok_or_else(|| {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "github_project_not_found",
            "Project not found",
            "The project was not found.",
            request_id,
        )
    })?;
    Ok((workspace_id, project_id, role != "member"))
}

fn require_github_manager(
    can_manage: bool,
    request_id: Option<&RequestId>,
) -> Result<(), ApiError> {
    if can_manage {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "github_manager_required",
            "Manager required",
            "Only workspace owners and admins can change GitHub settings.",
            request_id,
        ))
    }
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/github", params(("workspace_id" = String, Path)), responses((status = 200, body = GithubWorkspaceSettings)))]
pub(crate) async fn github_workspace_settings(
    State(state): State<IntegrationState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<GithubWorkspaceSettings>, ApiError> {
    let id = request_id.as_ref().map(|Extension(id)| id);
    let (_, can_manage) = github_workspace_access(&state, &headers, &workspace, id).await?;
    let app: Option<String> =
        sqlx::query_scalar("SELECT slug FROM github_apps WHERE workspace_id = ?")
            .bind(&workspace)
            .fetch_optional(state.tasks.database().pool())
            .await
            .map_err(|_| ApiError::internal(id))?;
    Ok(Json(GithubWorkspaceSettings {
        install_url: app
            .as_ref()
            .map(|slug| format!("https://github.com/apps/{slug}/installations/new")),
        app_slug: app,
        repositories: github_repositories(&state, &workspace, id).await?,
        can_manage,
        key_configured: state
            .github_settings
            .as_ref()
            .is_some_and(|settings| settings.encryption_key.is_some()),
    }))
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/github", params(("workspace_id" = String, Path), ("project_id" = String, Path)), responses((status = 200, body = GithubProjectSettings)))]
pub(crate) async fn github_project_settings(
    State(state): State<IntegrationState>,
    Path((workspace, project)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<GithubProjectSettings>, ApiError> {
    let request_id = request_id.as_ref().map(|Extension(id)| id);
    let (_, _, can_manage) =
        github_project_access(&state, &headers, &workspace, &project, request_id).await?;
    let app: Option<String> =
        sqlx::query_scalar("SELECT slug FROM github_apps WHERE workspace_id = ?")
            .bind(&workspace)
            .fetch_optional(state.tasks.database().pool())
            .await
            .map_err(|_| ApiError::internal(request_id))?;
    let connection = sqlx::query("SELECT repository, label FROM github_project_connections WHERE workspace_id = ? AND project_id = ?")
        .bind(&workspace).bind(&project).fetch_optional(state.tasks.database().pool()).await.map_err(|_| ApiError::internal(request_id))?;
    Ok(Json(GithubProjectSettings {
        install_url: app
            .as_ref()
            .map(|slug| format!("https://github.com/apps/{slug}/installations/new")),
        app_slug: app,
        repository: connection.as_ref().map(|row| row.get("repository")),
        label: connection.as_ref().map(|row| row.get("label")),
        repositories: github_repositories(&state, &workspace, request_id).await?,
        can_manage,
        key_configured: state
            .github_settings
            .as_ref()
            .is_some_and(|settings| settings.encryption_key.is_some()),
    }))
}

async fn github_repositories(
    state: &IntegrationState,
    workspace: &str,
    request_id: Option<&RequestId>,
) -> Result<Vec<GithubRepositoryOption>, ApiError> {
    let rows = sqlx::query("SELECT github_installation_repositories.installation_id, github_installation_repositories.repository FROM github_installation_repositories JOIN github_installations ON github_installations.installation_id = github_installation_repositories.installation_id WHERE github_installations.workspace_id = ? ORDER BY repository")
        .bind(workspace).fetch_all(state.tasks.database().pool()).await.map_err(|_| ApiError::internal(request_id))?;
    Ok(rows
        .into_iter()
        .map(|row| GithubRepositoryOption {
            installation_id: row.get("installation_id"),
            repository: row.get("repository"),
        })
        .collect())
}

#[utoipa::path(put, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/github", params(("workspace_id" = String, Path), ("project_id" = String, Path)), request_body = GithubProjectConnectionBody, responses((status = 200, body = GithubProjectSettings)))]
pub(crate) async fn save_github_project_connection(
    State(state): State<IntegrationState>,
    Path((workspace, project)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<GithubProjectConnectionBody>,
) -> Result<Json<GithubProjectSettings>, ApiError> {
    let id = request_id.as_ref().map(|Extension(id)| id);
    let (_, _, can_manage) =
        github_project_access(&state, &headers, &workspace, &project, id).await?;
    require_github_manager(can_manage, id)?;
    let label = body.label.trim();
    if !valid_repository(&body.repository) || label.is_empty() || label.chars().count() > 50 {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_github_connection",
            "Invalid GitHub connection",
            "Enter an installed repository and a label of at most 50 characters.",
            id,
        ));
    }
    let installed: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM github_installation_repositories JOIN github_installations ON github_installations.installation_id = github_installation_repositories.installation_id WHERE github_installations.workspace_id = ? AND github_installation_repositories.installation_id = ? AND github_installation_repositories.repository = ?)")
        .bind(&workspace).bind(body.installation_id).bind(&body.repository).fetch_one(state.tasks.database().pool()).await.map_err(|_| ApiError::internal(id))?;
    if !installed {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "github_repository_not_installed",
            "Repository not installed",
            "Install the GitHub App on this repository first.",
            id,
        ));
    }
    let mut tx = state
        .tasks
        .database()
        .immediate_transaction()
        .await
        .map_err(|_| ApiError::internal(id))?;
    let changed = sqlx::query("INSERT INTO github_project_connections (workspace_id, project_id, installation_id, repository, label) VALUES (?, ?, ?, ?, ?) ON CONFLICT(project_id) DO UPDATE SET installation_id = excluded.installation_id, repository = excluded.repository, label = excluded.label WHERE installation_id != excluded.installation_id OR repository != excluded.repository OR label != excluded.label")
        .bind(&workspace).bind(&project).bind(body.installation_id).bind(&body.repository).bind(label).execute(&mut *tx).await;
    match changed {
        Ok(result) if result.rows_affected() > 0 => {
            record_github_change(&mut tx, &workspace, "github.connection.updated", id).await?;
        }
        Ok(_) => {}
        Err(sqlx::Error::Database(error)) if error.is_unique_violation() => {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "github_label_conflict",
                "GitHub label conflict",
                "Another project already uses this repository and label.",
                id,
            ));
        }
        Err(_) => return Err(ApiError::internal(id)),
    }
    tx.commit().await.map_err(|_| ApiError::internal(id))?;
    github_project_settings(
        State(state),
        Path((workspace, project)),
        headers,
        request_id,
    )
    .await
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/github", params(("workspace_id" = String, Path), ("project_id" = String, Path)), responses((status = 204)))]
pub(crate) async fn delete_github_project_connection(
    State(state): State<IntegrationState>,
    Path((workspace, project)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<StatusCode, ApiError> {
    let id = request_id.as_ref().map(|Extension(id)| id);
    let (_, _, can_manage) =
        github_project_access(&state, &headers, &workspace, &project, id).await?;
    require_github_manager(can_manage, id)?;
    let mut tx = state
        .tasks
        .database()
        .immediate_transaction()
        .await
        .map_err(|_| ApiError::internal(id))?;
    let removed = sqlx::query(
        "DELETE FROM github_project_connections WHERE workspace_id = ? AND project_id = ?",
    )
    .bind(&workspace)
    .bind(&project)
    .execute(&mut *tx)
    .await
    .map_err(|_| ApiError::internal(id))?;
    if removed.rows_affected() > 0 {
        record_github_change(&mut tx, &workspace, "github.connection.deleted", id).await?;
    }
    tx.commit().await.map_err(|_| ApiError::internal(id))?;
    Ok(StatusCode::NO_CONTENT)
}

fn github_registration_origin(
    settings: &GithubSettings,
    headers: &HeaderMap,
    request_id: Option<&RequestId>,
) -> Result<String, ApiError> {
    let candidate = if settings.allow_request_origin {
        let mut origins = headers.get_all(ORIGIN).iter();
        let origin = origins.next().and_then(|value| value.to_str().ok());
        if origins.next().is_some() {
            None
        } else {
            origin
        }
    } else {
        Some(settings.public_origin.as_str())
    };
    let parsed = candidate
        .filter(|value| value.len() <= MAX_URL_CHARS)
        .and_then(|value| reqwest::Url::parse(value).ok());
    if let Some(url) = parsed.filter(|url| {
        url.scheme() == "https"
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.path() == "/"
            && url.query().is_none()
            && url.fragment().is_none()
    }) {
        return Ok(url.origin().ascii_serialization());
    }
    Err(ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        "github_https_required",
        "HTTPS required",
        "Open Orbit through a public HTTPS URL before you register the App.",
        request_id,
    ))
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/github/manifest", params(("workspace_id" = String, Path)), request_body = GithubManifestBody, responses((status = 200, body = GithubManifestStart)))]
pub(crate) async fn start_github_manifest(
    State(state): State<IntegrationState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<GithubManifestBody>,
) -> Result<Json<GithubManifestStart>, ApiError> {
    let id = request_id.as_ref().map(|Extension(id)| id);
    let settings = github_settings_context(&state, id)?;
    app_key(settings, id)?;
    let (_, can_manage) = github_workspace_access(&state, &headers, &workspace, id).await?;
    require_github_manager(can_manage, id)?;
    let origin = github_registration_origin(settings, &headers, id)?;
    let organization = body.organization.as_deref().unwrap_or("").trim();
    if !organization.is_empty()
        && (organization.len() > 39
            || !organization
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-')
            || organization.starts_with('-')
            || organization.ends_with('-'))
    {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_github_organization",
            "Invalid organization",
            "Enter a valid GitHub organization name.",
            id,
        ));
    }
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM github_apps WHERE workspace_id = ?)")
            .bind(&workspace)
            .fetch_one(state.tasks.database().pool())
            .await
            .map_err(|_| ApiError::internal(id))?;
    if exists {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "github_app_exists",
            "GitHub App exists",
            "This workspace already has a GitHub App.",
            id,
        ));
    }
    let token = generate_opaque_token();
    let hash: [u8; 32] = Sha256::digest(token.as_bytes()).into();
    sqlx::query("DELETE FROM github_app_registrations WHERE expires_at <= ?")
        .bind(TimestampMillis::now().as_millis())
        .execute(state.tasks.database().pool())
        .await
        .map_err(|_| ApiError::internal(id))?;
    sqlx::query("INSERT INTO github_app_registrations (state_hash, workspace_id, expires_at, public_origin) VALUES (?, ?, ?, ?) ON CONFLICT(workspace_id) DO UPDATE SET state_hash = excluded.state_hash, expires_at = excluded.expires_at, public_origin = excluded.public_origin")
         .bind(hash.to_vec()).bind(&workspace).bind(TimestampMillis::now().as_millis() + 3_600_000).bind(&origin)
        .execute(state.tasks.database().pool()).await.map_err(|_| ApiError::internal(id))?;
    Ok(Json(GithubManifestStart {
        action: if organization.is_empty() {
            format!("https://github.com/settings/apps/new?state={token}")
        } else {
            format!(
                "https://github.com/organizations/{organization}/settings/apps/new?state={token}"
            )
        },
        manifest: serde_json::json!({
            "name": "Orbit",
            "url": origin,
            "hook_attributes": { "url": format!("{origin}/api/v1/integrations/github/webhook"), "active": true },
            "redirect_url": format!("{origin}/api/v1/integrations/github/manifest/callback"),
            "setup_url": format!("{origin}/settings/github?workspace={workspace}&github=installed"),
            "public": false,
            "default_permissions": { "issues": "read", "pull_requests": "read" },
            "default_events": ["issues", "pull_request"]
        }),
    }))
}

#[derive(Deserialize)]
struct GithubManifestConversion {
    id: i64,
    slug: String,
    pem: String,
    webhook_secret: String,
}

#[utoipa::path(get, path = "/api/v1/integrations/github/manifest/callback", params(("code" = String, Query), ("state" = String, Query)), responses((status = 303, description = "Return to workspace settings")))]
pub(crate) async fn github_manifest_callback(
    State(state): State<IntegrationState>,
    query: Result<Query<GithubManifestCallbackQuery>, axum::extract::rejection::QueryRejection>,
    request_id: Option<Extension<RequestId>>,
) -> Result<Redirect, ApiError> {
    let id = request_id.as_ref().map(|Extension(id)| id);
    let Query(query) = query.map_err(|_| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_github_registration",
            "Invalid registration",
            "The GitHub registration callback is invalid.",
            id,
        )
    })?;
    let settings = github_settings_context(&state, id)?;
    let key = app_key(settings, id)?;
    if query.state.len() > 200 || query.code.len() > 200 || query.code.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_github_registration",
            "Invalid registration",
            "The GitHub registration callback is invalid.",
            id,
        ));
    }
    let hash: [u8; 32] = Sha256::digest(query.state.as_bytes()).into();
    let pending = sqlx::query(
        "SELECT workspace_id, public_origin FROM github_app_registrations WHERE state_hash = ? AND expires_at > ?",
    )
    .bind(hash.to_vec())
    .bind(TimestampMillis::now().as_millis())
    .fetch_optional(state.tasks.database().pool())
    .await
    .map_err(|_| ApiError::internal(id))?
    .ok_or_else(|| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_github_registration",
            "Invalid registration",
            "The GitHub registration is invalid or expired.",
            id,
        )
    })?;
    let workspace: String = pending.get("workspace_id");
    let registered_origin: String = pending.get("public_origin");
    let response = settings
        .client
        .post(format!(
            "https://api.github.com/app-manifests/{}/conversions",
            query.code
        ))
        .header("User-Agent", "Orbit")
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await
        .map_err(|_| {
            ApiError::new(
                StatusCode::BAD_GATEWAY,
                "github_registration_failed",
                "GitHub registration failed",
                "Orbit could not contact GitHub.",
                id,
            )
        })?;
    if !response.status().is_success() {
        return Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "github_registration_failed",
            "GitHub registration failed",
            "GitHub did not accept the registration code.",
            id,
        ));
    }
    let app: GithubManifestConversion = response.json().await.map_err(|_| {
        ApiError::new(
            StatusCode::BAD_GATEWAY,
            "github_registration_failed",
            "GitHub registration failed",
            "GitHub returned an invalid App registration.",
            id,
        )
    })?;
    store_github_app(&state, &workspace, &hash, &key, &app, id).await?;
    let return_origin = if registered_origin.is_empty() {
        settings.public_origin.as_str()
    } else {
        registered_origin.as_str()
    };
    Ok(Redirect::to(&format!(
        "{}/settings/github?workspace={workspace}&github=registered",
        return_origin.trim_end_matches('/')
    )))
}

async fn store_github_app(
    state: &IntegrationState,
    workspace: &str,
    hash: &[u8; 32],
    key: &[u8; 32],
    app: &GithubManifestConversion,
    id: Option<&RequestId>,
) -> Result<(), ApiError> {
    if app.id <= 0 || app.slug.is_empty() || app.pem.is_empty() || app.webhook_secret.is_empty() {
        return Err(ApiError::internal(id));
    }
    let pem = encrypt_github_secret(key, &app.pem).map_err(|_| ApiError::internal(id))?;
    let secret =
        encrypt_github_secret(key, &app.webhook_secret).map_err(|_| ApiError::internal(id))?;
    let mut tx = state
        .tasks
        .database()
        .immediate_transaction()
        .await
        .map_err(|_| ApiError::internal(id))?;
    let consumed = sqlx::query("DELETE FROM github_app_registrations WHERE state_hash = ? AND workspace_id = ? AND expires_at > ?")
        .bind(hash.to_vec()).bind(workspace).bind(TimestampMillis::now().as_millis())
        .execute(&mut *tx).await.map_err(|_| ApiError::internal(id))?;
    if consumed.rows_affected() != 1 {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_github_registration",
            "Invalid registration",
            "The GitHub registration is invalid or expired.",
            id,
        ));
    }
    let inserted = sqlx::query("INSERT INTO github_apps (workspace_id, app_id, slug, private_key_encrypted, webhook_secret_encrypted, created_at) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(workspace).bind(app.id).bind(&app.slug).bind(pem).bind(secret).bind(TimestampMillis::now().as_millis()).execute(&mut *tx).await;
    match inserted {
        Ok(_) => {}
        Err(sqlx::Error::Database(error)) if error.is_unique_violation() => {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "github_app_exists",
                "GitHub App exists",
                "This workspace already has a GitHub App.",
                id,
            ));
        }
        Err(_) => return Err(ApiError::internal(id)),
    }
    record_github_change(&mut tx, workspace, "github.app.registered", id).await?;
    tx.commit().await.map_err(|_| ApiError::internal(id))?;
    Ok(())
}

#[utoipa::path(
    post,
    path = "/api/v1/integrations/github/webhook",
    request_body = Value,
    params(
        ("X-Hub-Signature-256" = String, Header, description = "GitHub HMAC-SHA256 signature"),
        ("X-GitHub-Event" = String, Header, description = "GitHub event type")
    ),
    responses((status = 200, description = "Webhook processed"), (status = 202, description = "Event ignored"))
)]
pub(crate) async fn github_webhook(
    State(state): State<IntegrationState>,
    request: Request,
) -> Result<StatusCode, ApiError> {
    github_webhook_from_database(&state, request).await
}

async fn github_webhook_from_database(
    state: &IntegrationState,
    request: Request,
) -> Result<StatusCode, ApiError> {
    let request_id = request.extensions().get::<RequestId>().cloned();
    let id = request_id.as_ref();
    if request
        .headers()
        .get("x-github-event")
        .and_then(|value| value.to_str().ok())
        == Some("ping")
    {
        // GitHub sends the first ping before the manifest callback can save the webhook secret.
        return Ok(StatusCode::OK);
    }
    let settings = github_settings_context(state, id)?;
    let key = app_key(settings, id)?;
    let signature = request
        .headers()
        .get("x-hub-signature-256")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let event = request
        .headers()
        .get("x-github-event")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let body = to_bytes(request.into_body(), 1_048_576)
        .await
        .map_err(|_| {
            ApiError::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "github_payload_too_large",
                "GitHub payload too large",
                "The webhook payload is too large.",
                id,
            )
        })?;
    let rows = sqlx::query("SELECT workspace_id, webhook_secret_encrypted FROM github_apps")
        .fetch_all(state.tasks.database().pool())
        .await
        .map_err(|_| ApiError::internal(id))?;
    let mut workspace = None;
    for row in &rows {
        let secret: Vec<u8> = row.get("webhook_secret_encrypted");
        let secret = decrypt_github_secret(&key, &secret).map_err(|_| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "app_key_invalid",
                "App key invalid",
                "Orbit cannot read the saved GitHub credentials.",
                id,
            )
        })?;
        if verify_github_signature(secret.as_bytes(), &signature, &body).is_ok() {
            workspace = Some(row.get::<String, _>("workspace_id"));
            break;
        }
    }
    let workspace = workspace.ok_or_else(|| {
        ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_github_signature",
            "Invalid GitHub signature",
            "The webhook signature is invalid.",
            id,
        )
    })?;
    let payload: Value = serde_json::from_slice(&body).map_err(|_| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_github_payload",
            "Invalid GitHub payload",
            "The webhook payload is invalid.",
            id,
        )
    })?;
    let action = payload.get("action").and_then(Value::as_str).unwrap_or("");
    if matches!(event.as_str(), "installation" | "installation_repositories") {
        update_github_installation(state, &workspace, &payload, &event, action, id).await?;
        return Ok(StatusCode::OK);
    }
    if !matches!(event.as_str(), "issues" | "pull_request") {
        return Ok(StatusCode::ACCEPTED);
    }
    let repository = payload
        .pointer("/repository/full_name")
        .and_then(Value::as_str)
        .unwrap_or("");
    let installation_id = payload
        .pointer("/installation/id")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let connections = sqlx::query("SELECT project_id, label FROM github_project_connections WHERE workspace_id = ? AND repository = ? AND installation_id = ?")
        .bind(&workspace).bind(repository).bind(installation_id).fetch_all(state.tasks.database().pool()).await.map_err(|_| ApiError::internal(id))?;
    if connections.is_empty() {
        return Ok(StatusCode::ACCEPTED);
    }
    let is_issue = event == "issues";
    if !matches!(
        action,
        "opened" | "edited" | "labeled" | "unlabeled" | "closed" | "reopened" | "synchronize"
    ) {
        return Ok(StatusCode::ACCEPTED);
    }
    let kind = if is_issue { "issue" } else { "pull_request" };
    let item = payload.get(kind).ok_or_else(|| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_github_payload",
            "Invalid GitHub payload",
            "The GitHub issue or pull request is missing.",
            id,
        )
    })?;
    if is_issue && item.get("pull_request").is_some() {
        return Ok(StatusCode::ACCEPTED);
    }
    let labels = item
        .get("labels")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_github_payload",
                "Invalid GitHub payload",
                "The GitHub labels are missing.",
                id,
            )
        })?;
    let number = item
        .get("number")
        .or_else(|| payload.get("number"))
        .and_then(Value::as_i64)
        .filter(|number| *number > 0)
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_github_payload",
                "Invalid GitHub payload",
                "The GitHub issue or pull request number is invalid.",
                id,
            )
        })?;
    let selected: Vec<_> = connections
        .iter()
        .filter(|row| {
            labels.iter().any(|label| {
                label.get("name").and_then(Value::as_str)
                    == Some(row.get::<String, _>("label").as_str())
            })
        })
        .collect();
    let connection = match selected.as_slice() {
        [] => {
            let mut tx = state
                .tasks
                .database()
                .immediate_transaction()
                .await
                .map_err(|_| ApiError::internal(id))?;
            let paused = sqlx::query("UPDATE github_issue_links SET sync_paused = 1 WHERE workspace_id = ? AND repository = ? AND issue_number = ? AND kind = ? AND sync_paused = 0")
                .bind(&workspace).bind(repository).bind(number).bind(kind)
                .execute(&mut *tx).await.map_err(|_| ApiError::internal(id))?;
            if paused.rows_affected() > 0 {
                record_github_change(&mut tx, &workspace, "github.work_item.paused", id).await?;
            }
            tx.commit().await.map_err(|_| ApiError::internal(id))?;
            return Ok(StatusCode::ACCEPTED);
        }
        [connection] => connection,
        _ => {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "github_labels_ambiguous",
                "GitHub labels conflict",
                "This GitHub issue or pull request has labels for more than one Orbit project.",
                id,
            ));
        }
    };
    let project_id: Id = connection
        .get::<String, _>("project_id")
        .parse()
        .map_err(|_| ApiError::internal(id))?;
    let workspace_id: Id = workspace.parse().map_err(|_| ApiError::internal(id))?;
    let actor: String = sqlx::query_scalar(
        "SELECT user_id FROM memberships WHERE workspace_id = ? AND role = 'owner' LIMIT 1",
    )
    .bind(&workspace)
    .fetch_one(state.tasks.database().pool())
    .await
    .map_err(|_| ApiError::internal(id))?;
    let actor_id: Id = actor.parse().map_err(|_| ApiError::internal(id))?;
    let title = item
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let description = item
        .get("body")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    if title.is_empty()
        || title.chars().count() > 500
        || description.chars().count() > MAX_DESCRIPTION_CHARS
    {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_github_issue",
            "Invalid GitHub item",
            "The title or body is too long or empty.",
            id,
        ));
    }
    let state_value = if !is_issue && item.get("merged").and_then(Value::as_bool) == Some(true) {
        "merged"
    } else if item.get("state").and_then(Value::as_str) == Some("closed") {
        "closed"
    } else {
        "open"
    };
    state
        .tasks
        .sync_github_work_item(
            workspace_id,
            actor_id,
            GithubWorkItem {
                repository: repository.to_owned(),
                number,
                project_id,
                title: title.to_owned(),
                description,
                kind,
                state: state_value,
                state_changed: matches!(action, "closed" | "reopened"),
            },
            id.map_or("github-webhook", RequestId::as_str),
            TimestampMillis::now(),
        )
        .await
        .map_err(|cause| match cause {
            TaskError::IntegrationConflict => ApiError::new(
                StatusCode::CONFLICT,
                "github_project_conflict",
                "GitHub project conflict",
                "This GitHub item is already linked to a task in another project.",
                id,
            ),
            _ => {
                tracing::warn!(error = ?cause, "GitHub work item sync failed");
                ApiError::internal(id)
            }
        })?;
    Ok(StatusCode::OK)
}

async fn update_github_installation(
    state: &IntegrationState,
    workspace: &str,
    payload: &Value,
    event: &str,
    action: &str,
    request_id: Option<&RequestId>,
) -> Result<(), ApiError> {
    let installation_id = payload
        .pointer("/installation/id")
        .and_then(Value::as_i64)
        .filter(|id| *id > 0)
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_github_payload",
                "Invalid GitHub payload",
                "The installation ID is missing.",
                request_id,
            )
        })?;
    let mut tx = state
        .tasks
        .database()
        .immediate_transaction()
        .await
        .map_err(|_| ApiError::internal(request_id))?;
    let mut changed = false;
    if event == "installation" && action == "deleted" {
        changed |= sqlx::query(
            "DELETE FROM github_installations WHERE workspace_id = ? AND installation_id = ?",
        )
        .bind(workspace)
        .bind(installation_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| ApiError::internal(request_id))?
        .rows_affected()
            > 0;
    } else {
        let account = payload
            .pointer("/installation/account/login")
            .and_then(Value::as_str)
            .unwrap_or("");
        let installed = sqlx::query("INSERT INTO github_installations (installation_id, workspace_id, account_login) VALUES (?, ?, ?) ON CONFLICT(installation_id) DO UPDATE SET account_login = excluded.account_login WHERE workspace_id = excluded.workspace_id AND account_login != excluded.account_login")
            .bind(installation_id).bind(workspace).bind(account).execute(&mut *tx).await.map_err(|_| ApiError::internal(request_id))?;
        if installed.rows_affected() == 0 && !sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM github_installations WHERE installation_id = ? AND workspace_id = ?)")
            .bind(installation_id).bind(workspace).fetch_one(&mut *tx).await.map_err(|_| ApiError::internal(request_id))? {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "github_installation_forbidden",
                "Installation forbidden",
                "This installation belongs to another workspace.",
                request_id,
            ));
        }
        changed |= installed.rows_affected() > 0;
        if event == "installation" && action == "created" {
            changed |= sqlx::query(
                "DELETE FROM github_installation_repositories WHERE installation_id = ?",
            )
            .bind(installation_id)
            .execute(&mut *tx)
            .await
            .map_err(|_| ApiError::internal(request_id))?
            .rows_affected()
                > 0;
        }
        let added = if event == "installation_repositories" {
            "repositories_added"
        } else {
            "repositories"
        };
        if let Some(repositories) = payload.get(added).and_then(Value::as_array) {
            for repository in repositories {
                if let Some(name) = repository
                    .get("full_name")
                    .and_then(Value::as_str)
                    .filter(|name| valid_repository(name))
                {
                    changed |= sqlx::query("INSERT OR IGNORE INTO github_installation_repositories (installation_id, repository) VALUES (?, ?)")
                        .bind(installation_id).bind(name).execute(&mut *tx).await.map_err(|_| ApiError::internal(request_id))?.rows_affected() > 0;
                }
            }
        }
        if let Some(repositories) = payload
            .get("repositories_removed")
            .and_then(Value::as_array)
        {
            for repository in repositories {
                if let Some(name) = repository.get("full_name").and_then(Value::as_str) {
                    changed |= sqlx::query("DELETE FROM github_installation_repositories WHERE installation_id = ? AND repository = ?")
                        .bind(installation_id).bind(name).execute(&mut *tx).await.map_err(|_| ApiError::internal(request_id))?.rows_affected() > 0;
                    changed |= sqlx::query("DELETE FROM github_project_connections WHERE installation_id = ? AND repository = ?")
                        .bind(installation_id).bind(name).execute(&mut *tx).await.map_err(|_| ApiError::internal(request_id))?.rows_affected() > 0;
                }
            }
        }
    }
    if changed {
        record_github_change(
            &mut tx,
            workspace,
            "github.installation.updated",
            request_id,
        )
        .await?;
    }
    tx.commit()
        .await
        .map_err(|_| ApiError::internal(request_id))?;
    Ok(())
}

fn verify_github_signature(secret: &[u8], signature: &str, body: &[u8]) -> Result<(), ()> {
    let hex = signature.strip_prefix("sha256=").ok_or(())?;
    if hex.len() != 64 {
        return Err(());
    }
    let mut bytes = [0u8; 32];
    for (index, chunk) in hex.as_bytes().chunks_exact(2).enumerate() {
        let pair = std::str::from_utf8(chunk).map_err(|_| ())?;
        bytes[index] = u8::from_str_radix(pair, 16).map_err(|_| ())?;
    }
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(secret).map_err(|_| ())?;
    mac.update(body);
    mac.verify_slice(&bytes).map_err(|_| ())
}

fn valid_repository(value: &str) -> bool {
    let mut parts = value.split('/');
    let valid = |part: &str| {
        !part.is_empty()
            && part
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
    };
    matches!((parts.next(), parts.next(), parts.next()), (Some(owner), Some(repo), None) if valid(owner) && valid(repo))
}

pub(crate) struct ApiJson<T>(T);

impl<S, T> FromRequest<S> for ApiJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = ApiError;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        let request_id = request.extensions().get::<RequestId>().cloned();
        Json::<T>::from_request(request, state)
            .await
            .map(|Json(value)| Self(value))
            .map_err(|_| {
                ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "Invalid request",
                    "The request body is not valid for this endpoint.",
                    request_id.as_ref(),
                )
            })
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DiscordEventBody {
    event_id: String,
    message: String,
    message_url: String,
    project_id: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct DiscordEventResponse {
    task: TaskRecord,
    duplicate: bool,
}

#[utoipa::path(
    post,
    path = "/api/v1/integrations/discord/events",
    request_body = DiscordEventBody,
    responses(
        (status = 201, description = "Task created", body = DiscordEventResponse),
        (status = 200, description = "Existing task returned for a repeated event", body = DiscordEventResponse)
    ),
    security(("bearerAuth" = []))
)]
pub(crate) async fn create_discord_event(
    State(state): State<IntegrationState>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<DiscordEventBody>,
) -> Result<Response, ApiError> {
    let token = bearer_token(&headers).ok_or_else(|| {
        ApiError::new(
            StatusCode::UNAUTHORIZED,
            "api_token_required",
            "API token required",
            "Supply an API token with the Bearer authorization scheme.",
            request_id.as_ref().map(|value| &value.0),
        )
    })?;
    let principal = state
        .tokens
        .authenticate(token, ApiTokenScope::Write, TimestampMillis::now())
        .await
        .map_err(|error| token_problem(error, request_id.as_ref().map(|value| &value.0)))?;
    let project_id = match body.project_id.as_deref() {
        Some(value) => value
            .parse()
            .ok()
            .filter(|id| principal.project_ids.contains(id)),
        None if principal.project_ids.len() == 1 => principal.project_ids.first().copied(),
        None => None,
    }
    .ok_or_else(|| validation("project_id", request_id.as_ref().map(|value| &value.0)))?;
    let event_id = body.event_id.trim();
    let message = body.message.as_str();
    let message_url = body.message_url.trim();
    if event_id.is_empty() || event_id.chars().count() > MAX_EVENT_ID_CHARS {
        return Err(validation(
            "event_id",
            request_id.as_ref().map(|value| &value.0),
        ));
    }
    if message.trim().is_empty() {
        return Err(validation(
            "message",
            request_id.as_ref().map(|value| &value.0),
        ));
    }
    if message_url.chars().count() > MAX_URL_CHARS || !valid_discord_url(message_url) {
        return Err(validation(
            "message_url",
            request_id.as_ref().map(|value| &value.0),
        ));
    }
    let description = message.to_owned();
    if description.chars().count() > MAX_DESCRIPTION_CHARS {
        return Err(validation(
            "message",
            request_id.as_ref().map(|value| &value.0),
        ));
    }
    let payload_hash: [u8; 32] = Sha256::new()
        .chain_update(message.as_bytes())
        .chain_update([0])
        .chain_update(message_url.as_bytes())
        .finalize()
        .into();
    let (task, created) = state
        .tasks
        .create_discord_task(
            principal.workspace_id,
            principal.creator_id,
            principal.service_account_id,
            DiscordTask {
                event_id: event_id.to_owned(),
                payload_hash,
                project_id,
                title: discord_title(message),
                description,
                source_url: message_url.to_owned(),
            },
            request_id
                .as_ref()
                .map_or("unknown", |value| value.0.as_str()),
            TimestampMillis::now(),
        )
        .await
        .map_err(|error| task_problem(error, request_id.as_ref().map(|value| &value.0)))?;
    Ok((
        if created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        Json(DiscordEventResponse {
            task,
            duplicate: !created,
        }),
    )
        .into_response())
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(AUTHORIZATION)?.to_str().ok()?;
    let token = value.strip_prefix("Bearer ")?;
    (!token.is_empty() && !token.chars().any(char::is_whitespace)).then_some(token)
}

fn valid_discord_url(value: &str) -> bool {
    value.starts_with("https://discord.com/channels/") && !value.chars().any(char::is_whitespace)
}

fn discord_title(message: &str) -> String {
    let line = message
        .lines()
        .find_map(|line| {
            let collapsed = line.split_whitespace().collect::<Vec<_>>().join(" ");
            (!collapsed.is_empty()).then_some(collapsed)
        })
        .unwrap_or_else(|| "Discord message".to_owned());
    if line.chars().count() <= 40 {
        line
    } else {
        format!("{}...", line.chars().take(37).collect::<String>())
    }
}

fn token_problem(error: ApiTokenError, request_id: Option<&RequestId>) -> ApiError {
    match error {
        ApiTokenError::Forbidden => ApiError::new(
            StatusCode::FORBIDDEN,
            "api_token_scope_forbidden",
            "API token scope forbidden",
            "This API token does not have the write scope.",
            request_id,
        ),
        ApiTokenError::NotFound => ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_api_token",
            "Invalid API token",
            "The API token is invalid or revoked.",
            request_id,
        ),
        ApiTokenError::Unavailable(_) | ApiTokenError::InvalidIdentifier => {
            ApiError::internal(request_id)
        }
    }
}

fn task_problem(error: TaskError, request_id: Option<&RequestId>) -> ApiError {
    match error {
        TaskError::IntegrationConflict => ApiError::new(
            StatusCode::CONFLICT,
            "integration_event_conflict",
            "Integration event conflict",
            "This event ID was already used with different content.",
            request_id,
        ),
        TaskError::NotFound => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "integration_project_unavailable",
            "Integration project unavailable",
            "The token project or its initial status is unavailable.",
            request_id,
        ),
        TaskError::Invalid { field } => validation(field, request_id),
        TaskError::Conflict
        | TaskError::GithubContentReadOnly
        | TaskError::RestoreConflict { .. }
        | TaskError::VersionConflict { .. }
        | TaskError::InvalidCursor
        | TaskError::Unavailable(_) => ApiError::internal(request_id),
    }
}

fn validation(field: &'static str, request_id: Option<&RequestId>) -> ApiError {
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        "validation_failed",
        "Validation failed",
        match field {
            "event_id" => "Event IDs must contain between 1 and 200 characters.",
            "project_id" => "Select a project that the API token can write to.",
            "message_url" => {
                "Message URLs must be Discord channel message URLs of at most 2048 characters."
            }
            _ => {
                "Messages must contain text and the stored description must not exceed 100000 characters."
            }
        },
        request_id,
    )
}

pub(crate) struct ApiError {
    status: StatusCode,
    problem: Box<Problem>,
}

impl ApiError {
    fn new(
        status: StatusCode,
        code: &str,
        title: &str,
        detail: &str,
        request_id: Option<&RequestId>,
    ) -> Self {
        Self {
            status,
            problem: Box::new(Problem {
                type_uri: format!("https://docs.orbit.dev/problems/{code}"),
                title: title.to_owned(),
                status: status.as_u16(),
                code: code.to_owned(),
                detail: detail.to_owned(),
                instance: INSTANCE.to_owned(),
                request_id: request_id
                    .map_or_else(|| RequestId::new().to_string(), ToString::to_string),
                errors: None,
                conflict: None,
            }),
        }
    }

    fn internal(request_id: Option<&RequestId>) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "Internal server error",
            "An unexpected error occurred. Use the request ID when contacting support.",
            request_id,
        )
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            [(axum::http::header::CONTENT_TYPE, "application/problem+json")],
            Json(self.problem),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use orbit_platform::{PasswordService, TestDatabase, TimestampMillis};
    use sha2::{Digest, Sha256};
    use sqlx::Row;

    use crate::repositories::api_tokens::ApiTokenRepository;
    use crate::repositories::identity::{IdentityRepository, SetupRequest};
    use crate::repositories::tasks::TaskRepository;

    use super::{
        GithubManifestConversion, IntegrationState, decrypt_github_secret, discord_title,
        encrypt_github_secret, store_github_app,
    };

    #[test]
    fn github_credentials_are_encrypted_and_detect_tampering() {
        let key = [7u8; 32];
        let encrypted = encrypt_github_secret(&key, "private-key").unwrap();
        assert!(!encrypted.windows(11).any(|part| part == b"private-key"));
        assert_eq!(
            decrypt_github_secret(&key, &encrypted).unwrap(),
            "private-key"
        );
        assert!(decrypt_github_secret(&[8u8; 32], &encrypted).is_err());
        let mut altered = encrypted;
        *altered.last_mut().unwrap() ^= 1;
        assert!(decrypt_github_secret(&key, &altered).is_err());
    }

    #[tokio::test]
    async fn manifest_conversion_credentials_are_saved_encrypted_and_state_is_consumed() {
        let database = TestDatabase::new().await.unwrap();
        let identity = IdentityRepository::new((*database).clone());
        let now = TimestampMillis::now();
        identity
            .store_setup_token(
                "setup",
                TimestampMillis::from_millis(now.as_millis() + 60_000),
            )
            .await
            .unwrap();
        let setup = identity
            .complete_setup(
                SetupRequest {
                    token: "setup".to_owned(),
                    email: "owner@example.com".to_owned(),
                    display_name: "Owner".to_owned(),
                    password_hash: PasswordService::default()
                        .hash("correct horse battery")
                        .unwrap(),
                    workspace_name: "Orbit".to_owned(),
                    project_name: "General".to_owned(),
                },
                now,
            )
            .await
            .unwrap();
        let state = IntegrationState::new(
            Arc::new(ApiTokenRepository::new((*database).clone())),
            Arc::new(TaskRepository::new((*database).clone())),
        );
        let hash: [u8; 32] = Sha256::digest(b"one-time-state").into();
        sqlx::query("INSERT INTO github_app_registrations (state_hash, workspace_id, expires_at) VALUES (?, ?, ?)")
             .bind(hash.to_vec()).bind(setup.workspace_id.to_string())
            .bind(now.as_millis() + 60_000).execute(database.pool()).await.unwrap();
        let app = GithubManifestConversion {
            id: 77,
            slug: "orbit-test".to_owned(),
            pem: "private-key".to_owned(),
            webhook_secret: "hook-secret".to_owned(),
        };
        assert!(
            store_github_app(
                &state,
                &setup.workspace_id.to_string(),
                &hash,
                &[7u8; 32],
                &app,
                None
            )
            .await
            .is_ok()
        );
        let row = sqlx::query("SELECT app_id, slug, private_key_encrypted, webhook_secret_encrypted FROM github_apps WHERE workspace_id = ?")
            .bind(setup.workspace_id.to_string()).fetch_one(database.pool()).await.unwrap();
        assert_eq!(row.get::<i64, _>("app_id"), 77);
        assert_eq!(row.get::<String, _>("slug"), "orbit-test");
        assert_eq!(
            decrypt_github_secret(&[7u8; 32], &row.get::<Vec<u8>, _>("private_key_encrypted"))
                .unwrap(),
            "private-key"
        );
        assert_eq!(
            decrypt_github_secret(
                &[7u8; 32],
                &row.get::<Vec<u8>, _>("webhook_secret_encrypted")
            )
            .unwrap(),
            "hook-secret"
        );
        let pending: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM github_app_registrations WHERE state_hash = ?",
        )
        .bind(hash.to_vec())
        .fetch_one(database.pool())
        .await
        .unwrap();
        assert_eq!(pending, 0);
    }

    #[test]
    fn discord_titles_use_the_first_nonempty_line_and_unicode_character_limit() {
        assert_eq!(
            discord_title("\n  A   short title  \nbody"),
            "A short title"
        );
        let title = discord_title("1234567890123456789012345678901234567890extra");
        assert_eq!(title, "1234567890123456789012345678901234567...");
        assert_eq!(title.chars().count(), 40);
    }
}
