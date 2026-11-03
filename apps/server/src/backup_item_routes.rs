//! Instance admins browse a backup snapshot and copy one page (with its sub-pages) or one task
//! into the live workspace, while the server runs. Live data is never changed except for the
//! new rows. See [`crate::repositories::backup_items`].

use std::sync::Arc;

use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use orbit_platform::{
    BackupError, BackupService, Id, RequestId, SnapshotReader, UploadError, UploadService,
};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::auth_routes::CookieMode;
use crate::repositories::backup_items::{
    BackupItem, BackupItemError, BackupItemKind, BackupItemRepository, BackupWorkspace,
    RestoreSource, RestoredItem,
};
use crate::repositories::identity::IdentityRepository;
use crate::task_routes::{ApiError, ApiJson, ApiQuery, authenticate_session, request_id_value};

#[derive(Clone)]
pub struct BackupItemState {
    identity: Arc<IdentityRepository>,
    backups: Option<BackupService>,
    items: Arc<BackupItemRepository>,
    cookie_mode: CookieMode,
}

impl BackupItemState {
    #[must_use]
    pub fn new(
        identity: Arc<IdentityRepository>,
        backups: Option<BackupService>,
        uploads: UploadService,
        cookie_mode: CookieMode,
    ) -> Self {
        Self {
            items: Arc::new(BackupItemRepository::new(
                identity.database().clone(),
                uploads,
            )),
            identity,
            backups,
            cookie_mode,
        }
    }
}

pub fn backup_item_router(state: BackupItemState) -> Router {
    Router::new()
        .route(
            "/api/v1/admin/backups/{backup_id}/workspaces",
            get(list_backup_workspaces),
        )
        .route(
            "/api/v1/admin/backups/{backup_id}/workspaces/{workspace_id}/items",
            get(list_backup_items),
        )
        .route(
            "/api/v1/admin/backups/{backup_id}/workspaces/{workspace_id}/restore",
            post(restore_backup_item),
        )
        .with_state(state)
}

#[derive(Serialize, ToSchema)]
pub struct BackupWorkspaceList {
    items: Vec<BackupWorkspace>,
}

#[derive(Serialize, ToSchema)]
pub struct BackupItemList {
    items: Vec<BackupItem>,
}

#[derive(Deserialize, IntoParams, ToSchema)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
pub struct BackupItemQuery {
    kind: BackupItemKind,
    /// Only items whose title contains this text (any case).
    q: Option<String>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RestoreBackupItem {
    kind: BackupItemKind,
    /// The item's id in the snapshot.
    id: String,
    /// Pages: also copy the sub-pages. Ignored for tasks.
    #[serde(default)]
    include_children: bool,
}

/// The workspaces in the snapshot; `live` ones can receive a restore.
#[utoipa::path(get, path = "/api/v1/admin/backups/{backup_id}/workspaces", params(("backup_id" = String, Path)), responses((status = 200, body = BackupWorkspaceList)))]
async fn list_backup_workspaces(
    State(state): State<BackupItemState>,
    Path(backup_id): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<BackupWorkspaceList>, ApiError> {
    let instance = format!("/api/v1/admin/backups/{backup_id}/workspaces");
    let request_id = request_id.as_ref();
    let (_, _, reader) = open(&state, &headers, &backup_id, &instance, request_id).await?;
    let items = state
        .items
        .workspaces(&reader)
        .await
        .map_err(|error| problem(error, &instance, request_id))?;
    Ok(Json(BackupWorkspaceList { items }))
}

/// Pages or tasks of a workspace in the snapshot, newest first, at most 200.
#[utoipa::path(get, path = "/api/v1/admin/backups/{backup_id}/workspaces/{workspace_id}/items", params(("backup_id" = String, Path), ("workspace_id" = String, Path), BackupItemQuery), responses((status = 200, body = BackupItemList)))]
async fn list_backup_items(
    State(state): State<BackupItemState>,
    Path((backup_id, workspace)): Path<(String, String)>,
    ApiQuery(query): ApiQuery<BackupItemQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<BackupItemList>, ApiError> {
    let instance = format!("/api/v1/admin/backups/{backup_id}/workspaces/{workspace}/items");
    let request_id = request_id.as_ref();
    let (_, _, reader) = open(&state, &headers, &backup_id, &instance, request_id).await?;
    let workspace_id = workspace_id(&workspace, &instance, request_id)?;
    let fail = |error| problem(error, &instance, request_id);
    state
        .items
        .require_workspace(&reader, workspace_id)
        .await
        .map_err(fail)?;
    let items = state
        .items
        .items(&reader, workspace_id, query.kind, query.q.as_deref())
        .await
        .map_err(fail)?;
    Ok(Json(BackupItemList { items }))
}

/// Copies a page (with `include_children`, its sub-pages) or a task from the snapshot into the
/// live workspace as a new item. Comments, history and relations are not copied.
#[utoipa::path(post, path = "/api/v1/admin/backups/{backup_id}/workspaces/{workspace_id}/restore", params(("backup_id" = String, Path), ("workspace_id" = String, Path)), request_body = RestoreBackupItem, responses((status = 201, body = RestoredItem)))]
async fn restore_backup_item(
    State(state): State<BackupItemState>,
    Path((backup_id, workspace)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<RestoreBackupItem>,
) -> Result<(StatusCode, Json<RestoredItem>), ApiError> {
    let instance = format!("/api/v1/admin/backups/{backup_id}/workspaces/{workspace}/restore");
    let request_id = request_id.as_ref();
    let (actor_id, snapshot, reader) =
        open(&state, &headers, &backup_id, &instance, request_id).await?;
    let workspace_id = workspace_id(&workspace, &instance, request_id)?;
    let fail = |error| problem(error, &instance, request_id);
    state
        .items
        .require_workspace(&reader, workspace_id)
        .await
        .map_err(fail)?;
    let item_id: Id = body
        .id
        .parse()
        .map_err(|_| fail(BackupItemError::ItemNotFound))?;
    let source = RestoreSource {
        snapshot: &snapshot,
        reader: &reader,
        workspace_id,
        actor_id,
        request_id: request_id_value(request_id),
    };
    let restored = match body.kind {
        BackupItemKind::Page => {
            state
                .items
                .restore_page(source, item_id, body.include_children)
                .await
        }
        BackupItemKind::Task => state.items.restore_task(source, item_id).await,
    }
    .map_err(|error| {
        if matches!(
            error,
            BackupItemError::Database(_) | BackupItemError::Upload(_)
        ) {
            tracing::error!(%error, backup_id, "backup item restore failed");
        }
        fail(error)
    })?;
    Ok((StatusCode::CREATED, Json(restored)))
}

/// Checks that the caller is an instance admin, then verifies the snapshot (cached for a few
/// minutes) and opens it for reading.
async fn open(
    state: &BackupItemState,
    headers: &HeaderMap,
    backup_id: &str,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<(Id, orbit_platform::BackupSnapshot, SnapshotReader), ApiError> {
    let session = authenticate_session(
        &state.identity,
        state.cookie_mode,
        headers,
        instance,
        request_id,
    )
    .await?;
    let admin = state
        .identity
        .is_installation_admin(session.user.id)
        .await
        .map_err(|_| internal(instance, request_id))?;
    if !admin {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "installation_admin_required",
            "Installation administrator required",
            "Only an installation administrator may do this.",
            instance,
            request_id,
        ));
    }
    let backups = state.backups.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "backup_unavailable",
            "Backup unavailable",
            "The backup service is not available.",
            instance,
            request_id,
        )
    })?;
    let snapshot = backups
        .verified_snapshot(backup_id)
        .await
        .map_err(|error| match error {
            BackupError::NotFound { .. } => ApiError::new(
                StatusCode::NOT_FOUND,
                "backup_not_found",
                "Backup not found",
                "The backup does not exist.",
                instance,
                request_id,
            ),
            error => {
                tracing::warn!(%error, backup_id, "backup snapshot cannot be read");
                unreadable(instance, request_id)
            }
        })?;
    let reader = backups.snapshot_reader(&snapshot).await.map_err(|error| {
        tracing::warn!(%error, backup_id, "backup snapshot cannot be read");
        unreadable(instance, request_id)
    })?;
    Ok((session.user.id, snapshot, reader))
}

fn workspace_id(
    value: &str,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<Id, ApiError> {
    value
        .parse()
        .map_err(|_| problem(BackupItemError::WorkspaceNotFound, instance, request_id))
}

fn unreadable(instance: &str, request_id: Option<&Extension<RequestId>>) -> ApiError {
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        "backup_unreadable",
        "Backup unreadable",
        "Orbit could not read this backup, or it lacks data this restore needs.",
        instance,
        request_id,
    )
}

fn internal(instance: &str, request_id: Option<&Extension<RequestId>>) -> ApiError {
    ApiError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal_error",
        "Internal server error",
        "An unexpected error occurred. Use the request ID when contacting support.",
        instance,
        request_id,
    )
}

fn problem(
    error: BackupItemError,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> ApiError {
    match error {
        BackupItemError::WorkspaceNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            "workspace_not_found",
            "Workspace not found",
            "The workspace is not in this backup, or it no longer exists.",
            instance,
            request_id,
        ),
        BackupItemError::ItemNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            "item_not_found",
            "Item not found",
            "The item is not in this workspace of the backup.",
            instance,
            request_id,
        ),
        BackupItemError::ProjectMissing => ApiError::new(
            StatusCode::CONFLICT,
            "project_missing",
            "Project missing",
            "The task's project no longer exists, so the task cannot be restored.",
            instance,
            request_id,
        ),
        BackupItemError::TeamspaceMissing => ApiError::new(
            StatusCode::CONFLICT,
            "teamspace_missing",
            "Teamspace missing",
            "The page's teamspace no longer exists, or the owner of the private page left the workspace.",
            instance,
            request_id,
        ),
        BackupItemError::Unreadable(error) => {
            tracing::warn!(%error, "backup snapshot cannot be read");
            unreadable(instance, request_id)
        }
        BackupItemError::TrialLimit(reached) => {
            ApiError::trial_limit(reached, instance, request_id)
        }
        BackupItemError::Upload(UploadError::StorageQuotaExceeded { .. }) => ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "storage_quota_exceeded",
            "Storage quota exceeded",
            "The restored files would take the workspace over its storage quota.",
            instance,
            request_id,
        ),
        BackupItemError::Upload(_) | BackupItemError::Database(_) => internal(instance, request_id),
    }
}
