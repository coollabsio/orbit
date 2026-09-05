use std::error::Error as StdError;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::extract::{
    DefaultBodyLimit, Extension, FromRequest, FromRequestParts, Multipart, Path, Query, Request,
    State,
};
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_TYPE, COOKIE};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::TryStreamExt;
use orbit_platform::{
    AuthorizedAttachment, BlobStoreError, Id, RequestId, TimestampMillis, UploadError,
    UploadService,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::Row;
use tokio_util::io::{ReaderStream, StreamReader};
use tokio_util::sync::CancellationToken;
use utoipa::{IntoParams, ToSchema};

use crate::audit::{self, AuditOutcome};
use crate::auth_routes::CookieMode;
use crate::repositories::attachments::{
    AttachmentRepository, AttachmentRepositoryError, CreatedAttachment,
};
use crate::repositories::identity::AuthenticatedSession;
use crate::repositories::identity::IdentityRepository;
use crate::repositories::tasks::{CommentRecord, TaskError, TaskRepository};

const DAY_MILLIS: i64 = 24 * 60 * 60 * 1_000;
const RECONCILE_INTERVAL: Duration = Duration::from_secs(60 * 60);

#[derive(Clone)]
pub struct AttachmentState {
    identity: Arc<IdentityRepository>,
    tasks: Arc<TaskRepository>,
    uploads: UploadService,
    attachments: Arc<AttachmentRepository>,
    cookie_mode: CookieMode,
}

impl AttachmentState {
    #[must_use]
    pub fn new(
        identity: Arc<IdentityRepository>,
        uploads: UploadService,
        cookie_mode: CookieMode,
    ) -> Self {
        let attachments = Arc::new(AttachmentRepository::new(
            identity.database().clone(),
            uploads.clone(),
        ));
        Self {
            tasks: Arc::new(TaskRepository::new(identity.database().clone())),
            identity,
            uploads,
            attachments,
            cookie_mode,
        }
    }

    /// Reconciles stale pending uploads and orphaned files once at startup and then hourly.
    pub async fn run_reconciliation_service(
        &self,
        shutdown: CancellationToken,
    ) -> Result<(), UploadError> {
        self.uploads
            .reconcile(TimestampMillis::now().as_millis())
            .await?;
        let mut interval = tokio::time::interval(RECONCILE_INTERVAL);
        interval.tick().await;
        loop {
            tokio::select! {
                () = shutdown.cancelled() => return Ok(()),
                _ = interval.tick() => {
                    self.uploads.reconcile(TimestampMillis::now().as_millis()).await?;
                }
            }
        }
    }

    pub async fn reconcile_at(&self, now_millis: i64) -> Result<(), UploadError> {
        self.uploads.reconcile(now_millis).await.map(|_| ())
    }
}

pub fn attachment_router(state: AttachmentState) -> Router {
    let request_limit =
        usize::try_from(state.uploads.limits().max_request_bytes()).unwrap_or(usize::MAX);
    Router::new()
        .route(
            "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/attachments",
            get(list_task_attachments).post(upload_task_attachments),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/attachments/{attachment_id}",
            axum::routing::delete(delete_task_attachment),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/attachments/{attachment_id}/download",
            get(download_task_attachment),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/comments/{comment_id}/attachments",
            get(list_comment_attachments).post(upload_comment_attachments),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/comments/{comment_id}/attachments/{attachment_id}",
            axum::routing::delete(delete_comment_attachment),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/comments/{comment_id}/attachments/{attachment_id}/download",
            get(download_comment_attachment),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/comments/attachments",
            post(create_attachment_comment),
        )
        // Multipart remains streaming; this bounds boundaries, headers, and ignored fields too.
        .layer(DefaultBodyLimit::max(request_limit))
        .with_state(state)
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct AttachmentRecord {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub workspace_id: Id,
    #[schema(value_type = String)]
    pub task_id: Id,
    #[schema(value_type = Option<String>)]
    pub comment_id: Option<Id>,
    #[schema(value_type = String)]
    pub owner_id: Id,
    pub display_name: String,
    pub media_type: String,
    pub byte_size: u64,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
}

impl From<CreatedAttachment> for AttachmentRecord {
    fn from(value: CreatedAttachment) -> Self {
        Self {
            id: value.id,
            workspace_id: value.workspace_id,
            task_id: value.task_id,
            comment_id: value.comment_id,
            owner_id: value.owner_id,
            display_name: value.display_name,
            media_type: value.media_type,
            byte_size: value.byte_size,
            created_at: value.created_at,
        }
    }
}

#[derive(Serialize, ToSchema)]
struct AttachmentPage {
    items: Vec<AttachmentRecord>,
    next_cursor: Option<String>,
}

#[derive(Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
struct PageQuery {
    cursor: Option<String>,
    limit: Option<usize>,
}

struct AttachmentQuery<T>(T);

impl<S, T> FromRequestParts<S> for AttachmentQuery<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = AttachmentApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let instance = parts.uri.path().to_owned();
        let request_id = parts.extensions.get::<RequestId>().cloned().map(Extension);
        Query::<T>::from_request_parts(parts, state)
            .await
            .map(|Query(value)| Self(value))
            .map_err(|_| AttachmentApiError::invalid_request(&instance, request_id.as_ref()))
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AttachmentCursor {
    version: u8,
    scope: String,
    created_at: i64,
    id: String,
}

#[derive(Serialize, ToSchema)]
struct AttachmentComment {
    comment: CommentRecord,
    attachments: Vec<AttachmentRecord>,
}

#[derive(ToSchema)]
#[allow(dead_code)]
struct AttachmentUploadBody {
    #[schema(value_type = String, format = Binary)]
    file: Vec<u8>,
}

#[derive(ToSchema)]
#[schema(value_type = String, format = Binary)]
#[allow(dead_code)]
struct AttachmentDownload(Vec<u8>);

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/attachments", params(PageQuery, ("workspace_id" = String, Path), ("task_id" = String, Path)), responses((status = 200, body = AttachmentPage)))]
async fn list_task_attachments(
    State(state): State<AttachmentState>,
    Path((workspace, task)): Path<(String, String)>,
    AttachmentQuery(page): AttachmentQuery<PageQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<AttachmentPage>, AttachmentApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/tasks/{task}/attachments");
    let (workspace_id, task_id, _) = authorize_task(
        &state,
        &headers,
        &workspace,
        &task,
        &instance,
        request_id.as_ref(),
    )
    .await?;
    Ok(Json(
        attachments(
            &state,
            workspace_id,
            task_id,
            None,
            page,
            &instance,
            request_id.as_ref(),
        )
        .await?,
    ))
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/comments/{comment_id}/attachments", params(PageQuery, ("workspace_id" = String, Path), ("task_id" = String, Path), ("comment_id" = String, Path)), responses((status = 200, body = AttachmentPage)))]
async fn list_comment_attachments(
    State(state): State<AttachmentState>,
    Path((workspace, task, comment)): Path<(String, String, String)>,
    AttachmentQuery(page): AttachmentQuery<PageQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<AttachmentPage>, AttachmentApiError> {
    let instance =
        format!("/api/v1/workspaces/{workspace}/tasks/{task}/comments/{comment}/attachments");
    let (workspace_id, task_id, _) = authorize_task(
        &state,
        &headers,
        &workspace,
        &task,
        &instance,
        request_id.as_ref(),
    )
    .await?;
    let comment_id = parse_id(&comment, &instance, request_id.as_ref())?;
    require_comment(
        &state,
        workspace_id,
        task_id,
        comment_id,
        &instance,
        request_id.as_ref(),
    )
    .await?;
    Ok(Json(
        attachments(
            &state,
            workspace_id,
            task_id,
            Some(comment_id),
            page,
            &instance,
            request_id.as_ref(),
        )
        .await?,
    ))
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/attachments", params(("workspace_id" = String, Path), ("task_id" = String, Path)), request_body(content = AttachmentUploadBody, content_type = "multipart/form-data"), responses((status = 201, body = AttachmentRecord)))]
async fn upload_task_attachments(
    State(state): State<AttachmentState>,
    Path((workspace, task)): Path<(String, String)>,
    request: Request,
) -> Result<Response, AttachmentApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/tasks/{task}/attachments");
    let request_id = request
        .extensions()
        .get::<RequestId>()
        .cloned()
        .map(Extension);
    let (workspace_id, task_id, session) = authorize_task(
        &state,
        request.headers(),
        &workspace,
        &task,
        &instance,
        request_id.as_ref(),
    )
    .await?;
    let records = upload_fields(
        &state,
        request,
        workspace_id,
        task_id,
        None,
        &session,
        &instance,
        request_id.as_ref(),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(records[0].clone())).into_response())
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/comments/{comment_id}/attachments", params(("workspace_id" = String, Path), ("task_id" = String, Path), ("comment_id" = String, Path)), request_body(content = AttachmentUploadBody, content_type = "multipart/form-data"), responses((status = 201, body = AttachmentRecord)))]
async fn upload_comment_attachments(
    State(state): State<AttachmentState>,
    Path((workspace, task, comment)): Path<(String, String, String)>,
    request: Request,
) -> Result<Response, AttachmentApiError> {
    let instance =
        format!("/api/v1/workspaces/{workspace}/tasks/{task}/comments/{comment}/attachments");
    let request_id = request
        .extensions()
        .get::<RequestId>()
        .cloned()
        .map(Extension);
    let (workspace_id, task_id, session) = authorize_task(
        &state,
        request.headers(),
        &workspace,
        &task,
        &instance,
        request_id.as_ref(),
    )
    .await?;
    let comment_id = parse_id(&comment, &instance, request_id.as_ref())?;
    require_comment(
        &state,
        workspace_id,
        task_id,
        comment_id,
        &instance,
        request_id.as_ref(),
    )
    .await?;
    let records = upload_fields(
        &state,
        request,
        workspace_id,
        task_id,
        Some(comment_id),
        &session,
        &instance,
        request_id.as_ref(),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(records[0].clone())).into_response())
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/comments/attachments", params(("workspace_id" = String, Path), ("task_id" = String, Path)), request_body(content = AttachmentUploadBody, content_type = "multipart/form-data"), responses((status = 201, body = AttachmentComment)))]
async fn create_attachment_comment(
    State(state): State<AttachmentState>,
    Path((workspace, task)): Path<(String, String)>,
    request: Request,
) -> Result<Response, AttachmentApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/tasks/{task}/comments/attachments");
    let request_id = request
        .extensions()
        .get::<RequestId>()
        .cloned()
        .map(Extension);
    let (workspace_id, task_id, session) = authorize_task(
        &state,
        request.headers(),
        &workspace,
        &task,
        &instance,
        request_id.as_ref(),
    )
    .await?;
    let mut staged = stage_request(
        &state,
        request,
        workspace_id,
        session.user.id,
        &instance,
        request_id.as_ref(),
    )
    .await?;
    let upload = staged.upload().clone();
    let (attachment, comment) = match state
        .attachments
        .finalize_attachment_comment(
            &session,
            workspace_id,
            task_id,
            request_id_value(request_id.as_ref()),
            &upload,
        )
        .await
    {
        Ok(result) => result,
        Err(error) => {
            staged.discard().await;
            return Err(AttachmentApiError::repository(
                error,
                &instance,
                request_id.as_ref(),
            ));
        }
    };
    staged.complete();
    Ok((
        StatusCode::CREATED,
        Json(AttachmentComment {
            comment,
            attachments: vec![attachment.into()],
        }),
    )
        .into_response())
}

#[allow(clippy::too_many_arguments)]
async fn upload_fields(
    state: &AttachmentState,
    request: Request,
    workspace_id: Id,
    task_id: Id,
    comment_id: Option<Id>,
    session: &AuthenticatedSession,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<Vec<AttachmentRecord>, AttachmentApiError> {
    let mut staged_uploads = stage_request(
        state,
        request,
        workspace_id,
        session.user.id,
        instance,
        request_id,
    )
    .await?;
    let staged = staged_uploads.upload().clone();
    let attachment = match state
        .attachments
        .finalize(session, workspace_id, task_id, comment_id, &staged)
        .await
    {
        Ok(attachment) => attachment,
        Err(error) => {
            staged_uploads.discard().await;
            return Err(AttachmentApiError::repository(error, instance, request_id));
        }
    };
    staged_uploads.complete();
    Ok(vec![attachment.into()])
}

async fn stage_request(
    state: &AttachmentState,
    request: Request,
    workspace_id: Id,
    actor_id: Id,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<StagedRequest, AttachmentApiError> {
    let mut multipart = Multipart::from_request(request, state)
        .await
        .map_err(|error| {
            let length_limited = has_length_limit(&error);
            if length_limited || error.into_response().status() == StatusCode::PAYLOAD_TOO_LARGE {
                AttachmentApiError::request_too_large(instance, request_id)
            } else {
                AttachmentApiError::invalid_multipart(instance, request_id)
            }
        })?;
    let mut received = 0_u64;
    let mut staged_uploads = StagedRequest::new(state.uploads.clone());
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(field)) => field,
            Ok(None) => break,
            Err(error) => {
                staged_uploads.discard().await;
                return Err(if multipart_too_large(&error) {
                    AttachmentApiError::request_too_large(instance, request_id)
                } else {
                    AttachmentApiError::invalid_multipart(instance, request_id)
                });
            }
        };
        if field.name() != Some("file") {
            if field.file_name().is_some() {
                staged_uploads.discard().await;
                return Err(AttachmentApiError::invalid_multipart(instance, request_id));
            }
            continue;
        }
        if !staged_uploads.is_empty() {
            staged_uploads.discard().await;
            return Err(AttachmentApiError::invalid_multipart(instance, request_id));
        }
        let name = field.file_name().unwrap_or("attachment").to_owned();
        let reader = StreamReader::new(field.map_err(|error| {
            if multipart_too_large(&error) {
                io::Error::new(io::ErrorKind::FileTooLarge, error.to_string())
            } else {
                io::Error::new(io::ErrorKind::InvalidData, error.to_string())
            }
        }));
        let staged = match state
            .uploads
            .stage_in_request(workspace_id, actor_id, &name, received, reader)
            .await
        {
            Ok(staged) => staged,
            Err(error) => {
                staged_uploads.discard().await;
                return Err(AttachmentApiError::upload(error, instance, request_id));
            }
        };
        received = received.saturating_add(staged.size_bytes);
        staged_uploads.push(staged);
    }
    if staged_uploads.is_empty() {
        return Err(AttachmentApiError::invalid_multipart(instance, request_id));
    }
    Ok(staged_uploads)
}

fn multipart_too_large(error: &axum::extract::multipart::MultipartError) -> bool {
    error.status() == StatusCode::PAYLOAD_TOO_LARGE || has_length_limit(error)
}

fn has_length_limit(error: &(dyn StdError + 'static)) -> bool {
    let mut source = Some(error);
    while let Some(error) = source {
        if error.is::<http_body_util::LengthLimitError>() {
            return true;
        }
        source = error.source();
    }
    false
}

struct StagedRequest {
    uploads: Vec<orbit_platform::StagedUpload>,
    service: UploadService,
}

impl StagedRequest {
    fn new(service: UploadService) -> Self {
        Self {
            uploads: Vec::new(),
            service,
        }
    }

    fn is_empty(&self) -> bool {
        self.uploads.is_empty()
    }

    fn push(&mut self, upload: orbit_platform::StagedUpload) {
        self.uploads.push(upload);
    }

    fn upload(&self) -> &orbit_platform::StagedUpload {
        &self.uploads[0]
    }

    fn complete(&mut self) {
        self.uploads.clear();
    }

    async fn discard(&mut self) {
        // Keep ownership in the guard across every await so cancellation falls through to Drop.
        for upload in self.uploads.clone() {
            let _ = self.service.discard(&upload).await;
        }
        self.uploads.clear();
    }
}

impl Drop for StagedRequest {
    fn drop(&mut self) {
        let uploads = std::mem::take(&mut self.uploads);
        if uploads.is_empty() {
            return;
        }
        let service = self.service.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                for upload in uploads {
                    let _ = service.discard(&upload).await;
                }
            });
        }
    }
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/attachments/{attachment_id}", params(("workspace_id" = String, Path), ("task_id" = String, Path), ("attachment_id" = String, Path)), responses((status = 204)))]
async fn delete_task_attachment(
    State(state): State<AttachmentState>,
    Path((workspace, task, attachment)): Path<(String, String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<StatusCode, AttachmentApiError> {
    delete_attachment(
        &state,
        &headers,
        &workspace,
        &task,
        None,
        &attachment,
        request_id.as_ref(),
    )
    .await
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/comments/{comment_id}/attachments/{attachment_id}", params(("workspace_id" = String, Path), ("task_id" = String, Path), ("comment_id" = String, Path), ("attachment_id" = String, Path)), responses((status = 204)))]
async fn delete_comment_attachment(
    State(state): State<AttachmentState>,
    Path((workspace, task, comment, attachment)): Path<(String, String, String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<StatusCode, AttachmentApiError> {
    delete_attachment(
        &state,
        &headers,
        &workspace,
        &task,
        Some(&comment),
        &attachment,
        request_id.as_ref(),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn delete_attachment(
    state: &AttachmentState,
    headers: &HeaderMap,
    workspace: &str,
    task: &str,
    comment: Option<&str>,
    attachment: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<StatusCode, AttachmentApiError> {
    let instance = if let Some(comment) = comment {
        format!(
            "/api/v1/workspaces/{workspace}/tasks/{task}/comments/{comment}/attachments/{attachment}"
        )
    } else {
        format!("/api/v1/workspaces/{workspace}/tasks/{task}/attachments/{attachment}")
    };
    let (workspace_id, task_id, session) =
        authorize_task(state, headers, workspace, task, &instance, request_id).await?;
    let actor_id = session.user.id;
    let attachment_id = parse_id(attachment, &instance, request_id)?;
    let comment_id = comment
        .map(|value| parse_id(value, &instance, request_id))
        .transpose()?;
    let now = TimestampMillis::now();
    let mut tx = state.identity.database().immediate_transaction().await?;
    let current_access: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tasks JOIN projects ON projects.id = tasks.project_id \
         JOIN workspaces ON workspaces.id = tasks.workspace_id \
         JOIN memberships ON memberships.workspace_id = tasks.workspace_id AND memberships.user_id = ? \
         JOIN users ON users.id = memberships.user_id WHERE tasks.id = ? AND tasks.workspace_id = ? \
         AND tasks.deleted_at IS NULL AND projects.deleted_at IS NULL \
         AND workspaces.deleted_at IS NULL AND users.suspended_at IS NULL",
    )
    .bind(actor_id.to_string())
    .bind(task_id.to_string())
    .bind(workspace_id.to_string())
    .fetch_one(&mut *tx)
    .await?;
    if current_access != 1 {
        return Err(AttachmentApiError::not_found(&instance, request_id));
    }
    let row = sqlx::query(
        "SELECT blob_id FROM attachment_references WHERE id = ? AND workspace_id = ? \
         AND task_id = ? AND comment_id IS ?",
    )
    .bind(attachment_id.to_string())
    .bind(workspace_id.to_string())
    .bind(task_id.to_string())
    .bind(comment_id.map(|id| id.to_string()))
    .fetch_optional(&mut *tx)
    .await?;
    let Some(row) = row else {
        return Err(AttachmentApiError::not_found(&instance, request_id));
    };
    let blob_id: String = row.try_get("blob_id")?;
    sqlx::query("DELETE FROM attachment_references WHERE id = ?")
        .bind(attachment_id.to_string())
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE attachment_blobs SET quarantine_until = MAX(quarantine_until, ?) \
         WHERE id = ? AND NOT EXISTS (SELECT 1 FROM attachment_references WHERE blob_id = ?)",
    )
    .bind(now.as_millis().saturating_add(DAY_MILLIS))
    .bind(&blob_id)
    .bind(&blob_id)
    .execute(&mut *tx)
    .await?;
    audit::record(
        &mut tx,
        workspace_id,
        Some(actor_id),
        "attachment.deleted",
        AuditOutcome::Success,
        "attachment",
        Some(attachment_id),
        request_id_value(request_id),
        json!({"task_id": task_id, "comment_id": comment_id}),
        now,
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/attachments/{attachment_id}/download", params(("workspace_id" = String, Path), ("task_id" = String, Path), ("attachment_id" = String, Path)), responses((status = 200, body = AttachmentDownload, content_type = "application/octet-stream")))]
async fn download_task_attachment(
    State(state): State<AttachmentState>,
    Path((workspace, task, attachment)): Path<(String, String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Response, AttachmentApiError> {
    download_attachment(
        &state,
        &headers,
        &workspace,
        &task,
        None,
        &attachment,
        request_id.as_ref(),
    )
    .await
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/comments/{comment_id}/attachments/{attachment_id}/download", params(("workspace_id" = String, Path), ("task_id" = String, Path), ("comment_id" = String, Path), ("attachment_id" = String, Path)), responses((status = 200, body = AttachmentDownload, content_type = "application/octet-stream")))]
async fn download_comment_attachment(
    State(state): State<AttachmentState>,
    Path((workspace, task, comment, attachment)): Path<(String, String, String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Response, AttachmentApiError> {
    download_attachment(
        &state,
        &headers,
        &workspace,
        &task,
        Some(&comment),
        &attachment,
        request_id.as_ref(),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn download_attachment(
    state: &AttachmentState,
    headers: &HeaderMap,
    workspace: &str,
    task: &str,
    comment: Option<&str>,
    attachment: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<Response, AttachmentApiError> {
    let instance = if let Some(comment) = comment {
        format!(
            "/api/v1/workspaces/{workspace}/tasks/{task}/comments/{comment}/attachments/{attachment}/download"
        )
    } else {
        format!("/api/v1/workspaces/{workspace}/tasks/{task}/attachments/{attachment}/download")
    };
    let (workspace_id, task_id, session) =
        authorize_task(state, headers, workspace, task, &instance, request_id).await?;
    let actor_id = session.user.id;
    let attachment_id = parse_id(attachment, &instance, request_id)?;
    let comment_id = comment
        .map(|value| parse_id(value, &instance, request_id))
        .transpose()?;
    let download = state
        .uploads
        .download(|| async {
            authorized_attachment(
                state,
                workspace_id,
                task_id,
                comment_id,
                attachment_id,
                actor_id,
            )
            .await
            .map_err(|error| match error {
                AttachmentLookupError::NotFound => UploadError::Unauthorized,
                AttachmentLookupError::Database(error) => UploadError::Database(error),
            })
        })
        .await
        .map_err(|error| AttachmentApiError::upload(error, &instance, request_id))?;
    let content_type = HeaderValue::from_str(&download.metadata.content_type)
        .map_err(|_| AttachmentApiError::internal(&instance, request_id))?;
    let disposition = HeaderValue::from_str(&download.metadata.content_disposition)
        .map_err(|_| AttachmentApiError::internal(&instance, request_id))?;
    let mut response = Response::new(Body::from_stream(ReaderStream::new(download.reader)));
    response.headers_mut().insert(CONTENT_TYPE, content_type);
    response
        .headers_mut()
        .insert(CONTENT_DISPOSITION, disposition);
    response.headers_mut().insert(
        "x-content-type-options",
        HeaderValue::from_static(download.metadata.x_content_type_options),
    );
    Ok(response)
}

async fn authorize_task(
    state: &AttachmentState,
    headers: &HeaderMap,
    workspace: &str,
    task: &str,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<(Id, Id, AuthenticatedSession), AttachmentApiError> {
    // This function intentionally runs before Multipart is constructed or polled.
    let token = cookie_value(headers, state.cookie_mode.session_cookie_name())
        .ok_or_else(|| AttachmentApiError::unauthorized(instance, request_id))?;
    let session = state
        .identity
        .authenticate_session(&token, TimestampMillis::now())
        .await
        .map_err(|_| AttachmentApiError::unauthorized(instance, request_id))?;
    let workspace_id = parse_id(workspace, instance, request_id)?;
    let task_id = parse_id(task, instance, request_id)?;
    state
        .tasks
        .get_task(workspace_id, task_id, session.user.id)
        .await
        .map_err(|error| AttachmentApiError::task(error, instance, request_id))?;
    Ok((workspace_id, task_id, session))
}

async fn require_comment(
    state: &AttachmentState,
    workspace_id: Id,
    task_id: Id,
    comment_id: Id,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<(), AttachmentApiError> {
    let exists: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM task_comments WHERE id = ? AND workspace_id = ? AND task_id = ?",
    )
    .bind(comment_id.to_string())
    .bind(workspace_id.to_string())
    .bind(task_id.to_string())
    .fetch_one(state.identity.database().pool())
    .await?;
    if exists == 1 {
        Ok(())
    } else {
        Err(AttachmentApiError::not_found(instance, request_id))
    }
}

async fn attachments(
    state: &AttachmentState,
    workspace_id: Id,
    task_id: Id,
    comment_id: Option<Id>,
    page: PageQuery,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<AttachmentPage, AttachmentApiError> {
    let scope = format!(
        "{workspace_id}:{task_id}:{}",
        comment_id.map_or_else(|| "task".to_owned(), |id| id.to_string())
    );
    let after = page
        .cursor
        .as_deref()
        .map(|encoded| {
            decode_cursor(encoded)
                .and_then(|cursor| {
                    if cursor.version != 1
                        || cursor.scope != scope
                        || cursor.id.parse::<Id>().is_err()
                    {
                        None
                    } else {
                        Some((cursor.created_at, cursor.id))
                    }
                })
                .ok_or_else(|| AttachmentApiError::invalid_cursor(instance, request_id))
        })
        .transpose()?;
    let limit = page.limit.unwrap_or(50).clamp(1, 100);
    let fetch_limit = i64::try_from(limit + 1).expect("attachment page limit fits i64");
    let comment = comment_id.map(|id| id.to_string());
    let rows = if let Some((created_at, id)) = after {
        sqlx::query(
            "SELECT id, workspace_id, task_id, comment_id, owner_id, display_name, media_type, \
             byte_size, created_at FROM attachment_references WHERE workspace_id = ? AND task_id = ? \
             AND comment_id IS ? AND (created_at > ? OR (created_at = ? AND id > ?)) \
             ORDER BY created_at, id LIMIT ?",
        )
        .bind(workspace_id.to_string()).bind(task_id.to_string()).bind(&comment)
        .bind(created_at).bind(created_at).bind(id).bind(fetch_limit)
        .fetch_all(state.identity.database().pool()).await?
    } else {
        sqlx::query(
            "SELECT id, workspace_id, task_id, comment_id, owner_id, display_name, media_type, \
             byte_size, created_at FROM attachment_references WHERE workspace_id = ? AND task_id = ? \
             AND comment_id IS ? ORDER BY created_at, id LIMIT ?",
        )
        .bind(workspace_id.to_string()).bind(task_id.to_string()).bind(&comment).bind(fetch_limit)
        .fetch_all(state.identity.database().pool()).await?
    };
    let mut items: Vec<_> = rows
        .into_iter()
        .map(attachment_from_row)
        .collect::<Result<_, _>>()?;
    let has_more = items.len() > limit;
    items.truncate(limit);
    let next_cursor = if has_more {
        items
            .last()
            .map(|record| {
                encode_cursor(&AttachmentCursor {
                    version: 1,
                    scope,
                    created_at: record.created_at.as_millis(),
                    id: record.id.to_string(),
                })
            })
            .transpose()
            .map_err(|_| AttachmentApiError::internal(instance, request_id))?
    } else {
        None
    };
    Ok(AttachmentPage { items, next_cursor })
}

fn encode_cursor(cursor: &AttachmentCursor) -> Result<String, serde_json::Error> {
    Ok(serde_json::to_vec(cursor)?
        .into_iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn decode_cursor(encoded: &str) -> Option<AttachmentCursor> {
    if !encoded.len().is_multiple_of(2) || encoded.len() > 8_192 {
        return None;
    }
    let bytes = (0..encoded.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&encoded[index..index + 2], 16).ok())
        .collect::<Option<Vec<_>>>()?;
    serde_json::from_slice(&bytes).ok()
}

fn attachment_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<AttachmentRecord, AttachmentApiError> {
    Ok(AttachmentRecord {
        id: parse_db_id(row.try_get("id")?)?,
        workspace_id: parse_db_id(row.try_get("workspace_id")?)?,
        task_id: parse_db_id(row.try_get("task_id")?)?,
        comment_id: row
            .try_get::<Option<String>, _>("comment_id")?
            .map(parse_db_id)
            .transpose()?,
        owner_id: parse_db_id(row.try_get("owner_id")?)?,
        display_name: row.try_get("display_name")?,
        media_type: row.try_get("media_type")?,
        byte_size: u64::try_from(row.try_get::<i64, _>("byte_size")?)
            .map_err(|_| AttachmentApiError::database_value())?,
        created_at: TimestampMillis::from_millis(row.try_get("created_at")?),
    })
}

enum AttachmentLookupError {
    NotFound,
    Database(sqlx::Error),
}

async fn authorized_attachment(
    state: &AttachmentState,
    workspace_id: Id,
    task_id: Id,
    comment_id: Option<Id>,
    attachment_id: Id,
    actor_id: Id,
) -> Result<AuthorizedAttachment, AttachmentLookupError> {
    let row = sqlx::query(
        "SELECT attachment_blobs.storage_key, attachment_references.media_type, \
         attachment_references.display_name FROM attachment_references JOIN attachment_blobs \
         ON attachment_blobs.id = attachment_references.blob_id \
         AND attachment_blobs.workspace_id = attachment_references.workspace_id \
         JOIN tasks ON tasks.id = attachment_references.task_id \
         AND tasks.workspace_id = attachment_references.workspace_id \
         JOIN projects ON projects.id = tasks.project_id \
         JOIN workspaces ON workspaces.id = tasks.workspace_id \
         JOIN memberships ON memberships.workspace_id = tasks.workspace_id AND memberships.user_id = ? \
         JOIN users ON users.id = memberships.user_id WHERE attachment_references.id = ? \
         AND attachment_references.workspace_id = ? AND attachment_references.task_id = ? \
         AND attachment_references.comment_id IS ? AND tasks.deleted_at IS NULL \
         AND projects.deleted_at IS NULL AND workspaces.deleted_at IS NULL \
         AND users.suspended_at IS NULL",
    )
    .bind(actor_id.to_string())
    .bind(attachment_id.to_string())
    .bind(workspace_id.to_string())
    .bind(task_id.to_string())
    .bind(comment_id.map(|id| id.to_string()))
    .fetch_optional(state.identity.database().pool())
    .await
    .map_err(AttachmentLookupError::Database)?
    .ok_or(AttachmentLookupError::NotFound)?;
    Ok(AuthorizedAttachment {
        storage_key: row
            .try_get("storage_key")
            .map_err(AttachmentLookupError::Database)?,
        media_type: row
            .try_get("media_type")
            .map_err(AttachmentLookupError::Database)?,
        display_name: row
            .try_get("display_name")
            .map_err(AttachmentLookupError::Database)?,
    })
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .map(str::trim)
        .find_map(|pair| pair.strip_prefix(&format!("{name}=")).map(str::to_owned))
}

fn parse_id(
    value: &str,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<Id, AttachmentApiError> {
    value
        .parse()
        .map_err(|_| AttachmentApiError::not_found(instance, request_id))
}

fn parse_db_id(value: String) -> Result<Id, AttachmentApiError> {
    value
        .parse()
        .map_err(|_| AttachmentApiError::database_value())
}

fn request_id_value(request_id: Option<&Extension<RequestId>>) -> &str {
    request_id.map_or("unknown", |Extension(value)| value.as_str())
}

#[derive(Serialize, ToSchema)]
#[schema(as = AttachmentProblem)]
pub(crate) struct AttachmentProblem {
    #[serde(rename = "type")]
    type_uri: String,
    title: &'static str,
    status: u16,
    code: &'static str,
    detail: &'static str,
    instance: String,
    request_id: String,
}

struct AttachmentApiError {
    status: StatusCode,
    body: Box<AttachmentProblem>,
}

impl AttachmentApiError {
    fn new(
        status: StatusCode,
        code: &'static str,
        title: &'static str,
        detail: &'static str,
        instance: &str,
        request_id: Option<&Extension<RequestId>>,
    ) -> Self {
        Self {
            status,
            body: Box::new(AttachmentProblem {
                type_uri: format!("https://docs.orbit.dev/problems/{code}"),
                title,
                status: status.as_u16(),
                code,
                detail,
                instance: instance.to_owned(),
                request_id: request_id_value(request_id).to_owned(),
            }),
        }
    }

    fn unauthorized(instance: &str, request_id: Option<&Extension<RequestId>>) -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "authentication_required",
            "Authentication required",
            "A valid session is required.",
            instance,
            request_id,
        )
    }

    fn not_found(instance: &str, request_id: Option<&Extension<RequestId>>) -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            "attachment_not_found",
            "Attachment not found",
            "The requested attachment was not found.",
            instance,
            request_id,
        )
    }

    fn invalid_multipart(instance: &str, request_id: Option<&Extension<RequestId>>) -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "invalid_multipart",
            "Invalid multipart upload",
            "The request must contain at least one valid file field.",
            instance,
            request_id,
        )
    }

    fn invalid_request(instance: &str, request_id: Option<&Extension<RequestId>>) -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Invalid request",
            "The query parameters are not valid for this endpoint.",
            instance,
            request_id,
        )
    }

    fn request_too_large(instance: &str, request_id: Option<&Extension<RequestId>>) -> Self {
        Self::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "upload_too_large",
            "Upload too large",
            "The upload exceeds the configured request size limit.",
            instance,
            request_id,
        )
    }

    fn invalid_cursor(instance: &str, request_id: Option<&Extension<RequestId>>) -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "invalid_cursor",
            "Invalid cursor",
            "The pagination cursor is invalid for this collection.",
            instance,
            request_id,
        )
    }

    fn internal(instance: &str, request_id: Option<&Extension<RequestId>>) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "Internal server error",
            "An unexpected error occurred. Use the request ID when contacting support.",
            instance,
            request_id,
        )
    }

    fn task(error: TaskError, instance: &str, request_id: Option<&Extension<RequestId>>) -> Self {
        match error {
            TaskError::NotFound => Self::not_found(instance, request_id),
            _ => Self::internal(instance, request_id),
        }
    }

    fn upload(
        error: UploadError,
        instance: &str,
        request_id: Option<&Extension<RequestId>>,
    ) -> Self {
        match error {
            UploadError::FileTooLarge { .. } | UploadError::RequestTooLarge { .. } => Self::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "upload_too_large",
                "Upload too large",
                "The upload exceeds the configured size limit.",
                instance,
                request_id,
            ),
            UploadError::InvalidDisplayName => Self::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "validation_failed",
                "Validation failed",
                "The attachment filename is invalid.",
                instance,
                request_id,
            ),
            UploadError::BlobStore(BlobStoreError::Io { source, .. })
                if source.kind() == io::ErrorKind::FileTooLarge =>
            {
                Self::request_too_large(instance, request_id)
            }
            UploadError::BlobStore(BlobStoreError::Io { source, .. })
                if source.kind() == io::ErrorKind::InvalidData =>
            {
                Self::invalid_multipart(instance, request_id)
            }
            UploadError::Unauthorized => Self::not_found(instance, request_id),
            _ => Self::internal(instance, request_id),
        }
    }

    fn repository(
        error: AttachmentRepositoryError,
        instance: &str,
        request_id: Option<&Extension<RequestId>>,
    ) -> Self {
        match error {
            AttachmentRepositoryError::NotFound => Self::not_found(instance, request_id),
            AttachmentRepositoryError::Upload(error) => Self::upload(error, instance, request_id),
            AttachmentRepositoryError::Database(_) => Self::internal(instance, request_id),
        }
    }

    fn database_value() -> Self {
        Self::internal("attachment metadata", None)
    }
}

impl From<sqlx::Error> for AttachmentApiError {
    fn from(_: sqlx::Error) -> Self {
        Self::database_value()
    }
}

impl IntoResponse for AttachmentApiError {
    fn into_response(self) -> Response {
        let mut response = (self.status, Json(self.body)).into_response();
        response.headers_mut().insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/problem+json"),
        );
        response
    }
}
