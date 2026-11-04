//! `billing.stripe_customer_email`: sets the email of a workspace's Stripe customer to its owner's new address, so
//! receipts follow the owner. It is queued in the transaction that confirms the email change. A job whose address is
//! no longer the owner's skips, so jobs that run late or out of order converge on the current address.

use std::time::Duration;

use orbit_platform::{Id, Job, JobError, JobKind, JobStore, JobStoreError, TimestampMillis};
use serde_json::{Value, json};
use sqlx::{Any, Transaction};

use super::Billing;
use super::stripe::StripeError;

pub const CUSTOMER_EMAIL_JOB: &str = "billing.stripe_customer_email";

pub(super) fn job_kind() -> JobKind {
    JobKind::new(CUSTOMER_EMAIL_JOB).with_retry_policy(
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

pub(crate) async fn enqueue_in(
    transaction: &mut Transaction<'_, Any>,
    customer_id: &str,
    user_id: Id,
    email: &str,
    now: TimestampMillis,
) -> Result<(), JobStoreError> {
    let job = Job::new(
        job_kind(),
        json!({
            "customer_id": customer_id,
            "user_id": user_id.to_string(),
            "email": email,
        }),
        now,
    );
    JobStore::enqueue_in(transaction, &job).await.map(|_| ())
}

pub(super) async fn update(billing: &Billing, payload: &Value) -> Result<(), JobError> {
    let field = |name: &str| {
        payload
            .get(name)
            .and_then(Value::as_str)
            .ok_or_else(|| JobError::Permanent(format!("{CUSTOMER_EMAIL_JOB} needs a {name}")))
    };
    let (customer_id, user_id, email) = (field("customer_id")?, field("user_id")?, field("email")?);
    let current: Option<String> =
        sqlx::query_scalar("SELECT email FROM users WHERE id = $1 AND deleted_at IS NULL")
            .bind(user_id)
            .fetch_optional(billing.database().pool())
            .await
            .map_err(|error| JobError::Retryable(error.to_string()))?;
    if current.as_deref() != Some(email) {
        return Ok(());
    }
    match billing
        .stripe()
        .update_customer_email(customer_id, email)
        .await
    {
        // No such customer: nothing to update.
        Ok(()) | Err(StripeError::Api { status: 404, .. }) => Ok(()),
        Err(error) => Err(JobError::Retryable(error.to_string())),
    }
}
