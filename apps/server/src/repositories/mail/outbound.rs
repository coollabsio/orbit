//! Send, undo and scheduled send (spec §5.3). Sending a draft makes one `mail_outbound` row that
//! is `held` until `release_at` (the member's undo time, or the scheduled time) and enqueues the
//! `mail.send` job for that time. The draft stays, so Undo can open it again. The release (the
//! job) and the undo both change the row with one `UPDATE … WHERE status = 'held'`, so exactly
//! one of them wins a race.

use std::collections::BTreeSet;

use orbit_platform::{Id, Job, JobKind, JobStore, Row, TimestampMillis};
use serde_json::json;
use sqlx::any::AnyRow;
use sqlx::{Any, Transaction};

use super::domains::parse_id;
use super::drafts::{Draft, load_draft};
use super::limits::load_limits;
use super::routes::{RouteSettings, SendRouteKind};
use super::signatures::check_sendable;
use super::{MailError, MailRepository, SendReadiness};

/// The job that releases and delivers an outbound message.
pub const SEND_JOB: &str = "mail.send";
/// The default undo time of a member, in seconds.
const DEFAULT_UNDO_SECONDS: i64 = 10;
/// A scheduled send is at most this far ahead (one year).
const SCHEDULE_MAX_MS: i64 = 366 * 24 * 60 * 60 * 1000;
/// What the body and the headers of a message cost besides its files, as slack.
const HEADER_BYTES: i64 = 4096;

/// `mail.send`: eight at a time on the cluster, two of them for one workspace. The payload holds
/// IDs only.
#[must_use]
pub fn mail_send_kind() -> JobKind {
    JobKind::new(SEND_JOB)
        .with_concurrency_limit(8)
        .with_workspace_limit(2)
}

/// A message waiting to go, going or gone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outbound {
    pub id: Id,
    /// `held`, `cancelled`, `released` or `done`.
    pub status: String,
    pub release_at: TimestampMillis,
    pub scheduled: bool,
    /// Why a held message waits beyond its time (`rate_limit`, `paused`).
    pub hold_reason: Option<String>,
}

/// A row of the Scheduled and Outbox lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutboundItem {
    pub outbound: Outbound,
    pub draft_id: Option<Id>,
    pub subject: String,
    pub to: Vec<String>,
}

/// Which messages a list shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutboundView {
    /// Held for a scheduled time.
    Scheduled,
    /// Held for the undo time or a limit, and released but not yet finished.
    Outbox,
}

const COLUMNS: &str = "id, status, release_at, scheduled, hold_reason";

fn outbound(row: &AnyRow) -> Result<Outbound, MailError> {
    Ok(Outbound {
        id: parse_id(&row.try_get::<String, _>("id")?)?,
        status: row.try_get("status")?,
        release_at: TimestampMillis::from_millis(row.try_get("release_at")?),
        scheduled: row.try_get::<i64, _>("scheduled")? != 0,
        hold_reason: row.try_get("hold_reason")?,
    })
}

fn send_job(outbound_id: Id, workspace_id: Id, release_at: TimestampMillis) -> Job {
    Job::new(
        mail_send_kind(),
        json!({ "outbound_id": outbound_id.to_string() }),
        release_at,
    )
    .with_workspace(workspace_id.to_string())
}

async fn enqueue(transaction: &mut Transaction<'_, Any>, job: &Job) -> Result<(), MailError> {
    JobStore::enqueue_in(transaction, job)
        .await
        .map_err(|error| match error {
            orbit_platform::JobStoreError::Database(error) => MailError::Unavailable(error),
            other => MailError::Unavailable(sqlx::Error::Protocol(other.to_string())),
        })?;
    Ok(())
}

/// The distinct normalized recipients of a draft; a bad address is `invalid_address`.
fn recipients(draft: &Draft) -> Result<BTreeSet<String>, MailError> {
    let mut all = BTreeSet::new();
    for address in draft.to.iter().chain(&draft.cc).chain(&draft.bcc) {
        let address = orbit_mail::normalize_address(address.trim())
            .map_err(|_| MailError::Unprocessable("invalid_address"))?;
        all.insert(address.to_string());
    }
    if all.is_empty() {
        return Err(MailError::Unprocessable("no_recipients"));
    }
    Ok(all)
}

/// The size of the message as it is sent: the text and base64 for each file that is not a link.
fn encoded_size(draft: &Draft) -> i64 {
    let files: i64 = draft
        .files
        .iter()
        .filter(|file| !file.as_link)
        .map(|file| (file.size.max(0) + 2) / 3 * 4)
        .sum();
    HEADER_BYTES + draft.subject.len() as i64 + draft.body_html.len() as i64 + files
}

impl MailRepository {
    /// Sends the member's draft (spec §5.3): checks, then holds it for the undo time (or until
    /// `scheduled_at`) and queues the job. A second call for the same draft returns the same row;
    /// the row of an undone send is held again. `NotFound` for a draft that is not the member's.
    pub async fn send_draft(
        &self,
        membership_id: Id,
        draft_id: Id,
        scheduled_at: Option<TimestampMillis>,
        now: TimestampMillis,
    ) -> Result<Outbound, MailError> {
        if let Some(at) = scheduled_at
            && (at <= now || at.as_millis() > now.as_millis() + SCHEDULE_MAX_MS)
        {
            return Err(MailError::Invalid {
                field: "scheduled_at",
            });
        }
        let mut read = self.database.pool().begin().await?;
        let draft = load_draft(&mut read, membership_id, draft_id).await?;
        let workspace: String =
            sqlx::query_scalar("SELECT workspace_id FROM mail_drafts WHERE id = $1")
                .bind(draft_id.to_string())
                .fetch_one(&mut *read)
                .await?;
        let workspace_id = parse_id(&workspace)?;
        if let Some(row) = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {COLUMNS} FROM mail_outbound WHERE draft_id = $1 AND status <> 'cancelled'"
        )))
        .bind(draft_id.to_string())
        .fetch_optional(&mut *read)
        .await?
        {
            read.commit().await?;
            return outbound(&row);
        }
        // Send-as is checked again now: the member may have left the mailbox since the draft.
        check_sendable(
            &mut read,
            workspace_id,
            membership_id,
            draft.from_address_id,
        )
        .await?;
        let (address_mailbox, domain): (String, String) = {
            let row = sqlx::query("SELECT mailbox_id, domain_id FROM mail_addresses WHERE id = $1")
                .bind(draft.from_address_id.to_string())
                .fetch_one(&mut *read)
                .await?;
            (row.try_get("mailbox_id")?, row.try_get("domain_id")?)
        };
        let mailbox_id = parse_id(&address_mailbox)?;
        let domain_id = parse_id(&domain)?;
        if let Some(thread_id) = draft.thread_id {
            // D13: a reply goes from an address of the thread's mailbox.
            let thread_mailbox: Option<String> =
                sqlx::query_scalar("SELECT mailbox_id FROM mail_threads WHERE id = $1")
                    .bind(thread_id.to_string())
                    .fetch_optional(&mut *read)
                    .await?;
            if thread_mailbox.as_deref() != Some(address_mailbox.as_str()) {
                return Err(MailError::NotFound);
            }
        }
        let paused: Option<i64> =
            sqlx::query_scalar("SELECT paused_at FROM mail_send_policies WHERE workspace_id = $1")
                .bind(workspace_id.to_string())
                .fetch_optional(&mut *read)
                .await?
                .flatten();
        let route: Option<(String, String, String)> = {
            let row = sqlx::query(
                "SELECT r.id, r.kind, r.settings FROM mail_domains d \
                 JOIN mail_send_routes r ON r.id = COALESCE(d.send_route_id, \
                 (SELECT s.mail_send_route_id FROM instance_settings s WHERE s.id = 1)) \
                 WHERE d.id = $1",
            )
            .bind(domain_id.to_string())
            .fetch_optional(&mut *read)
            .await?;
            row.map(|row| {
                Ok::<_, MailError>((
                    row.try_get("id")?,
                    row.try_get("kind")?,
                    row.try_get("settings")?,
                ))
            })
            .transpose()?
        };
        read.commit().await?;

        if let SendReadiness::NotReady(reason) =
            self.send_readiness(domain_id, Some(mailbox_id)).await?
        {
            return Err(MailError::NotReady(reason));
        }
        if paused.is_some() {
            return Err(MailError::Conflict("sending_paused"));
        }
        let all = recipients(&draft)?;
        let route_id = route.as_ref().map(|(id, _, _)| parse_id(id)).transpose()?;
        let max_bytes = route
            .map(|(_, kind, settings)| {
                let kind: SendRouteKind = kind.parse().map_err(|()| MailError::NotFound)?;
                let settings: RouteSettings = serde_json::from_str(&settings).unwrap_or_default();
                Ok::<_, MailError>(match settings.max_message_bytes {
                    0 => kind.default_max_bytes(),
                    bytes => bytes,
                })
            })
            .transpose()?
            .unwrap_or(0);
        if max_bytes > 0 && encoded_size(&draft) > max_bytes {
            return Err(MailError::Unprocessable("message_too_large"));
        }

        let mut transaction = self.database.immediate_transaction().await?;
        let limits = load_limits(&mut transaction, workspace_id).await?;
        if all.len() as i64 > limits.max_recipients {
            return Err(MailError::Unprocessable("too_many_recipients"));
        }
        // The draft must be the one that was checked.
        if load_draft(&mut transaction, membership_id, draft_id)
            .await?
            .version
            != draft.version
        {
            return Err(MailError::Conflict("draft_conflict"));
        }
        let existing = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {COLUMNS} FROM mail_outbound WHERE draft_id = $1"
        )))
        .bind(draft_id.to_string())
        .fetch_optional(&mut *transaction)
        .await?;
        if let Some(row) = &existing {
            let found = outbound(row)?;
            if found.status != "cancelled" {
                return Ok(found);
            }
        }
        let undo: Option<i64> = sqlx::query_scalar(
            "SELECT undo_send_seconds FROM mail_member_settings WHERE membership_id = $1",
        )
        .bind(membership_id.to_string())
        .fetch_optional(&mut *transaction)
        .await?;
        let held_until = now.as_millis() + undo.unwrap_or(DEFAULT_UNDO_SECONDS) * 1000;
        let mut release_at = scheduled_at.map_or(held_until, |at| at.as_millis().max(held_until));
        // A message over a window is not refused: it waits in the Outbox for room.
        let mut hold_reason = None;
        if let Some(room) = self
            .window_allows(
                &mut transaction,
                workspace_id,
                membership_id,
                all.len() as i64,
                now,
                None,
            )
            .await?
        {
            release_at = release_at.max(room.as_millis());
            hold_reason = Some("rate_limit");
        }
        let release = TimestampMillis::from_millis(release_at);
        let id = match &existing {
            Some(row) => outbound(row)?.id,
            None => Id::new_v7(),
        };
        if existing.is_some() {
            sqlx::query(
                "UPDATE mail_outbound SET status = 'held', release_at = $1, scheduled = $2, \
                 hold_reason = $3, route_id = $4, mailbox_id = $5, updated_at = $6 \
                 WHERE id = $7 AND status = 'cancelled'",
            )
            .bind(release_at)
            .bind(i64::from(scheduled_at.is_some()))
            .bind(hold_reason)
            .bind(route_id.map(|id| id.to_string()))
            .bind(mailbox_id.to_string())
            .bind(now.as_millis())
            .bind(id.to_string())
            .execute(&mut *transaction)
            .await?;
        } else {
            sqlx::query(
                "INSERT INTO mail_outbound (id, workspace_id, membership_id, mailbox_id, draft_id, \
                 route_id, status, release_at, scheduled, hold_reason, created_at, updated_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, 'held', $7, $8, $9, $10, $10)",
            )
            .bind(id.to_string())
            .bind(workspace_id.to_string())
            .bind(membership_id.to_string())
            .bind(mailbox_id.to_string())
            .bind(draft_id.to_string())
            .bind(route_id.map(|id| id.to_string()))
            .bind(release_at)
            .bind(i64::from(scheduled_at.is_some()))
            .bind(hold_reason)
            .bind(now.as_millis())
            .execute(&mut *transaction)
            .await?;
        }
        enqueue(&mut transaction, &send_job(id, workspace_id, release)).await?;
        transaction.commit().await?;
        Ok(Outbound {
            id,
            status: "held".to_owned(),
            release_at: release,
            scheduled: scheduled_at.is_some(),
            hold_reason: hold_reason.map(str::to_owned),
        })
    }

    /// Undo or cancel (spec §5.3): the held message becomes `cancelled` and its draft is returned
    /// for the composer. `Conflict("already_sent")` for a message that is no longer held. The
    /// change is one conditional `UPDATE`, so it races safely with [`Self::claim_release`].
    pub async fn cancel_outbound(
        &self,
        membership_id: Id,
        outbound_id: Id,
        now: TimestampMillis,
    ) -> Result<Draft, MailError> {
        let mut transaction = self.database.immediate_transaction().await?;
        let result = sqlx::query(
            "UPDATE mail_outbound SET status = 'cancelled', hold_reason = NULL, updated_at = $1 \
             WHERE id = $2 AND membership_id = $3 AND status = 'held'",
        )
        .bind(now.as_millis())
        .bind(outbound_id.to_string())
        .bind(membership_id.to_string())
        .execute(&mut *transaction)
        .await?;
        if result.rows_affected() != 1 {
            let exists: Option<String> = sqlx::query_scalar(
                "SELECT id FROM mail_outbound WHERE id = $1 AND membership_id = $2",
            )
            .bind(outbound_id.to_string())
            .bind(membership_id.to_string())
            .fetch_optional(&mut *transaction)
            .await?;
            return Err(if exists.is_some() {
                MailError::Conflict("already_sent")
            } else {
                MailError::NotFound
            });
        }
        let draft: Option<String> =
            sqlx::query_scalar("SELECT draft_id FROM mail_outbound WHERE id = $1")
                .bind(outbound_id.to_string())
                .fetch_one(&mut *transaction)
                .await?;
        let draft_id = parse_id(&draft.ok_or(MailError::NotFound)?)?;
        let found = load_draft(&mut transaction, membership_id, draft_id).await?;
        transaction.commit().await?;
        Ok(found)
    }

    /// The release side of the race with [`Self::cancel_outbound`]: `held → released` when the
    /// message is due. `true` for the one caller that got it.
    pub async fn claim_release(
        &self,
        outbound_id: Id,
        now: TimestampMillis,
    ) -> Result<bool, MailError> {
        let result = sqlx::query(
            "UPDATE mail_outbound SET status = 'released', hold_reason = NULL, updated_at = $1 \
             WHERE id = $2 AND status = 'held' AND release_at <= $1",
        )
        .bind(now.as_millis())
        .bind(outbound_id.to_string())
        .execute(self.database.pool())
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// The member's Scheduled or Outbox list, the next to go first.
    pub async fn list_outbound(
        &self,
        membership_id: Id,
        view: OutboundView,
    ) -> Result<Vec<OutboundItem>, MailError> {
        let condition = match view {
            OutboundView::Scheduled => "o.status = 'held' AND o.scheduled = 1",
            OutboundView::Outbox => {
                "((o.status = 'held' AND o.scheduled = 0) OR o.status = 'released')"
            }
        };
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT o.id, o.status, o.release_at, o.scheduled, o.hold_reason, o.draft_id, \
             d.subject, d.to_addrs FROM mail_outbound o \
             LEFT JOIN mail_drafts d ON d.id = o.draft_id \
             WHERE o.membership_id = $1 AND {condition} ORDER BY o.release_at, o.id"
        )))
        .bind(membership_id.to_string())
        .fetch_all(self.database.pool())
        .await?;
        rows.iter()
            .map(|row| {
                let draft_id: Option<String> = row.try_get("draft_id")?;
                let to: Option<String> = row.try_get("to_addrs")?;
                Ok(OutboundItem {
                    outbound: outbound(row)?,
                    draft_id: draft_id.map(|id| parse_id(&id)).transpose()?,
                    subject: row
                        .try_get::<Option<String>, _>("subject")?
                        .unwrap_or_default(),
                    to: to
                        .and_then(|json| serde_json::from_str(&json).ok())
                        .unwrap_or_default(),
                })
            })
            .collect()
    }
}
