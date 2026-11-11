//! `workspace_subscriptions` and `stripe_events` (Orbit Cloud billing, see `crate::billing`).

use orbit_platform::{Database, Id, Row, TimestampMillis, TrialConfig};
use serde_json::json;
use sqlx::{Any, Transaction};

use crate::audit::{self, AuditOutcome};
use crate::billing::{SubscriptionStatus, TrialLimit, WorkspaceAccess, access, grants_access};

/// A workspace's billing row as stored, with its trial start (the owner's, see [`crate::billing::access`]).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WorkspaceSubscription {
    /// The root user's first workspace, which needs no subscription.
    pub exempt: bool,
    /// When the trial started, copied from the creator's account (Unix milliseconds).
    pub trial_started_at: i64,
    pub stripe_customer_id: Option<String>,
    pub plan: Option<String>,
    /// `None` without a subscription, or for a status this server does not know.
    pub status: Option<SubscriptionStatus>,
    pub cancel_at_period_end: bool,
    /// Unix milliseconds.
    pub current_period_end: Option<i64>,
}

impl WorkspaceSubscription {
    #[must_use]
    pub fn access(&self, now: TimestampMillis, trial: &TrialConfig) -> WorkspaceAccess {
        access(
            self.exempt,
            self.status,
            self.trial_started_at,
            now.as_millis(),
            trial.millis(),
        )
    }

    /// When the trial ends (or ended), Unix milliseconds.
    #[must_use]
    pub fn trial_ends_at(&self, trial: &TrialConfig) -> i64 {
        self.trial_started_at.saturating_add(trial.millis())
    }
}

/// A subscription as Stripe reports it, mapped to Orbit's names.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubscriptionSnapshot {
    pub customer_id: String,
    pub subscription_id: String,
    pub plan: Option<String>,
    /// Stripe's status string, stored as is (an unknown one reads as read-only).
    pub status: String,
    pub cancel_at_period_end: bool,
    pub current_period_end: Option<i64>,
}

/// Set on a database whose server runs Orbit Cloud billing ([`crate::billing::Billing::new`]),
/// with its trial, so writers outside the guard need no [`crate::billing::Billing`].
struct BillingOn(TrialConfig);

/// Turns the read-only rule on for `database`; [`write_access`] answers `Full` until then.
pub(crate) fn enable(database: &Database, trial: TrialConfig) {
    database.extension(|| BillingOn(trial));
}

/// Whether this server runs Orbit Cloud billing.
pub(crate) fn enabled(database: &Database) -> bool {
    database.existing_extension::<BillingOn>().is_some()
}

/// Inside the transaction that moves a workspace to the trash: with billing on, queues the
/// cancellation of its Stripe subscription unless that has already ended.
pub(crate) async fn cancel_on_delete(
    database: &Database,
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
    now: TimestampMillis,
) -> Result<(), sqlx::Error> {
    if !enabled(database) {
        return Ok(());
    }
    let subscription_id: Option<String> = sqlx::query_scalar(
        "SELECT stripe_subscription_id FROM workspace_subscriptions WHERE workspace_id = $1 \
         AND stripe_subscription_id IS NOT NULL \
         AND (status IS NULL OR status NOT IN ('canceled', 'incomplete_expired'))",
    )
    .bind(workspace_id.to_string())
    .fetch_optional(&mut **transaction)
    .await?;
    if let Some(subscription_id) = subscription_id {
        crate::billing::enqueue_cancel_in(transaction, workspace_id, &subscription_id, now)
            .await
            .map_err(|error| sqlx::Error::Protocol(error.to_string()))?;
    }
    Ok(())
}

/// Whether something may write to the workspace: `Full` on a self-hosted server (no query),
/// else the workspace's subscription decides. Every writer outside the HTTP guard (co-editing
/// sockets, webhooks, integration jobs) asks this.
pub async fn write_access(
    database: &Database,
    workspace_id: Id,
) -> Result<WorkspaceAccess, sqlx::Error> {
    let Some(billing) = database.existing_extension::<BillingOn>() else {
        return Ok(WorkspaceAccess::Full);
    };
    workspace_access(database, workspace_id, &billing.0).await
}

/// Workspaces by trial state, for the Admin area. Deleted workspaces are not counted.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize, utoipa::ToSchema)]
pub struct TrialCounts {
    /// On a trial now: writable within the trial limits.
    pub ongoing: i64,
    /// The trial ended without a subscription, so the workspace is read-only.
    pub expired: i64,
}

/// The [`TrialCounts`] of the instance; `None` on a self-hosted server, which has no trials.
pub async fn trial_counts(database: &Database) -> Result<Option<TrialCounts>, sqlx::Error> {
    let Some(billing) = database.existing_extension::<BillingOn>() else {
        return Ok(None);
    };
    let rows = sqlx::query(
        "SELECT workspaces.trial_started_at, workspaces.billing_exempt, s.status FROM workspaces \
         LEFT JOIN workspace_subscriptions s ON s.workspace_id = workspaces.id \
         WHERE workspaces.deleted_at IS NULL",
    )
    .fetch_all(database.pool())
    .await?;
    let now = TimestampMillis::now().as_millis();
    let mut counts = TrialCounts::default();
    for row in rows {
        let status = row
            .get::<Option<String>, _>("status")
            .as_deref()
            .and_then(SubscriptionStatus::parse);
        match access(
            row.get("billing_exempt"),
            status,
            row.get("trial_started_at"),
            now,
            billing.0.millis(),
        ) {
            WorkspaceAccess::Trial => counts.ongoing += 1,
            WorkspaceAccess::ReadOnly => counts.expired += 1,
            WorkspaceAccess::Full => {}
        }
    }
    Ok(Some(counts))
}

/// The access of one workspace; one primary-key lookup, run by the guard on each write.
pub async fn workspace_access(
    database: &Database,
    workspace_id: Id,
    trial: &TrialConfig,
) -> Result<WorkspaceAccess, sqlx::Error> {
    Ok(subscription(database, workspace_id)
        .await?
        .access(TimestampMillis::now(), trial))
}

/// The workspace's subscription; the default (no trial left) when the workspace does not exist.
pub async fn subscription(
    database: &Database,
    workspace_id: Id,
) -> Result<WorkspaceSubscription, sqlx::Error> {
    subscription_in(database.pool(), workspace_id).await
}

async fn subscription_in<'e>(
    executor: impl sqlx::Executor<'e, Database = Any>,
    workspace_id: Id,
) -> Result<WorkspaceSubscription, sqlx::Error> {
    let row = sqlx::query(
        "SELECT workspaces.trial_started_at, workspaces.billing_exempt, s.stripe_customer_id, s.plan, s.status, \
         s.cancel_at_period_end, s.current_period_end FROM workspaces \
         LEFT JOIN workspace_subscriptions s ON s.workspace_id = workspaces.id \
         WHERE workspaces.id = $1",
    )
    .bind(workspace_id.to_string())
    .fetch_optional(executor)
    .await?;
    Ok(row.map_or_else(WorkspaceSubscription::default, |row| {
        WorkspaceSubscription {
            exempt: row.get("billing_exempt"),
            trial_started_at: row.get("trial_started_at"),
            stripe_customer_id: row.get("stripe_customer_id"),
            plan: row.get("plan"),
            status: row
                .get::<Option<String>, _>("status")
                .as_deref()
                .and_then(SubscriptionStatus::parse),
            cancel_at_period_end: row
                .get::<Option<bool>, _>("cancel_at_period_end")
                .unwrap_or(false),
            current_period_end: row.get("current_period_end"),
        }
    }))
}

/// How much of `limit` the workspace uses.
pub async fn trial_usage(
    database: &Database,
    workspace_id: Id,
    limit: TrialLimit,
    now: TimestampMillis,
) -> Result<i64, sqlx::Error> {
    trial_usage_in(database.pool(), workspace_id, limit, now).await
}

async fn trial_usage_in<'e>(
    executor: impl sqlx::Executor<'e, Database = Any>,
    workspace_id: Id,
    limit: TrialLimit,
    now: TimestampMillis,
) -> Result<i64, sqlx::Error> {
    let query = match limit {
        TrialLimit::Tasks => sqlx::query_scalar(
            "SELECT CAST(COUNT(*) AS BIGINT) FROM tasks \
             WHERE workspace_id = $1 AND deleted_at IS NULL",
        )
        .bind(workspace_id.to_string()),
        TrialLimit::Pages => sqlx::query_scalar(
            "SELECT CAST(COUNT(*) AS BIGINT) FROM pages \
             WHERE workspace_id = $1 AND deleted_at IS NULL",
        )
        .bind(workspace_id.to_string()),
        TrialLimit::Members => sqlx::query_scalar(
            "SELECT CAST((SELECT COUNT(*) FROM memberships WHERE workspace_id = $1 AND role <> 'guest') \
             + (SELECT COUNT(*) FROM workspace_invitations WHERE workspace_id = $1 AND role <> 'guest' \
             AND accepted_at IS NULL AND revoked_at IS NULL AND replaced_at IS NULL \
             AND expires_at > $2) AS BIGINT)",
        )
        .bind(workspace_id.to_string())
        .bind(now.as_millis()),
    };
    query.fetch_one(executor).await
}

/// A trial workspace refused new records: it would hold more than the trial allows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TrialLimitReached {
    pub limit: TrialLimit,
    /// The configured limit.
    pub allowed: i64,
}

impl TrialLimitReached {
    /// The `detail` of the 402 `trial_limit_reached` problem.
    #[must_use]
    pub fn detail(self) -> String {
        format!(
            "The trial allows {} {}. Choose a plan to add more.",
            self.allowed,
            self.limit.noun()
        )
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TrialCapacityError {
    #[error("trial limit reached")]
    Reached(TrialLimitReached),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// Inside the transaction that adds (creates, restores, copies or imports) `added` records of
/// `limit`: refuses when a trial workspace would then hold more than the trial allows. Free
/// without billing (self-hosted) and for workspaces with full access. Writers serialize on
/// `immediate_transaction`, so concurrent creates cannot pass the limit together.
pub(crate) async fn ensure_trial_capacity(
    database: &Database,
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
    limit: TrialLimit,
    added: i64,
) -> Result<(), TrialCapacityError> {
    let Some(billing) = database.existing_extension::<BillingOn>() else {
        return Ok(());
    };
    if added <= 0 {
        return Ok(());
    }
    let now = TimestampMillis::now();
    let subscription = subscription_in(&mut **transaction, workspace_id).await?;
    if subscription.access(now, &billing.0) != WorkspaceAccess::Trial {
        return Ok(());
    }
    let allowed = limit.limit(&billing.0);
    let used = trial_usage_in(&mut **transaction, workspace_id, limit, now).await?;
    if used.saturating_add(added) > allowed {
        return Err(TrialCapacityError::Reached(TrialLimitReached {
            limit,
            allowed,
        }));
    }
    Ok(())
}

/// Saves the workspace's Stripe customer unless it already has one; returns the one it has.
pub async fn save_customer(
    database: &Database,
    workspace_id: Id,
    customer_id: &str,
    now: TimestampMillis,
) -> Result<String, sqlx::Error> {
    sqlx::query(
        "INSERT INTO workspace_subscriptions (workspace_id, stripe_customer_id, updated_at) \
         VALUES ($1, $2, $3) ON CONFLICT (workspace_id) DO NOTHING",
    )
    .bind(workspace_id.to_string())
    .bind(customer_id)
    .bind(now.as_millis())
    .execute(database.pool())
    .await?;
    sqlx::query_scalar(
        "SELECT stripe_customer_id FROM workspace_subscriptions WHERE workspace_id = $1",
    )
    .bind(workspace_id.to_string())
    .fetch_one(database.pool())
    .await
}

/// Records a billing action of a member (checkout started, portal opened) in the audit log.
pub async fn record_audit(
    database: &Database,
    workspace_id: Id,
    actor_id: Id,
    action: &str,
    request_id: &str,
    metadata: serde_json::Value,
    now: TimestampMillis,
) -> Result<(), sqlx::Error> {
    let mut transaction = database.immediate_transaction().await?;
    audit::record(
        &mut transaction,
        workspace_id,
        Some(actor_id),
        action,
        AuditOutcome::Success,
        "workspace",
        Some(workspace_id),
        request_id,
        metadata,
        now,
    )
    .await?;
    transaction.commit().await
}

/// Records a webhook event; `false` when it was already recorded (a redelivery).
pub async fn record_event(
    database: &Database,
    event_id: &str,
    now: TimestampMillis,
) -> Result<bool, sqlx::Error> {
    const KEEP_MILLIS: i64 = 30 * 24 * 60 * 60 * 1000;
    let inserted = sqlx::query(
        "INSERT INTO stripe_events (event_id, received_at) VALUES ($1, $2) \
         ON CONFLICT (event_id) DO NOTHING",
    )
    .bind(event_id)
    .bind(now.as_millis())
    .execute(database.pool())
    .await?
    .rows_affected()
        == 1;
    sqlx::query("DELETE FROM stripe_events WHERE received_at < $1")
        .bind(now.as_millis() - KEEP_MILLIS)
        .execute(database.pool())
        .await?;
    Ok(inserted)
}

/// Forgets an event, so Stripe's next delivery of it is processed again.
pub async fn forget_event(database: &Database, event_id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM stripe_events WHERE event_id = $1")
        .bind(event_id)
        .execute(database.pool())
        .await
        .map(|_| ())
}

/// Stores the subscription of a workspace and audits a status change. A workspace that has a
/// different subscription keeps it unless this one grants access: a late event about an old,
/// cancelled subscription must not lock a workspace that subscribed again. Returns `false` when
/// the workspace no longer exists. A change of access re-checks the workspace's co-editing sockets.
pub async fn apply_subscription(
    database: &Database,
    workspace_id: Id,
    snapshot: &SubscriptionSnapshot,
    now: TimestampMillis,
) -> Result<bool, sqlx::Error> {
    let grants = grants_access(SubscriptionStatus::parse(&snapshot.status));
    let mut transaction = database.immediate_transaction().await?;
    let exists: Option<String> = sqlx::query_scalar("SELECT id FROM workspaces WHERE id = $1")
        .bind(workspace_id.to_string())
        .fetch_optional(&mut *transaction)
        .await?;
    if exists.is_none() {
        transaction.rollback().await?;
        return Ok(false);
    }
    let previous: Option<Option<String>> =
        sqlx::query_scalar("SELECT status FROM workspace_subscriptions WHERE workspace_id = $1")
            .bind(workspace_id.to_string())
            .fetch_optional(&mut *transaction)
            .await?;
    let previous = previous.flatten();
    let written = sqlx::query(
        "INSERT INTO workspace_subscriptions (workspace_id, stripe_customer_id, \
         stripe_subscription_id, plan, status, cancel_at_period_end, current_period_end, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
         ON CONFLICT (workspace_id) DO UPDATE SET stripe_customer_id = excluded.stripe_customer_id, \
         stripe_subscription_id = excluded.stripe_subscription_id, plan = excluded.plan, \
         status = excluded.status, cancel_at_period_end = excluded.cancel_at_period_end, \
         current_period_end = excluded.current_period_end, updated_at = excluded.updated_at \
         WHERE workspace_subscriptions.stripe_subscription_id IS NULL \
         OR workspace_subscriptions.stripe_subscription_id = excluded.stripe_subscription_id \
         OR $9 = 1",
    )
    .bind(workspace_id.to_string())
    .bind(&snapshot.customer_id)
    .bind(&snapshot.subscription_id)
    .bind(snapshot.plan.as_deref())
    .bind(&snapshot.status)
    .bind(i64::from(snapshot.cancel_at_period_end))
    .bind(snapshot.current_period_end)
    .bind(now.as_millis())
    .bind(i64::from(grants))
    .execute(&mut *transaction)
    .await?
    .rows_affected()
        == 1;
    if written && previous.as_deref() != Some(snapshot.status.as_str()) {
        audit::record(
            &mut transaction,
            workspace_id,
            None,
            "billing.subscription_status_changed",
            AuditOutcome::Success,
            "workspace",
            Some(workspace_id),
            "stripe-webhook",
            json!({
                "from": previous,
                "to": snapshot.status,
                "plan": snapshot.plan,
            }),
            now,
        )
        .await?;
    }
    transaction.commit().await?;
    let previously_granted = grants_access(previous.as_deref().and_then(SubscriptionStatus::parse));
    if written && grants != previously_granted {
        // Open co-editing sockets on every server check their access again: one that may edit a
        // workspace that just became read-only closes, and the client reconnects read-only.
        crate::collab::CollabHub::revalidate_database(database, Some(workspace_id));
    }
    Ok(true)
}
