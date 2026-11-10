//! Incoming webhooks: a secret URL that posts messages into one channel as an integration
//! (`chat_routes::hooks`). Only a SHA-256 hash of the secret is stored, so the URL is shown once,
//! when the webhook is made or its secret rotated. The channel's creator and the workspace's
//! owners and admins manage them; a webhook keeps working when the member who made it leaves.

use orbit_platform::{Id, Row, TimestampMillis, generate_opaque_token};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::any::AnyRow;
use subtle::ConstantTimeEq;
use utoipa::ToSchema;

use super::{ChatError, ChatRepository, load_access, parse_id};

/// Live webhooks in one channel.
const MAX_WEBHOOKS_PER_CHANNEL: i64 = 10;
const NAME_MAX_CHARS: usize = 80;
const ICON_URL_MAX_CHARS: usize = 2048;

const WEBHOOK_COLUMNS: &str = "id, conversation_id, name, icon_url, created_by, created_at, rotated_at, last_used_at, request_url";
/// Deliveries kept for each webhook.
const MAX_DELIVERIES: i64 = 20;

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct WebhookRecord {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub conversation_id: Id,
    /// The author name of its messages, unless a message sets `username`.
    pub name: String,
    /// An https image for the avatar of its messages; null for the default one.
    #[schema(required = true)]
    pub icon_url: Option<String>,
    #[schema(value_type = String)]
    pub created_by: Id,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
    #[schema(value_type = Option<String>, format = DateTime, required = true)]
    pub rotated_at: Option<TimestampMillis>,
    #[schema(value_type = Option<String>, format = DateTime, required = true)]
    pub last_used_at: Option<TimestampMillis>,
    /// Where clicks on its buttons go; null when its buttons are not interactive.
    #[schema(required = true)]
    pub request_url: Option<String>,
}

/// One click that Orbit sent to a webhook's request URL.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct DeliveryRecord {
    #[schema(value_type = String)]
    pub id: Id,
    pub action_id: String,
    /// The HTTP status of the answer; null when there was none.
    #[schema(required = true)]
    pub status: Option<i64>,
    /// Why there was no good answer: `timeout`, `refused`, `blocked_address`, …
    #[schema(required = true)]
    pub error: Option<String>,
    pub duration_ms: i64,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
}

/// A webhook and the secret of its URL, which no later read returns.
#[derive(Clone, Debug)]
pub struct WebhookWithSecret {
    pub webhook: WebhookRecord,
    pub secret: String,
}

/// The name and icon of a webhook.
#[derive(Clone, Debug)]
pub struct WebhookInput {
    pub name: String,
    pub icon_url: Option<String>,
}

/// The integration a bot message is posted as, and the member who set it up (the message's
/// `author_id`).
#[derive(Clone, Debug)]
pub struct BotSender {
    pub webhook_id: Id,
    pub created_by: Id,
    pub name: String,
    pub icon_url: Option<String>,
}

/// Where a webhook whose secret matched posts.
#[derive(Clone, Debug)]
pub struct HookTarget {
    pub workspace_id: Id,
    pub conversation_id: Id,
    pub sender: BotSender,
}

/// A trimmed name of 1 to 80 characters. `field` names the input in the error.
pub(super) fn clean_bot_name(name: &str, field: &'static str) -> Result<String, ChatError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > NAME_MAX_CHARS {
        return Err(ChatError::Invalid { field });
    }
    Ok(name.to_owned())
}

/// An absolute https URL: the production CSP loads no plain http images.
pub(super) fn clean_icon_url(url: &str, field: &'static str) -> Result<String, ChatError> {
    let url = url.trim();
    let valid = url.chars().count() <= ICON_URL_MAX_CHARS
        && url::Url::parse(url)
            .is_ok_and(|parsed| parsed.scheme() == "https" && parsed.host_str().is_some());
    if valid {
        Ok(url.to_owned())
    } else {
        Err(ChatError::Invalid { field })
    }
}

fn clean_input(input: WebhookInput) -> Result<WebhookInput, ChatError> {
    Ok(WebhookInput {
        name: clean_bot_name(&input.name, "name")?,
        icon_url: input
            .icon_url
            .as_deref()
            .filter(|url| !url.trim().is_empty())
            .map(|url| clean_icon_url(url, "icon_url"))
            .transpose()?,
    })
}

fn new_secret() -> (String, [u8; 32]) {
    let secret = generate_opaque_token();
    let hash = Sha256::digest(secret.as_bytes()).into();
    (secret, hash)
}

fn webhook_from_row(row: &AnyRow) -> Result<WebhookRecord, ChatError> {
    let time = |column: &str| {
        row.get::<Option<i64>, _>(column)
            .map(TimestampMillis::from_millis)
    };
    Ok(WebhookRecord {
        id: parse_id(row.get("id"))?,
        conversation_id: parse_id(row.get("conversation_id"))?,
        name: row.get("name"),
        icon_url: row.get("icon_url"),
        created_by: parse_id(row.get("created_by"))?,
        created_at: TimestampMillis::from_millis(row.get("created_at")),
        rotated_at: time("rotated_at"),
        last_used_at: time("last_used_at"),
        request_url: row.get("request_url"),
    })
}

/// A request URL Orbit may send clicks to: https on a public host. `development` also takes http
/// and local hosts, so a receiver on the developer's machine can be tested. The address of a name
/// is checked again for each click (`PublicResolver`).
pub(crate) fn clean_request_url(url: &str, development: bool) -> Result<String, ChatError> {
    let invalid = ChatError::Invalid {
        field: "request_url",
    };
    let url = url.trim();
    if url.chars().count() > 2048 {
        return Err(invalid);
    }
    let parsed = url::Url::parse(url).map_err(|_| ChatError::Invalid {
        field: "request_url",
    })?;
    let scheme_ok = parsed.scheme() == "https" || (development && parsed.scheme() == "http");
    let public = match parsed.host() {
        Some(url::Host::Ipv4(ip)) => orbit_platform::is_public_ip(ip.into()),
        Some(url::Host::Ipv6(ip)) => orbit_platform::is_public_ip(ip.into()),
        Some(url::Host::Domain(name)) => {
            let name = name.to_ascii_lowercase();
            name.contains('.')
                && ![".localhost", ".local", ".internal", ".lan", ".home.arpa"]
                    .iter()
                    .any(|suffix| name.ends_with(suffix))
        }
        None => false,
    };
    if !scheme_ok || parsed.host().is_none() || !(public || development) {
        return Err(invalid);
    }
    Ok(url.to_owned())
}

fn encrypt(key: &[u8; 32], secret: &str) -> Result<Vec<u8>, ChatError> {
    crate::secret_box::encrypt_secret(key, secret)
        .map_err(|()| ChatError::Conflict("The signing secret could not be stored."))
}

pub(super) fn decrypt(key: &[u8; 32], stored: &[u8]) -> Result<String, ChatError> {
    crate::secret_box::decrypt_secret(key, stored).map_err(|()| {
        ChatError::Conflict(
            "The signing secret cannot be read with this server's app key. Rotate it.",
        )
    })
}

async fn load_webhook(
    conn: &mut sqlx::AnyConnection,
    conversation_id: Id,
    webhook_id: Id,
) -> Result<WebhookRecord, ChatError> {
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT {WEBHOOK_COLUMNS} FROM chat_webhooks \
         WHERE id = $1 AND conversation_id = $2 AND revoked_at IS NULL"
    )))
    .bind(webhook_id.to_string())
    .bind(conversation_id.to_string())
    .fetch_optional(&mut *conn)
    .await?
    .as_ref()
    .map(webhook_from_row)
    .transpose()?
    .ok_or(ChatError::NotFound)
}

impl ChatRepository {
    /// The live webhooks of a channel, oldest first. Managers of the channel only.
    pub async fn list_webhooks(
        &self,
        workspace_id: Id,
        actor_id: Id,
        conversation_id: Id,
    ) -> Result<Vec<WebhookRecord>, ChatError> {
        let mut conn = self.database.pool().acquire().await?;
        let access = load_access(&mut conn, workspace_id, actor_id, conversation_id).await?;
        access.require_manage()?;
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {WEBHOOK_COLUMNS} FROM chat_webhooks \
             WHERE conversation_id = $1 AND revoked_at IS NULL ORDER BY created_at, id"
        )))
        .bind(access.conversation.id.to_string())
        .fetch_all(&mut *conn)
        .await?
        .iter()
        .map(webhook_from_row)
        .collect()
    }

    pub async fn create_webhook(
        &self,
        workspace_id: Id,
        actor_id: Id,
        conversation_id: Id,
        input: WebhookInput,
    ) -> Result<WebhookWithSecret, ChatError> {
        let input = clean_input(input)?;
        let now = TimestampMillis::now();
        let mut tx = self.database.immediate_transaction().await?;
        let access = load_access(&mut tx, workspace_id, actor_id, conversation_id).await?;
        access.require_manage()?;
        access.require_open()?;
        let live: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM chat_webhooks WHERE conversation_id = $1 AND revoked_at IS NULL",
        )
        .bind(conversation_id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        if live >= MAX_WEBHOOKS_PER_CHANNEL {
            return Err(ChatError::Conflict(
                "A channel can have at most 10 incoming webhooks.",
            ));
        }
        let id = Id::new_v7();
        let (secret, hash) = new_secret();
        sqlx::query(
            "INSERT INTO chat_webhooks (id, workspace_id, conversation_id, name, icon_url, \
             secret_hash, created_by, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(id.to_string())
        .bind(workspace_id.to_string())
        .bind(conversation_id.to_string())
        .bind(&input.name)
        .bind(input.icon_url.as_deref())
        .bind(hash.to_vec())
        .bind(actor_id.to_string())
        .bind(now.as_millis())
        .execute(&mut *tx)
        .await?;
        let webhook = load_webhook(&mut tx, conversation_id, id).await?;
        tx.commit().await?;
        Ok(WebhookWithSecret { webhook, secret })
    }

    /// Renames a webhook or changes its icon. Messages it posted keep their name and icon.
    pub async fn update_webhook(
        &self,
        workspace_id: Id,
        actor_id: Id,
        conversation_id: Id,
        webhook_id: Id,
        input: WebhookInput,
    ) -> Result<WebhookRecord, ChatError> {
        let input = clean_input(input)?;
        let mut tx = self.database.immediate_transaction().await?;
        let access = load_access(&mut tx, workspace_id, actor_id, conversation_id).await?;
        access.require_manage()?;
        access.require_open()?;
        load_webhook(&mut tx, conversation_id, webhook_id).await?;
        sqlx::query("UPDATE chat_webhooks SET name = $1, icon_url = $2 WHERE id = $3")
            .bind(&input.name)
            .bind(input.icon_url.as_deref())
            .bind(webhook_id.to_string())
            .execute(&mut *tx)
            .await?;
        let webhook = load_webhook(&mut tx, conversation_id, webhook_id).await?;
        tx.commit().await?;
        Ok(webhook)
    }

    /// A new secret for the same webhook. The old URL stops working at once.
    pub async fn rotate_webhook(
        &self,
        workspace_id: Id,
        actor_id: Id,
        conversation_id: Id,
        webhook_id: Id,
    ) -> Result<WebhookWithSecret, ChatError> {
        let now = TimestampMillis::now();
        let mut tx = self.database.immediate_transaction().await?;
        let access = load_access(&mut tx, workspace_id, actor_id, conversation_id).await?;
        access.require_manage()?;
        access.require_open()?;
        load_webhook(&mut tx, conversation_id, webhook_id).await?;
        let (secret, hash) = new_secret();
        sqlx::query("UPDATE chat_webhooks SET secret_hash = $1, rotated_at = $2 WHERE id = $3")
            .bind(hash.to_vec())
            .bind(now.as_millis())
            .bind(webhook_id.to_string())
            .execute(&mut *tx)
            .await?;
        let webhook = load_webhook(&mut tx, conversation_id, webhook_id).await?;
        tx.commit().await?;
        Ok(WebhookWithSecret { webhook, secret })
    }

    /// Sets where clicks on the webhook's buttons go (`None` turns clicks off). The first URL makes
    /// the signing secret, which stays when the URL changes.
    #[allow(clippy::too_many_arguments)]
    pub async fn set_request_url(
        &self,
        workspace_id: Id,
        actor_id: Id,
        conversation_id: Id,
        webhook_id: Id,
        url: Option<&str>,
        key: &[u8; 32],
        development: bool,
    ) -> Result<WebhookRecord, ChatError> {
        let url = url
            .filter(|url| !url.trim().is_empty())
            .map(|url| clean_request_url(url, development))
            .transpose()?;
        let mut tx = self.database.immediate_transaction().await?;
        let access = load_access(&mut tx, workspace_id, actor_id, conversation_id).await?;
        access.require_manage()?;
        load_webhook(&mut tx, conversation_id, webhook_id).await?;
        let has_secret: Option<Vec<u8>> =
            sqlx::query_scalar("SELECT signing_secret FROM chat_webhooks WHERE id = $1")
                .bind(webhook_id.to_string())
                .fetch_one(&mut *tx)
                .await?;
        let secret = match has_secret {
            Some(stored) => stored,
            None => encrypt(key, &generate_opaque_token())?,
        };
        sqlx::query("UPDATE chat_webhooks SET request_url = $1, signing_secret = $2 WHERE id = $3")
            .bind(url.as_deref())
            .bind(secret)
            .bind(webhook_id.to_string())
            .execute(&mut *tx)
            .await?;
        let webhook = load_webhook(&mut tx, conversation_id, webhook_id).await?;
        tx.commit().await?;
        Ok(webhook)
    }

    /// The signing secret of a webhook, for its managers. `rotate` makes a new one first: the
    /// integration must then check clicks with the new secret.
    pub async fn signing_secret(
        &self,
        workspace_id: Id,
        actor_id: Id,
        conversation_id: Id,
        webhook_id: Id,
        key: &[u8; 32],
        rotate: bool,
    ) -> Result<String, ChatError> {
        let mut tx = self.database.immediate_transaction().await?;
        let access = load_access(&mut tx, workspace_id, actor_id, conversation_id).await?;
        access.require_manage()?;
        load_webhook(&mut tx, conversation_id, webhook_id).await?;
        let stored: Option<Vec<u8>> =
            sqlx::query_scalar("SELECT signing_secret FROM chat_webhooks WHERE id = $1")
                .bind(webhook_id.to_string())
                .fetch_one(&mut *tx)
                .await?;
        let secret = match stored {
            Some(stored) if !rotate => decrypt(key, &stored)?,
            None if !rotate => {
                return Err(ChatError::Conflict("Set a request URL first."));
            }
            _ => {
                let secret = generate_opaque_token();
                sqlx::query("UPDATE chat_webhooks SET signing_secret = $1 WHERE id = $2")
                    .bind(encrypt(key, &secret)?)
                    .bind(webhook_id.to_string())
                    .execute(&mut *tx)
                    .await?;
                secret
            }
        };
        tx.commit().await?;
        Ok(secret)
    }

    /// The newest clicks sent to a webhook, newest first.
    pub async fn list_deliveries(
        &self,
        workspace_id: Id,
        actor_id: Id,
        conversation_id: Id,
        webhook_id: Id,
    ) -> Result<Vec<DeliveryRecord>, ChatError> {
        let mut conn = self.database.pool().acquire().await?;
        let access = load_access(&mut conn, workspace_id, actor_id, conversation_id).await?;
        access.require_manage()?;
        load_webhook(&mut conn, conversation_id, webhook_id).await?;
        sqlx::query(
            "SELECT id, action_id, status, error, duration_ms, created_at FROM chat_webhook_deliveries \
             WHERE webhook_id = $1 ORDER BY created_at DESC, id DESC LIMIT $2",
        )
        .bind(webhook_id.to_string())
        .bind(MAX_DELIVERIES)
        .fetch_all(&mut *conn)
        .await?
        .iter()
        .map(|row| {
            Ok(DeliveryRecord {
                id: parse_id(row.get("id"))?,
                action_id: row.get("action_id"),
                status: row.get("status"),
                error: row.get("error"),
                duration_ms: row.get("duration_ms"),
                created_at: TimestampMillis::from_millis(row.get("created_at")),
            })
        })
        .collect()
    }

    /// Keeps the result of one click for the webhook's list, and drops the ones past the newest 20.
    pub async fn record_delivery(
        &self,
        webhook_id: Id,
        action_id: &str,
        status: Option<u16>,
        error: Option<&str>,
        duration_ms: i64,
    ) -> Result<(), ChatError> {
        let mut tx = self.database.immediate_transaction().await?;
        sqlx::query(
            "INSERT INTO chat_webhook_deliveries (id, webhook_id, action_id, status, error, duration_ms, \
             created_at) VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(Id::new_v7().to_string())
        .bind(webhook_id.to_string())
        .bind(action_id)
        .bind(status.map(i64::from))
        .bind(error)
        .bind(duration_ms)
        .bind(TimestampMillis::now().as_millis())
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "DELETE FROM chat_webhook_deliveries WHERE webhook_id = $1 AND id NOT IN \
             (SELECT id FROM chat_webhook_deliveries WHERE webhook_id = $1 \
             ORDER BY created_at DESC, id DESC LIMIT $2)",
        )
        .bind(webhook_id.to_string())
        .bind(MAX_DELIVERIES)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Deletes a webhook for good. The messages it posted stay.
    pub async fn revoke_webhook(
        &self,
        workspace_id: Id,
        actor_id: Id,
        conversation_id: Id,
        webhook_id: Id,
    ) -> Result<(), ChatError> {
        let now = TimestampMillis::now();
        let mut tx = self.database.immediate_transaction().await?;
        let access = load_access(&mut tx, workspace_id, actor_id, conversation_id).await?;
        access.require_manage()?;
        load_webhook(&mut tx, conversation_id, webhook_id).await?;
        sqlx::query("UPDATE chat_webhooks SET revoked_at = $1 WHERE id = $2")
            .bind(now.as_millis())
            .bind(webhook_id.to_string())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// The live webhook whose id and secret match, in a workspace that is not trashed. `None`
    /// for an unknown id, a wrong secret and a revoked webhook alike.
    pub async fn find_webhook(
        &self,
        webhook_id: Id,
        secret: &str,
    ) -> Result<Option<HookTarget>, ChatError> {
        let row = sqlx::query(
            "SELECT h.workspace_id, h.conversation_id, h.name, h.icon_url, h.created_by, \
             h.secret_hash FROM chat_webhooks h JOIN workspaces w ON w.id = h.workspace_id \
             WHERE h.id = $1 AND h.revoked_at IS NULL AND w.deleted_at IS NULL",
        )
        .bind(webhook_id.to_string())
        .fetch_optional(self.database.pool())
        .await?;
        let supplied: [u8; 32] = Sha256::digest(secret.as_bytes()).into();
        let Some(row) = row.filter(|row| {
            bool::from(
                row.get::<Vec<u8>, _>("secret_hash")
                    .as_slice()
                    .ct_eq(&supplied),
            )
        }) else {
            return Ok(None);
        };
        Ok(Some(HookTarget {
            workspace_id: parse_id(row.get("workspace_id"))?,
            conversation_id: parse_id(row.get("conversation_id"))?,
            sender: BotSender {
                webhook_id,
                created_by: parse_id(row.get("created_by"))?,
                name: row.get("name"),
                icon_url: row.get("icon_url"),
            },
        }))
    }
}
