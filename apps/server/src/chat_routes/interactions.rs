//! Clicks on an integration's buttons, and the settings that make them work. A click is sent at
//! once, signed as Slack signs it, to the webhook's request URL: one try, 3 seconds, no redirects.
//! The integration answers later through the click's `response_url` (`hooks.rs`).

use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, Uri};
use hmac::{Hmac, Mac};
use orbit_platform::{PublicResolver, is_public_ip};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use tokio::sync::Semaphore;
use url::{Host, Url};
use utoipa::ToSchema;

use super::{Call, ChatState, RequestIdExtension};
use crate::repositories::chat::{Click, DeliveryRecord, WebhookRecord};
use crate::task_routes::{ApiError, ApiJson};

const TIMEOUT: Duration = Duration::from_secs(3);
/// Clicks sent at the same time by one server. More wait.
const MAX_SENDS: usize = 16;
const USER_AGENT: &str = "Orbit-Interactivity/1.0 (+https://orbit.dev)";

/// Sends clicks. Outside development it reaches public addresses only: the resolver drops the
/// others, and an IP address in a URL is checked here.
pub struct ClickSender {
    client: reqwest::Client,
    sends: Semaphore,
    development: bool,
}

impl ClickSender {
    #[must_use]
    pub fn new(development: bool) -> Self {
        let mut builder = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(TIMEOUT)
            .connect_timeout(TIMEOUT)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none());
        if !development {
            builder = builder.dns_resolver(Arc::new(PublicResolver));
        }
        Self {
            client: builder.build().expect("the click client builds"),
            sends: Semaphore::new(MAX_SENDS),
            development,
        }
    }

    /// Sends one click as a form with the JSON in `payload`, as Slack does.
    async fn send(&self, click: &Click) -> Result<u16, &'static str> {
        let body = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("payload", &click.payload.to_string())
            .finish();
        self.post_signed(
            &click.request_url,
            &click.signing_secret,
            "application/x-www-form-urlencoded",
            body,
        )
        .await
    }

    /// Posts a body signed with `signing_secret` to a request URL. `Ok(status)` for any answer;
    /// `Err(reason)` when there was none.
    pub(crate) async fn post_signed(
        &self,
        request_url: &str,
        signing_secret: &str,
        content_type: &'static str,
        body: String,
    ) -> Result<u16, &'static str> {
        let url = Url::parse(request_url).map_err(|_| "invalid_url")?;
        if !sendable(&url, self.development) {
            return Err("blocked_address");
        }
        let _permit = self.sends.acquire().await.map_err(|_| "unavailable")?;
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .to_string();
        let signature = sign(signing_secret, &timestamp, &body);
        let response = self
            .client
            .post(url)
            .header("Content-Type", content_type)
            .header("X-Slack-Request-Timestamp", &timestamp)
            .header("X-Slack-Signature", &signature)
            .header("X-Orbit-Request-Timestamp", &timestamp)
            .header("X-Orbit-Signature", &signature)
            .body(body)
            .send()
            .await
            .map_err(|error| {
                if error.is_timeout() {
                    "timeout"
                } else if error.is_connect() {
                    "unreachable"
                } else {
                    "failed"
                }
            })?;
        Ok(response.status().as_u16())
    }
}

/// Whether an outgoing request may go to `url`: in development anywhere, else https to a public
/// IP address or a name (the client's `PublicResolver` checks the name's addresses).
pub(crate) fn sendable(url: &Url, development: bool) -> bool {
    if development {
        return true;
    }
    url.scheme() == "https"
        && match url.host() {
            Some(Host::Ipv4(ip)) => is_public_ip(IpAddr::V4(ip)),
            Some(Host::Ipv6(ip)) => is_public_ip(IpAddr::V6(ip)),
            Some(Host::Domain(_)) => true,
            None => false,
        }
}

/// Slack's request signature: `v0=` and the hex HMAC-SHA256 of `v0:{timestamp}:{body}`.
fn sign(secret: &str, timestamp: &str, body: &str) -> String {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC takes a key of any length");
    mac.update(format!("v0:{timestamp}:{body}").as_bytes());
    let digest = mac.finalize().into_bytes();
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("v0={hex}")
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChatActionBody {
    /// The `block_id` of the block with the button.
    block_id: String,
    /// The button's `action_id`.
    action_id: String,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct ChatActionResult {
    ok: bool,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChatInteractivityBody {
    /// Where clicks go: https (http and local hosts in development). Null turns clicks and
    /// events off.
    request_url: Option<String>,
    /// Also send each member's message in the channel to the request URL (`event_callback`),
    /// and let the webhook's secret read the channel. Needs a request URL.
    #[serde(default)]
    #[schema(required = false)]
    events: bool,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct ChatSigningSecret {
    /// Checks the `X-Slack-Signature` of each click.
    signing_secret: String,
}

fn app_key(state: &ChatState, call: &Call<'_>) -> Result<[u8; 32], ApiError> {
    state.app_key.ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "app_key_missing",
            "App key missing",
            "Interactive buttons need the server's app key (ORBIT_APP_KEY).",
            &call.instance,
            call.request_id,
        )
    })
}

/// Sends a click on an integration's button to the integration. The answer comes later through
/// the click's `response_url`; this only says whether the integration took the click.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/chat/messages/{message_id}/actions", params(("workspace_id" = String, Path), ("message_id" = String, Path)), request_body = ChatActionBody, responses((status = 200, body = ChatActionResult)))]
pub(crate) async fn click_chat_action(
    State(state): State<ChatState>,
    Path((workspace, message)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<ChatActionBody>,
) -> Result<Json<ChatActionResult>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let message_id = call.id(&message)?;
    let key = app_key(&state, &call)?;
    let click = state
        .chat
        .prepare_click(
            call.workspace_id,
            call.actor_id,
            message_id,
            &body.block_id,
            &body.action_id,
            &key,
            &state.public_origin,
        )
        .await
        .map_err(|error| call.problem(error))?;
    let started = Instant::now();
    let result = state.clicks.send(&click).await;
    let duration_ms = i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX);
    let (status, error) = match result {
        Ok(status) => (Some(status), None),
        Err(reason) => (None, Some(reason)),
    };
    if let Err(error) = state
        .chat
        .record_delivery(
            click.webhook_id,
            &click.action_id,
            status,
            error,
            duration_ms,
        )
        .await
    {
        tracing::warn!(error = %error, "chat click delivery record");
    }
    match (status, error) {
        (Some(status), _) if (200..300).contains(&status) => {
            Ok(Json(ChatActionResult { ok: true }))
        }
        (status, error) => Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "integration_failed",
            "Integration failed",
            "The integration did not take the click.",
            &call.instance,
            call.request_id,
        )
        .with_detail(match (status, error) {
            (Some(status), _) => format!("The integration answered HTTP {status}."),
            (None, Some("timeout")) => "The integration did not answer in 3 seconds.".to_owned(),
            (None, Some("blocked_address")) => {
                "The request URL is not a public https address.".to_owned()
            }
            (None, _) => "The integration could not be reached.".to_owned(),
        })),
    }
}

/// Sets where clicks on a webhook's buttons go, or turns clicks off (`request_url: null`).
#[utoipa::path(put, path = "/api/v1/workspaces/{workspace_id}/chat/conversations/{conversation_id}/webhooks/{webhook_id}/interactivity", params(("workspace_id" = String, Path), ("conversation_id" = String, Path), ("webhook_id" = String, Path)), request_body = ChatInteractivityBody, responses((status = 200, body = WebhookRecord)))]
pub(crate) async fn set_chat_webhook_interactivity(
    State(state): State<ChatState>,
    Path((workspace, conversation, webhook)): Path<(String, String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<ChatInteractivityBody>,
) -> Result<Json<WebhookRecord>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let conversation_id = call.id(&conversation)?;
    let webhook_id = call.id(&webhook)?;
    let key = app_key(&state, &call)?;
    call.read(
        state
            .chat
            .set_request_url(
                call.workspace_id,
                call.actor_id,
                conversation_id,
                webhook_id,
                body.request_url.as_deref(),
                body.events,
                &key,
                state.clicks.development,
            )
            .await,
    )
}

async fn signing_secret(
    state: &ChatState,
    path: (String, String, String),
    headers: &HeaderMap,
    uri: &Uri,
    request_id: &RequestIdExtension,
    rotate: bool,
) -> Result<Json<ChatSigningSecret>, ApiError> {
    let (workspace, conversation, webhook) = path;
    let call = Call::enter(state, headers, uri, &workspace, request_id).await?;
    let conversation_id = call.id(&conversation)?;
    let webhook_id = call.id(&webhook)?;
    let key = app_key(state, &call)?;
    call.read(
        state
            .chat
            .signing_secret(
                call.workspace_id,
                call.actor_id,
                conversation_id,
                webhook_id,
                &key,
                rotate,
            )
            .await
            .map(|signing_secret| ChatSigningSecret { signing_secret }),
    )
}

/// The signing secret that checks this webhook's clicks.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/chat/conversations/{conversation_id}/webhooks/{webhook_id}/signing-secret", params(("workspace_id" = String, Path), ("conversation_id" = String, Path), ("webhook_id" = String, Path)), responses((status = 200, body = ChatSigningSecret)))]
pub(crate) async fn get_chat_webhook_signing_secret(
    State(state): State<ChatState>,
    Path(path): Path<(String, String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<ChatSigningSecret>, ApiError> {
    signing_secret(&state, path, &headers, &uri, &request_id, false).await
}

/// A new signing secret. Clicks are signed with it at once.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/chat/conversations/{conversation_id}/webhooks/{webhook_id}/signing-secret/rotate", params(("workspace_id" = String, Path), ("conversation_id" = String, Path), ("webhook_id" = String, Path)), responses((status = 200, body = ChatSigningSecret)))]
pub(crate) async fn rotate_chat_webhook_signing_secret(
    State(state): State<ChatState>,
    Path(path): Path<(String, String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<ChatSigningSecret>, ApiError> {
    signing_secret(&state, path, &headers, &uri, &request_id, true).await
}

/// The last 20 clicks sent to a webhook, newest first.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/chat/conversations/{conversation_id}/webhooks/{webhook_id}/deliveries", params(("workspace_id" = String, Path), ("conversation_id" = String, Path), ("webhook_id" = String, Path)), responses((status = 200, body = Vec<DeliveryRecord>)))]
pub(crate) async fn list_chat_webhook_deliveries(
    State(state): State<ChatState>,
    Path((workspace, conversation, webhook)): Path<(String, String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<Vec<DeliveryRecord>>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let conversation_id = call.id(&conversation)?;
    let webhook_id = call.id(&webhook)?;
    call.read(
        state
            .chat
            .list_deliveries(
                call.workspace_id,
                call.actor_id,
                conversation_id,
                webhook_id,
            )
            .await,
    )
}
