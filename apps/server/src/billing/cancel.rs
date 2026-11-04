//! `billing.stripe_cancel`: cancels the subscription of a deleted workspace at Stripe. It is
//! queued in the transaction that moves the workspace to the trash, carries the subscription id
//! (the workspace may be purged before it runs), and then queues `billing.stripe_sync`.

use std::time::Duration;

use orbit_platform::{Id, Job, JobError, JobKind, JobStore, JobStoreError, TimestampMillis};
use serde_json::{Value, json};
use sqlx::{Any, Transaction};

use super::stripe::StripeError;
use super::{Billing, sync};

pub const CANCEL_JOB: &str = "billing.stripe_cancel";

pub(super) fn job_kind() -> JobKind {
    JobKind::new(CANCEL_JOB).with_retry_policy(
        10,
        vec![
            Duration::from_secs(10),
            Duration::from_secs(60),
            Duration::from_secs(5 * 60),
            Duration::from_secs(15 * 60),
            Duration::from_secs(60 * 60),
            Duration::from_secs(3 * 60 * 60),
            Duration::from_secs(6 * 60 * 60),
            Duration::from_secs(12 * 60 * 60),
            Duration::from_secs(24 * 60 * 60),
        ],
    )
}

pub(crate) async fn enqueue_in(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
    subscription_id: &str,
    now: TimestampMillis,
) -> Result<(), JobStoreError> {
    let job = Job::new(
        job_kind(),
        json!({
            "subscription_id": subscription_id,
            "workspace_id": workspace_id.to_string(),
        }),
        now,
    )
    .with_workspace(workspace_id.to_string());
    JobStore::enqueue_in(transaction, &job).await.map(|_| ())
}

pub(super) async fn cancel(billing: &Billing, payload: &Value) -> Result<(), JobError> {
    let subscription_id = payload
        .get("subscription_id")
        .and_then(Value::as_str)
        .ok_or_else(|| JobError::Permanent(format!("{CANCEL_JOB} needs a subscription_id")))?;
    let workspace_id = payload
        .get("workspace_id")
        .and_then(Value::as_str)
        .and_then(|id| id.parse::<Id>().ok());
    let stripe = billing.stripe();
    match stripe.cancel_subscription(subscription_id).await {
        Ok(_) => {}
        // No such subscription: nothing left to cancel.
        Err(StripeError::Api { status: 404, .. }) => return Ok(()),
        // Stripe refuses to cancel a subscription that already ended; a retry after a cancel
        // that succeeded but whose answer was lost lands here too.
        Err(error @ StripeError::Api { .. }) => match stripe.subscription(subscription_id).await {
            Ok(subscription)
                if matches!(
                    subscription.status.as_str(),
                    "canceled" | "incomplete_expired"
                ) => {}
            Err(StripeError::Api { status: 404, .. }) => return Ok(()),
            _ => return Err(JobError::Retryable(error.to_string())),
        },
        Err(error) => return Err(JobError::Retryable(error.to_string())),
    }
    sync::enqueue(billing.database(), subscription_id, workspace_id)
        .await
        .map_err(|error| JobError::Retryable(format!("queueing the sync failed: {error}")))
}
