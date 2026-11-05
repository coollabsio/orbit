//! Incoming webhooks. Channel managers make, rotate and delete them; an external tool posts a
//! message with `POST /api/v1/hooks/chat/{webhook_id}/{secret}` and no session. The secret is in
//! the path, so error bodies name `/api/v1/hooks/chat/{webhook_id}` only.

use axum::Json;
use axum::body::to_bytes;
use axum::extract::{Path, Request, State};
use axum::http::HeaderName;
use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderMap, StatusCode, Uri};
use orbit_platform::Id;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::{Call, ChatState, RequestIdExtension, problem};
use crate::billing::WorkspaceAccess;
use crate::billing::guard::READ_ONLY_DETAIL;
use crate::push::PushService;
use crate::repositories::billing;
use crate::repositories::chat::{
    BotPost, ChatError, MESSAGE_MAX_CHARS, MessageBlock, WebhookInput, WebhookRecord,
    WebhookWithSecret, attachment_blocks, check_message_blocks, cut, emoji_name,
    mrkdwn_to_markdown, parse_blocks,
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

/// What a tool posts: the payload of a Slack incoming webhook. Fields that Orbit does not use
/// (`channel`, `thread_ts`, `unfurl_links`) are ignored, so a Slack payload posts as it is.
#[derive(Deserialize, ToSchema)]
pub(crate) struct IncomingWebhookPayload {
    /// Slack mrkdwn or Markdown; mentions are tokens: `<@user_id>`, `<#channel_id>`,
    /// `<!channel>`, `<!here>`. Longer than 4000 characters, it is cut. With `blocks` or
    /// `attachments` it is the plain text for notifications and search, and may be left out.
    #[schema(required = false)]
    text: Option<String>,
    /// `false` shows `text` as it is, without formatting (Slack's `mrkdwn`).
    #[schema(required = false)]
    mrkdwn: Option<bool>,
    /// Block Kit blocks (`header`, `section`, `context`, `image`, `divider`, `markdown`,
    /// `rich_text`) shown in place of `text`. Other block types are left out.
    #[schema(value_type = Option<Vec<MessageBlock>>, required = false)]
    blocks: Option<serde_json::Value>,
    /// Slack's legacy attachments: boxes with a colored bar, title, text and fields.
    #[schema(value_type = Option<Vec<serde_json::Value>>, required = false)]
    attachments: Option<serde_json::Value>,
    /// The author name of this message only (cut to 80 characters).
    #[schema(required = false)]
    username: Option<String>,
    /// The avatar of this message only: an https image. Other URLs are ignored.
    #[schema(required = false)]
    icon_url: Option<String>,
    /// The avatar of this message only as an emoji (`:ghost:`); it wins over `icon_url`.
    #[schema(required = false)]
    icon_emoji: Option<String>,
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

/// Slack's answer to a hook: `200` and the text `ok`.
type HookOk = (StatusCode, [(HeaderName, &'static str); 1], &'static str);

fn hook_ok() -> HookOk {
    (
        StatusCode::OK,
        [(CONTENT_TYPE, "text/plain; charset=utf-8")],
        "ok",
    )
}

type RequestIdRef<'a> = Option<&'a axum::Extension<orbit_platform::RequestId>>;

/// The JSON of a hook body: a JSON body, or a form with the JSON in `payload` (as Slack takes).
async fn read_json<T: DeserializeOwned>(
    request: Request,
    instance: &str,
    request_id: RequestIdRef<'_>,
) -> Result<T, ApiError> {
    let media_type = request
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(|value| value.trim().to_ascii_lowercase())
        .unwrap_or_default();
    let form = match media_type.as_str() {
        "application/json" => false,
        "application/x-www-form-urlencoded" => true,
        _ => {
            return Err(ApiError::new(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "unsupported_media_type",
                "Unsupported media type",
                "Send the message as JSON (Content-Type: application/json), or as a form with the JSON in `payload`.",
                instance,
                request_id,
            ));
        }
    };
    let bytes = to_bytes(request.into_body(), MAX_HOOK_BODY_BYTES)
        .await
        .map_err(|_| {
            ApiError::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "payload_too_large",
                "Payload too large",
                "A webhook message can be at most 64 KiB of JSON.",
                instance,
                request_id,
            )
        })?;
    let json = if form {
        url::form_urlencoded::parse(&bytes)
            .find(|(name, _)| name == "payload")
            .map(|(_, value)| value.into_owned().into_bytes())
            .unwrap_or_default()
    } else {
        bytes.to_vec()
    };
    serde_json::from_slice(&json).map_err(|_| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_json",
            "Invalid JSON",
            "The body must be a JSON object with `text`, `blocks` or `attachments`.",
            instance,
            request_id,
        )
    })
}

/// A Slack payload as an integration's post: blocks and attachments checked, mrkdwn converted,
/// what Orbit cannot show cut or left out.
fn build_post(
    payload: IncomingWebhookPayload,
    instance: &str,
    request_id: RequestIdRef<'_>,
) -> Result<BotPost, ApiError> {
    let invalid = |detail: String| {
        ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed",
            "Validation failed",
            "blocks",
            instance,
            request_id,
        )
        .with_detail(detail)
    };
    let mut blocks = match payload.blocks.as_ref().filter(|value| !value.is_null()) {
        Some(value) => parse_blocks(value).map_err(|error| invalid(error.0))?,
        None => Vec::new(),
    };
    let attachments = match payload
        .attachments
        .as_ref()
        .filter(|value| !value.is_null())
    {
        Some(value) => attachment_blocks(value).map_err(|error| invalid(error.0))?,
        None => Vec::new(),
    };
    let text = payload
        .text
        .filter(|text| !text.trim().is_empty())
        .map(|text| mrkdwn_to_markdown(&text, payload.mrkdwn != Some(false)));
    // Without blocks Slack shows `text` above the attachments; blocks take its place.
    if blocks.is_empty()
        && !attachments.is_empty()
        && let Some(text) = &text
    {
        blocks.push(MessageBlock::Markdown {
            text: cut(text, MESSAGE_MAX_CHARS),
        });
    }
    blocks.extend(attachments);
    check_message_blocks(&blocks).map_err(|error| invalid(error.0))?;
    Ok(BotPost {
        text: text
            .map(|text| cut(&text, MESSAGE_MAX_CHARS))
            .unwrap_or_default(),
        blocks: (!blocks.is_empty()).then_some(blocks),
        // Slack takes any name and icon; Orbit cuts the name and leaves out what it cannot show.
        username: payload
            .username
            .map(|name| cut(name.trim(), 80))
            .filter(|name| !name.is_empty()),
        icon_url: payload.icon_url.filter(|url| {
            url::Url::parse(url.trim()).is_ok_and(|parsed| parsed.scheme() == "https")
                && url.len() <= 2048
        }),
        icon_emoji: payload.icon_emoji.as_deref().and_then(emoji_name),
    })
}

/// Refuses writes to a read-only workspace (Orbit Cloud without a subscription).
async fn require_writable(
    state: &ChatState,
    workspace_id: Id,
    instance: &str,
    request_id: RequestIdRef<'_>,
) -> Result<(), ApiError> {
    let access = billing::write_access(state.identity.database(), workspace_id)
        .await
        .map_err(|error| problem(ChatError::Unavailable(error), instance, request_id))?;
    if access == WorkspaceAccess::ReadOnly {
        return Err(ApiError::new(
            StatusCode::PAYMENT_REQUIRED,
            "workspace_read_only",
            "Workspace is read-only",
            READ_ONLY_DETAIL,
            instance,
            request_id,
        ));
    }
    Ok(())
}

/// Posts a message into the webhook's channel as the webhook, and answers `ok` as Slack does. No
/// session: the secret in the path is the credential. An unknown id, a wrong secret and a
/// deleted webhook all answer 404. The body is JSON, or a form with the JSON in `payload`.
#[utoipa::path(post, path = "/api/v1/hooks/chat/{webhook_id}/{secret}", params(("webhook_id" = String, Path), ("secret" = String, Path)), request_body = IncomingWebhookPayload, responses((status = 200, description = "`ok`", content_type = "text/plain", body = String)))]
pub(crate) async fn post_incoming_webhook(
    State(state): State<ChatState>,
    Path((webhook, secret)): Path<(String, String)>,
    request_id: RequestIdExtension,
    request: Request,
) -> Result<HookOk, ApiError> {
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
    let payload: IncomingWebhookPayload = read_json(request, &instance, request_id).await?;
    let post = build_post(payload, &instance, request_id)?;
    require_writable(&state, target.workspace_id, &instance, request_id).await?;

    let workspace_id = target.workspace_id;
    // The webhook is no member, so `publish` refreshes the states of every connected member,
    // the webhook's creator too.
    let actor_id = target.sender.webhook_id;
    let (written, _) = state
        .publish(workspace_id, actor_id, state.chat.post_as_bot(target, post))
        .await
        .map_err(&fail)?;
    PushService::of(state.identity.database()).chat_message(&written.value);
    Ok(hook_ok())
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
