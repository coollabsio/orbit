//! Clicks on an integration's buttons. A click becomes a Slack `block_actions` payload for the
//! webhook's request URL (the route sends it, signed) with a `response_url`: a token that lets the
//! integration answer five times in thirty minutes by replacing or deleting the message, posting a
//! new one, or showing a reply to the member who clicked only.

use orbit_platform::{Id, Row, TimestampMillis, generate_opaque_token};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::blocks::find_button;
use super::messages::{bot_message_parts, remove_message, rewrite_message, webhook_interactive};
use super::webhooks::{BotSender, HookTarget, decrypt};
use super::{
    BotPost, ChatError, ChatEvent, ChatRepository, ConversationKind, Events, MentionsRecord,
    MessageKind, MessageRecord, Written, finish, load_conversation, load_message,
    load_message_access, mentioned_user_ids, parse_id,
};

/// How long and how often an integration may answer one click (as Slack's `response_url`).
const RESPONSE_TTL_MS: i64 = 30 * 60 * 1000;
const RESPONSE_USES: i64 = 5;

/// A click, ready to send: where, the secret to sign with, and the payload.
pub struct Click {
    pub webhook_id: Id,
    pub action_id: String,
    pub request_url: String,
    pub signing_secret: String,
    pub payload: Value,
}

/// What an answer through a `response_url` acts on.
pub struct ResponseTarget {
    pub target: HookTarget,
    /// The message with the button.
    pub message_id: Id,
    /// The member who clicked: the reader of an ephemeral reply.
    pub user_id: Id,
}

fn token_hash(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

impl ChatRepository {
    /// Checks a click by `actor_id` and makes its payload and response token. Only the button's
    /// position comes from the member; its `value` is read from the stored message.
    #[allow(clippy::too_many_arguments)]
    pub async fn prepare_click(
        &self,
        workspace_id: Id,
        actor_id: Id,
        message_id: Id,
        block_id: &str,
        action_id: &str,
        key: &[u8; 32],
        public_origin: &str,
    ) -> Result<Click, ChatError> {
        let now = TimestampMillis::now();
        let mut tx = self.database.immediate_transaction().await?;
        let (access, message) =
            load_message_access(&mut tx, workspace_id, actor_id, message_id).await?;
        access.require_member()?;
        access.require_open()?;
        let (Some(bot), Some(blocks), false) = (&message.bot, &message.blocks, message.deleted)
        else {
            return Err(ChatError::NotFound);
        };
        let button = find_button(blocks, block_id, action_id).ok_or(ChatError::NotFound)?;
        let webhook = sqlx::query(
            "SELECT request_url, signing_secret FROM chat_webhooks \
             WHERE id = $1 AND revoked_at IS NULL AND request_url IS NOT NULL",
        )
        .bind(bot.webhook_id.to_string())
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(ChatError::Forbidden(
            "This integration does not take clicks.",
        ))?;
        let request_url: String = webhook.get("request_url");
        let signing_secret = decrypt(
            key,
            &webhook
                .get::<Option<Vec<u8>>, _>("signing_secret")
                .unwrap_or_default(),
        )?;
        let user = sqlx::query("SELECT display_name FROM users WHERE id = $1")
            .bind(actor_id.to_string())
            .fetch_one(&mut *tx)
            .await?;
        let display_name: String = user.get("display_name");
        let workspace_name: String =
            sqlx::query_scalar("SELECT name FROM workspaces WHERE id = $1")
                .bind(workspace_id.to_string())
                .fetch_one(&mut *tx)
                .await?;

        sqlx::query("DELETE FROM chat_action_responses WHERE expires_at < $1")
            .bind(now.as_millis())
            .execute(&mut *tx)
            .await?;
        let token = generate_opaque_token();
        sqlx::query(
            "INSERT INTO chat_action_responses (token_hash, webhook_id, message_id, conversation_id, \
             user_id, uses_left, expires_at, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(token_hash(&token))
        .bind(bot.webhook_id.to_string())
        .bind(message.id.to_string())
        .bind(message.conversation_id.to_string())
        .bind(actor_id.to_string())
        .bind(RESPONSE_USES)
        .bind(now.as_millis() + RESPONSE_TTL_MS)
        .bind(now.as_millis())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;

        let seconds = |millis: i64| format!("{}.{:03}", millis / 1000, millis % 1000);
        let payload = json!({
            "type": "block_actions",
            "api_app_id": bot.webhook_id.to_string(),
            "token": "",
            "trigger_id": generate_opaque_token(),
            "team": { "id": workspace_id.to_string(), "domain": workspace_name },
            "user": {
                "id": actor_id.to_string(),
                "username": display_name,
                "name": display_name,
                "team_id": workspace_id.to_string(),
            },
            "channel": {
                "id": message.conversation_id.to_string(),
                "name": access.conversation.name,
            },
            "container": {
                "type": "message",
                "message_ts": message.id.to_string(),
                "channel_id": message.conversation_id.to_string(),
                "is_ephemeral": false,
            },
            "message": {
                "type": "message",
                "ts": message.id.to_string(),
                "text": message.body,
                "blocks": blocks,
            },
            "response_url": format!(
                "{}/api/v1/hooks/chat/{}/responses/{token}",
                public_origin.trim_end_matches('/'),
                bot.webhook_id
            ),
            "actions": [{
                "type": "button",
                "action_id": action_id,
                "block_id": block_id,
                "text": button.text,
                "value": button.value,
                "style": button.style,
                "action_ts": seconds(now.as_millis()),
            }],
        });
        Ok(Click {
            webhook_id: bot.webhook_id,
            action_id: action_id.to_owned(),
            request_url,
            signing_secret,
            payload,
        })
    }

    /// Spends one use of a response token. `None` for an unknown, used-up or expired token, or a
    /// deleted webhook.
    pub async fn use_response_token(
        &self,
        webhook_id: Id,
        token: &str,
    ) -> Result<Option<ResponseTarget>, ChatError> {
        let now = TimestampMillis::now();
        let mut tx = self.database.immediate_transaction().await?;
        let row = sqlx::query(
            "SELECT r.message_id, r.conversation_id, r.user_id, h.workspace_id, h.name, h.icon_url, \
             h.created_by FROM chat_action_responses r \
             JOIN chat_webhooks h ON h.id = r.webhook_id AND h.revoked_at IS NULL \
             JOIN workspaces w ON w.id = h.workspace_id AND w.deleted_at IS NULL \
             WHERE r.token_hash = $1 AND r.webhook_id = $2 AND r.uses_left > 0 AND r.expires_at >= $3",
        )
        .bind(token_hash(token))
        .bind(webhook_id.to_string())
        .bind(now.as_millis())
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        sqlx::query(
            "UPDATE chat_action_responses SET uses_left = uses_left - 1 WHERE token_hash = $1",
        )
        .bind(token_hash(token))
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Some(ResponseTarget {
            message_id: parse_id(row.get("message_id"))?,
            user_id: parse_id(row.get("user_id"))?,
            target: HookTarget {
                workspace_id: parse_id(row.get("workspace_id"))?,
                conversation_id: parse_id(row.get("conversation_id"))?,
                sender: BotSender {
                    webhook_id,
                    created_by: parse_id(row.get("created_by"))?,
                    name: row.get("name"),
                    icon_url: row.get("icon_url"),
                },
            },
        }))
    }

    /// The message with the button, if it is still this webhook's and in an open channel.
    async fn original(
        conn: &mut sqlx::AnyConnection,
        response: &ResponseTarget,
    ) -> Result<MessageRecord, ChatError> {
        let conversation = load_conversation(
            conn,
            response.target.workspace_id,
            response.target.conversation_id,
        )
        .await?
        .filter(|conversation| conversation.kind != ConversationKind::Dm)
        .ok_or(ChatError::NotFound)?;
        if conversation.archived_at.is_some() {
            return Err(ChatError::Forbidden("This channel is archived."));
        }
        load_message(conn, response.message_id)
            .await?
            .filter(|message| {
                !message.deleted
                    && message.bot.as_ref().map(|bot| bot.webhook_id)
                        == Some(response.target.sender.webhook_id)
            })
            .ok_or(ChatError::NotFound)
    }

    /// `replace_original`: the message gets the answer's text, blocks, name and icon. It is not
    /// marked as edited, and its mentions are counted again.
    pub async fn replace_bot_message(
        &self,
        response: &ResponseTarget,
        post: BotPost,
    ) -> Result<Written<MessageRecord>, ChatError> {
        let (body, bot, blocks) = bot_message_parts(post, &response.target.sender)?;
        let mut tx = self.database.immediate_transaction().await?;
        let message = Self::original(&mut tx, response).await?;
        sqlx::query(
            "UPDATE chat_messages SET bot_name = $1, bot_icon_url = $2, bot_icon_emoji = $3 WHERE id = $4",
        )
        .bind(&bot.name)
        .bind(bot.icon_url.as_deref())
        .bind(bot.icon_emoji.as_deref())
        .bind(message.id.to_string())
        .execute(&mut *tx)
        .await?;
        let (events, next) =
            rewrite_message(&mut tx, &message, &body, blocks.as_deref(), None).await?;
        finish(tx, events, next).await
    }

    /// `delete_original`: the integration owns its message, so no member permission applies.
    pub async fn delete_bot_message(
        &self,
        response: &ResponseTarget,
    ) -> Result<Written<()>, ChatError> {
        let now = TimestampMillis::now();
        let mut tx = self.database.immediate_transaction().await?;
        let message = Self::original(&mut tx, response).await?;
        let events = remove_message(
            &mut tx,
            response.target.workspace_id,
            response.target.sender.webhook_id,
            &message,
            now,
        )
        .await?;
        finish(tx, events, ()).await
    }

    /// `ephemeral`: a message that only the member who clicked sees, live, until they reload. It
    /// is not stored.
    pub async fn ephemeral_bot_message(
        &self,
        response: &ResponseTarget,
        post: BotPost,
    ) -> Result<Written<MessageRecord>, ChatError> {
        let (body, bot, blocks) = bot_message_parts(post, &response.target.sender)?;
        let mut conn = self.database.pool().acquire().await?;
        let interactive = webhook_interactive(&mut conn, bot.webhook_id).await?;
        let message = MessageRecord {
            id: Id::new_v7(),
            conversation_id: response.target.conversation_id,
            thread_root_id: None,
            kind: MessageKind::Message,
            author_id: response.target.sender.created_by,
            mentions: MentionsRecord {
                user_ids: mentioned_user_ids(&body),
                channel: false,
                here: false,
            },
            body,
            created_at: TimestampMillis::now(),
            edited_at: None,
            deleted: false,
            attachments: Vec::new(),
            reactions: Vec::new(),
            pinned: false,
            also_in_channel: false,
            nonce: None,
            reply_count: 0,
            last_reply_at: None,
            reply_user_ids: Vec::new(),
            last_reply: None,
            reply_to_id: None,
            reply_to: None,
            forwarded: None,
            sticker_id: None,
            sticker: None,
            bot: Some(super::BotRecord { interactive, ..bot }),
            blocks,
            last_reply_id: None,
        };
        let mut events = Events::default();
        events.user(
            response.user_id,
            ChatEvent::MessageEphemeral {
                message: message.clone(),
            },
        );
        Ok(events.written(message))
    }
}
