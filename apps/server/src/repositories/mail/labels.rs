//! Mail labels (spec §5): one set for the workspace, names unique without regard to case.
//! Members read them (to show and apply them); owners and admins create, change and delete.

use orbit_platform::{Id, Row, TimestampMillis};
use serde_json::json;
use sqlx::any::AnyRow;
use sqlx::{Any, Transaction};

use super::domains::{manager, parse_id};
use super::{MailError, MailRepository, record_mail_audit};
use crate::repositories::membership;

const LABEL_NAME_MAX_CHARS: usize = 80;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Label {
    pub id: Id,
    pub name: String,
    /// `#rrggbb`, lower case.
    pub color: String,
    pub created_at: TimestampMillis,
    pub updated_at: TimestampMillis,
}

fn label(row: &AnyRow) -> Result<Label, MailError> {
    Ok(Label {
        id: parse_id(&row.try_get::<String, _>("id")?)?,
        name: row.try_get("name")?,
        color: row.try_get("color")?,
        created_at: TimestampMillis::from_millis(row.try_get("created_at")?),
        updated_at: TimestampMillis::from_millis(row.try_get("updated_at")?),
    })
}

fn clean_label_name(name: &str) -> Result<String, MailError> {
    let name = name.trim();
    let count = name.chars().count();
    if count == 0 || count > LABEL_NAME_MAX_CHARS || name.chars().any(char::is_control) {
        return Err(MailError::Invalid { field: "name" });
    }
    Ok(name.to_owned())
}

/// `#rrggbb` (any case) → lower case.
fn clean_color(color: &str) -> Result<String, MailError> {
    let color = color.trim();
    if color.len() == 7
        && color.starts_with('#')
        && color[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        Ok(color.to_ascii_lowercase())
    } else {
        Err(MailError::Invalid { field: "color" })
    }
}

async fn load(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
    label_id: Id,
) -> Result<Label, MailError> {
    let row = sqlx::query(
        "SELECT id, name, color, created_at, updated_at FROM mail_labels \
         WHERE id = $1 AND workspace_id = $2",
    )
    .bind(label_id.to_string())
    .bind(workspace_id.to_string())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(MailError::NotFound)?;
    label(&row)
}

/// Refuses a name another label of the workspace has, whatever its case (the same `lower()` as
/// the unique index).
async fn check_name_free(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
    name: &str,
    except: Option<Id>,
) -> Result<(), MailError> {
    let found: Option<String> = sqlx::query_scalar(
        "SELECT id FROM mail_labels WHERE workspace_id = $1 AND lower(name) = lower($2) \
         AND id <> $3",
    )
    .bind(workspace_id.to_string())
    .bind(name)
    .bind(except.map_or_else(String::new, |id| id.to_string()))
    .fetch_optional(&mut **transaction)
    .await?;
    if found.is_some() {
        Err(MailError::Conflict("label_exists"))
    } else {
        Ok(())
    }
}

/// The labels of the workspace by name.
pub(super) async fn load_labels(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
) -> Result<Vec<Label>, MailError> {
    let rows = sqlx::query(
        "SELECT id, name, color, created_at, updated_at FROM mail_labels \
         WHERE workspace_id = $1 ORDER BY lower(name), id",
    )
    .bind(workspace_id.to_string())
    .fetch_all(&mut **transaction)
    .await?;
    rows.iter().map(label).collect()
}

impl MailRepository {
    /// The labels of the workspace by name, for any member.
    pub async fn list_labels(
        &self,
        workspace_id: Id,
        actor_id: Id,
    ) -> Result<Vec<Label>, MailError> {
        let mut transaction = self.database.pool().begin().await?;
        membership::actor(&mut *transaction, workspace_id, actor_id)
            .await?
            .ok_or(MailError::NotFound)?;
        let labels = load_labels(&mut transaction, workspace_id).await?;
        transaction.commit().await?;
        Ok(labels)
    }

    pub async fn create_label(
        &self,
        workspace_id: Id,
        actor_id: Id,
        name: &str,
        color: &str,
        request_id: &str,
    ) -> Result<Label, MailError> {
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        manager(&mut transaction, workspace_id, actor_id).await?;
        let name = clean_label_name(name)?;
        let color = clean_color(color)?;
        check_name_free(&mut transaction, workspace_id, &name, None).await?;
        let label_id = Id::new_v7();
        sqlx::query(
            "INSERT INTO mail_labels (id, workspace_id, name, color, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $5)",
        )
        .bind(label_id.to_string())
        .bind(workspace_id.to_string())
        .bind(&name)
        .bind(&color)
        .bind(now.as_millis())
        .execute(&mut *transaction)
        .await?;
        record_mail_audit(
            &mut transaction,
            workspace_id,
            actor_id,
            "mail.label_created",
            "mail_label",
            label_id,
            request_id,
            json!({ "name": name, "color": color }),
            now,
        )
        .await?;
        let created = load(&mut transaction, workspace_id, label_id).await?;
        transaction.commit().await?;
        Ok(created)
    }

    /// Renames a label or changes its color; `None` keeps the value.
    pub async fn update_label(
        &self,
        workspace_id: Id,
        actor_id: Id,
        label_id: Id,
        name: Option<&str>,
        color: Option<&str>,
        request_id: &str,
    ) -> Result<Label, MailError> {
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        manager(&mut transaction, workspace_id, actor_id).await?;
        let found = load(&mut transaction, workspace_id, label_id).await?;
        let name = name
            .map(clean_label_name)
            .transpose()?
            .unwrap_or(found.name.clone());
        let color = color
            .map(clean_color)
            .transpose()?
            .unwrap_or(found.color.clone());
        if name == found.name && color == found.color {
            transaction.commit().await?;
            return Ok(found);
        }
        check_name_free(&mut transaction, workspace_id, &name, Some(label_id)).await?;
        sqlx::query("UPDATE mail_labels SET name = $1, color = $2, updated_at = $3 WHERE id = $4")
            .bind(&name)
            .bind(&color)
            .bind(now.as_millis())
            .bind(label_id.to_string())
            .execute(&mut *transaction)
            .await?;
        record_mail_audit(
            &mut transaction,
            workspace_id,
            actor_id,
            "mail.label_updated",
            "mail_label",
            label_id,
            request_id,
            json!({ "name": name, "color": color }),
            now,
        )
        .await?;
        let updated = load(&mut transaction, workspace_id, label_id).await?;
        transaction.commit().await?;
        Ok(updated)
    }

    /// Deletes a label; threads lose it.
    pub async fn delete_label(
        &self,
        workspace_id: Id,
        actor_id: Id,
        label_id: Id,
        request_id: &str,
    ) -> Result<(), MailError> {
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        manager(&mut transaction, workspace_id, actor_id).await?;
        let found = load(&mut transaction, workspace_id, label_id).await?;
        sqlx::query("DELETE FROM mail_labels WHERE id = $1")
            .bind(label_id.to_string())
            .execute(&mut *transaction)
            .await?;
        record_mail_audit(
            &mut transaction,
            workspace_id,
            actor_id,
            "mail.label_deleted",
            "mail_label",
            label_id,
            request_id,
            json!({ "name": found.name }),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }
}
