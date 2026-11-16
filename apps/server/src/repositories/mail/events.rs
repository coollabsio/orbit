//! Provider events (spec §5.7): what a send route's provider reports after it took a message.
//!
//! - **Matching.** An event names a recipient by the provider's message ID, else by the
//!   correlation value Orbit put on the message (the outbound recipient's ID,
//!   `{outbound}:{recipient}`, or the outbound's ID with the address), else by the sent
//!   message's `Message-ID` header with the address. The recipient's message must have gone
//!   through the route in the event's URL (`mail_outbound.route_id`), so a route's secret only
//!   reaches the messages of that route. Anything else is ignored.
//! - **Retry.** A recipient retried after the event happened (or with no time on the event)
//!   ignores it: the provider's message ID and the correlation stay the same after **Retry**.
//! - **Effects.** A bounce of a `delivered` or `deferred` recipient makes it `bounced`: `hard`
//!   when the provider calls it permanent, then the address is suppressed by the one rule
//!   (`super::suppresses`) on the diagnostic's codes, or always when the diagnostic has no code
//!   (the provider already classified it permanent); `gave_up` otherwise (never suppressed). The
//!   thread goes back to the Inbox, unread. A bounce of a recipient still `sending` asks the
//!   provider to send the event again ([`EventOutcome::Busy`]). A complaint suppresses the
//!   address (reason `complaint`) and leaves the recipient as it is. A temporary failure makes a
//!   `delivered` recipient `deferred` with no next attempt: the provider retries, Orbit does
//!   not. A delivery makes a `deferred` recipient `delivered`. Final statuses (`bounced`,
//!   `suppressed`, `cancelled`) never change.

use orbit_mail::events::{ParsedEvent, ProviderEvent};
use orbit_platform::{Id, Row, TimestampMillis};
use sqlx::any::AnyRow;
use sqlx::{Any, Transaction};

use super::domains::parse_id;
use super::outbound::{BounceKind, bounced_thread_to_inbox, plain_line, update_summary};
use super::suppressions::{SuppressionReason, bounce_codes, suppresses};
use super::{MailError, MailRepository};

const HARD_DEFAULT: &str = "The provider reported a permanent failure.";
const SOFT_DEFAULT: &str = "The provider gave up delivering the message.";
const DEFERRED_DEFAULT: &str = "The provider is still trying to deliver the message.";
const COMPLAINT_DETAIL: &str = "The recipient marked the message as spam.";

/// What became of one event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventOutcome {
    /// A recipient changed, or its address was suppressed.
    Applied,
    /// No recipient of the route matches, the event is older than a retry, or it changes nothing.
    Ignored,
    /// A bounce of a recipient still being sent: the provider should send it again later.
    Busy,
}

const RECIPIENT_COLUMNS: &str =
    "r.id, r.outbound_id, r.workspace_id, r.address, r.status, r.retried_at";

struct Found {
    id: Id,
    outbound_id: Id,
    workspace_id: Id,
    address: String,
    status: String,
    retried_at: Option<i64>,
}

fn found(row: &AnyRow) -> Result<Found, MailError> {
    Ok(Found {
        id: parse_id(&row.try_get::<String, _>("id")?)?,
        outbound_id: parse_id(&row.try_get::<String, _>("outbound_id")?)?,
        workspace_id: parse_id(&row.try_get::<String, _>("workspace_id")?)?,
        address: row.try_get("address")?,
        status: row.try_get("status")?,
        retried_at: row.try_get("retried_at")?,
    })
}

/// The first row whose address is the event's (any ASCII case).
fn pick(rows: &[AnyRow], address: &str) -> Result<Option<Found>, MailError> {
    for row in rows {
        let found = found(row)?;
        if found.address.eq_ignore_ascii_case(address) {
            return Ok(Some(found));
        }
    }
    Ok(None)
}

/// The recipient of the route that the event is about (see the module docs).
async fn match_recipient(
    transaction: &mut Transaction<'_, Any>,
    route_id: Id,
    event: &ParsedEvent,
) -> Result<Option<Found>, MailError> {
    let route = route_id.to_string();
    let address = event.recipient.trim();
    if let Some(id) = &event.provider_message_id {
        // stored as the provider answered: Mailgun's has `<>`, its events do not
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {RECIPIENT_COLUMNS} FROM mail_outbound_recipients r \
             JOIN mail_outbound o ON o.id = r.outbound_id \
             WHERE r.provider_message_id IN ($1, $2) AND o.route_id = $3"
        )))
        .bind(id)
        .bind(format!("<{id}>"))
        .bind(&route)
        .fetch_all(&mut **transaction)
        .await?;
        if let Some(found) = pick(&rows, address)? {
            return Ok(Some(found));
        }
    }
    if let Some(correlation) = &event.correlation {
        let rows = match correlation.split_once(':') {
            Some((outbound, recipient)) => {
                let (Ok(outbound), Ok(recipient)) = (
                    outbound.trim().parse::<Id>(),
                    recipient.trim().parse::<Id>(),
                ) else {
                    return Ok(None);
                };
                sqlx::query(sqlx::AssertSqlSafe(format!(
                    "SELECT {RECIPIENT_COLUMNS} FROM mail_outbound_recipients r \
                     JOIN mail_outbound o ON o.id = r.outbound_id \
                     WHERE r.id = $1 AND r.outbound_id = $2 AND o.route_id = $3"
                )))
                .bind(recipient.to_string())
                .bind(outbound.to_string())
                .bind(&route)
                .fetch_all(&mut **transaction)
                .await?
            }
            None => {
                let Ok(id) = correlation.trim().parse::<Id>() else {
                    return Ok(None);
                };
                // a recipient's ID (one envelope each), or the message's (one call for all)
                sqlx::query(sqlx::AssertSqlSafe(format!(
                    "SELECT {RECIPIENT_COLUMNS} FROM mail_outbound_recipients r \
                     JOIN mail_outbound o ON o.id = r.outbound_id \
                     WHERE (r.id = $1 OR r.outbound_id = $1) AND o.route_id = $2"
                )))
                .bind(id.to_string())
                .bind(&route)
                .fetch_all(&mut **transaction)
                .await?
            }
        };
        if let Some(found) = pick(&rows, address)? {
            return Ok(Some(found));
        }
    }
    if let Some(message_id) = &event.message_id {
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {RECIPIENT_COLUMNS} FROM mail_messages m \
             JOIN mail_outbound o ON o.message_id = m.id \
             JOIN mail_outbound_recipients r ON r.outbound_id = o.id \
             WHERE m.message_id_header = $1 AND o.route_id = $2"
        )))
        .bind(message_id)
        .bind(&route)
        .fetch_all(&mut **transaction)
        .await?;
        if let Some(found) = pick(&rows, address)? {
            return Ok(Some(found));
        }
    }
    Ok(None)
}

impl MailRepository {
    /// Applies one verified event of route `route_id` (see the module docs), in one writer
    /// transaction; the thread's readers get `thread.updated` when a recipient changed.
    pub async fn apply_provider_event(
        &self,
        route_id: Id,
        event: &ParsedEvent,
        now: TimestampMillis,
    ) -> Result<EventOutcome, MailError> {
        let mut transaction = self.database.immediate_transaction().await?;
        let Some(recipient) = match_recipient(&mut transaction, route_id, event).await? else {
            return Ok(EventOutcome::Ignored);
        };
        // after a retry, the same IDs name the new send too: only a later event is about it
        let retried = recipient.retried_at;
        if let Some(retried) = retried
            && event.at_ms.is_none_or(|at| at < retried)
        {
            return Ok(EventOutcome::Ignored);
        }
        // `retried_at` is checked again in each update, against a retry in between
        let at = event.at_ms.unwrap_or(now.as_millis());
        let mut changed_status = false;
        let outcome = match &event.event {
            ProviderEvent::Bounced { hard, diagnostic } => {
                if recipient.status == "sending" {
                    return Ok(EventOutcome::Busy);
                }
                let kind = if *hard {
                    BounceKind::Hard
                } else {
                    BounceKind::GaveUp
                };
                let response = plain_line(diagnostic.as_deref().unwrap_or(if *hard {
                    HARD_DEFAULT
                } else {
                    SOFT_DEFAULT
                }));
                let changed = sqlx::query(
                    "UPDATE mail_outbound_recipients SET status = 'bounced', bounce_kind = $1, \
                     last_response = $2, next_attempt_at = NULL, updated_at = $3 \
                     WHERE id = $4 AND status IN ('delivered', 'deferred') \
                     AND (retried_at IS NULL OR retried_at <= $5)",
                )
                .bind(kind.as_str())
                .bind(&response)
                .bind(now.as_millis())
                .bind(recipient.id.to_string())
                .bind(at)
                .execute(&mut *transaction)
                .await?
                .rows_affected();
                if changed == 0 {
                    EventOutcome::Ignored
                } else {
                    let (reply, enhanced) =
                        diagnostic.as_deref().map(bounce_codes).unwrap_or_default();
                    // a provider's permanent failure without a code is its own verdict
                    let codeless = reply.is_none() && enhanced.is_none();
                    if *hard && (codeless || suppresses(kind, enhanced.as_deref(), reply)) {
                        Self::add_suppression(
                            &mut transaction,
                            recipient.workspace_id,
                            &recipient.address,
                            SuppressionReason::HardBounce,
                            Some(&response),
                            Some(recipient.id),
                            now,
                        )
                        .await?;
                    }
                    bounced_thread_to_inbox(&mut transaction, recipient.outbound_id, now).await?;
                    changed_status = true;
                    EventOutcome::Applied
                }
            }
            ProviderEvent::Complained => {
                Self::add_suppression(
                    &mut transaction,
                    recipient.workspace_id,
                    &recipient.address,
                    SuppressionReason::Complaint,
                    Some(COMPLAINT_DETAIL),
                    Some(recipient.id),
                    now,
                )
                .await?;
                EventOutcome::Applied
            }
            ProviderEvent::Deferred { diagnostic } => {
                let response = plain_line(diagnostic.as_deref().unwrap_or(DEFERRED_DEFAULT));
                let changed = sqlx::query(
                    "UPDATE mail_outbound_recipients SET status = 'deferred', \
                     next_attempt_at = NULL, last_response = $1, updated_at = $2 \
                     WHERE id = $3 AND status = 'delivered' \
                     AND (retried_at IS NULL OR retried_at <= $4)",
                )
                .bind(&response)
                .bind(now.as_millis())
                .bind(recipient.id.to_string())
                .bind(at)
                .execute(&mut *transaction)
                .await?
                .rows_affected();
                changed_status = changed > 0;
                if changed_status {
                    EventOutcome::Applied
                } else {
                    EventOutcome::Ignored
                }
            }
            ProviderEvent::Delivered => {
                let changed = sqlx::query(
                    "UPDATE mail_outbound_recipients SET status = 'delivered', \
                     next_attempt_at = NULL, last_response = NULL, updated_at = $1 \
                     WHERE id = $2 AND status = 'deferred' \
                     AND (retried_at IS NULL OR retried_at <= $3)",
                )
                .bind(now.as_millis())
                .bind(recipient.id.to_string())
                .bind(at)
                .execute(&mut *transaction)
                .await?
                .rows_affected();
                changed_status = changed > 0;
                if changed_status {
                    EventOutcome::Applied
                } else {
                    EventOutcome::Ignored
                }
            }
        };
        if changed_status {
            update_summary(&mut transaction, recipient.outbound_id, now).await?;
        }
        transaction.commit().await?;
        if changed_status {
            self.announce_outbounds(&[recipient.outbound_id]).await;
        }
        Ok(outcome)
    }
}
