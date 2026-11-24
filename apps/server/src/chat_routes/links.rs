//! Previews of the URLs in messages, and outside images through Orbit.

use axum::Json;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::header::{
    CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_SECURITY_POLICY, CONTENT_TYPE,
    X_CONTENT_TYPE_OPTIONS,
};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri};
use axum::response::Response;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use super::{Call, ChatState, RequestIdExtension};
use crate::link_preview::LinkPreview;
use crate::task_routes::{ApiError, ApiQuery, authenticate_session};

#[derive(Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub(crate) struct ChatLinkPreviewQuery {
    /// An absolute `http(s)` URL.
    url: String,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct ChatLinkPreview {
    /// `null`: the URL has nothing to show, or the server may not or cannot read it.
    preview: Option<LinkPreview>,
}

/// What the URL of a message shows as a card. The server reads the page; the answer is kept for a while.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/chat/link-preview", params(ChatLinkPreviewQuery, ("workspace_id" = String, Path)), responses((status = 200, body = ChatLinkPreview)))]
pub(crate) async fn get_chat_link_preview(
    State(state): State<ChatState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiQuery(query): ApiQuery<ChatLinkPreviewQuery>,
) -> Result<Json<ChatLinkPreview>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    state
        .chat
        .require_member(call.workspace_id, call.actor_id)
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(ChatLinkPreview {
        preview: state.link_previews.preview(&query.url).await,
    }))
}

#[derive(Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub(crate) struct ContentImageQuery {
    /// An absolute `http(s)` image URL on another server.
    u: String,
}

/// Image bytes in the API description.
#[derive(ToSchema)]
#[schema(value_type = String, format = Binary)]
#[allow(dead_code)]
struct ContentImageBytes(Vec<u8>);

/// An image on another server, for a signed-in user: the images of messages, comments, pages and link
/// cards load through Orbit, so the other server sees Orbit's address, not the reader's, and gets no
/// cookie and no `Referer`. Nothing is stored. The fetch rules (no private targets on any hop, 5 MB,
/// 10 s, PNG/JPEG/GIF/WebP/AVIF only) are the mail image proxy's, with at most 6 fetches at once
/// for one user.
#[utoipa::path(get, path = "/api/v1/image", params(ContentImageQuery), responses((status = 200, body = ContentImageBytes, content_type = "application/octet-stream"), (status = 401, description = "authentication_required"), (status = 502, description = "image_unavailable: not a public http(s) URL, not an allowed image, too large, or the other server failed")))]
pub(crate) async fn get_content_image(
    State(state): State<ChatState>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiQuery(query): ApiQuery<ContentImageQuery>,
) -> Result<Response, ApiError> {
    let instance = uri.path();
    let session = authenticate_session(
        &state.identity,
        state.cookie_mode,
        &headers,
        instance,
        request_id.as_ref(),
    )
    .await?;
    // One answer for every failure: a 404 for a private target would tell a user which host names
    // resolve inside Orbit's network.
    let image = state
        .image_proxy
        .fetch_for(session.user.id, &query.u)
        .await
        .map_err(|error| {
            tracing::debug!(%error, "content image proxy");
            ApiError::new(
                StatusCode::BAD_GATEWAY,
                "image_unavailable",
                "Image unavailable",
                "The image could not be loaded.",
                instance,
                request_id.as_ref(),
            )
        })?;
    let mut response = Response::new(Body::from(image.bytes));
    let headers = response.headers_mut();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static(image.content_type));
    headers.insert(CONTENT_DISPOSITION, HeaderValue::from_static("inline"));
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(
        CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'; sandbox"),
    );
    // The browser keeps the image for a day, so the other server sees fewer of the reads.
    headers.insert(
        CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=86400"),
    );
    Ok(response)
}
