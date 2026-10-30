//! `billing.stripe_sync`: re-reads one subscription from Stripe and stores it. Webhooks only say
//! which subscription changed, so events that arrive late, twice or out of order all converge on
//! Stripe's current state.

use std::time::Duration;

use orbit_platform::{
    Database, Id, Job, JobError, JobKind, JobKindRegistrationError, JobQueue, JobStore,
    JobStoreError, TimestampMillis, Worker,
};
use serde_json::{Value, json};

use super::Billing;
use crate::repositories::billing::{self as repository, SubscriptionSnapshot};

pub const SYNC_JOB: &str = "billing.stripe_sync";

fn job_kind() -> JobKind {
    JobKind::new(SYNC_JOB).with_retry_policy(
        8,
        vec![
            Duration::from_secs(10),
            Duration::from_secs(60),
            Duration::from_secs(5 * 60),
            Duration::from_secs(15 * 60),
            Duration::from_secs(60 * 60),
            Duration::from_secs(3 * 60 * 60),
            Duration::from_secs(6 * 60 * 60),
        ],
    )
}

/// Queues a sync of `subscription_id`. `workspace_id` is the checkout's `client_reference_id`,
/// used when the subscription itself carries no `workspace_id` metadata.
pub(super) async fn enqueue(
    database: &Database,
    subscription_id: &str,
    workspace_id: Option<Id>,
) -> Result<(), JobStoreError> {
    let mut job = Job::new(
        job_kind(),
        json!({
            "subscription_id": subscription_id,
            "workspace_id": workspace_id.map(|id| id.to_string()),
        }),
        TimestampMillis::now(),
    );
    if let Some(workspace_id) = workspace_id {
        job = job.with_workspace(workspace_id.to_string());
    }
    JobQueue::new(JobStore::new(database.clone()))
        .enqueue(job)
        .await
        .map(|_| ())
}

pub fn register_jobs(worker: Worker, billing: Billing) -> Result<Worker, JobKindRegistrationError> {
    let canceling = billing.clone();
    let updating = billing.clone();
    worker
        .with_handler(job_kind(), move |context| {
            let billing = billing.clone();
            async move { sync(&billing, &context.job.payload).await }
        })?
        .with_handler(super::cancel::job_kind(), move |context| {
            let billing = canceling.clone();
            async move { super::cancel::cancel(&billing, &context.job.payload).await }
        })?
        .with_handler(super::customer_email::job_kind(), move |context| {
            let billing = updating.clone();
            async move { super::customer_email::update(&billing, &context.job.payload).await }
        })
}

async fn sync(billing: &Billing, payload: &Value) -> Result<(), JobError> {
    let subscription_id = payload
        .get("subscription_id")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            JobError::Permanent("billing.stripe_sync needs a subscription_id".to_owned())
        })?;
    let subscription = billing
        .stripe()
        .subscription(subscription_id)
        .await
        .map_err(|error| JobError::Retryable(error.to_string()))?;
    let workspace_id = subscription
        .metadata
        .get("workspace_id")
        .map(String::as_str)
        .or_else(|| payload.get("workspace_id").and_then(Value::as_str))
        .and_then(|id| id.parse::<Id>().ok())
        .ok_or_else(|| {
            JobError::Permanent(format!(
                "subscription {subscription_id} names no Orbit workspace"
            ))
        })?;
    let plan = subscription
        .price_ids()
        .find_map(|price| billing.config().plan_for_price(price))
        .map(|plan| plan.key.clone());
    let snapshot = SubscriptionSnapshot {
        customer_id: subscription.customer.clone(),
        subscription_id: subscription.id.clone(),
        plan,
        status: subscription.status.clone(),
        cancel_at_period_end: subscription.cancels(),
        current_period_end: subscription.period_end().map(|seconds| seconds * 1000),
    };
    let stored = repository::apply_subscription(
        billing.database(),
        workspace_id,
        &snapshot,
        TimestampMillis::now(),
    )
    .await
    .map_err(|error| JobError::Retryable(format!("saving the subscription failed: {error}")))?;
    if !stored {
        tracing::info!(%workspace_id, subscription_id, "subscription of a deleted workspace ignored");
    }
    Ok(())
}
