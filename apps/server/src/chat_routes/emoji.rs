//! Custom emoji: the list, upload (one multipart `file` field), delete, and the image.

use axum::Json;
use axum::extract::{Multipart, Path, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use utoipa::{IntoParams, ToSchema};

use super::{Call, ChatState, RequestIdExtension};
use crate::repositories::chat::{ChatError, CustomEmojiRecord, EMOJI_MAX_BYTES};
use crate::task_routes::{ApiError, ApiQuery};

/// The request limit of an upload: the image and the multipart framing around it.
pub(super) const UPLOAD_REQUEST_BYTES: usize = EMOJI_MAX_BYTES + 16 * 1024;

/// The image type by its magic bytes; anything else (SVG included) is refused.
fn emoji_mime_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else {
        None
    }
}

#[derive(Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub(crate) struct ChatEmojiQuery {
    /// 2 to 32 characters of `a-z 0-9 _`. Upper case letters are stored in lower case.
    name: String,
}

#[derive(ToSchema)]
#[allow(dead_code)]
pub(crate) struct ChatEmojiUploadBody {
    /// A PNG, JPEG, WebP or GIF image of at most 256 KiB.
    #[schema(value_type = String, format = Binary)]
    file: Vec<u8>,
}

#[derive(ToSchema)]
#[schema(value_type = String, format = Binary)]
#[allow(dead_code)]
pub(crate) struct ChatEmojiImage(Vec<u8>);

/// The custom emoji of the workspace, by name.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/chat/emoji", params(("workspace_id" = String, Path)), responses((status = 200, body = Vec<CustomEmojiRecord>)))]
pub(crate) async fn list_chat_emoji(
    State(state): State<ChatState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<Vec<CustomEmojiRecord>>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    call.read(
        state
            .chat
            .list_emoji(call.workspace_id, call.actor_id)
            .await,
    )
}

/// Adds a custom emoji. Only workspace owners and admins. A workspace has at most 200.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/chat/emoji", params(ChatEmojiQuery, ("workspace_id" = String, Path)), request_body(content = ChatEmojiUploadBody, content_type = "multipart/form-data"), responses((status = 201, body = CustomEmojiRecord)))]
pub(crate) async fn create_chat_emoji(
    State(state): State<ChatState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiQuery(query): ApiQuery<ChatEmojiQuery>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<CustomEmojiRecord>), ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    // Authorize before the body is read.
    state
        .chat
        .authorize_emoji(call.workspace_id, call.actor_id)
        .await
        .map_err(|error| call.problem(error))?;
    let invalid_multipart = || {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_multipart",
            "Invalid multipart upload",
            "Send the image as the multipart field \"file\".",
            &call.instance,
            call.request_id,
        )
    };
    let too_large = || {
        ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "emoji_too_large",
            "Image too large",
            "The image must be at most 256 KiB.",
            &call.instance,
            call.request_id,
        )
    };
    let mut image = None;
    while let Some(field) = multipart.next_field().await.map_err(|error| {
        if error.status() == StatusCode::PAYLOAD_TOO_LARGE {
            too_large()
        } else {
            invalid_multipart()
        }
    })? {
        if field.name() == Some("file") {
            let bytes = field.bytes().await.map_err(|error| {
                if error.status() == StatusCode::PAYLOAD_TOO_LARGE {
                    too_large()
                } else {
                    invalid_multipart()
                }
            })?;
            image = Some(bytes);
        }
    }
    let image = image.ok_or_else(invalid_multipart)?;
    if image.len() > EMOJI_MAX_BYTES {
        return Err(too_large());
    }
    let mime_type = emoji_mime_type(&image).ok_or_else(|| {
        ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_emoji",
            "Invalid image",
            "Upload a PNG, JPEG, WebP or GIF image.",
            &call.instance,
            call.request_id,
        )
    })?;
    let (written, _) = state
        .publish(
            call.workspace_id,
            call.actor_id,
            state.chat.create_emoji(
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

/// Deletes a custom emoji. Only workspace owners and admins. Messages and reactions that name
/// it keep their `:name:` text.
#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/chat/emoji/{emoji_id}", params(("workspace_id" = String, Path), ("emoji_id" = String, Path)), responses((status = 204)))]
pub(crate) async fn delete_chat_emoji(
    State(state): State<ChatState>,
    Path((workspace, emoji)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<StatusCode, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let emoji_id = call.id(&emoji)?;
    state
        .publish(
            call.workspace_id,
            call.actor_id,
            state
                .chat
                .delete_emoji(call.workspace_id, call.actor_id, emoji_id),
        )
        .await
        .map_err(|error| call.problem(error))?;
    Ok(StatusCode::NO_CONTENT)
}

/// The image of a custom emoji. It never changes, so browsers keep it.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/chat/emoji/{emoji_id}/image", params(("workspace_id" = String, Path), ("emoji_id" = String, Path)), responses((status = 200, body = ChatEmojiImage, content_type = "image/*")))]
pub(crate) async fn get_chat_emoji_image(
    State(state): State<ChatState>,
    Path((workspace, emoji)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Response, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let emoji_id = call.id(&emoji)?;
    let image = state
        .chat
        .emoji_image(call.workspace_id, call.actor_id, emoji_id)
        .await
        .map_err(|error| call.problem(error))?;
    let content_type = HeaderValue::from_str(&image.mime_type).map_err(|_| {
        call.problem(ChatError::Unavailable(sqlx::Error::Protocol(
            "the stored emoji type is not a header value".to_owned(),
        )))
    })?;
    let mut response = image.bytes.into_response();
    let response_headers = response.headers_mut();
    response_headers.insert(CONTENT_TYPE, content_type);
    response_headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    // an emoji that is replaced is a new id, so a new URL
    response_headers.insert(
        CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=31536000, immutable"),
    );
    Ok(response)
}
