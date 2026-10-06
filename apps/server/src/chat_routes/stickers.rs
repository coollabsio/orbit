//! Custom stickers: the list, upload (one multipart `file` field), delete, and the image.

use axum::Json;
use axum::extract::{Multipart, Path, State};
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::Response;
use serde::Deserialize;
use utoipa::{IntoParams, ToSchema};

use super::images::{self, ImageRules};
use super::{Call, ChatState, RequestIdExtension};
use crate::repositories::chat::{CustomStickerRecord, STICKER_MAX_BYTES};
use crate::task_routes::{ApiError, ApiQuery};

pub(super) const RULES: ImageRules = ImageRules {
    max_bytes: STICKER_MAX_BYTES,
    too_large_code: "sticker_too_large",
    too_large_detail: "The image must be at most 512 KiB.",
    invalid_code: "invalid_sticker",
};

#[derive(Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub(crate) struct ChatStickerQuery {
    /// 2 to 30 characters, without control characters. Trimmed, and stored as typed.
    name: String,
}

#[derive(ToSchema)]
#[allow(dead_code)]
pub(crate) struct ChatStickerUploadBody {
    /// A PNG, JPEG, WebP or GIF image of at most 512 KiB.
    #[schema(value_type = String, format = Binary)]
    file: Vec<u8>,
}

#[derive(ToSchema)]
#[schema(value_type = String, format = Binary)]
#[allow(dead_code)]
pub(crate) struct ChatStickerImage(Vec<u8>);

/// The custom stickers of the workspace, by name.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/chat/stickers", params(("workspace_id" = String, Path)), responses((status = 200, body = Vec<CustomStickerRecord>)))]
pub(crate) async fn list_chat_stickers(
    State(state): State<ChatState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<Vec<CustomStickerRecord>>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    call.read(
        state
            .chat
            .list_stickers(call.workspace_id, call.actor_id)
            .await,
    )
}

/// Adds a custom sticker. Only workspace owners and admins. A workspace has at most 100.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/chat/stickers", params(ChatStickerQuery, ("workspace_id" = String, Path)), request_body(content = ChatStickerUploadBody, content_type = "multipart/form-data"), responses((status = 201, body = CustomStickerRecord)))]
pub(crate) async fn create_chat_sticker(
    State(state): State<ChatState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiQuery(query): ApiQuery<ChatStickerQuery>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<CustomStickerRecord>), ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    // Authorize before the body is read.
    state
        .chat
        .authorize_sticker(call.workspace_id, call.actor_id)
        .await
        .map_err(|error| call.problem(error))?;
    let (image, mime_type) = images::read_upload(&call, &mut multipart, &RULES).await?;
    let (written, _) = state
        .publish(
            call.workspace_id,
            call.actor_id,
            state.chat.create_sticker(
                call.workspace_id,
                call.actor_id,
                &query.name,
                mime_type,
                &image,
            ),
        )
        .await
        .map_err(|error| call.problem(error))?;
    Ok((StatusCode::CREATED, Json(written.value)))
}

/// Deletes a custom sticker. Only workspace owners and admins. Messages that were sent with
/// it keep its id.
#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/chat/stickers/{sticker_id}", params(("workspace_id" = String, Path), ("sticker_id" = String, Path)), responses((status = 204)))]
pub(crate) async fn delete_chat_sticker(
    State(state): State<ChatState>,
    Path((workspace, sticker)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<StatusCode, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let sticker_id = call.id(&sticker)?;
    state
        .publish(
            call.workspace_id,
            call.actor_id,
            state
                .chat
                .delete_sticker(call.workspace_id, call.actor_id, sticker_id),
        )
        .await
        .map_err(|error| call.problem(error))?;
    Ok(StatusCode::NO_CONTENT)
}

/// The image of a custom sticker. It never changes, so browsers keep it.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/chat/stickers/{sticker_id}/image", params(("workspace_id" = String, Path), ("sticker_id" = String, Path)), responses((status = 200, body = ChatStickerImage, content_type = "image/*")))]
pub(crate) async fn get_chat_sticker_image(
    State(state): State<ChatState>,
    Path((workspace, sticker)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Response, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let sticker_id = call.id(&sticker)?;
    let image = state
        .chat
        .sticker_image(call.workspace_id, call.actor_id, sticker_id)
        .await
        .map_err(|error| call.problem(error))?;
    images::response(&call, &image.mime_type, image.bytes)
}
