//! Billing routes: the workspace's plan, Stripe Checkout and Billing Portal redirects (owner only),
//! and the Stripe webhook. Mounted only in Orbit Cloud, so they answer 404 elsewhere.

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use orbit_domain::Permission;
use orbit_platform::{Id, RequestId, TimestampMillis};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

use super::stripe::{CheckoutRequest, verify_signature};
use super::{Billing, SubscriptionStatus, TrialLimit, WorkspaceAccess, sync};
use crate::auth_routes::CookieMode;
use crate::repositories::billing as repository;
use crate::repositories::identity::{AuthenticatedSession, IdentityRepository};
use crate::repositories::membership;
use crate::task_routes::{ApiError, ApiJson, authenticate_session};

const WEBHOOK: &str = "/api/v1/billing/stripe/webhook";
/// The Stripe events that can change a workspace's subscription.
const SYNCED_EVENTS: [&str; 4] = [
    "checkout.session.completed",
    "customer.subscription.created",
    "customer.subscription.updated",
    "customer.subscription.deleted",
];

#[derive(Clone)]
pub(crate) struct BillingState {
    billing: Billing,
    identity: Arc<IdentityRepository>,
    cookie_mode: CookieMode,
}

pub fn router(
    billing: Billing,
    identity: Arc<IdentityRepository>,
    cookie_mode: CookieMode,
) -> Router {
    Router::new()
        .route(
            "/api/v1/workspaces/{workspace_id}/billing",
            get(get_billing),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/billing/checkout",
            post(billing_checkout),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/billing/portal",
            post(billing_portal),
        )
        .route(WEBHOOK, post(stripe_webhook))
        .with_state(BillingState {
            billing,
            identity,
            cookie_mode,
        })
}

/// The workspace's subscription and what the caller may do about it.
#[derive(Debug, Serialize, ToSchema)]
pub struct BillingView {
    /// The subscribed plan's key, when its Stripe price is one of the configured plans.
    plan: Option<String>,
    status: Option<SubscriptionStatus>,
    access: WorkspaceAccess,
    /// The root user's first workspace: full access without a subscription.
    exempt: bool,
    /// The subscription ends at `current_period_end` instead of renewing.
    cancel_at_period_end: bool,
    #[schema(value_type = Option<String>, format = DateTime)]
    current_period_end: Option<TimestampMillis>,
    /// The plans a workspace can subscribe to, in order.
    plans: Vec<PlanView>,
    /// The workspace has a Stripe customer, so the Billing Portal can open.
    has_customer: bool,
    /// The caller may start a checkout and open the Billing Portal (the owner).
    can_manage: bool,
    /// How long a new workspace's trial lasts; 0 when new workspaces have none.
    trial_days: u32,
    /// Only while `access` is `trial`.
    trial: Option<TrialView>,
}

/// A plan as Stripe describes its price. When Stripe cannot be reached, only `key` and a `name`
/// made from it are set.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct PlanView {
    /// What `BillingCheckoutBody.plan` takes.
    pub(super) key: String,
    /// Plans of one product are the same plan billed at different intervals.
    pub(super) product_id: Option<String>,
    /// The product's name.
    pub(super) name: String,
    /// In the currency's minor units (cents).
    pub(super) amount: Option<i64>,
    /// ISO 4217, lowercase (`usd`).
    pub(super) currency: Option<String>,
    pub(super) interval: Option<PlanInterval>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlanInterval {
    Month,
    Year,
}

/// A trial workspace's end and usage of its limits.
#[derive(Debug, Serialize, ToSchema)]
pub struct TrialView {
    #[schema(value_type = String, format = DateTime)]
    ends_at: TimestampMillis,
    tasks: Usage,
    pages: Usage,
    members: Usage,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Usage {
    used: i64,
    limit: i64,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct BillingCheckoutBody {
    /// One of `BillingView.plans`.
    plan: String,
}

/// A Stripe page to send the browser to.
#[derive(Serialize, ToSchema)]
pub struct BillingRedirect {
    url: String,
}

#[utoipa::path(
    get,
    path = "/api/v1/workspaces/{workspace_id}/billing",
    params(("workspace_id" = String, Path)),
    responses((status = 200, body = BillingView))
)]
pub(crate) async fn get_billing(
    State(state): State<BillingState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<BillingView>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/billing");
    let id = request_id.as_ref();
    let (workspace_id, actor, _) = member(&state, &workspace, &headers, &instance, id).await?;
    let database = state.billing.database();
    let subscription = repository::subscription(database, workspace_id)
        .await
        .map_err(|_| internal(&instance, id))?;
    let now = TimestampMillis::now();
    let trial_config = &state.billing.config().trial;
    let access = subscription.access(now, trial_config);
    let trial = if access == WorkspaceAccess::Trial {
        let usage = async |limit: TrialLimit| {
            repository::trial_usage(database, workspace_id, limit, now)
                .await
                .map(|used| Usage {
                    used,
                    limit: limit.limit(trial_config),
                })
                .map_err(|_| internal(&instance, id))
        };
        Some(TrialView {
            ends_at: TimestampMillis::from_millis(subscription.trial_ends_at(trial_config)),
            tasks: usage(TrialLimit::Tasks).await?,
            pages: usage(TrialLimit::Pages).await?,
            members: usage(TrialLimit::Members).await?,
        })
    } else {
        None
    };
    Ok(Json(BillingView {
        plan: subscription.plan.clone(),
        status: subscription.status,
        access,
        exempt: subscription.exempt,
        cancel_at_period_end: subscription.cancel_at_period_end,
        current_period_end: subscription
            .current_period_end
            .map(TimestampMillis::from_millis),
        plans: state.billing.plans().await,
        has_customer: subscription.stripe_customer_id.is_some(),
        can_manage: actor.can(Permission::BillingManage),
        trial_days: trial_config.days,
        trial,
    }))
}

/// Starts a Stripe Checkout for `plan` and returns its URL. Refused while the workspace already
/// has full access (a subscription, or the exempt workspace), or while Stripe reports a live
/// subscription of the workspace's customer: plan changes and cancellations go through the
/// Billing Portal. Open Checkout Sessions of the customer expire first.
#[utoipa::path(
    post,
    path = "/api/v1/workspaces/{workspace_id}/billing/checkout",
    params(("workspace_id" = String, Path)),
    request_body = BillingCheckoutBody,
    responses((status = 200, body = BillingRedirect))
)]
pub(crate) async fn billing_checkout(
    State(state): State<BillingState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<BillingCheckoutBody>,
) -> Result<Json<BillingRedirect>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/billing/checkout");
    let id = request_id.as_ref();
    let (workspace_id, email) = manager(&state, &workspace, &headers, &instance, id).await?;
    let plan = state.billing.config().plan(&body.plan).ok_or_else(|| {
        ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "unknown_plan",
            "Unknown plan",
            "The plan is not offered.",
            instance.clone(),
            id,
        )
    })?;
    let database = state.billing.database();
    let subscription = repository::subscription(database, workspace_id)
        .await
        .map_err(|_| internal(&instance, id))?;
    if subscription.access(TimestampMillis::now(), &state.billing.config().trial)
        == WorkspaceAccess::Full
    {
        return Err(subscription_active(&instance, id));
    }
    let customer_id = match subscription.stripe_customer_id {
        Some(customer_id) => {
            refuse_duplicate_checkout(&state, workspace_id, &customer_id, &instance, id).await?;
            customer_id
        }
        None => {
            let name: String = sqlx::query_scalar("SELECT name FROM workspaces WHERE id = $1")
                .bind(workspace_id.to_string())
                .fetch_one(database.pool())
                .await
                .map_err(|_| internal(&instance, id))?;
            let created = state
                .billing
                .stripe()
                .create_customer(workspace_id, &name, &email)
                .await
                .map_err(|error| stripe_failed(&error, &instance, id))?;
            repository::save_customer(database, workspace_id, &created, TimestampMillis::now())
                .await
                .map_err(|_| internal(&instance, id))?
        }
    };
    let origin = state.billing.public_origin();
    let url = state
        .billing
        .stripe()
        .create_checkout_session(&CheckoutRequest {
            customer_id: &customer_id,
            price_id: &plan.price_id,
            workspace_id,
            success_url: &format!("{origin}/settings/billing?checkout=success"),
            cancel_url: &format!("{origin}/settings/billing?checkout=cancel"),
        })
        .await
        .map_err(|error| stripe_failed(&error, &instance, id))?;
    Ok(Json(BillingRedirect { url }))
}

/// Asks Stripe, not the stored state, so a subscription whose webhook has not arrived yet also
/// counts: a live one refuses the checkout (and queues the sync Orbit missed), and open Checkout
/// Sessions expire, so a second tab cannot subscribe the workspace twice.
async fn refuse_duplicate_checkout(
    state: &BillingState,
    workspace_id: Id,
    customer_id: &str,
    instance: &str,
    id: Option<&Extension<RequestId>>,
) -> Result<(), ApiError> {
    let stripe = state.billing.stripe();
    let subscriptions = stripe
        .customer_subscriptions(customer_id)
        .await
        .map_err(|error| stripe_failed(&error, instance, id))?;
    if let Some(live) = subscriptions
        .iter()
        .find(|subscription| super::grants_access(SubscriptionStatus::parse(&subscription.status)))
    {
        if let Err(error) =
            sync::enqueue(state.billing.database(), &live.id, Some(workspace_id)).await
        {
            tracing::warn!(%error, "queueing the Stripe sync failed");
        }
        return Err(subscription_active(instance, id));
    }
    stripe
        .expire_open_checkout_sessions(customer_id)
        .await
        .map_err(|error| stripe_failed(&error, instance, id))
}

fn subscription_active(instance: &str, id: Option<&Extension<RequestId>>) -> ApiError {
    ApiError::new(
        StatusCode::CONFLICT,
        "subscription_active",
        "Subscription active",
        "The workspace already has a subscription. Change it in the billing portal.",
        instance,
        id,
    )
}

/// Opens the Stripe Billing Portal (payment method, invoices, plan changes, cancellation).
#[utoipa::path(
    post,
    path = "/api/v1/workspaces/{workspace_id}/billing/portal",
    params(("workspace_id" = String, Path)),
    responses((status = 200, body = BillingRedirect))
)]
pub(crate) async fn billing_portal(
    State(state): State<BillingState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<BillingRedirect>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/billing/portal");
    let id = request_id.as_ref();
    let (workspace_id, _) = manager(&state, &workspace, &headers, &instance, id).await?;
    let customer_id = repository::subscription(state.billing.database(), workspace_id)
        .await
        .map_err(|_| internal(&instance, id))?
        .stripe_customer_id
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::CONFLICT,
                "billing_customer_missing",
                "No billing account",
                "The workspace has not subscribed yet. Choose a plan first.",
                instance.clone(),
                id,
            )
        })?;
    let return_url = format!("{}/settings/billing", state.billing.public_origin());
    let url = state
        .billing
        .stripe()
        .create_portal_session(&customer_id, &return_url)
        .await
        .map_err(|error| stripe_failed(&error, &instance, id))?;
    Ok(Json(BillingRedirect { url }))
}

/// Stripe webhook (snapshot events). Verifies `Stripe-Signature`, records the event id once and
/// queues `billing.stripe_sync` for events that can change a subscription; others are ignored.
#[utoipa::path(
    post,
    path = "/api/v1/billing/stripe/webhook",
    request_body = Value,
    params(("Stripe-Signature" = String, Header, description = "Stripe webhook signature")),
    responses((status = 200, description = "Event accepted or ignored"))
)]
pub(crate) async fn stripe_webhook(
    State(state): State<BillingState>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    body: Bytes,
) -> Result<StatusCode, ApiError> {
    let id = request_id.as_ref();
    let bad_request = |code, title, detail| {
        ApiError::new(StatusCode::BAD_REQUEST, code, title, detail, WEBHOOK, id)
    };
    let signature = headers
        .get("stripe-signature")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    verify_signature(
        state.billing.config().webhook_secret.expose(),
        signature,
        &body,
        TimestampMillis::now().as_millis() / 1000,
    )
    .map_err(|_| {
        bad_request(
            "invalid_stripe_signature",
            "Invalid Stripe signature",
            "The webhook signature is missing, invalid or too old.",
        )
    })?;
    let event: Value = serde_json::from_slice(&body).map_err(|_| {
        bad_request(
            "invalid_stripe_event",
            "Invalid Stripe event",
            "The webhook payload is not a Stripe event.",
        )
    })?;
    let (Some(event_id), Some(kind)) = (
        event.get("id").and_then(Value::as_str),
        event.get("type").and_then(Value::as_str),
    ) else {
        return Err(bad_request(
            "invalid_stripe_event",
            "Invalid Stripe event",
            "The webhook payload is not a Stripe event.",
        ));
    };
    if !SYNCED_EVENTS.contains(&kind) {
        return Ok(StatusCode::OK);
    }
    let object = event.pointer("/data/object").unwrap_or(&Value::Null);
    let (subscription_id, workspace) = if kind == "checkout.session.completed" {
        (
            object.get("subscription").and_then(Value::as_str),
            object.get("client_reference_id").and_then(Value::as_str),
        )
    } else {
        (
            object.get("id").and_then(Value::as_str),
            object
                .pointer("/metadata/workspace_id")
                .and_then(Value::as_str),
        )
    };
    // A checkout in payment mode has no subscription: nothing to sync.
    let Some(subscription_id) = subscription_id else {
        return Ok(StatusCode::OK);
    };
    let workspace_id = workspace.and_then(|value| value.parse::<Id>().ok());
    let database = state.billing.database();
    let new = repository::record_event(database, event_id, TimestampMillis::now())
        .await
        .map_err(|_| internal(WEBHOOK, id))?;
    if new && let Err(error) = sync::enqueue(database, subscription_id, workspace_id).await {
        tracing::warn!(%error, event_id, "queueing the Stripe sync failed");
        // Stripe redelivers after an error; let that delivery queue the job.
        let _ = repository::forget_event(database, event_id).await;
        return Err(internal(WEBHOOK, id));
    }
    Ok(StatusCode::OK)
}

async fn member(
    state: &BillingState,
    workspace: &str,
    headers: &HeaderMap,
    instance: &str,
    id: Option<&Extension<RequestId>>,
) -> Result<(Id, orbit_domain::Actor, AuthenticatedSession), ApiError> {
    let session =
        authenticate_session(&state.identity, state.cookie_mode, headers, instance, id).await?;
    let not_found = || {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "billing_workspace_not_found",
            "Workspace not found",
            "The workspace does not exist or you are not a member.",
            instance,
            id,
        )
    };
    let workspace_id: Id = workspace.parse().map_err(|_| not_found())?;
    let actor = membership::actor(
        state.billing.database().pool(),
        workspace_id,
        session.user.id,
    )
    .await
    .map_err(|_| internal(instance, id))?
    .ok_or_else(not_found)?;
    Ok((workspace_id, actor, session))
}

/// The workspace and the caller's email, for a caller who may manage billing.
async fn manager(
    state: &BillingState,
    workspace: &str,
    headers: &HeaderMap,
    instance: &str,
    id: Option<&Extension<RequestId>>,
) -> Result<(Id, String), ApiError> {
    let (workspace_id, actor, session) = member(state, workspace, headers, instance, id).await?;
    if !actor.can(Permission::BillingManage) {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "billing_owner_required",
            "Owner required",
            "Only the workspace owner manages billing.",
            instance,
            id,
        ));
    }
    Ok((workspace_id, session.user.email))
}

fn internal(instance: &str, id: Option<&Extension<RequestId>>) -> ApiError {
    ApiError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal_error",
        "Internal error",
        "The request could not be completed.",
        instance,
        id,
    )
}

fn stripe_failed(
    error: &super::stripe::StripeError,
    instance: &str,
    id: Option<&Extension<RequestId>>,
) -> ApiError {
    tracing::warn!(%error, "Stripe request failed");
    ApiError::new(
        StatusCode::BAD_GATEWAY,
        "stripe_unavailable",
        "Billing unavailable",
        "Stripe could not be reached. Try again in a moment.",
        instance,
        id,
    )
}
