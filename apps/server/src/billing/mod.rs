//! Orbit Cloud billing (`ORBIT_CLOUD=true`): one flat-price Stripe subscription per workspace.
//! A new workspace is on a trial ([`TrialConfig`]): writable, within the [`TrialLimit`]s. After
//! that, a workspace whose subscription does not grant access is read-only (see [`access`]): reads
//! work, writes answer 402 `workspace_read_only` ([`guard`]). On a self-hosted server none of this
//! is mounted.

mod cancel;
mod customer_email;
pub mod guard;
pub(crate) mod routes;
pub mod stripe;
mod sync;

use std::sync::Arc;

use futures_util::future::try_join_all;
use orbit_platform::{BillingConfig, Database, Plan, TrialConfig};
use serde::Serialize;
use tokio::sync::OnceCell;
use utoipa::ToSchema;

pub use cancel::CANCEL_JOB;
pub(crate) use cancel::enqueue_in as enqueue_cancel_in;
pub use customer_email::CUSTOMER_EMAIL_JOB;
pub(crate) use customer_email::enqueue_in as enqueue_customer_email_in;
pub use routes::router;
use routes::{PlanInterval, PlanView};
pub use sync::{SYNC_JOB, register_jobs};

/// A Stripe subscription status (`subscription.status`).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionStatus {
    Incomplete,
    IncompleteExpired,
    Trialing,
    Active,
    PastDue,
    Canceled,
    Unpaid,
    Paused,
}

impl SubscriptionStatus {
    pub const ALL: [Self; 8] = [
        Self::Incomplete,
        Self::IncompleteExpired,
        Self::Trialing,
        Self::Active,
        Self::PastDue,
        Self::Canceled,
        Self::Unpaid,
        Self::Paused,
    ];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Incomplete => "incomplete",
            Self::IncompleteExpired => "incomplete_expired",
            Self::Trialing => "trialing",
            Self::Active => "active",
            Self::PastDue => "past_due",
            Self::Canceled => "canceled",
            Self::Unpaid => "unpaid",
            Self::Paused => "paused",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|status| status.as_str() == value)
    }
}

/// What members of a workspace may do.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceAccess {
    Full,
    /// Writable within the [`TrialLimit`]s until the trial ends.
    Trial,
    ReadOnly,
}

/// Whether a subscription status grants full access. `past_due` keeps it while Stripe retries the
/// card; no subscription, or any other status, does not.
#[must_use]
pub const fn grants_access(status: Option<SubscriptionStatus>) -> bool {
    match status {
        Some(
            SubscriptionStatus::Active | SubscriptionStatus::Trialing | SubscriptionStatus::PastDue,
        ) => true,
        Some(
            SubscriptionStatus::Incomplete
            | SubscriptionStatus::IncompleteExpired
            | SubscriptionStatus::Canceled
            | SubscriptionStatus::Unpaid
            | SubscriptionStatus::Paused,
        )
        | None => false,
    }
}

/// The only rule that maps a workspace to access: the billing-exempt workspace (the root user's
/// first) and a granting subscription are full access, otherwise the workspace is on a trial until
/// `trial_millis` after its creation, then read-only.
#[must_use]
pub const fn access(
    exempt: bool,
    status: Option<SubscriptionStatus>,
    created_at_ms: i64,
    now_ms: i64,
    trial_millis: i64,
) -> WorkspaceAccess {
    if exempt || grants_access(status) {
        WorkspaceAccess::Full
    } else if now_ms < created_at_ms.saturating_add(trial_millis) {
        WorkspaceAccess::Trial
    } else {
        WorkspaceAccess::ReadOnly
    }
}

/// What a trial workspace may hold; creating one more answers 402 `trial_limit_reached`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrialLimit {
    /// Tasks not in the trash.
    Tasks,
    /// Pages not in the trash.
    Pages,
    /// Members and pending invitations.
    Members,
}

impl TrialLimit {
    #[must_use]
    pub const fn limit(self, trial: &TrialConfig) -> i64 {
        match self {
            Self::Tasks => trial.tasks,
            Self::Pages => trial.pages,
            Self::Members => trial.members,
        }
    }

    #[must_use]
    pub const fn noun(self) -> &'static str {
        match self {
            Self::Tasks => "tasks",
            Self::Pages => "pages",
            Self::Members => "members",
        }
    }
}

/// Billing for the routes, the guard and the sync job.
#[derive(Clone)]
pub struct Billing {
    inner: Arc<Inner>,
}

struct Inner {
    config: BillingConfig,
    database: Database,
    stripe: stripe::StripeClient,
    public_origin: String,
    /// Stripe's description of the configured prices. A price's amount and interval never change;
    /// a renamed product shows after a restart.
    plans: OnceCell<Vec<PlanView>>,
}

impl Billing {
    #[must_use]
    pub fn new(config: BillingConfig, database: Database, public_origin: &str) -> Self {
        let stripe = stripe::StripeClient::new(&config);
        crate::repositories::billing::enable(&database, config.trial);
        Self {
            inner: Arc::new(Inner {
                config,
                database,
                stripe,
                public_origin: public_origin.trim_end_matches('/').to_owned(),
                plans: OnceCell::new(),
            }),
        }
    }

    pub(crate) fn config(&self) -> &BillingConfig {
        &self.inner.config
    }

    pub(crate) fn database(&self) -> &Database {
        &self.inner.database
    }

    pub(crate) fn stripe(&self) -> &stripe::StripeClient {
        &self.inner.stripe
    }

    pub(crate) fn public_origin(&self) -> &str {
        &self.inner.public_origin
    }

    /// The configured plans as Stripe describes them, fetched once per process. While Stripe
    /// fails, each plan has only its key and a name made from it, and the next call asks again.
    pub(crate) async fn plans(&self) -> Vec<PlanView> {
        let plans = &self.inner.config.plans;
        let fetched = self
            .inner
            .plans
            .get_or_try_init(|| {
                try_join_all(plans.iter().map(async |plan| {
                    let price = self.inner.stripe.price(&plan.price_id).await?;
                    Ok::<_, stripe::StripeError>(PlanView {
                        key: plan.key.clone(),
                        product_id: Some(price.product.id),
                        name: price.product.name,
                        amount: price.unit_amount,
                        currency: Some(price.currency),
                        interval: match price.recurring.as_ref().map(|r| r.interval.as_str()) {
                            Some("month") => Some(PlanInterval::Month),
                            Some("year") => Some(PlanInterval::Year),
                            _ => None,
                        },
                    })
                }))
            })
            .await;
        match fetched {
            Ok(views) => views.clone(),
            Err(error) => {
                tracing::warn!(%error, "fetching the plan prices from Stripe failed");
                plans.iter().map(fallback_plan).collect()
            }
        }
    }
}

fn fallback_plan(plan: &Plan) -> PlanView {
    let mut chars = plan.key.chars();
    let name = chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default();
    PlanView {
        key: plan.key.clone(),
        product_id: None,
        name,
        amount: None,
        currency: None,
        interval: None,
    }
}

#[cfg(test)]
mod tests {
    use super::{SubscriptionStatus, WorkspaceAccess, access};

    const TRIAL_MILLIS: i64 = 3 * 24 * 60 * 60 * 1000;

    #[test]
    fn paying_subscriptions_grant_full_access_and_others_a_trial_then_read_only() {
        const CREATED: i64 = 1_000_000;
        let in_trial = CREATED + TRIAL_MILLIS - 1;
        let after_trial = CREATED + TRIAL_MILLIS;
        let table: Vec<(Option<&str>, WorkspaceAccess, WorkspaceAccess)> = [None]
            .into_iter()
            .chain(SubscriptionStatus::ALL.map(Some))
            .map(|status| {
                (
                    status.map(SubscriptionStatus::as_str),
                    access(false, status, CREATED, in_trial, TRIAL_MILLIS),
                    access(false, status, CREATED, after_trial, TRIAL_MILLIS),
                )
            })
            .collect();

        use WorkspaceAccess::{Full, ReadOnly, Trial};
        assert_eq!(
            table,
            [
                (None, Trial, ReadOnly),
                (Some("incomplete"), Trial, ReadOnly),
                (Some("incomplete_expired"), Trial, ReadOnly),
                (Some("trialing"), Full, Full),
                (Some("active"), Full, Full),
                (Some("past_due"), Full, Full),
                (Some("canceled"), Trial, ReadOnly),
                (Some("unpaid"), Trial, ReadOnly),
                (Some("paused"), Trial, ReadOnly),
            ]
        );
    }

    #[test]
    fn the_exempt_workspace_has_full_access_whatever_its_subscription_and_age() {
        let after_trial = TRIAL_MILLIS;
        let table: Vec<WorkspaceAccess> = [None]
            .into_iter()
            .chain(SubscriptionStatus::ALL.map(Some))
            .map(|status| access(true, status, 0, after_trial, TRIAL_MILLIS))
            .collect();
        assert_eq!(table, [WorkspaceAccess::Full; 9]);
    }

    #[test]
    fn statuses_round_trip_and_unknown_ones_are_rejected() {
        for status in SubscriptionStatus::ALL {
            assert_eq!(SubscriptionStatus::parse(status.as_str()), Some(status));
        }
        assert_eq!(SubscriptionStatus::parse("ended"), None);
    }
}
