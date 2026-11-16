//! The `mail.send` job (spec §5.3). Its first run for a held message releases it
//! ([`MailRepository::release`]: build, sign, seal, file into Sent and the thread, queue the
//! recipients) and tells the mailbox's readers with `thread.updated`. A run for a message that is
//! not due or not held any more (undone, an old job of a message sent again) releases nothing.
//! Every run then delivers what is due of a released message ([`deliver_due`]) and queues the
//! next run for when a recipient is due again.
//!
//! A lasting release failure keeps the message held with its reason (for example
//! `dkim_inactive`: never sent unsigned, or `release_error` for an error without a code) and
//! fails the job; a database or blob store error holds it with `release_failed` and is retried.

use std::sync::Arc;

use orbit_platform::{Id, JobError, TimestampMillis};
use serde_json::{Value, json};

use super::DeliveryContext;
use super::deliver::{NextRun, deliver_due};
use crate::live::{LiveHub, NewEvent, Recipients};
use crate::mail_intake::MailIntake;
use crate::repositories::mail::{MAIL_TOPIC, MailError, MailRepository, Released};

/// What the `mail.send` job works with.
#[derive(Clone)]
pub struct SendJobState {
    pub mail: Arc<MailRepository>,
    /// Public addresses only (`allow_private = false`): direct sending and workspace routes.
    /// Its `hostname` is the `HELO` recorded on a sent message's receipt.
    pub delivery: DeliveryContext,
    /// `allow_private = true`: instance routes other than direct (a relay on the instance's own
    /// network, such as Mailpit in development).
    pub instance_delivery: DeliveryContext,
    /// Local delivery (D11).
    pub intake: MailIntake,
    pub live: LiveHub,
}

/// The outbound id of a `mail.send` payload (`{"outbound_id": "<id>"}`).
pub fn send_job_outbound(payload: &Value) -> Result<Id, JobError> {
    payload
        .get("outbound_id")
        .and_then(Value::as_str)
        .and_then(|id| id.parse::<Id>().ok())
        .ok_or_else(|| JobError::Permanent("mail.send needs an outbound_id".to_owned()))
}

/// One run of `mail.send` for `outbound_id` at `now` (the database's time).
pub async fn run_send_job(
    state: &SendJobState,
    outbound_id: Id,
    now: TimestampMillis,
) -> Result<(), JobError> {
    let error = match state
        .mail
        .release(outbound_id, state.delivery.hostname.as_deref(), now)
        .await
    {
        Ok(released) => {
            if let Some(released) = released {
                announce(&state.live, &released).await;
            }
            if let NextRun::At(at) = deliver_due(state, outbound_id, now).await? {
                // Never sooner than a second from now, so a run cannot spin.
                let at = TimestampMillis::from_millis(at.as_millis().max(now.as_millis() + 1000));
                state
                    .mail
                    .queue_send_run(outbound_id, at)
                    .await
                    .map_err(|error| JobError::Retryable(error.to_string()))?;
            }
            return Ok(());
        }
        Err(error) => error,
    };
    let (reason, retry) = release_failure(&error);
    if let Err(record) = state.mail.hold_release(outbound_id, reason, now).await {
        tracing::warn!(error = %record, outbound_id = %outbound_id, "recording a mail release failure");
    }
    let text = format!("{reason}: {error}");
    tracing::warn!(error = %text, outbound_id = %outbound_id, "mail release failed");
    if retry {
        Err(JobError::Retryable(text))
    } else {
        Err(JobError::Permanent(text))
    }
}

/// The hold reason of a failed release and whether the job retries it. Only `release_failed`
/// (a passing database or blob store error) is retried and swept again; any other error is
/// permanent: `release_error` when it has no code of its own, which the sweep leaves alone.
#[must_use]
pub fn release_failure(error: &MailError) -> (&'static str, bool) {
    match error {
        MailError::Unprocessable(code) => (*code, false),
        MailError::QuotaExceeded => ("storage_quota", false),
        MailError::Crypto => ("decrypt_failed", false),
        MailError::Unavailable(_) | MailError::Upload(_) => ("release_failed", true),
        _ => ("release_error", false),
    }
}

/// `thread.updated` to the mailbox's readers; the message is stored either way.
async fn announce(live: &LiveHub, released: &Released) {
    if released.users.is_empty() {
        return;
    }
    let events = [NewEvent {
        recipients: Recipients::Users(released.users.clone()),
        topic: MAIL_TOPIC,
        event: json!({
            "type": "thread.updated",
            "mailbox_id": released.mailbox_id.to_string(),
            "thread_id": released.thread_id.to_string(),
        }),
    }];
    if let Err(error) = live.publish(released.workspace_id, &events).await {
        tracing::warn!(error = %error, "mail events for the live socket");
    }
}
