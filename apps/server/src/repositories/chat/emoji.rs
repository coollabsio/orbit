//! Custom emoji: the small named images of a workspace, written `:name:` in a message and as a
//! reaction. The bytes live in the database, as profile pictures do.

use orbit_domain::{Actor, Permission};
use orbit_platform::{Id, TimestampMillis};
use serde::Serialize;
use sqlx::Row;
use sqlx::sqlite::SqliteRow;
use utoipa::ToSchema;

use super::{ChatError, ChatEvent, ChatRepository, Events, Written, finish, load_actor, parse_id};

/// Largest accepted emoji image.
pub const EMOJI_MAX_BYTES: usize = 256 * 1024;
/// Emoji in one workspace.
const MAX_EMOJI: i64 = 200;
const NAME_MIN_CHARS: usize = 2;
const NAME_MAX_CHARS: usize = 32;

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct CustomEmojiRecord {
    #[schema(value_type = String)]
    pub id: Id,
    /// 2 to 32 characters of `a-z 0-9 _`; written `:name:`.
    pub name: String,
    /// A GIF.
    pub animated: bool,
    /// Same-origin image path. The image of an id never changes.
    pub url: String,
    #[schema(value_type = String)]
    pub created_by: Id,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
}

/// The stored image of an emoji.
#[derive(Clone, Debug)]
pub struct CustomEmojiImage {
    pub mime_type: String,
    pub bytes: Vec<u8>,
}

fn record(workspace_id: Id, row: &SqliteRow) -> Result<CustomEmojiRecord, ChatError> {
    let id = parse_id(row.get("id"))?;
    Ok(CustomEmojiRecord {
        id,
        name: row.get("name"),
        animated: row.get::<String, _>("mime_type") == "image/gif",
        url: format!("/api/v1/workspaces/{workspace_id}/chat/emoji/{id}/image"),
        created_by: parse_id(row.get("created_by"))?,
        created_at: TimestampMillis::from_millis(row.get("created_at")),
    })
}

/// Lower case, then 2 to 32 characters of `a-z 0-9 _`.
fn clean_name(name: &str) -> Result<String, ChatError> {
    let name = name.to_ascii_lowercase();
    let valid = (NAME_MIN_CHARS..=NAME_MAX_CHARS).contains(&name.len())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');
    if valid {
        Ok(name)
    } else {
        Err(ChatError::Invalid { field: "name" })
    }
}

fn require_manager(actor: Actor) -> Result<(), ChatError> {
    if actor.can(Permission::ChatManage) {
        Ok(())
    } else {
        Err(ChatError::Forbidden(
            "Only workspace owners and admins can add and delete emoji.",
        ))
    }
}

impl ChatRepository {
    /// The emoji of the workspace by name, for every member.
    pub async fn list_emoji(
        &self,
        workspace_id: Id,
        actor_id: Id,
    ) -> Result<Vec<CustomEmojiRecord>, ChatError> {
        let mut conn = self.database.pool().acquire().await?;
        load_actor(&mut conn, workspace_id, actor_id).await?;
        sqlx::query(
            "SELECT id, name, mime_type, created_by, created_at FROM custom_emoji \
             WHERE workspace_id = ? ORDER BY name",
        )
        .bind(workspace_id.to_string())
        .fetch_all(&mut *conn)
        .await?
        .iter()
        .map(|row| record(workspace_id, row))
        .collect()
    }

    /// Succeeds for a chat manager of the live workspace. Run before reading an upload body;
    /// [`Self::create_emoji`] checks again in its transaction.
    pub async fn authorize_emoji(&self, workspace_id: Id, actor_id: Id) -> Result<(), ChatError> {
        let mut conn = self.database.pool().acquire().await?;
        require_manager(load_actor(&mut conn, workspace_id, actor_id).await?)
    }

    /// Adds an emoji. `mime_type` is what the caller read from the bytes.
    pub async fn create_emoji(
        &self,
        workspace_id: Id,
        actor_id: Id,
        name: &str,
        mime_type: &'static str,
        bytes: &[u8],
    ) -> Result<Written<CustomEmojiRecord>, ChatError> {
        let now = TimestampMillis::now();
        let mut tx = self.database.immediate_transaction().await?;
        require_manager(load_actor(&mut tx, workspace_id, actor_id).await?)?;
        let name = clean_name(name)?;
        if bytes.is_empty() || bytes.len() > EMOJI_MAX_BYTES {
            return Err(ChatError::Invalid { field: "file" });
        }
        let (count, taken): (i64, bool) = sqlx::query_as(
            "SELECT COUNT(*), COALESCE(MAX(name = ?), 0) FROM custom_emoji WHERE workspace_id = ?",
        )
        .bind(&name)
        .bind(workspace_id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        if taken {
            return Err(ChatError::EmojiNameTaken);
        }
        if count >= MAX_EMOJI {
            return Err(ChatError::EmojiLimit);
        }
        let id = Id::new_v7();
        sqlx::query(
            "INSERT INTO custom_emoji (id, workspace_id, name, mime_type, bytes, created_by, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id.to_string())
        .bind(workspace_id.to_string())
        .bind(&name)
        .bind(mime_type)
        .bind(bytes)
        .bind(actor_id.to_string())
        .bind(now.as_millis())
        .execute(&mut *tx)
        .await?;
        let mut events = Events::default();
        events.workspace(ChatEvent::EmojiChanged);
        finish(
            tx,
            events,
            CustomEmojiRecord {
                id,
                name,
                animated: mime_type == "image/gif",
                url: format!("/api/v1/workspaces/{workspace_id}/chat/emoji/{id}/image"),
                created_by: actor_id,
                created_at: now,
            },
        )
        .await
    }

    /// Deletes an emoji. Messages and reactions that name it keep their `:name:` text.
    pub async fn delete_emoji(
        &self,
        workspace_id: Id,
        actor_id: Id,
        emoji_id: Id,
    ) -> Result<Written<()>, ChatError> {
        let mut tx = self.database.immediate_transaction().await?;
        require_manager(load_actor(&mut tx, workspace_id, actor_id).await?)?;
        let deleted = sqlx::query("DELETE FROM custom_emoji WHERE id = ? AND workspace_id = ?")
            .bind(emoji_id.to_string())
            .bind(workspace_id.to_string())
            .execute(&mut *tx)
            .await?
            .rows_affected();
        if deleted == 0 {
            return Err(ChatError::NotFound);
        }
        let mut events = Events::default();
        events.workspace(ChatEvent::EmojiChanged);
        finish(tx, events, ()).await
    }

    /// The image of an emoji, for every member of its workspace.
    pub async fn emoji_image(
        &self,
        workspace_id: Id,
        actor_id: Id,
        emoji_id: Id,
    ) -> Result<CustomEmojiImage, ChatError> {
        let mut conn = self.database.pool().acquire().await?;
        load_actor(&mut conn, workspace_id, actor_id).await?;
        let row = sqlx::query(
            "SELECT mime_type, bytes FROM custom_emoji WHERE id = ? AND workspace_id = ?",
        )
        .bind(emoji_id.to_string())
        .bind(workspace_id.to_string())
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(ChatError::NotFound)?;
        Ok(CustomEmojiImage {
            mime_type: row.get("mime_type"),
            bytes: row.get("bytes"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_emoji_name_is_lower_case_letters_digits_and_underscores() {
        assert_eq!(clean_name("Party_Parrot2").unwrap(), "party_parrot2");
        assert_eq!(clean_name("ok").unwrap(), "ok");
        assert_eq!(clean_name(&"a".repeat(32)).unwrap().len(), 32);
        for bad in [
            "",
            "a",
            "with space",
            "dash-ed",
            ":colon:",
            "é_e",
            &"a".repeat(33),
        ] {
            assert!(clean_name(bad).is_err(), "{bad:?} is accepted");
        }
    }
}
