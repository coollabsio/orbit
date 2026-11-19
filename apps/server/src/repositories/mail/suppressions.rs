//! Bounces and the suppression list (spec §5.7).
//!
//! - **Bounce addresses.** `direct` and `smtp` send each recipient with the envelope sender
//!   `bounce+{token}@{from domain}`; the token holds the outbound recipient's ID and a MAC
//!   ([`MailRepository::bounce_key`]). An address is valid only when the MAC checks, the
//!   recipient exists and it went less than [`BOUNCE_TOKEN_TTL_MS`] ago
//!   ([`MailRepository::bounce_recipient`]). A message to it is a delivery status notification
//!   (RFC 3464): it is read ([`MailRepository::process_dsn`]) and dropped, never filed.
//! - **Suppression.** One rule, [`suppresses`], decides whether a bounce puts the address on the
//!   workspace's list: only a [`BounceKind::Hard`] bounce whose status says the address is bad
//!   (`5.1.x` or `5.2.1`, or with no enhanced status the reply `550`, `551` or `553`). A remote
//!   policy, content or size refusal (`5.7.x`, `5.3.x`, `5.6.x`) bounces the recipient but does
//!   not suppress the address. It applies to bounces at send time, to DSNs and to provider
//!   events.
//! - A suppressed address is skipped when sending (status `suppressed`). Managers list and
//!   remove entries; a retry of a recipient removes its entry.

use orbit_mail::{DsnAction, DsnRecipient, is_hard, parse_bounce_local_part};
use orbit_platform::{Id, Row, TimestampMillis};
use serde_json::json;
use sqlx::any::AnyRow;
use sqlx::{Any, Transaction};

use super::domains::{manager, parse_id};
use super::outbound::{
    BounceKind, RecipientStatus, bounced_thread_to_inbox, enqueue, is_automatic, plain_line,
    send_job, update_summary,
};
use super::{MailError, MailRepository};

/// How long after it went a recipient's bounce address stays valid.
pub const BOUNCE_TOKEN_TTL_MS: i64 = 7 * 24 * 3_600_000;

/// Entries in one page of the suppression list.
pub const SUPPRESSION_PAGE: i64 = 50;

const BOUNCE_PREFIX: &str = "bounce+";

/// Whether a local part is a bounce address (`bounce+…`, any case).
#[must_use]
pub fn is_bounce_local(local: &str) -> bool {
    local
        .get(..BOUNCE_PREFIX.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(BOUNCE_PREFIX))
}

/// Why an address is on the suppression list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SuppressionReason {
    HardBounce,
    Complaint,
    Manual,
}

impl SuppressionReason {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HardBounce => "hard_bounce",
            Self::Complaint => "complaint",
            Self::Manual => "manual",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "hard_bounce" => Some(Self::HardBounce),
            "complaint" => Some(Self::Complaint),
            "manual" => Some(Self::Manual),
            _ => None,
        }
    }
}

/// One entry of the suppression list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suppression {
    /// Lower case.
    pub address: String,
    pub reason: SuppressionReason,
    /// The bounce's response or the complaint.
    pub detail: Option<String>,
    /// The sent recipient that bounced.
    pub recipient_id: Option<Id>,
    pub created_at: TimestampMillis,
}

/// A page of the suppression list, newest first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SuppressionPage {
    pub items: Vec<Suppression>,
    /// Pass for the next page; `None` on the last one.
    pub cursor: Option<String>,
}

fn suppression(row: &AnyRow) -> Result<Suppression, MailError> {
    let reason: String = row.try_get("reason")?;
    Ok(Suppression {
        address: row.try_get("address")?,
        reason: SuppressionReason::parse(&reason).ok_or(MailError::NotFound)?,
        detail: row.try_get("detail")?,
        recipient_id: row
            .try_get::<Option<String>, _>("recipient_id")?
            .and_then(|id| id.parse().ok()),
        created_at: TimestampMillis::from_millis(row.try_get("created_at")?),
    })
}

/// The reply code (`550`) and the enhanced status (`5.1.1`) in a response such as
/// `permanent error (550): 5.1.1 No such user` or a DSN's `smtp; 550 5.1.1 …`: the first token of
/// each shape.
#[must_use]
pub fn bounce_codes(response: &str) -> (Option<u16>, Option<String>) {
    let mut reply = None;
    let mut enhanced = None;
    for token in response.split_whitespace() {
        let token = token.trim_matches(|c: char| !c.is_ascii_alphanumeric());
        if reply.is_none()
            && token.len() == 3
            && token.starts_with(['4', '5'])
            && token.bytes().all(|b| b.is_ascii_digit())
        {
            reply = token.parse().ok();
        }
        if enhanced.is_none() && is_enhanced(token) {
            enhanced = Some(token.to_owned());
        }
    }
    (reply, enhanced)
}

/// `class.subject.detail` (RFC 3463): class 2, 4 or 5, then 1–3 digits twice.
fn is_enhanced(token: &str) -> bool {
    let mut parts = token.split('.');
    let (Some(class), Some(subject), Some(detail), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    let digits =
        |part: &str| (1..=3).contains(&part.len()) && part.bytes().all(|b| b.is_ascii_digit());
    matches!(class, "2" | "4" | "5") && digits(subject) && digits(detail)
}

/// The suppression rule (the only one): a [`BounceKind::Hard`] bounce whose enhanced status is
/// `5.1.x` (a bad address or domain) or `5.2.1` (a disabled mailbox) or, with no enhanced status,
/// whose reply is `550`, `551` or `553`. Every other bounce (a policy, content or size refusal,
/// Orbit's own refusal, a give-up) leaves the address off the list.
#[must_use]
pub fn suppresses(kind: BounceKind, enhanced: Option<&str>, reply: Option<u16>) -> bool {
    if kind != BounceKind::Hard {
        return false;
    }
    match enhanced {
        Some(code) => code.starts_with("5.1.") || code == "5.2.1",
        None => matches!(reply, Some(550 | 551 | 553)),
    }
}

/// [`suppresses`] with the codes read from a response ([`bounce_codes`]).
#[must_use]
pub fn suppresses_response(kind: BounceKind, response: &str) -> bool {
    let (reply, enhanced) = bounce_codes(response);
    suppresses(kind, enhanced.as_deref(), reply)
}

impl MailRepository {
    /// Puts `address` (stored lower case) on the workspace's suppression list in the caller's
    /// transaction. An address on it already keeps its first entry. Returns whether it was
    /// added.
    #[allow(clippy::too_many_arguments)]
    pub async fn add_suppression(
        transaction: &mut Transaction<'_, Any>,
        workspace_id: Id,
        address: &str,
        reason: SuppressionReason,
        detail: Option<&str>,
        recipient_id: Option<Id>,
        now: TimestampMillis,
    ) -> Result<bool, MailError> {
        let added = sqlx::query(
            "INSERT INTO mail_suppressions (workspace_id, address, reason, detail, recipient_id, \
             created_at) VALUES ($1, $2, $3, $4, $5, $6) \
             ON CONFLICT (workspace_id, address) DO NOTHING",
        )
        .bind(workspace_id.to_string())
        .bind(address.to_lowercase())
        .bind(reason.as_str())
        .bind(detail.map(plain_line))
        .bind(recipient_id.map(|id| id.to_string()))
        .bind(now.as_millis())
        .execute(&mut **transaction)
        .await?
        .rows_affected();
        Ok(added == 1)
    }

    /// Removes `address` from the list (owners and admins). `NotFound` when it is not on it.
    pub async fn remove_suppression(
        &self,
        workspace_id: Id,
        actor_id: Id,
        address: &str,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), MailError> {
        let mut transaction = self.database.immediate_transaction().await?;
        manager(&mut transaction, workspace_id, actor_id).await?;
        let address = address.trim().to_lowercase();
        let removed =
            sqlx::query("DELETE FROM mail_suppressions WHERE workspace_id = $1 AND address = $2")
                .bind(workspace_id.to_string())
                .bind(&address)
                .execute(&mut *transaction)
                .await?
                .rows_affected();
        if removed == 0 {
            return Err(MailError::NotFound);
        }
        crate::audit::record(
            &mut transaction,
            workspace_id,
            Some(actor_id),
            "mail.suppression_removed",
            crate::audit::AuditOutcome::Success,
            "mail_suppression",
            None,
            request_id,
            json!({ "address": address }),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// The workspace's list (owners and admins), newest first, [`SUPPRESSION_PAGE`] at a time:
    /// the addresses that contain `query` (any case), after `cursor`.
    pub async fn list_suppressions(
        &self,
        workspace_id: Id,
        actor_id: Id,
        query: Option<&str>,
        cursor: Option<&str>,
    ) -> Result<SuppressionPage, MailError> {
        let after = cursor
            .map(|cursor| {
                let (at, address) = cursor
                    .split_once(':')
                    .ok_or(MailError::Invalid { field: "cursor" })?;
                let at: i64 = at
                    .parse()
                    .map_err(|_| MailError::Invalid { field: "cursor" })?;
                Ok::<_, MailError>((at, address.to_owned()))
            })
            .transpose()?;
        let mut transaction = self.database.pool().begin().await?;
        manager(&mut transaction, workspace_id, actor_id).await?;
        let mut sql = orbit_platform::SqlBuilder::new(
            "SELECT address, reason, detail, recipient_id, created_at FROM mail_suppressions \
             WHERE workspace_id = ",
        );
        sql.push_bind(workspace_id.to_string());
        if let Some(query) = query.map(str::trim).filter(|query| !query.is_empty()) {
            let escaped = query
                .to_lowercase()
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_");
            sql.push(" AND address LIKE ");
            sql.push_bind(format!("%{escaped}%"));
            sql.push(" ESCAPE '\\'");
        }
        if let Some((at, address)) = after {
            sql.push(" AND (created_at < ");
            sql.push_bind(at);
            sql.push(" OR (created_at = ");
            sql.push_bind(at);
            sql.push(" AND address > ");
            sql.push_bind(address);
            sql.push("))");
        }
        sql.push(" ORDER BY created_at DESC, address LIMIT ");
        sql.push_bind(SUPPRESSION_PAGE + 1);
        let rows = sql.build().fetch_all(&mut *transaction).await?;
        transaction.commit().await?;
        let mut items = rows
            .iter()
            .map(suppression)
            .collect::<Result<Vec<_>, _>>()?;
        let more = items.len() > usize::try_from(SUPPRESSION_PAGE).unwrap_or(usize::MAX);
        items.truncate(usize::try_from(SUPPRESSION_PAGE).unwrap_or(usize::MAX));
        let cursor = items
            .last()
            .filter(|_| more)
            .map(|last| format!("{}:{}", last.created_at.as_millis(), last.address));
        Ok(SuppressionPage { items, cursor })
    }

    /// The entry of `address` (any case), when it is on the workspace's list.
    pub async fn suppression(
        &self,
        workspace_id: Id,
        address: &str,
    ) -> Result<Option<Suppression>, MailError> {
        let row = sqlx::query(
            "SELECT address, reason, detail, recipient_id, created_at FROM mail_suppressions \
             WHERE workspace_id = $1 AND address = $2",
        )
        .bind(workspace_id.to_string())
        .bind(address.trim().to_lowercase())
        .fetch_optional(self.database.pool())
        .await?;
        row.as_ref().map(suppression).transpose()
    }

    /// The entries of the workspace's suppression list that are among the member's own draft's
    /// recipients (To, Cc and Bcc), and nothing else of the list. `NotFound` for a draft that is
    /// not the member's. An address that is not valid is skipped: sending refuses it.
    pub async fn draft_suppressions(
        &self,
        membership_id: Id,
        workspace_id: Id,
        draft_id: Id,
    ) -> Result<Vec<Suppression>, MailError> {
        let draft = self.get_draft(membership_id, draft_id).await?;
        let mut seen = std::collections::BTreeSet::new();
        let mut found = Vec::new();
        for address in draft.to.iter().chain(&draft.cc).chain(&draft.bcc) {
            let Ok(address) = orbit_mail::normalize_address(address.trim()) else {
                continue;
            };
            let address = address.to_string();
            if !seen.insert(address.clone()) {
                continue;
            }
            if let Some(entry) = self.suppression(workspace_id, &address).await? {
                found.push(entry);
            }
        }
        Ok(found)
    }

    /// Whether `address` is on the workspace's list.
    pub async fn is_suppressed(&self, workspace_id: Id, address: &str) -> Result<bool, MailError> {
        Ok(self.suppression(workspace_id, address).await?.is_some())
    }

    /// The outbound recipient of a bounce address's local part (`bounce+{token}`): `None` when
    /// the token's MAC does not check, the recipient is gone, or it went (`released_at`)
    /// [`BOUNCE_TOKEN_TTL_MS`] or longer ago.
    pub async fn bounce_recipient(
        &self,
        local_part: &str,
        now: TimestampMillis,
    ) -> Result<Option<Id>, MailError> {
        let Some(bytes) = parse_bounce_local_part(local_part, &self.bounce_key()) else {
            return Ok(None);
        };
        let Some(recipient_id) = Id::from_bytes(bytes) else {
            return Ok(None);
        };
        let released: Option<Option<i64>> =
            sqlx::query_scalar("SELECT released_at FROM mail_outbound_recipients WHERE id = $1")
                .bind(recipient_id.to_string())
                .fetch_optional(self.database.pool())
                .await?;
        Ok(released
            .flatten()
            .filter(|at| now.as_millis() - at < BOUNCE_TOKEN_TTL_MS)
            .map(|_| recipient_id))
    }

    /// Applies a delivery status notification that came to the bounce address of `recipient_id`
    /// (spec §5.7). Only the report of the recipient's own address counts (any case): its
    /// `Final-Recipient`, else its `Original-Recipient` (an alias at the receiving side expanded
    /// it); a DSN cannot bounce another address. `failed` → `bounced` (`hard` with a `5.x.x`
    /// status, else `gave_up`: the remote side gave up). Only a recipient that is `delivered`
    /// (the next hop, a relay or a forwarding server, accepted it) or `deferred` changes, and
    /// only if it was never retried (`retried_at` is NULL): the token names the recipient, not
    /// the attempt, so after **Retry** a DSN may be about the send before it and is ignored.
    /// Then the suppression rule ([`suppresses`]), the thread to the Inbox unread, and
    /// `thread.updated`. `delayed` and `delivered` reports change nothing (a deferred recipient
    /// stays deferred). A failure report for a recipient that is still `sending` is an error (the
    /// intake answers with a temporary failure and the reporter retries). Returns whether the
    /// recipient changed.
    pub async fn process_dsn(
        &self,
        recipient_id: Id,
        dsn: &[DsnRecipient],
        now: TimestampMillis,
    ) -> Result<bool, MailError> {
        let mut transaction = self.database.immediate_transaction().await?;
        let Some(row) = sqlx::query(
            "SELECT outbound_id, workspace_id, address, status, retried_at \
             FROM mail_outbound_recipients WHERE id = $1",
        )
        .bind(recipient_id.to_string())
        .fetch_optional(&mut *transaction)
        .await?
        else {
            return Ok(false);
        };
        let address: String = row.try_get("address")?;
        let status: String = row.try_get("status")?;
        let retried: Option<i64> = row.try_get("retried_at")?;
        let failed = |report: &&DsnRecipient| report.action == DsnAction::Failed;
        let Some(report) = dsn
            .iter()
            .filter(failed)
            .find(|report| report.address.eq_ignore_ascii_case(&address))
            .or_else(|| {
                dsn.iter().filter(failed).find(|report| {
                    report
                        .original
                        .as_deref()
                        .is_some_and(|original| original.eq_ignore_ascii_case(&address))
                })
            })
        else {
            return Ok(false);
        };
        if retried.is_some() {
            return Ok(false);
        }
        if status == "sending" {
            // The DSN came before the attempt recorded its result (it may still wait for QUIT):
            // refuse it for now (a 4xx), so the reporting server sends it again later.
            return Err(MailError::Conflict("the recipient is still being sent"));
        }
        if !matches!(status.as_str(), "delivered" | "deferred") {
            return Ok(false);
        }
        let outbound_id = parse_id(&row.try_get::<String, _>("outbound_id")?)?;
        let workspace_id = parse_id(&row.try_get::<String, _>("workspace_id")?)?;
        // `smtp; 550 5.1.1 …`: the text after the diagnostic type.
        let diagnostic = report
            .diagnostic
            .as_deref()
            .map(|text| text.split_once(';').map_or(text, |(_, rest)| rest).trim())
            .filter(|text| !text.is_empty());
        let (reply, diagnostic_status) = diagnostic.map(bounce_codes).unwrap_or_default();
        let status_code = report.status.trim();
        let enhanced = if is_enhanced(status_code) {
            Some(status_code.to_owned())
        } else {
            diagnostic_status
        };
        let hard = match &enhanced {
            Some(code) => is_hard(code),
            None => reply.is_some_and(|reply| reply >= 500),
        };
        let kind = if hard {
            BounceKind::Hard
        } else {
            BounceKind::GaveUp
        };
        let response = plain_line(&match diagnostic {
            Some(text) => text.to_owned(),
            None => format!("{status_code} The receiving server could not deliver the message"),
        });
        let changed = sqlx::query(
            "UPDATE mail_outbound_recipients SET status = 'bounced', bounce_kind = $1, \
             last_response = $2, next_attempt_at = NULL, updated_at = $3 \
             WHERE id = $4 AND status IN ('delivered', 'deferred') AND retried_at IS NULL",
        )
        .bind(kind.as_str())
        .bind(&response)
        .bind(now.as_millis())
        .bind(recipient_id.to_string())
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if changed == 0 {
            return Ok(false);
        }
        if suppresses(kind, enhanced.as_deref(), reply) {
            Self::add_suppression(
                &mut transaction,
                workspace_id,
                &address,
                SuppressionReason::HardBounce,
                Some(&response),
                Some(recipient_id),
                now,
            )
            .await?;
        }
        if hard && !is_automatic(&mut transaction, outbound_id).await? {
            self.check_pause(&mut transaction, workspace_id, now)
                .await?;
        }
        bounced_thread_to_inbox(&mut transaction, outbound_id, now).await?;
        update_summary(&mut transaction, outbound_id, now).await?;
        transaction.commit().await?;
        self.announce_outbounds(&[outbound_id]).await;
        Ok(true)
    }

    /// **Retry** of a recipient that was not delivered (`bounced` or `suppressed`), by the
    /// message's author or a member of its mailbox (`NotFound` for anyone else): its address
    /// leaves the suppression list, and it is queued again as never tried (limits apply again)
    /// with a `mail.send` run now. A message that was `done` is released again. `retried_at` is
    /// set, so a late DSN of an earlier send is ignored ([`Self::process_dsn`]).
    pub async fn retry_recipient(
        &self,
        workspace_id: Id,
        membership_id: Id,
        outbound_id: Id,
        recipient_id: Id,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<RecipientStatus, MailError> {
        let mut transaction = self.database.immediate_transaction().await?;
        let Some(row) = sqlx::query(
            "SELECT o.status AS outbound_status, r.address, r.status FROM mail_outbound o \
             JOIN mail_outbound_recipients r ON r.outbound_id = o.id \
             WHERE o.id = $1 AND r.id = $2 AND o.workspace_id = $3 \
             AND (o.membership_id = $4 OR EXISTS (SELECT 1 FROM mailbox_members mm \
             WHERE mm.mailbox_id = o.mailbox_id AND mm.membership_id = $4))",
        )
        .bind(outbound_id.to_string())
        .bind(recipient_id.to_string())
        .bind(workspace_id.to_string())
        .bind(membership_id.to_string())
        .fetch_optional(&mut *transaction)
        .await?
        else {
            // the recipient row was purged after 90 days: only the summary is left
            let summarized: Option<String> = sqlx::query_scalar(
                "SELECT delivery_summary FROM mail_outbound WHERE id = $1 AND workspace_id = $2 \
                 AND (membership_id = $3 OR EXISTS (SELECT 1 FROM mailbox_members mm \
                 WHERE mm.mailbox_id = mail_outbound.mailbox_id AND mm.membership_id = $3))",
            )
            .bind(outbound_id.to_string())
            .bind(workspace_id.to_string())
            .bind(membership_id.to_string())
            .fetch_optional(&mut *transaction)
            .await?
            .flatten();
            return Err(if summarized.is_some() {
                MailError::Conflict("recipient_expired")
            } else {
                MailError::NotFound
            });
        };
        let outbound_status: String = row.try_get("outbound_status")?;
        let status: String = row.try_get("status")?;
        let address: String = row.try_get("address")?;
        if !matches!(outbound_status.as_str(), "released" | "done")
            || !matches!(status.as_str(), "bounced" | "suppressed")
        {
            return Err(MailError::Conflict("recipient_not_failed"));
        }
        let unsuppressed =
            sqlx::query("DELETE FROM mail_suppressions WHERE workspace_id = $1 AND address = $2")
                .bind(workspace_id.to_string())
                .bind(address.to_lowercase())
                .execute(&mut *transaction)
                .await?
                .rows_affected();
        sqlx::query(
            "UPDATE mail_outbound_recipients SET status = 'queued', attempts = 0, \
             attempt_id = NULL, next_attempt_at = NULL, last_response = NULL, bounce_kind = NULL, \
             released_at = NULL, retried_at = $1, updated_at = $1 WHERE id = $2",
        )
        .bind(now.as_millis())
        .bind(recipient_id.to_string())
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "UPDATE mail_outbound SET status = 'released', hold_reason = NULL, updated_at = $1 \
             WHERE id = $2 AND status = 'done'",
        )
        .bind(now.as_millis())
        .bind(outbound_id.to_string())
        .execute(&mut *transaction)
        .await?;
        enqueue(&mut transaction, &send_job(outbound_id, workspace_id, now)).await?;
        let actor: Option<String> =
            sqlx::query_scalar("SELECT user_id FROM memberships WHERE id = $1")
                .bind(membership_id.to_string())
                .fetch_optional(&mut *transaction)
                .await?;
        crate::audit::record(
            &mut transaction,
            workspace_id,
            actor.and_then(|id| id.parse().ok()),
            "mail.recipient_retried",
            crate::audit::AuditOutcome::Success,
            "mail_outbound",
            Some(outbound_id),
            request_id,
            json!({
                "recipient_id": recipient_id.to_string(),
                "address": address,
                "suppression_removed": unsuppressed > 0,
            }),
            now,
        )
        .await?;
        transaction.commit().await?;
        self.announce_outbounds(&[outbound_id]).await;
        self.outbound_recipients(outbound_id, now)
            .await?
            .into_iter()
            .find(|recipient| recipient.id == recipient_id)
            .ok_or(MailError::NotFound)
    }
}
