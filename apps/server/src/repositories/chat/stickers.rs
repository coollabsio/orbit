//! Custom stickers: the named images of a workspace, sent as a message of their own. The bytes
//! live in the database, as the emoji do.

use orbit_domain::{Actor, Permission};
use orbit_platform::{Id, TimestampMillis};
use serde::Serialize;
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};
use utoipa::ToSchema;

use super::{ChatError, ChatEvent, ChatRepository, Events, Written, finish, load_actor, parse_id};

/// Largest accepted sticker image.
pub const STICKER_MAX_BYTES: usize = 512 * 1024;
/// Stickers in one workspace.
const MAX_STICKERS: i64 = 100;
const NAME_MIN_CHARS: usize = 2;
const NAME_MAX_CHARS: usize = 30;

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct CustomStickerRecord {
    #[schema(value_type = String)]
    pub id: Id,
    /// 2 to 30 characters, as its author typed it. Unique in the workspace whatever the case.
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

/// The sticker of a message.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct StickerRecord {
    #[schema(value_type = String)]
    pub id: Id,
    pub name: String,
    /// Same-origin image path. The image of an id never changes.
    pub url: String,
}

/// The stored image of a sticker.
#[derive(Clone, Debug)]
pub struct CustomStickerImage {
    pub mime_type: String,
    pub bytes: Vec<u8>,
}

pub(super) fn image_url(workspace_id: Id, id: Id) -> String {
    format!("/api/v1/workspaces/{workspace_id}/chat/stickers/{id}/image")
}

fn record(workspace_id: Id, row: &SqliteRow) -> Result<CustomStickerRecord, ChatError> {
    let id = parse_id(row.get("id"))?;
    Ok(CustomStickerRecord {
        id,
        name: row.get("name"),
        animated: row.get::<String, _>("mime_type") == "image/gif",
        url: image_url(workspace_id, id),
        created_by: parse_id(row.get("created_by"))?,
        created_at: TimestampMillis::from_millis(row.get("created_at")),
    })
}

/// Trimmed, then 2 to 30 characters, none of them a control character.
fn clean_name(name: &str) -> Result<String, ChatError> {
    let name = name.trim();
    let valid = (NAME_MIN_CHARS..=NAME_MAX_CHARS).contains(&name.chars().count())
        && !name.chars().any(char::is_control);
    if valid {
        Ok(name.to_owned())
    } else {
        Err(ChatError::Invalid { field: "name" })
    }
}

fn require_manager(actor: Actor) -> Result<(), ChatError> {
    if actor.can(Permission::ChatManage) {
        Ok(())
    } else {
        Err(ChatError::Forbidden(
            "Only workspace owners and admins can add and delete stickers.",
        ))
    }
}

/// The sticker of a new message: it must be one of the workspace.
pub(super) async fn load_sticker(
    conn: &mut SqliteConnection,
    workspace_id: Id,
    sticker_id: Id,
) -> Result<StickerRecord, ChatError> {
    let name: String =
        sqlx::query_scalar("SELECT name FROM custom_stickers WHERE id = ? AND workspace_id = ?")
            .bind(sticker_id.to_string())
            .bind(workspace_id.to_string())
            .fetch_optional(&mut *conn)
            .await?
            .ok_or(ChatError::NotFound)?;
    Ok(StickerRecord {
        id: sticker_id,
        name,
        url: image_url(workspace_id, sticker_id),
    })
}

impl ChatRepository {
    /// The stickers of the workspace by name, for every member.
    pub async fn list_stickers(
        &self,
        workspace_id: Id,
        actor_id: Id,
    ) -> Result<Vec<CustomStickerRecord>, ChatError> {
        let mut conn = self.database.pool().acquire().await?;
        load_actor(&mut conn, workspace_id, actor_id).await?;
        sqlx::query(
            "SELECT id, name, mime_type, created_by, created_at FROM custom_stickers \
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
    /// [`Self::create_sticker`] checks again in its transaction.
    pub async fn authorize_sticker(&self, workspace_id: Id, actor_id: Id) -> Result<(), ChatError> {
        let mut conn = self.database.pool().acquire().await?;
        require_manager(load_actor(&mut conn, workspace_id, actor_id).await?)
    }

    /// Adds a sticker. `mime_type` is what the caller read from the bytes.
    pub async fn create_sticker(
        &self,
        workspace_id: Id,
        actor_id: Id,
        name: &str,
        mime_type: &'static str,
        bytes: &[u8],
    ) -> Result<Written<CustomStickerRecord>, ChatError> {
        let now = TimestampMillis::now();
        let mut tx = self.database.immediate_transaction().await?;
        require_manager(load_actor(&mut tx, workspace_id, actor_id).await?)?;
        let name = clean_name(name)?;
        if bytes.is_empty() || bytes.len() > STICKER_MAX_BYTES {
            return Err(ChatError::Invalid { field: "file" });
        }
        // `name` is NOCASE: the comparison ignores the case of ASCII letters.
        let (count, taken): (i64, bool) = sqlx::query_as(
            "SELECT COUNT(*), COALESCE(MAX(name = ?), 0) FROM custom_stickers WHERE workspace_id = ?",
        )
        .bind(&name)
        .bind(workspace_id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        if taken {
            return Err(ChatError::StickerNameTaken);
        }
        if count >= MAX_STICKERS {
            return Err(ChatError::StickerLimit);
        }
        let id = Id::new_v7();
        sqlx::query(
            "INSERT INTO custom_stickers (id, workspace_id, name, mime_type, bytes, created_by, created_at) \
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
        events.workspace(ChatEvent::StickersChanged);
        finish(
            tx,
            events,
            CustomStickerRecord {
                id,
                name,
                animated: mime_type == "image/gif",
                url: image_url(workspace_id, id),
                created_by: actor_id,
                created_at: now,
            },
        )
        .await
    }

    /// Deletes a sticker. Messages that were sent with it keep its id.
    pub async fn delete_sticker(
        &self,
        workspace_id: Id,
        actor_id: Id,
        sticker_id: Id,
    ) -> Result<Written<()>, ChatError> {
        let mut tx = self.database.immediate_transaction().await?;
        require_manager(load_actor(&mut tx, workspace_id, actor_id).await?)?;
        let deleted = sqlx::query("DELETE FROM custom_stickers WHERE id = ? AND workspace_id = ?")
            .bind(sticker_id.to_string())
            .bind(workspace_id.to_string())
            .execute(&mut *tx)
            .await?
            .rows_affected();
        if deleted == 0 {
            return Err(ChatError::NotFound);
        }
        let mut events = Events::default();
        events.workspace(ChatEvent::StickersChanged);
        finish(tx, events, ()).await
    }

    /// The image of a sticker, for every member of its workspace.
    pub async fn sticker_image(
        &self,
        workspace_id: Id,
        actor_id: Id,
        sticker_id: Id,
    ) -> Result<CustomStickerImage, ChatError> {
        let mut conn = self.database.pool().acquire().await?;
        load_actor(&mut conn, workspace_id, actor_id).await?;
        let row = sqlx::query(
            "SELECT mime_type, bytes FROM custom_stickers WHERE id = ? AND workspace_id = ?",
        )
        .bind(sticker_id.to_string())
        .bind(workspace_id.to_string())
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(ChatError::NotFound)?;
        Ok(CustomStickerImage {
            mime_type: row.get("mime_type"),
            bytes: row.get("bytes"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sticker_name_is_trimmed_and_kept_as_typed() {
        assert_eq!(clean_name("  Party Parrot!  ").unwrap(), "Party Parrot!");
        assert_eq!(clean_name("ok").unwrap(), "ok");
        assert_eq!(clean_name("é🎉").unwrap(), "é🎉");
        assert_eq!(clean_name(&"é".repeat(30)).unwrap().chars().count(), 30);
        for bad in ["", "a", " a ", "tab\there", "line\nbreak", &"a".repeat(31)] {
            assert!(clean_name(bad).is_err(), "{bad:?} is accepted");
        }
    }
}
