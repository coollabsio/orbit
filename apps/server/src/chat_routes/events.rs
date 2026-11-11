//! Two-way integrations. With events on, a webhook gets each member's message in its channel as a
//! Slack `event_callback`, signed as its clicks are (`interactions.rs`), and its secret reads the
//! channel and its threads with `GET /api/v1/hooks/chat/{webhook_id}/messages`. The integration
//! answers by posting to the webhook URL, with `thread_ts` for a reply in a thread (`hooks.rs`).

use std::collections::HashMap;
use std::time::Instant;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::http::header::AUTHORIZATION;
use orbit_platform::Id;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use utoipa::{IntoParams, ToSchema};

use super::{ChatState, RequestIdExtension, problem};
use crate::repositories::chat::{
    ChatError, EventTarget, MessageCursor, MessageKind, MessageRecord,
};
use crate::task_routes::ApiError;

/// Sends a member's new message to each webhook of its channel that has events on. It runs in the
/// background: the sender does not wait for the integrations, and a failed send is only recorded
/// in the webhook's deliveries.
pub(crate) fn send_message_events(state: &ChatState, workspace_id: Id, message: &MessageRecord) {
    if message.bot.is_some() || message.kind != MessageKind::Message {
        return;
    }
    let Some(key) = state.app_key else {
        return;
    };
    let state = state.clone();
    let message = message.clone();
    tokio::spawn(async move {
        let targets = match state
            .chat
            .event_targets(message.conversation_id, &key)
            .await
        {
            Ok(targets) if !targets.is_empty() => targets,
            Ok(_) => return,
            Err(error) => {
                tracing::warn!(error = %error, "chat event targets");
                return;
            }
        };
        let names = state
            .chat
            .display_names(vec![message.author_id])
            .await
            .unwrap_or_default();
        let user_name = names.get(&message.author_id).map(String::as_str);
        for target in targets {
            let event = event_payload(workspace_id, &target, &message, user_name);
            let kind = event["event"]["type"]
                .as_str()
                .unwrap_or("message")
                .to_owned();
            let started = Instant::now();
            let result = state
                .clicks
                .post_signed(
                    &target.request_url,
                    &target.signing_secret,
                    "application/json",
                    event.to_string(),
                )
                .await;
            let duration_ms = i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX);
            let (status, error) = match result {
                Ok(status) => (Some(status), None),
                Err(reason) => (None, Some(reason)),
            };
            if let Err(error) = state
                .chat
                .record_delivery(target.webhook_id, &kind, status, error, duration_ms)
                .await
            {
                tracing::warn!(error = %error, "chat event delivery record");
            }
        }
    });
}

/// Whether `text` names the integration as `@name` (any letter case), not as the start of a
/// longer word.
fn mentions_bot(text: &str, name: &str) -> bool {
    let text = text.to_lowercase();
    let needle = format!("@{}", name.to_lowercase());
    text.match_indices(&needle).any(|(start, _)| {
        text[start + needle.len()..]
            .chars()
            .next()
            .is_none_or(|next| !next.is_alphanumeric() && next != '_' && next != '-')
    })
}

/// The Slack Events API body for one message: `app_mention` when it names the integration,
/// `message` otherwise. Ids are Orbit ids: `ts` is the message id, `thread_ts` its thread's root.
fn event_payload(
    workspace_id: Id,
    target: &EventTarget,
    message: &MessageRecord,
    user_name: Option<&str>,
) -> Value {
    let kind = if mentions_bot(&message.body, &target.name) {
        "app_mention"
    } else {
        "message"
    };
    let mut event = json!({
        "type": kind,
        "channel": message.conversation_id.to_string(),
        "user": message.author_id.to_string(),
        "text": message.body,
        "ts": message.id.to_string(),
        "event_ts": message.id.to_string(),
    });
    if let Some(root_id) = message.thread_root_id {
        event["thread_ts"] = json!(root_id.to_string());
    }
    if let Some(name) = user_name {
        event["user_profile"] = json!({"name": name, "display_name": name, "real_name": name});
    }
    json!({
        "type": "event_callback",
        "team_id": workspace_id.to_string(),
        "api_app_id": target.webhook_id.to_string(),
        "event_id": format!("Ev{}", message.id),
        "event_time": message.created_at.as_millis() / 1000,
        "event": event,
    })
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct HookMessagesQuery {
    /// The root of a thread: its replies, oldest first. Without it, the channel's main list.
    thread_ts: Option<String>,
    /// Messages older than this message id, for the next page.
    before: Option<String>,
    /// 1–100 messages; 50 by default.
    limit: Option<usize>,
}

/// One message as an integration reads it, in Slack's shape.
#[derive(Serialize, ToSchema)]
#[schema(as = HookMessage)]
pub(crate) struct HookMessage {
    /// Always `message`.
    #[serde(rename = "type")]
    kind: &'static str,
    /// The message id.
    ts: String,
    /// The root of its thread; for a root with replies, its own id.
    #[serde(skip_serializing_if = "Option::is_none")]
    thread_ts: Option<String>,
    /// The member who wrote it; for an integration's message, the member who set it up.
    user: String,
    /// The display name of `user`, or the integration's name for its messages.
    username: Option<String>,
    /// The webhook that posted it; absent for members' messages.
    #[serde(skip_serializing_if = "Option::is_none")]
    bot_id: Option<String>,
    /// Markdown with mention tokens (`<@user_id>`); empty for a deleted message.
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    reply_count: Option<i64>,
}

#[derive(Serialize, ToSchema)]
#[schema(as = HookMessages)]
pub(crate) struct HookMessages {
    ok: bool,
    /// Oldest first.
    messages: Vec<HookMessage>,
    /// Send it as `before` for the older page; null on the oldest page.
    next_before: Option<String>,
}

/// Reads the webhook's channel, or one thread of it, for an integration with events on. The
/// credential is the webhook URL's secret in `Authorization: Bearer <secret>`.
#[utoipa::path(get, path = "/api/v1/hooks/chat/{webhook_id}/messages", params(("webhook_id" = String, Path), HookMessagesQuery), responses((status = 200, body = HookMessages)))]
pub(crate) async fn read_hook_messages(
    State(state): State<ChatState>,
    Path(webhook): Path<String>,
    Query(query): Query<HookMessagesQuery>,
    headers: HeaderMap,
    request_id: RequestIdExtension,
) -> Result<Json<HookMessages>, ApiError> {
    let instance = format!("/api/v1/hooks/chat/{webhook}/messages");
    let request_id = request_id.as_ref();
    let fail = |error: ChatError| match error {
        ChatError::NotFound => super::hooks::hook_problem(error, &instance, request_id),
        error => problem(error, &instance, request_id),
    };
    let secret = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or_default();
    let webhook_id: Id = webhook.parse().map_err(|_| fail(ChatError::NotFound))?;
    // A webhook without events can only post: its URL may sit in a CI config, so it must not
    // also read the channel.
    let target = state
        .chat
        .find_webhook(webhook_id, secret.trim())
        .await
        .map_err(&fail)?
        .filter(|target| target.events)
        .ok_or_else(|| fail(ChatError::NotFound))?;
    let id = |value: &str, field: &'static str| {
        value
            .parse::<Id>()
            .map_err(|_| fail(ChatError::Invalid { field }))
    };
    let thread_root_id = query
        .thread_ts
        .as_deref()
        .map(|value| id(value, "thread_ts"))
        .transpose()?;
    let cursor = MessageCursor {
        before: query
            .before
            .as_deref()
            .map(|value| id(value, "before"))
            .transpose()?,
        limit: query.limit,
        ..MessageCursor::default()
    };
    let page = state
        .chat
        .read_as_bot(&target, thread_root_id, cursor)
        .await
        .map_err(&fail)?;
    let mut authors: Vec<Id> = page.items.iter().map(|message| message.author_id).collect();
    authors.sort_unstable();
    authors.dedup();
    let names: HashMap<Id, String> = state.chat.display_names(authors).await.map_err(&fail)?;
    let messages = page
        .items
        .into_iter()
        .filter(|message| message.kind == MessageKind::Message)
        .map(|message| HookMessage {
            kind: "message",
            ts: message.id.to_string(),
            thread_ts: message
                .thread_root_id
                .or((message.reply_count > 0).then_some(message.id))
                .map(|id| id.to_string()),
            user: message.author_id.to_string(),
            username: match &message.bot {
                Some(bot) => Some(bot.name.clone()),
                None => names.get(&message.author_id).cloned(),
            },
            bot_id: message.bot.as_ref().map(|bot| bot.webhook_id.to_string()),
            text: if message.deleted {
                String::new()
            } else {
                message.body
            },
            reply_count: (message.thread_root_id.is_none() && message.reply_count > 0)
                .then_some(message.reply_count),
        })
        .collect();
    Ok(Json(HookMessages {
        ok: true,
        messages,
        next_before: page.before.map(|id| id.to_string()),
    }))
}

#[cfg(test)]
mod tests {
    use super::mentions_bot;

    #[test]
    fn a_mention_names_the_whole_bot_name() {
        assert!(mentions_bot("hey @Assistant, sum up", "assistant"));
        assert!(mentions_bot("@release bot ship it", "Release bot"));
        assert!(mentions_bot("ask @Assistant", "Assistant"));
        assert!(!mentions_bot("hey @Assistants", "Assistant"));
        assert!(!mentions_bot("hey @assistant-2", "Assistant"));
        assert!(!mentions_bot("no mention here", "Assistant"));
    }
}
