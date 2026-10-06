//! What the custom emoji and the custom stickers share: an image upload (one multipart `file`
//! field) that is checked by its magic bytes, and the answer that serves a stored image.

use axum::body::Bytes;
use axum::extract::Multipart;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};

use super::Call;
use crate::repositories::chat::ChatError;
use crate::task_routes::ApiError;

/// The limit of one kind of image and the problems of its upload.
pub(super) struct ImageRules {
    pub max_bytes: usize,
    pub too_large_code: &'static str,
    pub too_large_detail: &'static str,
    pub invalid_code: &'static str,
}

impl ImageRules {
    /// The request limit of an upload: the image and the multipart framing around it.
    pub(super) const fn request_bytes(&self) -> usize {
        self.max_bytes + 16 * 1024
    }
}

/// The image type by its magic bytes; anything else (SVG included) is refused.
fn mime_type(bytes: &[u8]) -> Option<&'static str> {
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

/// Reads the `file` field of an upload: the bytes of the image and its type.
pub(super) async fn read_upload(
    call: &Call<'_>,
    multipart: &mut Multipart,
    rules: &ImageRules,
) -> Result<(Bytes, &'static str), ApiError> {
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
            rules.too_large_code,
            "Image too large",
            rules.too_large_detail,
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
    if image.len() > rules.max_bytes {
        return Err(too_large());
    }
    let mime_type = mime_type(&image).ok_or_else(|| {
        ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            rules.invalid_code,
            "Invalid image",
            "Upload a PNG, JPEG, WebP or GIF image.",
            &call.instance,
            call.request_id,
        )
    })?;
    Ok((image, mime_type))
}

/// A stored image. An image that is replaced has a new id, so a new URL: browsers keep it.
pub(super) fn response(
    call: &Call<'_>,
    mime_type: &str,
    bytes: Vec<u8>,
) -> Result<Response, ApiError> {
    let content_type = HeaderValue::from_str(mime_type).map_err(|_| {
        call.problem(ChatError::Unavailable(sqlx::Error::Protocol(
            "the stored image type is not a header value".to_owned(),
        )))
    })?;
    let mut response = bytes.into_response();
    let headers = response.headers_mut();
    headers.insert(CONTENT_TYPE, content_type);
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=31536000, immutable"),
    );
    Ok(response)
}
