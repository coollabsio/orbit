//! Upload, remove and serve a project's icon image (issue #146).

use axum::Json;
use axum::extract::{DefaultBodyLimit, Extension, Multipart, Path, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{MethodRouter, get};
use orbit_platform::{RequestId, TimestampMillis, UploadError, UploadService};
use utoipa::ToSchema;

use crate::repositories::tasks::{ProjectIconError, ProjectRecord};
use crate::task_routes::{ApiError, TaskState, parse_id, request_id_value, scope, task_problem};

/// Largest accepted icon. The apps upload a 256 px square, far below this.
const MAX_ICON_BYTES: usize = 512 * 1024;

const INSTANCE: &str = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/icon";

pub(crate) fn routes() -> MethodRouter<TaskState> {
    get(project_icon)
        .put(upload_project_icon)
        .delete(remove_project_icon)
        // the image plus the multipart framing
        .layer(DefaultBodyLimit::max(MAX_ICON_BYTES + 16 * 1024))
}

#[derive(ToSchema)]
#[allow(dead_code)]
struct ProjectIconUploadBody {
    /// A PNG, JPEG or WebP image of at most 512 KiB.
    #[schema(value_type = String, format = Binary)]
    file: Vec<u8>,
}

#[derive(ToSchema)]
#[schema(value_type = String, format = Binary)]
#[allow(dead_code)]
struct ProjectIconImage(Vec<u8>);

/// Sets the project's icon, shown in place of its colour. Only a manager of the project may change it.
#[utoipa::path(put, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/icon", params(("workspace_id" = String, Path), ("project_id" = String, Path)), request_body(content = ProjectIconUploadBody, content_type = "multipart/form-data"), responses((status = 200, body = ProjectRecord), (status = 400, description = "invalid_multipart", body = crate::task_routes::ProblemBody, content_type = "application/problem+json"), (status = 403, description = "forbidden", body = crate::task_routes::ProblemBody, content_type = "application/problem+json"), (status = 404, description = "task_resource_not_found", body = crate::task_routes::ProblemBody, content_type = "application/problem+json"), (status = 413, description = "icon_too_large", body = crate::task_routes::ProblemBody, content_type = "application/problem+json"), (status = 422, description = "invalid_icon", body = crate::task_routes::ProblemBody, content_type = "application/problem+json")))]
pub(crate) async fn upload_project_icon(
    State(state): State<TaskState>,
    Path((workspace, project)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    mut multipart: Multipart,
) -> Result<Json<ProjectRecord>, ApiError> {
    let request_id = request_id.as_ref();
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, INSTANCE, request_id).await?;
    let project_id = parse_id(&project, INSTANCE, request_id)?;
    let invalid_multipart = || {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_multipart",
            "Invalid multipart upload",
            "Send the image as the multipart field \"file\".",
            INSTANCE,
            request_id,
        )
    };
    let too_large = || {
        ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "icon_too_large",
            "Image too large",
            "The image must be at most 512 KiB.",
            INSTANCE,
            request_id,
        )
    };
    let read_error = |error: axum::extract::multipart::MultipartError| {
        if error.status() == StatusCode::PAYLOAD_TOO_LARGE {
            too_large()
        } else {
            invalid_multipart()
        }
    };
    let mut image = None;
    while let Some(field) = multipart.next_field().await.map_err(read_error)? {
        if field.name() == Some("file") {
            image = Some(field.bytes().await.map_err(read_error)?);
        }
    }
    let image = image.ok_or_else(invalid_multipart)?;
    if image.is_empty() {
        return Err(invalid_multipart());
    }
    if image.len() > MAX_ICON_BYTES {
        return Err(too_large());
    }
    let mime_type = crate::auth_routes::avatar_mime_type(&image).ok_or_else(|| {
        ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_icon",
            "Invalid image",
            "Upload a PNG, JPEG or WebP image.",
            INSTANCE,
            request_id,
        )
    })?;
    state
        .tasks
        .set_project_icon(
            uploads(&state, request_id)?,
            workspace_id,
            project_id,
            actor_id,
            mime_type,
            &image,
            request_id_value(request_id),
            TimestampMillis::now(),
        )
        .await
        .map(Json)
        .map_err(|error| icon_problem(error, request_id))
}

/// Removes the project's icon, so the project shows its colour again.
#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/icon", params(("workspace_id" = String, Path), ("project_id" = String, Path)), responses((status = 200, body = ProjectRecord), (status = 403, description = "forbidden", body = crate::task_routes::ProblemBody, content_type = "application/problem+json"), (status = 404, description = "task_resource_not_found", body = crate::task_routes::ProblemBody, content_type = "application/problem+json")))]
pub(crate) async fn remove_project_icon(
    State(state): State<TaskState>,
    Path((workspace, project)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<ProjectRecord>, ApiError> {
    let request_id = request_id.as_ref();
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, INSTANCE, request_id).await?;
    let project_id = parse_id(&project, INSTANCE, request_id)?;
    state
        .tasks
        .remove_project_icon(
            workspace_id,
            project_id,
            actor_id,
            request_id_value(request_id),
            TimestampMillis::now(),
        )
        .await
        .map(Json)
        .map_err(|error| icon_problem(error, request_id))
}

/// The project's icon image, for everyone who sees the project. Use the `icon_url` of the project: it changes with
/// every new image, so the response can be cached for good.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/projects/{project_id}/icon", params(("workspace_id" = String, Path), ("project_id" = String, Path)), responses((status = 200, body = ProjectIconImage, content_type = "image/*"), (status = 404, description = "task_resource_not_found", body = crate::task_routes::ProblemBody, content_type = "application/problem+json")))]
pub(crate) async fn project_icon(
    State(state): State<TaskState>,
    Path((workspace, project)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Response, ApiError> {
    let request_id = request_id.as_ref();
    let (workspace_id, actor_id) =
        scope(&state, &headers, &workspace, INSTANCE, request_id).await?;
    let project_id = parse_id(&project, INSTANCE, request_id)?;
    // A project without an icon and a project the caller cannot see look the same.
    let icon = state
        .tasks
        .project_icon(
            uploads(&state, request_id)?,
            workspace_id,
            project_id,
            actor_id,
        )
        .await
        .map_err(|error| icon_problem(error, request_id))?
        .ok_or_else(|| {
            task_problem(
                crate::repositories::tasks::TaskError::NotFound,
                INSTANCE,
                request_id,
            )
        })?;
    let content_type = HeaderValue::from_str(&icon.mime_type)
        .map_err(|error| internal(request_id).cause(error))?;
    let mut response = icon.bytes.into_response();
    let response_headers = response.headers_mut();
    response_headers.insert(CONTENT_TYPE, content_type);
    response_headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    // the URL carries the upload time, so a new icon is a new URL
    response_headers.insert(
        CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=31536000, immutable"),
    );
    Ok(response)
}

fn uploads<'a>(
    state: &'a TaskState,
    request_id: Option<&Extension<RequestId>>,
) -> Result<&'a UploadService, ApiError> {
    state
        .uploads
        .as_ref()
        .ok_or_else(|| internal(request_id).cause("upload storage not configured"))
}

fn icon_problem(error: ProjectIconError, request_id: Option<&Extension<RequestId>>) -> ApiError {
    match error {
        ProjectIconError::Task(error) => task_problem(error, INSTANCE, request_id),
        ProjectIconError::Upload(UploadError::StorageQuotaExceeded { .. }) => {
            crate::attachment_routes::storage_quota_exceeded(INSTANCE, request_id)
        }
        ProjectIconError::Upload(error) => internal(request_id).cause(error),
    }
}

fn internal(request_id: Option<&Extension<RequestId>>) -> ApiError {
    ApiError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal_error",
        "Internal server error",
        "An unexpected error occurred. Use the request ID when contacting support.",
        INSTANCE,
        request_id,
    )
}
