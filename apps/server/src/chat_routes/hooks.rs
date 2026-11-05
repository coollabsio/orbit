//! Incoming webhooks. Channel managers make, rotate and delete them; an external tool posts a
//! message with `POST /api/v1/hooks/chat/{webhook_id}/{secret}` and no session. The secret is in
//! the path, so error bodies name `/api/v1/hooks/chat/{webhook_id}` only.

use axum::Json;
use axum::body::to_bytes;
use axum::extract::{Path, Request, State};
use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderMap, StatusCode, Uri};
use orbit_platform::Id;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::{Call, ChatState, RequestIdExtension, problem};
use crate::billing::WorkspaceAccess;
use crate::billing::guard::READ_ONLY_DETAIL;
use crate::push::PushService;
use crate::repositories::billing;
use crate::repositories::chat::{
    BotPost, ChatError, WebhookInput, WebhookRecord, WebhookWithSecret,
};
use crate::task_routes::{ApiError, ApiJson};

/// The largest hook body: far above 4000 characters of text and the Slack fields Orbit ignores.
const MAX_HOOK_BODY_BYTES: usize = 64 * 1024;

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChatWebhookBody {
    /// The author name of its messages: 1–80 characters.
    name: String,
    /// An https image for the avatar of its messages.
    #[serde(default)]
    #[schema(required = false)]
    icon_url: Option<String>,
}

impl From<ChatWebhookBody> for WebhookInput {
    fn from(body: ChatWebhookBody) -> Self {
        Self {
            name: body.name,
            icon_url: body.icon_url,
        }
    }
}

/// A webhook with its URL. The URL holds the secret and is returned only here.
#[derive(Serialize, ToSchema)]
pub(crate) struct ChatWebhookWithUrl {
    webhook: WebhookRecord,
    url: String,
}

/// What a tool posts. Fields that Orbit does not know (Slack's `channel`, `icon_emoji`,
/// `attachments`, `blocks` for now) are ignored, so a Slack payload still posts its `text`.
#[derive(Deserialize, ToSchema)]
pub(crate) struct IncomingWebhookPayload {
    /// Markdown, at most 4000 characters. Mentions are tokens: `<@user_id>`, `<#channel_id>`,
    /// `<!channel>`, `<!here>`.
    text: Option<String>,
    /// The author name of this message only (1–80 characters).
    #[schema(required = false)]
    username: Option<String>,
    /// The avatar of this message only: an https image.
    #[schema(required = false)]
    icon_url: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct IncomingWebhookResult {
    ok: bool,
    message_id: String,
}

fn with_url(state: &ChatState, created: WebhookWithSecret) -> ChatWebhookWithUrl {
    ChatWebhookWithUrl {
        url: format!(
            "{}/api/v1/hooks/chat/{}/{}",
            state.public_origin.trim_end_matches('/'),
            created.webhook.id,
            created.secret
        ),
        webhook: created.webhook,
    }
}

/// The live incoming webhooks of a channel. Only its creator and workspace owners and admins.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/chat/conversations/{conversation_id}/webhooks", params(("workspace_id" = String, Path), ("conversation_id" = String, Path)), responses((status = 200, body = Vec<WebhookRecord>)))]
pub(crate) async fn list_chat_webhooks(
    State(state): State<ChatState>,
    Path((workspace, conversation)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<Vec<WebhookRecord>>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let conversation_id = call.id(&conversation)?;
    call.read(
        state
            .chat
            .list_webhooks(call.workspace_id, call.actor_id, conversation_id)
            .await,
    )
}

/// Makes an incoming webhook for a channel (at most 10). The response has its URL, once.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/chat/conversations/{conversation_id}/webhooks", params(("workspace_id" = String, Path), ("conversation_id" = String, Path)), request_body = ChatWebhookBody, responses((status = 201, body = ChatWebhookWithUrl)))]
pub(crate) async fn create_chat_webhook(
    State(state): State<ChatState>,
    Path((workspace, conversation)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<ChatWebhookBody>,
) -> Result<(StatusCode, Json<ChatWebhookWithUrl>), ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let conversation_id = call.id(&conversation)?;
    let created = state
        .chat
        .create_webhook(
            call.workspace_id,
            call.actor_id,
            conversation_id,
            body.into(),
        )
        .await
        .map_err(|error| call.problem(error))?;
    Ok((StatusCode::CREATED, Json(with_url(&state, created))))
}

/// Renames a webhook or changes its icon. Its URL stays.
#[utoipa::path(patch, path = "/api/v1/workspaces/{workspace_id}/chat/conversations/{conversation_id}/webhooks/{webhook_id}", params(("workspace_id" = String, Path), ("conversation_id" = String, Path), ("webhook_id" = String, Path)), request_body = ChatWebhookBody, responses((status = 200, body = WebhookRecord)))]
pub(crate) async fn update_chat_webhook(
    State(state): State<ChatState>,
    Path((workspace, conversation, webhook)): Path<(String, String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<ChatWebhookBody>,
) -> Result<Json<WebhookRecord>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let conversation_id = call.id(&conversation)?;
    let webhook_id = call.id(&webhook)?;
    call.read(
        state
            .chat
            .update_webhook(
                call.workspace_id,
                call.actor_id,
                conversation_id,
                webhook_id,
                body.into(),
            )
            .await,
    )
}

/// A new URL for a webhook. The old URL stops working at once.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/chat/conversations/{conversation_id}/webhooks/{webhook_id}/rotate", params(("workspace_id" = String, Path), ("conversation_id" = String, Path), ("webhook_id" = String, Path)), responses((status = 200, body = ChatWebhookWithUrl)))]
pub(crate) async fn rotate_chat_webhook(
    State(state): State<ChatState>,
    Path((workspace, conversation, webhook)): Path<(String, String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<ChatWebhookWithUrl>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let conversation_id = call.id(&conversation)?;
    let webhook_id = call.id(&webhook)?;
    let rotated = state
        .chat
        .rotate_webhook(
            call.workspace_id,
            call.actor_id,
            conversation_id,
            webhook_id,
        )
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(with_url(&state, rotated)))
}

/// Deletes a webhook. Its URL stops working; the messages it posted stay.
#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/chat/conversations/{conversation_id}/webhooks/{webhook_id}", params(("workspace_id" = String, Path), ("conversation_id" = String, Path), ("webhook_id" = String, Path)), responses((status = 204)))]
pub(crate) async fn delete_chat_webhook(
    State(state): State<ChatState>,
    Path((workspace, conversation, webhook)): Path<(String, String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<StatusCode, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let conversation_id = call.id(&conversation)?;
    let webhook_id = call.id(&webhook)?;
    state
        .chat
        .revoke_webhook(
            call.workspace_id,
            call.actor_id,
            conversation_id,
            webhook_id,
        )
        .await
        .map_err(|error| call.problem(error))?;
    Ok(StatusCode::NO_CONTENT)
}

/// Posts a message into the webhook's channel as the webhook. No session: the secret in the
/// path is the credential. An unknown id, a wrong secret and a deleted webhook all answer 404.
#[utoipa::path(post, path = "/api/v1/hooks/chat/{webhook_id}/{secret}", params(("webhook_id" = String, Path), ("secret" = String, Path)), request_body = IncomingWebhookPayload, responses((status = 200, body = IncomingWebhookResult)))]
pub(crate) async fn post_incoming_webhook(
    State(state): State<ChatState>,
    Path((webhook, secret)): Path<(String, String)>,
    request_id: RequestIdExtension,
    request: Request,
) -> Result<Json<IncomingWebhookResult>, ApiError> {
    let instance = format!("/api/v1/hooks/chat/{webhook}");
    let request_id = request_id.as_ref();
    let fail = |error: ChatError| hook_problem(error, &instance, request_id);
    let webhook_id: Id = webhook.parse().map_err(|_| fail(ChatError::NotFound))?;
    let target = state
        .chat
        .find_webhook(webhook_id, &secret)
        .await
        .map_err(&fail)?
        .ok_or_else(|| fail(ChatError::NotFound))?;

    let json = request
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(';')
                .next()
                .is_some_and(|kind| kind.trim().eq_ignore_ascii_case("application/json"))
        });
    if !json {
        return Err(ApiError::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_media_type",
            "Unsupported media type",
            "Send the message as JSON with Content-Type: application/json.",
            &instance,
            request_id,
        ));
    }
    let bytes = to_bytes(request.into_body(), MAX_HOOK_BODY_BYTES)
        .await
        .map_err(|_| {
            ApiError::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "payload_too_large",
                "Payload too large",
                "A webhook message can be at most 64 KiB of JSON.",
                &instance,
                request_id,
            )
        })?;
    let payload: IncomingWebhookPayload = serde_json::from_slice(&bytes).map_err(|_| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_json",
            "Invalid JSON",
            "The body must be a JSON object with a `text` field.",
            &instance,
            request_id,
        )
    })?;

    let read_only = billing::write_access(state.identity.database(), target.workspace_id)
        .await
        .map_err(|error| fail(ChatError::Unavailable(error)))?
        == WorkspaceAccess::ReadOnly;
    if read_only {
        return Err(ApiError::new(
            StatusCode::PAYMENT_REQUIRED,
            "workspace_read_only",
            "Workspace is read-only",
            READ_ONLY_DETAIL,
            &instance,
            request_id,
        ));
    }

    let workspace_id = target.workspace_id;
    // The webhook is no member, so `publish` refreshes the states of every connected member,
    // the webhook's creator too.
    let actor_id = target.sender.webhook_id;
    let post = BotPost {
        text: payload.text.unwrap_or_default(),
        username: payload.username,
        icon_url: payload.icon_url,
    };
    let (written, _) = state
        .publish(workspace_id, actor_id, state.chat.post_as_bot(target, post))
        .await
        .map_err(&fail)?;
    PushService::of(state.identity.database()).chat_message(&written.value);
    Ok(Json(IncomingWebhookResult {
        ok: true,
        message_id: written.value.id.to_string(),
    }))
}

fn hook_problem(
    error: ChatError,
    instance: &str,
    request_id: Option<&axum::Extension<orbit_platform::RequestId>>,
) -> ApiError {
    match error {
        ChatError::NotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            "webhook_not_found",
            "Webhook not found",
            "This webhook URL is not valid. It may have been deleted or rotated.",
            instance,
            request_id,
        ),
        ChatError::Forbidden(_) => ApiError::new(
            StatusCode::FORBIDDEN,
            "chat_channel_archived",
            "Channel archived",
            "The channel of this webhook is archived.",
            instance,
            request_id,
        ),
        error => problem(error, instance, request_id),
    }
}
