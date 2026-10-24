//! Mail content (spec §6.5, §7 Content, §10): the view URL, the sandboxed HTML document, inline
//! parts, the image proxy, attachment and `.eml` downloads, and the per-sender image choice.
//!
//! The reading pane frames the document in `<iframe sandbox="allow-scripts allow-popups
//! allow-popups-to-escape-sandbox">` (opaque origin), so the frame sends no Orbit cookie. Its
//! URLs carry a token instead: HMAC-SHA256 under [`ViewKey`] (HKDF-SHA256 of the app key, label
//! `mail-view`) over the session, workspace, message, the load-images choice and the expiry.
//! Every use checks that the session is still live and that its user still reads the message, so
//! signing out or leaving the mailbox ends the URLs at once. A bad, expired or foreign token is
//! `mail_not_found`, like a message the caller does not read.
//!
//! With `ORBIT_MAIL_CONTENT_ORIGIN` the document, its parts and the proxy URLs use that origin;
//! `frame-ancestors` is the app's public origin either way.

use std::collections::HashMap;
use std::sync::Arc;

use axum::Json;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::header::{
    CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_SECURITY_POLICY, CONTENT_TYPE, REFERRER_POLICY,
    X_CONTENT_TYPE_OPTIONS,
};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri};
use axum::response::Response;
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use orbit_mail::{RenderOptions, csp_header, new_nonce, render_document, render_text_document};
use orbit_platform::{Id, TimestampMillis};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use utoipa::{IntoParams, ToSchema};

use super::{Call, MailState, RequestIdExtension, problem};
use crate::export::names::{ascii_file_name, rfc5987};
use crate::repositories::mail::MailError;
use crate::task_routes::{ApiError, ApiJson, ApiQuery};

/// How long a view URL may be opened.
pub const VIEW_URL_TTL_MS: i64 = 5 * 60 * 1000;
/// How long the part and image URLs inside a document work (the reader may scroll for a while).
pub const SUBRESOURCE_TTL_MS: i64 = 60 * 60 * 1000;
/// Longest token accepted.
const MAX_TOKEN_BYTES: usize = 512;
/// Inline part types (the spec's image types; no SVG).
const INLINE_IMAGE_TYPES: [&str; 5] = [
    "image/png",
    "image/jpeg",
    "image/gif",
    "image/webp",
    "image/avif",
];

/// What a signed mail content URL may open.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ViewPurpose {
    /// The HTML document of a message.
    View,
    /// An inline part (`cid:` image) of the message.
    Part,
    /// One remote image through the proxy (the URL is signed with it).
    Image,
}

impl ViewPurpose {
    fn label(self) -> &'static [u8] {
        match self {
            Self::View => b"view",
            Self::Part => b"part",
            Self::Image => b"image",
        }
    }
}

/// The claims of a signed mail content URL.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ViewGrant {
    pub session_id: Id,
    pub workspace_id: Id,
    pub message_id: Id,
    pub load_images: bool,
    pub expires_at: TimestampMillis,
}

impl ViewGrant {
    fn claims(&self) -> String {
        format!(
            "{}.{}.{}.{}.{}",
            self.session_id,
            self.workspace_id,
            self.message_id,
            u8::from(self.load_images),
            self.expires_at.as_millis()
        )
    }

    fn parse(claims: &str) -> Option<Self> {
        let mut fields = claims.split('.');
        let grant = Self {
            session_id: fields.next()?.parse().ok()?,
            workspace_id: fields.next()?.parse().ok()?,
            message_id: fields.next()?.parse().ok()?,
            load_images: match fields.next()? {
                "0" => false,
                "1" => true,
                _ => return None,
            },
            expires_at: TimestampMillis::from_millis(fields.next()?.parse().ok()?),
        };
        fields.next().is_none().then_some(grant)
    }
}

/// The key of mail content URLs, derived from the app key.
#[derive(Clone)]
pub struct ViewKey([u8; 32]);

impl ViewKey {
    #[must_use]
    pub fn new(app_key: &[u8; 32]) -> Self {
        let mut key = [0_u8; 32];
        Hkdf::<Sha256>::new(None, app_key)
            .expand(b"mail-view", &mut key)
            .expect("32 bytes is a valid HKDF-SHA256 length");
        Self(key)
    }

    fn mac(&self, purpose: ViewPurpose, claims: &str, extra: &str) -> Hmac<Sha256> {
        let mut mac =
            <Hmac<Sha256> as Mac>::new_from_slice(&self.0).expect("HMAC takes a key of any length");
        mac.update(purpose.label());
        mac.update(&[0]);
        mac.update(claims.as_bytes());
        mac.update(&[0]);
        mac.update(extra.as_bytes());
        mac
    }

    /// A URL-safe token for `grant`; `extra` is bound too (the image URL for
    /// [`ViewPurpose::Image`]).
    #[must_use]
    pub fn sign(&self, purpose: ViewPurpose, grant: &ViewGrant, extra: &str) -> String {
        let claims = grant.claims();
        let tag = self.mac(purpose, &claims, extra).finalize().into_bytes();
        format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(claims),
            URL_SAFE_NO_PAD.encode(tag)
        )
    }

    /// The grant of a token signed for `purpose` and `extra` that has not expired at `now`.
    #[must_use]
    pub fn verify(
        &self,
        purpose: ViewPurpose,
        token: &str,
        extra: &str,
        now: TimestampMillis,
    ) -> Option<ViewGrant> {
        if token.len() > MAX_TOKEN_BYTES {
            return None;
        }
        let (claims, tag) = token.split_once('.')?;
        let claims = String::from_utf8(URL_SAFE_NO_PAD.decode(claims).ok()?).ok()?;
        let tag = URL_SAFE_NO_PAD.decode(tag).ok()?;
        self.mac(purpose, &claims, extra).verify_slice(&tag).ok()?;
        ViewGrant::parse(&claims).filter(|grant| grant.expires_at > now)
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct MailViewUrlBody {
    /// Rewrite remote images to the image proxy (**Load images**); otherwise they are removed.
    load_images: bool,
}

/// A short-lived URL of the message's view document, bound to the caller's session.
#[derive(Serialize, ToSchema)]
pub(crate) struct MailViewUrl {
    /// Frame it in a sandboxed iframe (`allow-scripts allow-popups
    /// allow-popups-to-escape-sandbox`, never `allow-same-origin`).
    url: String,
    #[schema(value_type = String, format = DateTime)]
    expires_at: TimestampMillis,
}

#[derive(Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub(crate) struct MailAttachmentQuery {
    /// `1`: show an allowed image type (PNG, JPEG, GIF, WebP, AVIF) inline; anything else still
    /// downloads.
    inline: Option<String>,
}

#[derive(Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub(crate) struct MailImageQuery {
    /// The remote image URL.
    u: String,
    /// The signature from the view document.
    sig: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct MailSenderPrefBody {
    /// The sender address (any case).
    address: String,
    /// Load remote images of this sender's mail without asking (only while DMARC passes).
    load_images: bool,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct MailSenderPref {
    /// Lower case.
    address: String,
    load_images: bool,
}

/// Binary bodies in the API description.
#[derive(ToSchema)]
#[schema(value_type = String, format = Binary)]
#[allow(dead_code)]
struct MailBytes(Vec<u8>);

/// The view document (HTML).
#[derive(ToSchema)]
#[schema(value_type = String)]
#[allow(dead_code)]
struct MailDocument(String);

fn not_found(instance: &str, request_id: &RequestIdExtension) -> ApiError {
    problem(MailError::NotFound, instance, request_id.as_ref())
}

/// The user of a token's session, when it is still live.
async fn token_user(
    state: &MailState,
    grant: &ViewGrant,
    instance: &str,
    request_id: &RequestIdExtension,
) -> Result<Id, ApiError> {
    match state
        .identity
        .session_user(grant.session_id, TimestampMillis::now())
        .await
    {
        Ok(Some(user)) => Ok(user),
        Ok(None) => Err(not_found(instance, request_id)),
        Err(error) => {
            tracing::error!(%error, "mail content session");
            Err(ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Internal server error",
                "An unexpected error occurred. Use the request ID when contacting support.",
                instance,
                request_id.as_ref(),
            ))
        }
    }
}

fn header(value: &str) -> Option<HeaderValue> {
    HeaderValue::from_str(value).ok()
}

/// Bytes with the download safety headers.
fn bytes_response(bytes: Vec<u8>, content_type: &str, disposition: Option<&str>) -> Response {
    let mut response = Response::new(Body::from(bytes));
    let headers = response.headers_mut();
    if let Some(value) = header(content_type) {
        headers.insert(CONTENT_TYPE, value);
    }
    if let Some(value) = disposition.and_then(header) {
        headers.insert(CONTENT_DISPOSITION, value);
    }
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    response
}

fn disposition(kind: &str, name: &str) -> String {
    format!(
        "{kind}; filename=\"{}\"; filename*={}",
        ascii_file_name(name),
        rfc5987(name)
    )
}

/// A short-lived URL of a message's view document (spec §6.5 step 1).
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/mail/messages/{message_id}/view-url", params(("workspace_id" = String, Path), ("message_id" = String, Path)), request_body = MailViewUrlBody, responses((status = 200, body = MailViewUrl)))]
pub(crate) async fn create_mail_view_url(
    State(state): State<MailState>,
    Path((workspace, message)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<MailViewUrlBody>,
) -> Result<Json<MailViewUrl>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let message_id = call.id(&message)?;
    state
        .mail
        .check_message_reader(call.workspace_id, call.actor_id, message_id)
        .await
        .map_err(|error| call.problem(error))?;
    let expires_at =
        TimestampMillis::from_millis(TimestampMillis::now().as_millis() + VIEW_URL_TTL_MS);
    let token = state.view_key.sign(
        ViewPurpose::View,
        &ViewGrant {
            session_id: call.session_id,
            workspace_id: call.workspace_id,
            message_id,
            load_images: body.load_images,
            expires_at,
        },
        "",
    );
    Ok(Json(MailViewUrl {
        url: format!("{}/api/v1/mail/content/{token}", state.content_base()),
        expires_at,
    }))
}

/// The sanitized view document of a message (spec §6.5): served with a `sandbox` CSP, a nonce
/// for Orbit's height script only, `no-referrer` and `no-store`.
#[utoipa::path(get, path = "/api/v1/mail/content/{token}", params(("token" = String, Path, description = "From `view-url`")), responses((status = 200, body = MailDocument, content_type = "text/html")))]
pub(crate) async fn get_mail_content(
    State(state): State<MailState>,
    Path(token): Path<String>,
    headers: HeaderMap,
    request_id: RequestIdExtension,
) -> Result<Response, ApiError> {
    let instance = "/api/v1/mail/content";
    if !state.serves_content(&headers) {
        return Err(not_found(instance, &request_id));
    }
    let now = TimestampMillis::now();
    let grant = state
        .view_key
        .verify(ViewPurpose::View, &token, "", now)
        .ok_or_else(|| not_found(instance, &request_id))?;
    let user = token_user(&state, &grant, instance, &request_id).await?;
    let content = state
        .mail
        .message_content(grant.workspace_id, user, grant.message_id)
        .await
        .map_err(|error| problem(error, instance, request_id.as_ref()))?;

    let base = state.content_base().to_owned();
    let inner = ViewGrant {
        expires_at: TimestampMillis::from_millis(now.as_millis() + SUBRESOURCE_TTL_MS),
        ..grant.clone()
    };
    let part_token = state.view_key.sign(ViewPurpose::Part, &inner, "");
    let inline_parts: HashMap<String, Id> = content
        .parts
        .iter()
        .filter(|part| INLINE_IMAGE_TYPES.contains(&part.sniffed_type.as_str()))
        .filter_map(|part| Some((part.content_id.as_ref()?.to_lowercase(), part.id)))
        .collect();
    let cid_base = base.clone();
    let cid_url = Arc::new(move |content_id: &str| {
        inline_parts
            .get(&content_id.to_lowercase())
            .map(|id| format!("{cid_base}/api/v1/mail/parts/{part_token}/{id}"))
    });
    let proxy_url = grant.load_images.then(|| {
        let key = state.view_key.clone();
        let base = base.clone();
        Arc::new(move |url: &str| {
            let sig = key.sign(ViewPurpose::Image, &inner, url);
            let encoded: String = url::form_urlencoded::byte_serialize(url.as_bytes()).collect();
            format!("{base}/api/v1/mail/image?u={encoded}&sig={sig}")
        }) as orbit_mail::ProxyUrl
    });
    let nonce = new_nonce();
    let html = content
        .html_part
        .and_then(|part| orbit_mail::part_bytes(&content.raw, part))
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
    drop(content.raw);
    let document = match html {
        Some(html) => {
            render_document(
                &html,
                &RenderOptions {
                    cid_url,
                    proxy_url,
                    nonce: &nonce,
                },
            )
            .html
        }
        None => render_text_document(&content.body_text, &nonce),
    };

    let images = if base.is_empty() { "'self'" } else { &base };
    let ancestors = if state.public_origin.is_empty() {
        "'self'"
    } else {
        &state.public_origin
    };
    let csp = csp_header(&nonce, images, ancestors);
    let mut response = Response::new(Body::from(document));
    let headers = response.headers_mut();
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    headers.insert(
        CONTENT_SECURITY_POLICY,
        header(&csp).ok_or_else(|| not_found(instance, &request_id))?,
    );
    headers.insert(REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    Ok(response)
}

/// An inline image part of a message (`cid:`), for its view document. Only the allowed image
/// types; anything else is `mail_not_found`.
#[utoipa::path(get, path = "/api/v1/mail/parts/{token}/{attachment_id}", params(("token" = String, Path, description = "From the view document"), ("attachment_id" = String, Path)), responses((status = 200, body = MailBytes, content_type = "application/octet-stream")))]
pub(crate) async fn get_mail_part(
    State(state): State<MailState>,
    Path((token, attachment)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: RequestIdExtension,
) -> Result<Response, ApiError> {
    let instance = "/api/v1/mail/parts";
    if !state.serves_content(&headers) {
        return Err(not_found(instance, &request_id));
    }
    let grant = state
        .view_key
        .verify(ViewPurpose::Part, &token, "", TimestampMillis::now())
        .ok_or_else(|| not_found(instance, &request_id))?;
    let attachment_id: Id = attachment
        .parse()
        .map_err(|_| not_found(instance, &request_id))?;
    let user = token_user(&state, &grant, instance, &request_id).await?;
    let content = state
        .mail
        .attachment_content(
            grant.workspace_id,
            user,
            attachment_id,
            Some(grant.message_id),
        )
        .await
        .map_err(|error| problem(error, instance, request_id.as_ref()))?;
    let content_type =
        orbit_mail::sniff_image(&content.bytes).ok_or_else(|| not_found(instance, &request_id))?;
    Ok(bytes_response(content.bytes, content_type, Some("inline")))
}

/// A remote image of a message through Orbit (after **Load images**): signed by the view
/// document, bound to the session; private targets are refused on every hop; image types only.
#[utoipa::path(get, path = "/api/v1/mail/image", params(MailImageQuery), responses((status = 200, body = MailBytes, content_type = "application/octet-stream")))]
pub(crate) async fn get_mail_image(
    State(state): State<MailState>,
    headers: HeaderMap,
    request_id: RequestIdExtension,
    ApiQuery(query): ApiQuery<MailImageQuery>,
) -> Result<Response, ApiError> {
    let instance = "/api/v1/mail/image";
    if !state.serves_content(&headers) {
        return Err(not_found(instance, &request_id));
    }
    let grant = state
        .view_key
        .verify(
            ViewPurpose::Image,
            &query.sig,
            &query.u,
            TimestampMillis::now(),
        )
        .filter(|grant| grant.load_images)
        .ok_or_else(|| not_found(instance, &request_id))?;
    let user = token_user(&state, &grant, instance, &request_id).await?;
    state
        .mail
        .check_message_reader(grant.workspace_id, user, grant.message_id)
        .await
        .map_err(|error| problem(error, instance, request_id.as_ref()))?;
    match state.image_proxy.fetch(&query.u).await {
        Ok(image) => Ok(bytes_response(
            image.bytes,
            image.content_type,
            Some("inline"),
        )),
        Err(error) => {
            tracing::debug!(%error, "mail image proxy");
            Err(ApiError::new(
                StatusCode::BAD_GATEWAY,
                "mail_image_unavailable",
                "Image unavailable",
                "The image could not be loaded.",
                instance,
                request_id.as_ref(),
            ))
        }
    }
}

/// An attachment of a message: always a download with `nosniff`, except an allowed image type
/// with `inline=1`.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/mail/attachments/{attachment_id}", params(MailAttachmentQuery, ("workspace_id" = String, Path), ("attachment_id" = String, Path)), responses((status = 200, body = MailBytes, content_type = "application/octet-stream")))]
pub(crate) async fn download_mail_attachment(
    State(state): State<MailState>,
    Path((workspace, attachment)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiQuery(query): ApiQuery<MailAttachmentQuery>,
) -> Result<Response, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let attachment_id = call.id(&attachment)?;
    let content = state
        .mail
        .attachment_content(call.workspace_id, call.actor_id, attachment_id, None)
        .await
        .map_err(|error| call.problem(error))?;
    let image = orbit_mail::sniff_image(&content.bytes);
    let kind = if image.is_some() && query.inline.as_deref() == Some("1") {
        "inline"
    } else {
        "attachment"
    };
    let name = content
        .part
        .filename
        .clone()
        .unwrap_or_else(|| "attachment".to_owned());
    Ok(bytes_response(
        content.bytes,
        image.unwrap_or("application/octet-stream"),
        Some(&disposition(kind, &name)),
    ))
}

/// The original message as received (`.eml`).
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/mail/messages/{message_id}/raw", params(("workspace_id" = String, Path), ("message_id" = String, Path)), responses((status = 200, body = MailBytes, content_type = "message/rfc822")))]
pub(crate) async fn download_mail_message(
    State(state): State<MailState>,
    Path((workspace, message)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Response, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let message_id = call.id(&message)?;
    let (raw, subject) = state
        .mail
        .message_raw(call.workspace_id, call.actor_id, message_id)
        .await
        .map_err(|error| call.problem(error))?;
    let subject: String = subject.trim().chars().take(80).collect();
    let name = if subject.is_empty() {
        "message.eml".to_owned()
    } else {
        format!("{subject}.eml")
    };
    Ok(bytes_response(
        raw,
        "message/rfc822",
        Some(&disposition("attachment", &name)),
    ))
}

/// Always (or no longer) load remote images in mail from one sender, for the caller. The reading
/// pane offers it only when DMARC passed, and `load_images_default` needs DMARC to pass too.
#[utoipa::path(put, path = "/api/v1/workspaces/{workspace_id}/mail/sender-prefs", params(("workspace_id" = String, Path)), request_body = MailSenderPrefBody, responses((status = 200, body = MailSenderPref)))]
pub(crate) async fn set_mail_sender_pref(
    State(state): State<MailState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<MailSenderPrefBody>,
) -> Result<Json<MailSenderPref>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let address = state
        .mail
        .set_sender_pref(
            call.workspace_id,
            call.actor_id,
            &body.address,
            body.load_images,
            TimestampMillis::now(),
        )
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(MailSenderPref {
        address,
        load_images: body.load_images,
    }))
}
