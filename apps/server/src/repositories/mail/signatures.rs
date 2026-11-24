//! Sending identities of a member (spec §7, D13): the addresses the member can send as, their
//! signatures, personal snippets and the member's mail settings. Everything is scoped by the
//! caller's membership: an id of another member's row is `NotFound`.

use std::collections::HashMap;

use orbit_platform::{Id, Row, TimestampMillis};
use sqlx::any::AnyRow;
use sqlx::{Any, Transaction};

use super::domains::parse_id;
use super::reading::reader;
use super::{MailError, MailRepository, SendReadiness};

/// The undo-send delays in seconds; the first of the default is 10.
pub const UNDO_SEND_SECONDS: [i64; 5] = [0, 5, 10, 20, 30];
const DEFAULT_UNDO_SECONDS: i64 = 10;
const SIGNATURE_MAX_BYTES: usize = 20_000;
const SNIPPET_MAX_BYTES: usize = 100_000;
const SNIPPET_NAME_MAX_CHARS: usize = 80;

/// An address a member can send as.
#[derive(Clone, Debug)]
pub struct FromAddress {
    pub address_id: Id,
    pub mailbox_id: Id,
    pub address: String,
    /// The sender name on the mail: the address's own, else the mailbox name.
    pub display_name: String,
    pub signature_html: Option<String>,
    pub ready: SendReadiness,
    /// The largest message the route of the address's domain takes; `None` without a route.
    pub max_message_bytes: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snippet {
    pub id: Id,
    pub name: String,
    pub body_html: String,
    pub created_at: TimestampMillis,
    pub updated_at: TimestampMillis,
}

fn snippet(row: &AnyRow) -> Result<Snippet, MailError> {
    Ok(Snippet {
        id: parse_id(&row.try_get::<String, _>("id")?)?,
        name: row.try_get("name")?,
        body_html: row.try_get("body_html")?,
        created_at: TimestampMillis::from_millis(row.try_get("created_at")?),
        updated_at: TimestampMillis::from_millis(row.try_get("updated_at")?),
    })
}

fn clean_snippet_name(name: &str) -> Result<String, MailError> {
    let name = name.trim();
    let count = name.chars().count();
    if count == 0 || count > SNIPPET_NAME_MAX_CHARS || name.chars().any(char::is_control) {
        return Err(MailError::Invalid { field: "name" });
    }
    Ok(name.to_owned())
}

fn clean_body(body: &str, max: usize, field: &'static str) -> Result<String, MailError> {
    if body.len() > max {
        return Err(MailError::Invalid { field });
    }
    Ok(orbit_mail::clean_html(body))
}

/// `NotFound` unless the membership can send as the address (a member of its mailbox; for a
/// personal mailbox, its owner).
pub(super) async fn check_sendable(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
    membership_id: Id,
    address_id: Id,
) -> Result<(), MailError> {
    let found: Option<String> = sqlx::query_scalar(
        "SELECT a.id FROM mail_addresses a \
         JOIN mailboxes mb ON mb.id = a.mailbox_id \
         JOIN mailbox_members mm ON mm.mailbox_id = mb.id \
         WHERE a.id = $1 AND mb.workspace_id = $2 AND mm.membership_id = $3 \
         AND (mb.kind = 'shared' OR mb.owner_membership_id = $3)",
    )
    .bind(address_id.to_string())
    .bind(workspace_id.to_string())
    .bind(membership_id.to_string())
    .fetch_optional(&mut **transaction)
    .await?;
    found.map(|_| ()).ok_or(MailError::NotFound)
}

impl MailRepository {
    /// The membership of a user in the workspace (`NotFound` for an outsider or a guest).
    pub async fn mail_membership(&self, workspace_id: Id, user_id: Id) -> Result<Id, MailError> {
        let mut transaction = self.database.pool().begin().await?;
        let reader = reader(&mut transaction, workspace_id, user_id).await?;
        transaction.commit().await?;
        Ok(reader.membership_id)
    }

    /// The mailbox of a thread the membership reads (`NotFound` for any other).
    pub async fn thread_mailbox(
        &self,
        workspace_id: Id,
        membership_id: Id,
        thread_id: Id,
    ) -> Result<Id, MailError> {
        let found: Option<String> = sqlx::query_scalar(
            "SELECT t.mailbox_id FROM mail_threads t \
             JOIN mailbox_members mm ON mm.mailbox_id = t.mailbox_id \
             WHERE t.id = $1 AND t.workspace_id = $2 AND mm.membership_id = $3",
        )
        .bind(thread_id.to_string())
        .bind(workspace_id.to_string())
        .bind(membership_id.to_string())
        .fetch_optional(self.database.pool())
        .await?;
        parse_id(&found.ok_or(MailError::NotFound)?)
    }

    /// The addresses the member can send as, by mailbox name, the primary address first. With
    /// `thread_mailbox` (a reply or a forward, D13) only the addresses of that mailbox.
    pub async fn from_addresses(
        &self,
        membership_id: Id,
        workspace_id: Id,
        thread_mailbox: Option<Id>,
    ) -> Result<Vec<FromAddress>, MailError> {
        let mut sql = orbit_platform::SqlBuilder::new(
            "SELECT a.id, a.mailbox_id, a.local_part, a.domain_id, d.name AS domain, \
             COALESCE(a.sender_name, mb.name) AS sender_name \
             FROM mailbox_members mm \
             JOIN mailboxes mb ON mb.id = mm.mailbox_id \
             JOIN mail_addresses a ON a.mailbox_id = mb.id \
             JOIN mail_domains d ON d.id = a.domain_id \
             WHERE mm.membership_id = ",
        );
        sql.push_bind(membership_id.to_string());
        sql.push(" AND mb.workspace_id = ");
        sql.push_bind(workspace_id.to_string());
        sql.push(" AND (mb.kind = 'shared' OR mb.owner_membership_id = ");
        sql.push_bind(membership_id.to_string());
        sql.push(")");
        if let Some(mailbox) = thread_mailbox {
            sql.push(" AND mb.id = ");
            sql.push_bind(mailbox.to_string());
        }
        sql.push(
            " ORDER BY lower(mb.name), mb.id, CASE a.kind WHEN 'primary' THEN 0 ELSE 1 END, \
             a.local_part",
        );
        let rows = sql.build().fetch_all(self.database.pool()).await?;
        let signatures: HashMap<String, String> = sqlx::query(
            "SELECT address_id, body_html FROM mail_signatures \
             WHERE membership_id = $1 AND workspace_id = $2",
        )
        .bind(membership_id.to_string())
        .bind(workspace_id.to_string())
        .fetch_all(self.database.pool())
        .await?
        .iter()
        .map(|row| Ok((row.try_get("address_id")?, row.try_get("body_html")?)))
        .collect::<Result<_, MailError>>()?;
        let mut readiness: HashMap<(Id, Id), SendReadiness> = HashMap::new();
        let mut limits: HashMap<Id, Option<i64>> = HashMap::new();
        let mut addresses = Vec::with_capacity(rows.len());
        for row in &rows {
            let address_id = row.try_get::<String, _>("id")?;
            let mailbox_id = parse_id(&row.try_get::<String, _>("mailbox_id")?)?;
            let domain_id = parse_id(&row.try_get::<String, _>("domain_id")?)?;
            let ready = match readiness.get(&(domain_id, mailbox_id)) {
                Some(ready) => *ready,
                None => {
                    let ready = self.send_readiness(domain_id, Some(mailbox_id)).await?;
                    readiness.insert((domain_id, mailbox_id), ready);
                    ready
                }
            };
            let max_message_bytes = match limits.get(&domain_id) {
                Some(limit) => *limit,
                None => {
                    let limit = self.domain_max_message_bytes(domain_id).await?;
                    limits.insert(domain_id, limit);
                    limit
                }
            };
            addresses.push(FromAddress {
                mailbox_id,
                address: format!(
                    "{}@{}",
                    row.try_get::<String, _>("local_part")?,
                    row.try_get::<String, _>("domain")?
                ),
                display_name: row.try_get("sender_name")?,
                signature_html: signatures
                    .get(&address_id)
                    .filter(|html| !html.is_empty())
                    .cloned(),
                address_id: parse_id(&address_id)?,
                ready,
                max_message_bytes,
            });
        }
        Ok(addresses)
    }

    /// The address a reply defaults to (U6): the address of the mailbox that received the
    /// parent, an alias as well, without a `+tag`. `None` when the parent was not received there
    /// (the caller uses the primary address). `NotFound` unless the member reads the mailbox.
    pub async fn reply_from(
        &self,
        membership_id: Id,
        parent_message_id: Id,
        mailbox_id: Id,
    ) -> Result<Option<Id>, MailError> {
        let member: Option<String> = sqlx::query_scalar(
            "SELECT membership_id FROM mailbox_members WHERE mailbox_id = $1 AND membership_id = $2",
        )
        .bind(mailbox_id.to_string())
        .bind(membership_id.to_string())
        .fetch_optional(self.database.pool())
        .await?;
        member.ok_or(MailError::NotFound)?;
        let addresses = sqlx::query(
            "SELECT a.id, a.local_part, d.name AS domain FROM mail_addresses a \
             JOIN mail_domains d ON d.id = a.domain_id WHERE a.mailbox_id = $1",
        )
        .bind(mailbox_id.to_string())
        .fetch_all(self.database.pool())
        .await?;
        let received = sqlx::query(
            "SELECT rr.rcpt, rr.address_id FROM mail_receipt_recipients rr \
             JOIN mail_messages m ON m.receipt_id = rr.receipt_id \
             JOIN mail_thread_messages tm ON tm.message_id = m.id AND tm.mailbox_id = $2 \
             WHERE m.id = $1 ORDER BY rr.rcpt",
        )
        .bind(parent_message_id.to_string())
        .bind(mailbox_id.to_string())
        .fetch_all(self.database.pool())
        .await?;
        let own: Vec<(String, String)> = addresses
            .iter()
            .map(|row| {
                Ok((
                    row.try_get::<String, _>("id")?,
                    format!(
                        "{}@{}",
                        row.try_get::<String, _>("local_part")?,
                        row.try_get::<String, _>("domain")?
                    ),
                ))
            })
            .collect::<Result<_, MailError>>()?;
        // The address the sender wrote (its base) first; the stored address of a catch-all next.
        for row in &received {
            let rcpt = row.try_get::<String, _>("rcpt")?.to_ascii_lowercase();
            if let Some((local, domain)) = rcpt.rsplit_once('@') {
                let base = format!("{}@{domain}", orbit_mail::base_local(local));
                if let Some((id, _)) = own.iter().find(|(_, address)| *address == base) {
                    return Ok(Some(parse_id(id)?));
                }
            }
        }
        for row in &received {
            if let Some(id) = row.try_get::<Option<String>, _>("address_id")?
                && own.iter().any(|(own_id, _)| *own_id == id)
            {
                return Ok(Some(parse_id(&id)?));
            }
        }
        Ok(None)
    }

    /// The member's signature for an address they can send as.
    pub async fn signature(
        &self,
        membership_id: Id,
        workspace_id: Id,
        address_id: Id,
    ) -> Result<Option<String>, MailError> {
        let mut transaction = self.database.pool().begin().await?;
        check_sendable(&mut transaction, workspace_id, membership_id, address_id).await?;
        let found = sqlx::query_scalar(
            "SELECT body_html FROM mail_signatures WHERE membership_id = $1 AND address_id = $2",
        )
        .bind(membership_id.to_string())
        .bind(address_id.to_string())
        .fetch_optional(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(found)
    }

    /// Sets the signature of an address the member can send as; the HTML is sanitized. Returns
    /// what is stored.
    pub async fn put_signature(
        &self,
        membership_id: Id,
        workspace_id: Id,
        address_id: Id,
        body_html: &str,
    ) -> Result<String, MailError> {
        let body = clean_body(body_html, SIGNATURE_MAX_BYTES, "body_html")?;
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        check_sendable(&mut transaction, workspace_id, membership_id, address_id).await?;
        sqlx::query(
            "INSERT INTO mail_signatures \
             (id, workspace_id, membership_id, address_id, body_html, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $6) \
             ON CONFLICT (membership_id, address_id) \
             DO UPDATE SET body_html = excluded.body_html, updated_at = excluded.updated_at",
        )
        .bind(Id::new_v7().to_string())
        .bind(workspace_id.to_string())
        .bind(membership_id.to_string())
        .bind(address_id.to_string())
        .bind(&body)
        .bind(now.as_millis())
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(body)
    }

    pub async fn delete_signature(
        &self,
        membership_id: Id,
        workspace_id: Id,
        address_id: Id,
    ) -> Result<(), MailError> {
        let mut transaction = self.database.immediate_transaction().await?;
        check_sendable(&mut transaction, workspace_id, membership_id, address_id).await?;
        sqlx::query("DELETE FROM mail_signatures WHERE membership_id = $1 AND address_id = $2")
            .bind(membership_id.to_string())
            .bind(address_id.to_string())
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// The undo-send delay in seconds (10 until the member sets one).
    pub async fn member_settings(&self, membership_id: Id) -> Result<i64, MailError> {
        let found: Option<i64> = sqlx::query_scalar(
            "SELECT undo_send_seconds FROM mail_member_settings WHERE membership_id = $1",
        )
        .bind(membership_id.to_string())
        .fetch_optional(self.database.pool())
        .await?;
        Ok(found.unwrap_or(DEFAULT_UNDO_SECONDS))
    }

    /// Sets the undo-send delay: 0, 5, 10, 20 or 30 seconds.
    pub async fn put_member_settings(
        &self,
        membership_id: Id,
        undo_send_seconds: i64,
    ) -> Result<i64, MailError> {
        if !UNDO_SEND_SECONDS.contains(&undo_send_seconds) {
            return Err(MailError::Invalid {
                field: "undo_send_seconds",
            });
        }
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        sqlx::query(
            "INSERT INTO mail_member_settings (membership_id, undo_send_seconds, updated_at) \
             VALUES ($1, $2, $3) ON CONFLICT (membership_id) \
             DO UPDATE SET undo_send_seconds = excluded.undo_send_seconds, \
             updated_at = excluded.updated_at",
        )
        .bind(membership_id.to_string())
        .bind(undo_send_seconds)
        .bind(now.as_millis())
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(undo_send_seconds)
    }

    /// The member's snippets by name.
    pub async fn list_snippets(&self, membership_id: Id) -> Result<Vec<Snippet>, MailError> {
        let rows = sqlx::query(
            "SELECT id, name, body_html, created_at, updated_at FROM mail_snippets \
             WHERE membership_id = $1 ORDER BY lower(name), id",
        )
        .bind(membership_id.to_string())
        .fetch_all(self.database.pool())
        .await?;
        rows.iter().map(snippet).collect()
    }

    pub async fn create_snippet(
        &self,
        membership_id: Id,
        workspace_id: Id,
        name: &str,
        body_html: &str,
    ) -> Result<Snippet, MailError> {
        let name = clean_snippet_name(name)?;
        let body = clean_body(body_html, SNIPPET_MAX_BYTES, "body_html")?;
        let now = self.database.database_now().await?;
        let id = Id::new_v7();
        let mut transaction = self.database.immediate_transaction().await?;
        let member: Option<String> =
            sqlx::query_scalar("SELECT id FROM memberships WHERE id = $1 AND workspace_id = $2")
                .bind(membership_id.to_string())
                .bind(workspace_id.to_string())
                .fetch_optional(&mut *transaction)
                .await?;
        member.ok_or(MailError::NotFound)?;
        sqlx::query(
            "INSERT INTO mail_snippets \
             (id, workspace_id, membership_id, name, body_html, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $6)",
        )
        .bind(id.to_string())
        .bind(workspace_id.to_string())
        .bind(membership_id.to_string())
        .bind(&name)
        .bind(&body)
        .bind(now.as_millis())
        .execute(&mut *transaction)
        .await?;
        let row = sqlx::query(
            "SELECT id, name, body_html, created_at, updated_at FROM mail_snippets WHERE id = $1",
        )
        .bind(id.to_string())
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(MailError::NotFound)?;
        transaction.commit().await?;
        snippet(&row)
    }

    /// Changes a snippet of the member; `None` keeps the value. `NotFound` for another member's.
    pub async fn update_snippet(
        &self,
        membership_id: Id,
        snippet_id: Id,
        name: Option<&str>,
        body_html: Option<&str>,
    ) -> Result<Snippet, MailError> {
        let name = name.map(clean_snippet_name).transpose()?;
        let body = body_html
            .map(|body| clean_body(body, SNIPPET_MAX_BYTES, "body_html"))
            .transpose()?;
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        let row = sqlx::query(
            "SELECT id, name, body_html, created_at, updated_at FROM mail_snippets \
             WHERE id = $1 AND membership_id = $2",
        )
        .bind(snippet_id.to_string())
        .bind(membership_id.to_string())
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(MailError::NotFound)?;
        let found = snippet(&row)?;
        let name = name.unwrap_or(found.name);
        let body = body.unwrap_or(found.body_html);
        sqlx::query(
            "UPDATE mail_snippets SET name = $1, body_html = $2, updated_at = $3 \
             WHERE id = $4 AND membership_id = $5",
        )
        .bind(&name)
        .bind(&body)
        .bind(now.as_millis())
        .bind(snippet_id.to_string())
        .bind(membership_id.to_string())
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(Snippet {
            id: snippet_id,
            name,
            body_html: body,
            created_at: found.created_at,
            updated_at: now,
        })
    }

    pub async fn delete_snippet(&self, membership_id: Id, snippet_id: Id) -> Result<(), MailError> {
        let mut transaction = self.database.immediate_transaction().await?;
        let result = sqlx::query("DELETE FROM mail_snippets WHERE id = $1 AND membership_id = $2")
            .bind(snippet_id.to_string())
            .bind(membership_id.to_string())
            .execute(&mut *transaction)
            .await?;
        if result.rows_affected() == 0 {
            return Err(MailError::NotFound);
        }
        transaction.commit().await?;
        Ok(())
    }
}
