//! Contacts and address autocomplete (spec §5.12).
//!
//! `mail_contacts` holds, for each member, the addresses they sent mail to: a release adds or
//! updates a row for each recipient ([`upsert_contacts`]). The picker ([`MailRepository::search_contacts`])
//! returns at most [`CONTACT_LIMIT`] hits: workspace members first, then the member's contacts by a
//! score of send count and recency, then addresses seen as senders of mail received in the
//! member's OWN mailboxes (a mailbox the member reads, never another member's). A removed
//! contact (`hidden`) is left out of all three, until the member writes to it again.

use std::collections::HashSet;

use orbit_platform::{Id, Row, TimestampMillis};
use serde::Serialize;
use sqlx::{Any, Transaction};
use utoipa::ToSchema;

use super::{MailError, MailRepository};

/// The most hits one search returns.
pub const CONTACT_LIMIT: usize = 8;
/// A contact's score halves each 30 days since the last mail to it.
const HALF_LIFE_MS: f64 = 30.0 * 24.0 * 3600.0 * 1000.0;
/// Rows read from each source before they are ranked.
const SOURCE_ROWS: i64 = 100;
const QUERY_MAX_CHARS: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ContactKind {
    /// A member of the workspace.
    Member,
    /// An address the member wrote to.
    Contact,
    /// An address that sent mail to one of the member's mailboxes.
    Seen,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContactHit {
    pub address: String,
    pub name: Option<String>,
    pub kind: ContactKind,
}

/// `text` as lower case `LIKE` text (`\` escapes `%`, `_` and itself).
fn escape(text: &str) -> String {
    let mut out = String::new();
    for character in text.to_lowercase().chars() {
        if matches!(character, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(character);
    }
    out
}

/// Adds or updates the contacts of `membership_id` for the recipients of a sent message: a new
/// row (name when known), or one more send and the time; a known name is kept when the new one
/// is unknown. Writing to a removed contact brings it back. Bad addresses are skipped.
pub(super) async fn upsert_contacts(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
    membership_id: Id,
    recipients: &[(String, Option<String>)],
    now: TimestampMillis,
) -> Result<(), MailError> {
    let mut seen = HashSet::new();
    for (address, name) in recipients {
        let Ok(address) = orbit_mail::normalize_address(address.trim()) else {
            continue;
        };
        let address = address.to_string();
        if !seen.insert(address.clone()) {
            continue;
        }
        let name = name
            .as_deref()
            .map(|name| name.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|name| !name.is_empty() && name.chars().count() <= 200);
        sqlx::query(
            "INSERT INTO mail_contacts (workspace_id, membership_id, address, name, send_count, \
             last_sent_at, hidden, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, 1, $5, 0, $5, $5) \
             ON CONFLICT (membership_id, address) DO UPDATE SET \
             send_count = mail_contacts.send_count + 1, last_sent_at = excluded.last_sent_at, \
             name = COALESCE(excluded.name, mail_contacts.name), hidden = 0, \
             updated_at = excluded.updated_at",
        )
        .bind(workspace_id.to_string())
        .bind(membership_id.to_string())
        .bind(address)
        .bind(name)
        .bind(now.as_millis())
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}

impl MailRepository {
    /// The address picker (see the module docs): `q` matches a name or an address, any case; an
    /// empty `q` lists the members. At most `limit` (8) hits, no address twice.
    pub async fn search_contacts(
        &self,
        workspace_id: Id,
        membership_id: Id,
        q: &str,
        limit: usize,
        now: TimestampMillis,
    ) -> Result<Vec<ContactHit>, MailError> {
        let limit = limit.clamp(1, CONTACT_LIMIT);
        let q: String = q.trim().chars().take(QUERY_MAX_CHARS).collect();
        let like = format!("%{}%", escape(&q));
        let prefix = format!("{}%", escape(&q));
        let pool = self.database.pool();
        let mut hits: Vec<ContactHit> = Vec::new();
        let mut taken = HashSet::new();
        let hidden: HashSet<String> = sqlx::query_scalar(
            "SELECT address FROM mail_contacts WHERE membership_id = $1 AND hidden = 1",
        )
        .bind(membership_id.to_string())
        .fetch_all(pool)
        .await?
        .into_iter()
        .collect();

        // Members: the address of their personal mailbox, else their sign-in address.
        let members = sqlx::query(
            "SELECT u.display_name, u.email, \
             (SELECT lower(a.local_part || '@' || d.name) FROM mailboxes mb \
              JOIN mail_addresses a ON a.mailbox_id = mb.id AND a.kind = 'primary' \
              JOIN mail_domains d ON d.id = a.domain_id \
              WHERE mb.owner_membership_id = ms.id AND mb.kind = 'personal' LIMIT 1) AS mailbox_address \
             FROM memberships ms JOIN users u ON u.id = ms.user_id \
             WHERE ms.workspace_id = $1 AND ms.role <> 'guest' \
             AND (lower(u.display_name) LIKE $2 ESCAPE '\\' OR lower(u.email) LIKE $2 ESCAPE '\\') \
             ORDER BY (lower(u.display_name) LIKE $3 ESCAPE '\\') DESC, lower(u.display_name), u.id \
             LIMIT $4",
        )
        .bind(workspace_id.to_string())
        .bind(&like)
        .bind(&prefix)
        .bind(SOURCE_ROWS)
        .fetch_all(pool)
        .await?;
        for row in members {
            let email: String = row.try_get("email")?;
            let address = row
                .try_get::<Option<String>, _>("mailbox_address")?
                .unwrap_or_else(|| email.to_lowercase());
            let name: String = row.try_get("display_name")?;
            if hidden.contains(&address) || !taken.insert(address.clone()) {
                continue;
            }
            hits.push(ContactHit {
                address,
                name: Some(name).filter(|name| !name.is_empty()),
                kind: ContactKind::Member,
            });
        }
        // Past the limit the members are cut after their own ranking.
        let members_count = hits.len().min(limit);
        hits.truncate(members_count);

        if hits.len() < limit {
            let rows = sqlx::query(
                "SELECT address, name, send_count, last_sent_at FROM mail_contacts \
                 WHERE membership_id = $1 AND hidden = 0 AND send_count > 0 \
                 AND (lower(address) LIKE $2 ESCAPE '\\' OR lower(COALESCE(name, '')) LIKE $2 ESCAPE '\\') \
                 ORDER BY last_sent_at DESC, address LIMIT $3",
            )
            .bind(membership_id.to_string())
            .bind(&like)
            .bind(SOURCE_ROWS)
            .fetch_all(pool)
            .await?;
            let mut scored = Vec::new();
            for row in rows {
                let address: String = row.try_get("address")?;
                let name: Option<String> = row.try_get("name")?;
                let count: i64 = row.try_get("send_count")?;
                let last: Option<i64> = row.try_get("last_sent_at")?;
                let age = (now.as_millis() - last.unwrap_or(0)).max(0) as f64;
                let score = count as f64 * 0.5_f64.powf(age / HALF_LIFE_MS);
                scored.push((score, last.unwrap_or(0), address, name));
            }
            scored.sort_by(|a, b| {
                b.0.total_cmp(&a.0)
                    .then(b.1.cmp(&a.1))
                    .then_with(|| a.2.cmp(&b.2))
            });
            for (_, _, address, name) in scored {
                if hits.len() >= limit {
                    break;
                }
                if taken.insert(address.clone()) {
                    hits.push(ContactHit {
                        address,
                        name,
                        kind: ContactKind::Contact,
                    });
                }
            }
        }

        if hits.len() < limit && !q.is_empty() {
            // Senders of received mail in the mailboxes the member reads.
            let rows = sqlx::query(
                "SELECT lower(m.from_addr) AS address, MAX(m.from_name) AS name, \
                 MAX(m.received_at) AS last_at \
                 FROM mail_thread_messages tm \
                 JOIN mailbox_members mm ON mm.mailbox_id = tm.mailbox_id AND mm.membership_id = $1 \
                 JOIN mail_messages m ON m.id = tm.message_id \
                 WHERE tm.outgoing = 0 AND m.workspace_id = $2 AND m.from_addr IS NOT NULL \
                 AND (lower(m.from_addr) LIKE $3 ESCAPE '\\' OR lower(COALESCE(m.from_name, '')) LIKE $3 ESCAPE '\\') \
                 GROUP BY lower(m.from_addr) ORDER BY last_at DESC, address LIMIT $4",
            )
            .bind(membership_id.to_string())
            .bind(workspace_id.to_string())
            .bind(&like)
            .bind(SOURCE_ROWS)
            .fetch_all(pool)
            .await?;
            for row in rows {
                if hits.len() >= limit {
                    break;
                }
                let address: String = row.try_get("address")?;
                if hidden.contains(&address) || !taken.insert(address.clone()) {
                    continue;
                }
                hits.push(ContactHit {
                    address,
                    name: row
                        .try_get::<Option<String>, _>("name")?
                        .filter(|name| !name.trim().is_empty()),
                    kind: ContactKind::Seen,
                });
            }
        }
        Ok(hits)
    }

    /// Removes a contact from the member's list (and from their suggestions). A row for an
    /// address that was never a contact is made, so a seen address stays hidden too.
    pub async fn hide_contact(
        &self,
        workspace_id: Id,
        membership_id: Id,
        address: &str,
        now: TimestampMillis,
    ) -> Result<(), MailError> {
        let address = orbit_mail::normalize_address(address.trim())
            .map_err(|_| MailError::Invalid { field: "address" })?
            .to_string();
        sqlx::query(
            "INSERT INTO mail_contacts (workspace_id, membership_id, address, name, send_count, \
             last_sent_at, hidden, created_at, updated_at) VALUES ($1, $2, $3, NULL, 0, NULL, 1, $4, $4) \
             ON CONFLICT (membership_id, address) DO UPDATE SET hidden = 1, updated_at = excluded.updated_at",
        )
        .bind(workspace_id.to_string())
        .bind(membership_id.to_string())
        .bind(address)
        .bind(now.as_millis())
        .execute(self.database.pool())
        .await?;
        Ok(())
    }
}
