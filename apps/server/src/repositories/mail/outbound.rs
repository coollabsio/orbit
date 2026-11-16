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
use super::search::people;
use super::signatures::check_sendable;
use super::suppressions::{SuppressionReason, suppresses_response};
use super::{MailError, MailRepository, SendReadiness};
use crate::live::{LiveHub, NewEvent, Recipients};

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

pub(super) fn send_job(outbound_id: Id, workspace_id: Id, release_at: TimestampMillis) -> Job {
    Job::new(
        mail_send_kind(),
        json!({ "outbound_id": outbound_id.to_string() }),
        release_at,
    )
    .with_workspace(workspace_id.to_string())
}

pub(super) async fn enqueue(
    transaction: &mut Transaction<'_, Any>,
    job: &Job,
) -> Result<(), MailError> {
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
    // The body goes as HTML and as a text alternative, each as base64 (4/3).
    let body = (draft.body_html.len() + orbit_mail::html_to_text(&draft.body_html).len()) as i64;
    HEADER_BYTES + draft.subject.len() as i64 + (body + 2) / 3 * 4 + files
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
        self.send_draft_between(membership_id, draft_id, scheduled_at, now, async {})
            .await
    }

    /// [`Self::send_draft`], awaiting `between` after the checks and before the hold. For tests
    /// of a change that lands in between (file changes do not bump the draft's version).
    #[doc(hidden)]
    pub async fn send_draft_between(
        &self,
        membership_id: Id,
        draft_id: Id,
        scheduled_at: Option<TimestampMillis>,
        now: TimestampMillis,
        between: impl std::future::Future<Output = ()>,
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
        .await
        .map_err(|error| match error {
            MailError::NotFound => MailError::Forbidden("send_as_denied"),
            other => other,
        })?;
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
                return Err(MailError::Forbidden("send_as_denied"));
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

        between.await;
        let mut transaction = self.database.immediate_transaction().await?;
        let limits = load_limits(&mut transaction, workspace_id, self.cloud).await?;
        if all.len() as i64 > limits.max_recipients {
            return Err(MailError::Unprocessable("too_many_recipients"));
        }
        // The draft must be the one that was checked. File changes do not bump the version, so the
        // size is checked again on the files as they are now.
        let current = load_draft(&mut transaction, membership_id, draft_id).await?;
        if current.version != draft.version {
            return Err(MailError::Conflict("draft_conflict"));
        }
        if max_bytes > 0 && encoded_size(&current) > max_bytes {
            return Err(MailError::Unprocessable("message_too_large"));
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
            && room.as_millis() > release_at
        {
            release_at = room.as_millis();
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
    /// message is due. `true` for the one caller that got it. [`Self::release`] claims with the
    /// same condition inside its own transaction, so a failed build leaves the row held.
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
        // A held message reads its draft; a released one (its draft is deleted) the sent message.
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT o.id, o.status, o.release_at, o.scheduled, o.hold_reason, o.draft_id, \
             d.subject, d.to_addrs, m.subject AS sent_subject, m.to_addrs AS sent_to \
             FROM mail_outbound o \
             LEFT JOIN mail_drafts d ON d.id = o.draft_id \
             LEFT JOIN mail_messages m ON m.id = o.message_id \
             WHERE o.membership_id = $1 AND {condition} ORDER BY o.release_at, o.id"
        )))
        .bind(membership_id.to_string())
        .fetch_all(self.database.pool())
        .await?;
        rows.iter()
            .map(|row| {
                let draft_id: Option<String> = row.try_get("draft_id")?;
                let to: Option<String> = row.try_get("to_addrs")?;
                let to = match to {
                    Some(json) => serde_json::from_str(&json).unwrap_or_default(),
                    None => row
                        .try_get::<Option<String>, _>("sent_to")?
                        .map(|json| {
                            people(&json)
                                .into_iter()
                                .map(|person| person.address)
                                .collect()
                        })
                        .unwrap_or_default(),
                };
                let subject = match row.try_get::<Option<String>, _>("subject")? {
                    Some(subject) => subject,
                    None => row
                        .try_get::<Option<String>, _>("sent_subject")?
                        .unwrap_or_default(),
                };
                Ok(OutboundItem {
                    outbound: outbound(row)?,
                    draft_id: draft_id.map(|id| parse_id(&id)).transpose()?,
                    subject,
                    to,
                })
            })
            .collect()
    }
}

// ---- Delivery (spec §5.3 steps 5–7, §5.4) ----

/// The waits after each deferred attempt: 1 m, 5 m, 15 m, 30 m, 1 h, 2 h, 4 h, 8 h, 12 h, 24 h,
/// 24 h, 24 h (RFC 5321 §4.5.4.1). A recipient deferred again after the last one has bounced
/// ([`GAVE_UP`]).
pub const RETRY_STEPS_MS: [i64; 12] = [
    60_000,
    5 * 60_000,
    15 * 60_000,
    30 * 60_000,
    3_600_000,
    2 * 3_600_000,
    4 * 3_600_000,
    8 * 3_600_000,
    12 * 3_600_000,
    24 * 3_600_000,
    24 * 3_600_000,
    24 * 3_600_000,
];
/// A deferred recipient whose first attempt (`released_at`) is older than this shows **Delayed**.
pub const DELAYED_AFTER_MS: i64 = 4 * 3_600_000;
/// The deadline of all the adapter and intake calls of one delivery run, from its claim; the
/// recipients of calls not finished by then are deferred with "Timed out".
pub const SEND_TIMEOUT_MS: i64 = 20 * 60_000;
/// A recipient that stays `sending` longer than this was left by a worker that stopped between
/// the claim and the result: it is deferred and sent again (at least once, spec §5.4). Longer
/// than the run deadline [`SEND_TIMEOUT_MS`], so an attempt that is still running is never
/// handed to another;
/// the late result of an attempt that was handed on is ignored ([`MailRepository::record_outcomes`]).
pub const SENDING_LEASE_MS: i64 = 30 * 60_000;
/// The response of a recipient that was still deferred after the last retry step.
pub const GAVE_UP: &str = "Gave up after 5 days";
/// The response of a recipient when the sent message could not be read (then `: <error>`).
pub const UNREADABLE: &str = "The message could not be read";
/// Orbit's own refusal of a message larger than the route allows (the adapter pre-check).
pub const ROUTE_TOO_LARGE: &str = "5.3.4 The message is larger than the route allows";
/// This instance's intake refused a local message as too large.
pub const LOCAL_TOO_LARGE: &str = "552 5.3.4 The message is larger than this server accepts";
/// The local receiving domain is disabled, or suspended past its grace period.
pub const LOCAL_DOMAIN_UNAVAILABLE: &str = "550 5.7.1 The domain does not accept mail";
/// The local receiving mailbox is suspended.
pub const LOCAL_MAILBOX_SUSPENDED: &str = "550 5.2.1 The mailbox is not active";

/// Why a recipient was deferred; at the give-up it picks the [`BounceKind`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeferCause {
    /// A 4xx, a network error or a timeout of the next hop: [`BounceKind::GaveUp`].
    Temporary,
    /// The route is at fault (an adapter's route error): [`BounceKind::Route`].
    Route,
    /// The sent message could not be read: [`BounceKind::Unreadable`].
    Unreadable,
}

/// The result of one attempt for one recipient, as recorded. The kinds are data set by the code
/// that decided them (the adapter's flags, intake, delivery), never read from a response's
/// text, so a remote server cannot pass as Orbit or as the route.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttemptResult {
    Delivered { provider_message_id: Option<String> },
    Bounced { response: String, kind: BounceKind },
    Deferred { response: String, cause: DeferCause },
}

impl From<crate::mail_send::RecipientOutcome> for AttemptResult {
    /// An adapter's or intake's outcome: its flags pick the kind.
    fn from(outcome: crate::mail_send::RecipientOutcome) -> Self {
        use crate::mail_send::RecipientOutcome;
        match outcome {
            RecipientOutcome::Accepted {
                provider_message_id,
            } => Self::Delivered {
                provider_message_id,
            },
            RecipientOutcome::Bounced { response, rejected } => Self::Bounced {
                response,
                kind: if rejected {
                    BounceKind::Rejected
                } else {
                    BounceKind::Hard
                },
            },
            RecipientOutcome::Deferred {
                response,
                route_error,
            } => Self::Deferred {
                response,
                cause: if route_error {
                    DeferCause::Route
                } else {
                    DeferCause::Temporary
                },
            },
        }
    }
}

/// Why a bounced recipient bounced (`mail_outbound_recipients.bounce_kind`). Only [`Self::Hard`]
/// (and complaints) count toward the bounce rate and the suppression list: every other kind is
/// not a bad address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BounceKind {
    /// A 5xx refusal of the address (SMTP, a provider's API, or an unknown user at this
    /// instance's intake).
    Hard,
    /// Orbit refused the message itself, not the address: larger than the route or this server
    /// allows, or the local receiving domain or mailbox is unavailable.
    Rejected,
    /// Still deferred after the last retry step.
    GaveUp,
    /// Gave up, and the last attempt failed on the route (`Route error: …`).
    Route,
    /// The sent message could not be read.
    Unreadable,
}

impl BounceKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Hard => "hard",
            Self::Rejected => "rejected",
            Self::GaveUp => "gave_up",
            Self::Route => "route",
            Self::Unreadable => "unreadable",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "hard" => Some(Self::Hard),
            "rejected" => Some(Self::Rejected),
            "gave_up" => Some(Self::GaveUp),
            "route" => Some(Self::Route),
            "unreadable" => Some(Self::Unreadable),
            _ => None,
        }
    }
}

/// A recipient claimed for one attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecipientRow {
    pub id: Id,
    /// Normalized (`local@domain`, the domain in lower-case A-labels).
    pub address: String,
    /// `to`, `cc` or `bcc`.
    pub kind: String,
    /// With this attempt.
    pub attempts: i64,
}

/// A released message, as delivery needs it.
#[derive(Clone, Debug)]
pub struct OutboundDelivery {
    pub outbound_id: Id,
    pub workspace_id: Id,
    pub mailbox_id: Id,
    /// The sent message's receipt: its sealed raw bytes are what goes out.
    pub receipt_id: Id,
    /// The `From` address.
    pub from_address: String,
    /// The domain of the `From` address.
    pub from_domain: String,
    /// That domain's row in the workspace (its send route); `None` when it was removed.
    pub domain_id: Option<Id>,
}

/// What [`MailRepository::claim_recipients`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Claim {
    /// The message is not released (held, cancelled, done) or is gone.
    NotReleased,
    /// The workspace's sending is paused: the message waits with `hold_reason = 'paused'` and no
    /// run until the pause is lifted.
    Paused,
    /// Over a send limit: the message waits with `hold_reason = 'limit'` until then.
    Limited(TimestampMillis),
    /// These recipients are `sending` with the attempt's ID. `changed`: a status changed
    /// (claimed or suppressed).
    Claimed {
        recipients: Vec<RecipientRow>,
        changed: bool,
    },
}

/// What [`MailRepository::record_outcomes`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recorded {
    /// Every recipient is final: the message is `done`.
    pub done: bool,
    /// When a recipient is due again (a deferred one, or one another attempt is sending).
    pub next_at: Option<TimestampMillis>,
    /// A recipient bounced now: the thread went back to the Inbox, unread.
    pub bounced: bool,
}

/// The delivery status of one recipient (the Outbox and the message's status line).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecipientStatus {
    pub id: Id,
    pub address: String,
    pub kind: String,
    /// `queued`, `sending`, `delivered`, `deferred`, `bounced`, `suppressed` or `cancelled`.
    pub status: String,
    pub attempts: i64,
    pub next_attempt_at: Option<TimestampMillis>,
    pub last_response: Option<String>,
    pub provider_message_id: Option<String>,
    /// Deferred, and the first attempt is more than [`DELAYED_AFTER_MS`] ago.
    pub delayed: bool,
    /// Why it bounced, for a `bounced` recipient.
    pub bounce_kind: Option<BounceKind>,
}

/// The readers of an outbound message's thread, for `thread.updated`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThreadAudience {
    pub workspace_id: Id,
    pub mailbox_id: Id,
    pub thread_id: Id,
    pub users: Vec<Id>,
}

/// Recipients Orbit still has to send: queued, sending, or deferred to a next attempt. A
/// recipient that a provider's event deferred has no next attempt (the provider retries, Orbit
/// does not: `super::events`), so it does not keep the message open.
const OPEN: &str = "(status IN ('queued', 'sending') \
    OR (status = 'deferred' AND next_attempt_at IS NOT NULL))";

/// When a recipient of the message is due again: a queued one now, a deferred one at its next
/// attempt, one that another attempt is sending when its lease ends.
async fn next_due(
    transaction: &mut Transaction<'_, Any>,
    outbound_id: Id,
    now: TimestampMillis,
) -> Result<Option<TimestampMillis>, MailError> {
    let next: Option<i64> = sqlx::query_scalar(
        "SELECT MIN(CASE WHEN status = 'deferred' THEN next_attempt_at \
         WHEN status = 'queued' THEN $1 ELSE updated_at + $2 END) \
         FROM mail_outbound_recipients WHERE outbound_id = $3 \
         AND status IN ('queued', 'sending', 'deferred')",
    )
    .bind(now.as_millis())
    .bind(SENDING_LEASE_MS)
    .bind(outbound_id.to_string())
    .fetch_one(&mut **transaction)
    .await?;
    Ok(next.map(TimestampMillis::from_millis))
}

/// Kept of a recipient's last response in `delivery_summary`.
const SUMMARY_RESPONSE_BYTES: usize = 500;

/// The `delivery_summary` of the message: `[{address, kind, status, response, bounce_kind}]`.
pub(super) async fn delivery_summary(
    transaction: &mut Transaction<'_, Any>,
    outbound_id: Id,
) -> Result<String, MailError> {
    let rows = sqlx::query(
        "SELECT id, address, kind, status, last_response, bounce_kind FROM mail_outbound_recipients \
         WHERE outbound_id = $1 ORDER BY created_at, id",
    )
    .bind(outbound_id.to_string())
    .fetch_all(&mut **transaction)
    .await?;
    let mut summary = Vec::with_capacity(rows.len());
    for row in &rows {
        let mut response = row.try_get::<Option<String>, _>("last_response")?;
        if let Some(response) = &mut response {
            super::threads::truncate(response, SUMMARY_RESPONSE_BYTES);
        }
        summary.push(json!({
            "id": row.try_get::<String, _>("id")?,
            "address": row.try_get::<String, _>("address")?,
            "kind": row.try_get::<String, _>("kind")?,
            "status": row.try_get::<String, _>("status")?,
            "response": response,
            "bounce_kind": row.try_get::<Option<String>, _>("bounce_kind")?,
        }));
    }
    Ok(serde_json::Value::Array(summary).to_string())
}

/// Marks the message `done` with the summary of its recipients once none is open.
pub(super) async fn finish_if_done(
    transaction: &mut Transaction<'_, Any>,
    outbound_id: Id,
    now: TimestampMillis,
) -> Result<bool, MailError> {
    let open: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT CAST(COUNT(*) AS BIGINT) FROM mail_outbound_recipients \
         WHERE outbound_id = $1 AND {OPEN}"
    )))
    .bind(outbound_id.to_string())
    .fetch_one(&mut **transaction)
    .await?;
    if open > 0 {
        return Ok(false);
    }
    let summary = delivery_summary(transaction, outbound_id).await?;
    let done = sqlx::query(
        "UPDATE mail_outbound SET status = 'done', hold_reason = NULL, delivery_summary = $1, \
         updated_at = $2 WHERE id = $3 AND status = 'released'",
    )
    .bind(summary)
    .bind(now.as_millis())
    .bind(outbound_id.to_string())
    .execute(&mut **transaction)
    .await?
    .rows_affected();
    Ok(done == 1)
}

/// A message that is `done` already gets its summary written again (a later bounce changed a
/// recipient); one still going is marked `done` when nothing is open.
pub(super) async fn update_summary(
    transaction: &mut Transaction<'_, Any>,
    outbound_id: Id,
    now: TimestampMillis,
) -> Result<(), MailError> {
    if finish_if_done(transaction, outbound_id, now).await? {
        return Ok(());
    }
    let summary = delivery_summary(transaction, outbound_id).await?;
    sqlx::query(
        "UPDATE mail_outbound SET delivery_summary = $1, updated_at = $2 \
         WHERE id = $3 AND status = 'done'",
    )
    .bind(summary)
    .bind(now.as_millis())
    .bind(outbound_id.to_string())
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

/// A recipient of the message bounced: its thread goes back to the Inbox (from the archive), unread
/// for every reader (spec §5.7).
pub(super) async fn bounced_thread_to_inbox(
    transaction: &mut Transaction<'_, Any>,
    outbound_id: Id,
    now: TimestampMillis,
) -> Result<(), MailError> {
    let threads: Vec<String> = sqlx::query_scalar(
        "SELECT tm.thread_id FROM mail_outbound o \
         JOIN mail_thread_messages tm ON tm.message_id = o.message_id \
         AND tm.mailbox_id = o.mailbox_id WHERE o.id = $1",
    )
    .bind(outbound_id.to_string())
    .fetch_all(&mut **transaction)
    .await?;
    for thread_id in threads {
        sqlx::query(
            "UPDATE mail_threads SET folder = 'inbox', updated_at = $1 \
             WHERE id = $2 AND folder = 'archive'",
        )
        .bind(now.as_millis())
        .bind(&thread_id)
        .execute(&mut **transaction)
        .await?;
        sqlx::query("UPDATE mail_thread_user_state SET last_read_at = NULL WHERE thread_id = $1")
            .bind(&thread_id)
            .execute(&mut **transaction)
            .await?;
    }
    Ok(())
}

pub(super) fn plain_line(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(500)
        .collect::<String>()
        .trim()
        .to_owned()
}

impl MailRepository {
    /// The signed bytes of a sent message (its receipt's sealed raw), as delivery sends them.
    pub async fn read_sent_raw(&self, receipt_id: Id) -> Result<Vec<u8>, MailError> {
        self.read_raw(receipt_id).await
    }

    /// The released message `outbound_id` with what delivery needs; `None` when it is not
    /// released. A released message whose sent copy is gone (deleted from the thread and purged)
    /// cannot be sent: its open recipients are cancelled.
    pub async fn outbound_delivery(
        &self,
        outbound_id: Id,
        now: TimestampMillis,
    ) -> Result<Option<OutboundDelivery>, MailError> {
        let Some(row) = sqlx::query(
            "SELECT o.workspace_id, o.mailbox_id, m.receipt_id, r.mail_from \
             FROM mail_outbound o \
             LEFT JOIN mail_messages m ON m.id = o.message_id \
             LEFT JOIN mail_receipts r ON r.id = m.receipt_id \
             WHERE o.id = $1 AND o.status = 'released'",
        )
        .bind(outbound_id.to_string())
        .fetch_optional(self.database.pool())
        .await?
        else {
            return Ok(None);
        };
        let workspace_id = parse_id(&row.try_get::<String, _>("workspace_id")?)?;
        let mailbox_id = parse_id(&row.try_get::<String, _>("mailbox_id")?)?;
        let receipt: Option<String> = row.try_get("receipt_id")?;
        let from: Option<String> = row.try_get("mail_from")?;
        let (Some(receipt), Some(from_address)) = (receipt, from) else {
            self.cancel_open_recipients(outbound_id, "The sent message was deleted.", now)
                .await?;
            return Ok(None);
        };
        let from_domain = from_address
            .rsplit_once('@')
            .map(|(_, domain)| domain.to_owned())
            .unwrap_or_default();
        let domain: Option<String> =
            sqlx::query_scalar("SELECT id FROM mail_domains WHERE name = $1 AND workspace_id = $2")
                .bind(&from_domain)
                .bind(workspace_id.to_string())
                .fetch_optional(self.database.pool())
                .await?;
        Ok(Some(OutboundDelivery {
            outbound_id,
            workspace_id,
            mailbox_id,
            receipt_id: parse_id(&receipt)?,
            from_address,
            from_domain,
            domain_id: domain.map(|id| parse_id(&id)).transpose()?,
        }))
    }

    async fn cancel_open_recipients(
        &self,
        outbound_id: Id,
        reason: &str,
        now: TimestampMillis,
    ) -> Result<(), MailError> {
        let mut transaction = self.database.immediate_transaction().await?;
        sqlx::query(
            "UPDATE mail_outbound_recipients SET status = 'cancelled', last_response = $1, \
             next_attempt_at = NULL, updated_at = $2 WHERE outbound_id = $3 \
             AND (status = 'queued' OR (status = 'deferred' AND next_attempt_at IS NOT NULL))",
        )
        .bind(reason)
        .bind(now.as_millis())
        .bind(outbound_id.to_string())
        .execute(&mut *transaction)
        .await?;
        finish_if_done(&mut transaction, outbound_id, now).await?;
        transaction.commit().await?;
        self.announce_outbounds(&[outbound_id]).await;
        Ok(())
    }

    /// `thread.updated` to the readers of each message's thread (the delivery status changed);
    /// the change is stored either way.
    pub async fn announce_outbounds(&self, outbound_ids: &[Id]) {
        for outbound_id in outbound_ids {
            let audience = match self.outbound_audience(*outbound_id).await {
                Ok(Some(audience)) if !audience.users.is_empty() => audience,
                Ok(_) => continue,
                Err(error) => {
                    tracing::warn!(error = %error, outbound_id = %outbound_id, "mail delivery audience");
                    continue;
                }
            };
            let events = [NewEvent {
                recipients: Recipients::Users(audience.users),
                topic: super::MAIL_TOPIC,
                event: json!({
                    "type": "thread.updated",
                    "mailbox_id": audience.mailbox_id.to_string(),
                    "thread_id": audience.thread_id.to_string(),
                }),
            }];
            if let Err(error) = LiveHub::of(&self.database)
                .publish(audience.workspace_id, &events)
                .await
            {
                tracing::warn!(error = %error, "mail events for the live socket");
            }
        }
    }

    /// One attempt's claim (spec §5.4), in one writer transaction: a paused workspace holds the
    /// message (`paused`); recipients on the workspace's suppression list become `suppressed`;
    /// recipients never tried yet are checked against the send limits (without the message's own
    /// rows) and hold it (`limit`) when over: their `released_at` is set to now, so they keep
    /// their room against messages that come later (first come, first served), and set again
    /// when they go;
    /// a recipient that a provider's event deferred (no `next_attempt_at`) is left to the
    /// provider: never suppressed, claimed or cancelled here;
    /// then every queued recipient and every deferred one that is due becomes `sending` with
    /// `attempt_id`, and is returned. The caller sends to them after this commit.
    pub async fn claim_recipients(
        &self,
        outbound_id: Id,
        attempt_id: Id,
        now: TimestampMillis,
    ) -> Result<Claim, MailError> {
        let mut transaction = self.database.immediate_transaction().await?;
        let Some(row) = sqlx::query(
            "SELECT workspace_id, membership_id, hold_reason FROM mail_outbound \
             WHERE id = $1 AND status = 'released'",
        )
        .bind(outbound_id.to_string())
        .fetch_optional(&mut *transaction)
        .await?
        else {
            return Ok(Claim::NotReleased);
        };
        let workspace_id = parse_id(&row.try_get::<String, _>("workspace_id")?)?;
        let membership_id = parse_id(&row.try_get::<String, _>("membership_id")?)?;
        let hold: Option<String> = row.try_get("hold_reason")?;
        let paused: Option<i64> =
            sqlx::query_scalar("SELECT paused_at FROM mail_send_policies WHERE workspace_id = $1")
                .bind(workspace_id.to_string())
                .fetch_optional(&mut *transaction)
                .await?
                .flatten();
        if paused.is_some() {
            sqlx::query(
                "UPDATE mail_outbound SET hold_reason = 'paused', updated_at = $1 WHERE id = $2",
            )
            .bind(now.as_millis())
            .bind(outbound_id.to_string())
            .execute(&mut *transaction)
            .await?;
            transaction.commit().await?;
            return Ok(Claim::Paused);
        }
        let suppressed = sqlx::query(
            "UPDATE mail_outbound_recipients SET status = 'suppressed', \
             last_response = 'The address is on the suppression list.', next_attempt_at = NULL, \
             updated_at = $1 WHERE outbound_id = $2 \
             AND (status = 'queued' OR (status = 'deferred' AND next_attempt_at IS NOT NULL)) \
             AND EXISTS (SELECT 1 FROM mail_suppressions s \
             WHERE s.workspace_id = mail_outbound_recipients.workspace_id \
             AND s.address = lower(mail_outbound_recipients.address))",
        )
        .bind(now.as_millis())
        .bind(outbound_id.to_string())
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        let fresh: i64 = sqlx::query_scalar(
            "SELECT CAST(COUNT(*) AS BIGINT) FROM mail_outbound_recipients \
             WHERE outbound_id = $1 AND status = 'queued' AND attempts = 0",
        )
        .bind(outbound_id.to_string())
        .fetch_one(&mut *transaction)
        .await?;
        if fresh > 0
            && let Some(room) = self
                .window_allows(
                    &mut transaction,
                    workspace_id,
                    membership_id,
                    fresh,
                    now,
                    Some(outbound_id),
                )
                .await?
        {
            sqlx::query(
                "UPDATE mail_outbound_recipients SET released_at = $1, updated_at = $1 \
                 WHERE outbound_id = $2 AND status = 'queued' AND attempts = 0",
            )
            .bind(now.as_millis())
            .bind(outbound_id.to_string())
            .execute(&mut *transaction)
            .await?;
            sqlx::query(
                "UPDATE mail_outbound SET hold_reason = 'limit', updated_at = $1 WHERE id = $2",
            )
            .bind(now.as_millis())
            .bind(outbound_id.to_string())
            .execute(&mut *transaction)
            .await?;
            transaction.commit().await?;
            return Ok(Claim::Limited(room));
        }
        // They count against the limits from the time they go.
        sqlx::query(
            "UPDATE mail_outbound_recipients SET released_at = $1 \
             WHERE outbound_id = $2 AND status = 'queued' AND attempts = 0",
        )
        .bind(now.as_millis())
        .bind(outbound_id.to_string())
        .execute(&mut *transaction)
        .await?;
        if hold.is_some() {
            sqlx::query(
                "UPDATE mail_outbound SET hold_reason = NULL, updated_at = $1 WHERE id = $2",
            )
            .bind(now.as_millis())
            .bind(outbound_id.to_string())
            .execute(&mut *transaction)
            .await?;
        }
        let claimed = sqlx::query(
            "UPDATE mail_outbound_recipients SET status = 'sending', attempt_id = $1, \
             attempts = attempts + 1, next_attempt_at = NULL, updated_at = $2 \
             WHERE outbound_id = $3 AND (status = 'queued' \
             OR (status = 'deferred' AND next_attempt_at IS NOT NULL \
             AND next_attempt_at <= $2))",
        )
        .bind(attempt_id.to_string())
        .bind(now.as_millis())
        .bind(outbound_id.to_string())
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        let rows = sqlx::query(
            "SELECT id, address, kind, attempts FROM mail_outbound_recipients \
             WHERE outbound_id = $1 AND attempt_id = $2 AND status = 'sending' \
             ORDER BY created_at, id",
        )
        .bind(outbound_id.to_string())
        .bind(attempt_id.to_string())
        .fetch_all(&mut *transaction)
        .await?;
        let recipients = rows
            .iter()
            .map(|row| {
                Ok(RecipientRow {
                    id: parse_id(&row.try_get::<String, _>("id")?)?,
                    address: row.try_get("address")?,
                    kind: row.try_get("kind")?,
                    attempts: row.try_get("attempts")?,
                })
            })
            .collect::<Result<Vec<_>, MailError>>()?;
        if suppressed > 0 {
            finish_if_done(&mut transaction, outbound_id, now).await?;
        }
        transaction.commit().await?;
        Ok(Claim::Claimed {
            recipients,
            changed: suppressed > 0 || claimed > 0,
        })
    }

    /// The results of attempt `attempt_id`, in one writer transaction. Only recipients still
    /// `sending` with that attempt change (one that a lease end handed to another attempt is
    /// left to it, and a late result of this attempt changes nothing). Delivered → `delivered`;
    /// bounced → `bounced` with its [`BounceKind`]; deferred → `deferred` until the next step of
    /// [`RETRY_STEPS_MS`], or `bounced` with [`GAVE_UP`] after the last one, its kind from the
    /// last attempt's [`DeferCause`] (route → [`BounceKind::Route`], unreadable →
    /// [`BounceKind::Unreadable`], else [`BounceKind::GaveUp`]). No kind is read from a
    /// response's text. A hard bounce of a bad address (`super::suppresses`, which reads only
    /// its status codes) puts the address on the workspace's suppression list. A bounce moves
    /// the thread back to the Inbox (from the archive), unread for its readers. The message is
    /// `done` with its summary once every recipient is final.
    pub async fn record_outcomes(
        &self,
        outbound_id: Id,
        attempt_id: Id,
        outcomes: &[(Id, AttemptResult)],
        now: TimestampMillis,
    ) -> Result<Recorded, MailError> {
        let mut transaction = self.database.immediate_transaction().await?;
        let mut bounced = false;
        let mut hard_bounced = None;
        for (recipient_id, outcome) in outcomes {
            let row = sqlx::query(
                "SELECT attempts, workspace_id, address FROM mail_outbound_recipients \
                 WHERE id = $1 AND outbound_id = $2 AND attempt_id = $3 AND status = 'sending'",
            )
            .bind(recipient_id.to_string())
            .bind(outbound_id.to_string())
            .bind(attempt_id.to_string())
            .fetch_optional(&mut *transaction)
            .await?;
            let Some(row) = row else {
                continue;
            };
            let attempts: i64 = row.try_get("attempts")?;
            let (status, response, next, provider_id, kind) = match outcome {
                AttemptResult::Delivered {
                    provider_message_id,
                } => ("delivered", None, None, provider_message_id.clone(), None),
                AttemptResult::Bounced { response, kind } => (
                    "bounced",
                    Some(plain_line(response)),
                    None,
                    None,
                    Some(*kind),
                ),
                AttemptResult::Deferred { response, cause } => {
                    let step = usize::try_from(attempts - 1).ok();
                    match step.and_then(|step| RETRY_STEPS_MS.get(step)) {
                        Some(wait) => (
                            "deferred",
                            Some(plain_line(response)),
                            Some(now.as_millis() + wait),
                            None,
                            None,
                        ),
                        None => {
                            let kind = match cause {
                                DeferCause::Route => BounceKind::Route,
                                DeferCause::Unreadable => BounceKind::Unreadable,
                                DeferCause::Temporary => BounceKind::GaveUp,
                            };
                            (
                                "bounced",
                                Some(plain_line(&format!("{GAVE_UP}: {response}"))),
                                None,
                                None,
                                Some(kind),
                            )
                        }
                    }
                }
            };
            bounced |= status == "bounced";
            if kind == Some(BounceKind::Hard) {
                hard_bounced = Some(parse_id(&row.try_get::<String, _>("workspace_id")?)?);
            }
            sqlx::query(
                "UPDATE mail_outbound_recipients SET status = $1, last_response = $2, \
                 next_attempt_at = $3, provider_message_id = COALESCE($4, provider_message_id), \
                 bounce_kind = $5, updated_at = $6 WHERE id = $7",
            )
            .bind(status)
            .bind(response.clone())
            .bind(next)
            .bind(provider_id)
            .bind(kind.map(BounceKind::as_str))
            .bind(now.as_millis())
            .bind(recipient_id.to_string())
            .execute(&mut *transaction)
            .await?;
            if let (Some(kind), Some(response)) = (kind, &response)
                && suppresses_response(kind, response)
            {
                Self::add_suppression(
                    &mut transaction,
                    parse_id(&row.try_get::<String, _>("workspace_id")?)?,
                    &row.try_get::<String, _>("address")?,
                    SuppressionReason::HardBounce,
                    Some(response),
                    Some(*recipient_id),
                    now,
                )
                .await?;
            }
        }
        if bounced {
            bounced_thread_to_inbox(&mut transaction, outbound_id, now).await?;
        }
        if let Some(workspace_id) = hard_bounced {
            self.check_pause(&mut transaction, workspace_id, now)
                .await?;
        }
        let done = finish_if_done(&mut transaction, outbound_id, now).await?;
        let next_at = next_due(&mut transaction, outbound_id, now).await?;
        transaction.commit().await?;
        Ok(Recorded {
            done,
            next_at,
            bounced,
        })
    }

    /// When a recipient of the released message is due again; `None` when none is open.
    pub async fn next_delivery(
        &self,
        outbound_id: Id,
        now: TimestampMillis,
    ) -> Result<Option<TimestampMillis>, MailError> {
        let mut transaction = self.database.pool().begin().await?;
        let next = next_due(&mut transaction, outbound_id, now).await?;
        transaction.commit().await?;
        Ok(next)
    }

    /// Recipients left `sending` for longer than `lease_ms` (a worker stopped between the claim
    /// and the result) become `deferred` and due now: sent again, at least once (spec §5.4).
    /// Their threads' readers get `thread.updated`. Returns how many changed.
    pub async fn requeue_stale_sending(
        &self,
        lease_ms: i64,
        now: TimestampMillis,
    ) -> Result<u64, MailError> {
        let mut transaction = self.database.immediate_transaction().await?;
        let outbounds: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT outbound_id FROM mail_outbound_recipients \
             WHERE status = 'sending' AND updated_at <= $1",
        )
        .bind(now.as_millis() - lease_ms)
        .fetch_all(&mut *transaction)
        .await?;
        if outbounds.is_empty() {
            return Ok(0);
        }
        let changed = sqlx::query(
            "UPDATE mail_outbound_recipients SET status = 'deferred', next_attempt_at = $1, \
             last_response = 'The attempt was interrupted.', updated_at = $1 \
             WHERE status = 'sending' AND updated_at <= $2",
        )
        .bind(now.as_millis())
        .bind(now.as_millis() - lease_ms)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        transaction.commit().await?;
        let ids = outbounds
            .iter()
            .map(|id| parse_id(id))
            .collect::<Result<Vec<_>, _>>()?;
        self.announce_outbounds(&ids).await;
        Ok(changed)
    }

    /// Queues the next `mail.send` run of the message at `at`, unless one is queued by then.
    pub async fn queue_send_run(
        &self,
        outbound_id: Id,
        at: TimestampMillis,
    ) -> Result<(), MailError> {
        let mut transaction = self.database.immediate_transaction().await?;
        let workspace: Option<String> =
            sqlx::query_scalar("SELECT workspace_id FROM mail_outbound WHERE id = $1")
                .bind(outbound_id.to_string())
                .fetch_optional(&mut *transaction)
                .await?;
        let Some(workspace) = workspace else {
            return Ok(());
        };
        let queued: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM jobs WHERE kind = 'mail.send' AND state = 'queued' \
             AND payload_json = $1 AND available_at <= $2 LIMIT 1",
        )
        .bind(json!({ "outbound_id": outbound_id.to_string() }).to_string())
        .bind(at.as_millis())
        .fetch_optional(&mut *transaction)
        .await?;
        if queued.is_none() {
            enqueue(
                &mut transaction,
                &send_job(outbound_id, parse_id(&workspace)?, at),
            )
            .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    /// The delivery status of each recipient of the message at `now`.
    pub async fn outbound_recipients(
        &self,
        outbound_id: Id,
        now: TimestampMillis,
    ) -> Result<Vec<RecipientStatus>, MailError> {
        let rows = sqlx::query(
            "SELECT id, address, kind, status, attempts, next_attempt_at, last_response, \
             provider_message_id, released_at, bounce_kind FROM mail_outbound_recipients \
             WHERE outbound_id = $1 ORDER BY created_at, id",
        )
        .bind(outbound_id.to_string())
        .fetch_all(self.database.pool())
        .await?;
        if rows.is_empty() {
            // purged after 90 days: the summary written then is all that is left
            return self.summarized_recipients(outbound_id).await;
        }
        rows.iter()
            .map(|row| {
                let status: String = row.try_get("status")?;
                let released: Option<i64> = row.try_get("released_at")?;
                Ok(RecipientStatus {
                    id: parse_id(&row.try_get::<String, _>("id")?)?,
                    address: row.try_get("address")?,
                    kind: row.try_get("kind")?,
                    delayed: status == "deferred"
                        && released.is_some_and(|at| now.as_millis() - at > DELAYED_AFTER_MS),
                    status,
                    attempts: row.try_get("attempts")?,
                    next_attempt_at: row
                        .try_get::<Option<i64>, _>("next_attempt_at")?
                        .map(TimestampMillis::from_millis),
                    last_response: row.try_get("last_response")?,
                    provider_message_id: row.try_get("provider_message_id")?,
                    bounce_kind: row
                        .try_get::<Option<String>, _>("bounce_kind")?
                        .as_deref()
                        .and_then(BounceKind::parse),
                })
            })
            .collect()
    }

    /// The recipients of a message whose recipient rows were purged, from `delivery_summary`.
    async fn summarized_recipients(
        &self,
        outbound_id: Id,
    ) -> Result<Vec<RecipientStatus>, MailError> {
        let summary: Option<String> =
            sqlx::query_scalar("SELECT delivery_summary FROM mail_outbound WHERE id = $1")
                .bind(outbound_id.to_string())
                .fetch_optional(self.database.pool())
                .await?
                .flatten();
        let Some(summary) = summary else {
            return Ok(Vec::new());
        };
        let entries: Vec<serde_json::Value> = serde_json::from_str(&summary).unwrap_or_default();
        let text = |entry: &serde_json::Value, key: &str| {
            entry
                .get(key)
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        };
        Ok(entries
            .iter()
            .map(|entry| RecipientStatus {
                id: text(entry, "id")
                    .and_then(|id| id.parse().ok())
                    .unwrap_or_else(Id::new_v7),
                address: text(entry, "address").unwrap_or_default(),
                kind: text(entry, "kind").unwrap_or_else(|| "to".to_owned()),
                status: text(entry, "status").unwrap_or_else(|| "delivered".to_owned()),
                attempts: 0,
                next_attempt_at: None,
                last_response: text(entry, "response"),
                provider_message_id: None,
                delayed: false,
                bounce_kind: text(entry, "bounce_kind")
                    .as_deref()
                    .and_then(BounceKind::parse),
            })
            .collect())
    }

    /// The thread of the message's sent copy and its mailbox's readers; `None` when it is gone.
    pub async fn outbound_audience(
        &self,
        outbound_id: Id,
    ) -> Result<Option<ThreadAudience>, MailError> {
        let mut transaction = self.database.pool().begin().await?;
        let row = sqlx::query(
            "SELECT o.workspace_id, o.mailbox_id, tm.thread_id FROM mail_outbound o \
             JOIN mail_thread_messages tm ON tm.message_id = o.message_id \
             AND tm.mailbox_id = o.mailbox_id WHERE o.id = $1",
        )
        .bind(outbound_id.to_string())
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let mailbox_id = parse_id(&row.try_get::<String, _>("mailbox_id")?)?;
        let users = super::threads::mailbox_users(&mut transaction, mailbox_id).await?;
        let audience = ThreadAudience {
            workspace_id: parse_id(&row.try_get::<String, _>("workspace_id")?)?,
            mailbox_id,
            thread_id: parse_id(&row.try_get::<String, _>("thread_id")?)?,
            users,
        };
        transaction.commit().await?;
        Ok(Some(audience))
    }

    /// Whether mail to `domain` stays on this instance (spec D11): a domain of this instance,
    /// `verified` or `suspended`, whose last DNS check found its MX record right (the stored MX
    /// status is OK). Intake's recipient lookup then decides the rest: a suspended domain still
    /// takes mail during its grace period and refuses it after (a 5xx, recorded as such). A
    /// domain whose MX check is not OK goes out by the route.
    pub async fn is_local_domain(&self, domain: &str) -> Result<bool, MailError> {
        let id: Option<String> = sqlx::query_scalar(
            "SELECT id FROM mail_domains WHERE name = $1 AND status IN ('verified', 'suspended')",
        )
        .bind(domain)
        .fetch_optional(self.database.pool())
        .await?;
        let Some(id) = id else {
            return Ok(false);
        };
        Ok(self
            .domain_for_check(parse_id(&id)?)
            .await?
            .and_then(|domain| domain.report)
            .is_some_and(|report| report.mx == super::RecordStatus::Ok))
    }
}
