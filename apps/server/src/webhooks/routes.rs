//! `/api/v1/workspaces/{workspace_id}/webhooks`: workspace owners and admins
//! (`integrations.manage`) manage subscriptions and look at their deliveries.

use std::sync::Arc;

use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use orbit_domain::Permission;
use orbit_platform::{Id, RequestId, TimestampMillis};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{Any, Transaction};
use utoipa::ToSchema;

use super::WebhookEventType;
use super::delivery::new_secret;
use super::store::{self, Delivery, NewSubscription, Subscription};
use crate::audit::{self, AuditOutcome};
use crate::auth_routes::CookieMode;
use crate::repositories::chat::clean_request_url;
use crate::repositories::identity::IdentityRepository;
use crate::repositories::membership;
use crate::task_routes::{ApiError, ApiJson, authenticate_session, request_id_value, validation};

const BASE: &str = "/api/v1/workspaces/{workspace_id}/webhooks";
const MAX_NAME_CHARS: usize = 100;
const MAX_PROJECTS: usize = 200;

#[derive(Clone)]
pub struct WebhookState {
    identity: Arc<IdentityRepository>,
    cookie_mode: CookieMode,
    app_key: Option<[u8; 32]>,
    development: bool,
}

impl WebhookState {
    #[must_use]
    pub fn new(
        identity: Arc<IdentityRepository>,
        cookie_mode: CookieMode,
        app_key: Option<[u8; 32]>,
        development: bool,
    ) -> Self {
        Self {
            identity,
            cookie_mode,
            app_key,
            development,
        }
    }
}

pub fn webhook_router(state: WebhookState) -> Router {
    Router::new()
        .route(BASE, get(list_webhooks).post(create_webhook))
        .route(
            &format!("{BASE}/{{webhook_id}}"),
            get(get_webhook)
                .patch(update_webhook)
                .delete(delete_webhook),
        )
        .route(
            &format!("{BASE}/{{webhook_id}}/rotate-secret"),
            post(rotate_webhook_secret),
        )
        .route(
            &format!("{BASE}/{{webhook_id}}/deliveries"),
            get(list_webhook_deliveries),
        )
        .route(
            &format!("{BASE}/{{webhook_id}}/deliveries/{{delivery_id}}/retry"),
            post(retry_webhook_delivery),
        )
        .with_state(state)
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = WebhookCreateBody)]
pub(crate) struct CreateBody {
    /// 1 to 100 characters.
    name: String,
    /// A public https URL (development also takes http and local hosts).
    url: String,
    /// At least one.
    event_types: Vec<WebhookEventType>,
    /// Live projects of the workspace whose tasks send events; at least one.
    #[schema(value_type = Vec<String>)]
    project_ids: Vec<String>,
    #[serde(default = "enabled_by_default")]
    enabled: bool,
}

const fn enabled_by_default() -> bool {
    true
}

/// Omitted fields keep their value. Turning a subscription off cancels its pending deliveries.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = WebhookUpdateBody)]
pub(crate) struct UpdateBody {
    name: Option<String>,
    url: Option<String>,
    event_types: Option<Vec<WebhookEventType>>,
    #[schema(value_type = Option<Vec<String>>)]
    project_ids: Option<Vec<String>>,
    enabled: Option<bool>,
}

/// A subscription and its signing secret, which is shown only here.
#[derive(Serialize, ToSchema)]
#[schema(as = WebhookCreated)]
pub(crate) struct Created {
    #[serde(flatten)]
    #[schema(inline)]
    subscription: Subscription,
    /// `whsec_…`: verifies the `webhook-signature` header. Shown once.
    signing_secret: String,
}

#[derive(Serialize, ToSchema)]
#[schema(as = WebhookSigningSecret)]
pub(crate) struct SigningSecret {
    /// `whsec_…`: verifies the `webhook-signature` header from now on. Shown once.
    signing_secret: String,
}

/// One authorized request: the caller manages integrations of the workspace.
struct Call<'a> {
    state: &'a WebhookState,
    workspace_id: Id,
    actor_id: Id,
    instance: String,
    request_id: Option<&'a Extension<RequestId>>,
}

impl<'a> Call<'a> {
    async fn enter(
        state: &'a WebhookState,
        headers: &HeaderMap,
        uri: &Uri,
        workspace: &str,
        request_id: Option<&'a Extension<RequestId>>,
    ) -> Result<Self, ApiError> {
        let instance = uri.path().to_owned();
        let session = authenticate_session(
            &state.identity,
            state.cookie_mode,
            headers,
            &instance,
            request_id,
        )
        .await?;
        let workspace_id = workspace
            .parse()
            .map_err(|_| not_found(&instance, request_id))?;
        let call = Self {
            state,
            workspace_id,
            actor_id: session.user.id,
            instance,
            request_id,
        };
        let actor = membership::actor(
            state.identity.database().pool(),
            call.workspace_id,
            call.actor_id,
        )
        .await
        .map_err(|_| call.internal())?
        .ok_or_else(|| call.not_found())?;
        if !actor.can(Permission::IntegrationsManage) {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "webhook_manager_required",
                "Manager required",
                "Only workspace owners and admins can manage webhooks.",
                &call.instance,
                request_id,
            ));
        }
        Ok(call)
    }

    fn id(&self, value: &str) -> Result<Id, ApiError> {
        value.parse().map_err(|_| self.not_found())
    }

    fn not_found(&self) -> ApiError {
        not_found(&self.instance, self.request_id)
    }

    fn internal(&self) -> ApiError {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "Internal server error",
            "An unexpected error occurred. Use the request ID when contacting support.",
            &self.instance,
            self.request_id,
        )
    }

    fn invalid(&self, field: &'static str) -> ApiError {
        validation(field, &self.instance, self.request_id)
    }

    fn app_key(&self) -> Result<[u8; 32], ApiError> {
        self.state.app_key.ok_or_else(|| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "app_key_missing",
                "App key missing",
                "Webhook signing secrets need the server's app key (ORBIT_APP_KEY).",
                &self.instance,
                self.request_id,
            )
        })
    }

    fn seal(&self, secret: &str) -> Result<Vec<u8>, ApiError> {
        crate::secret_box::encrypt_secret(&self.app_key()?, secret).map_err(|()| self.internal())
    }

    async fn read(&self) -> Result<Transaction<'_, Any>, ApiError> {
        self.state
            .identity
            .database()
            .transaction()
            .await
            .map_err(|_| self.internal())
    }

    async fn write(&self) -> Result<Transaction<'_, Any>, ApiError> {
        self.state
            .identity
            .database()
            .immediate_transaction()
            .await
            .map_err(|_| self.internal())
    }

    async fn subscription(
        &self,
        transaction: &mut Transaction<'_, Any>,
        webhook: &str,
    ) -> Result<Subscription, ApiError> {
        store::get(transaction, self.workspace_id, self.id(webhook)?)
            .await
            .map_err(|_| self.internal())?
            .ok_or_else(|| self.not_found())
    }

    async fn audit(
        &self,
        transaction: &mut Transaction<'_, Any>,
        action: &str,
        webhook_id: Id,
        metadata: serde_json::Value,
        now: TimestampMillis,
    ) -> Result<(), ApiError> {
        audit::record(
            transaction,
            self.workspace_id,
            Some(self.actor_id),
            action,
            AuditOutcome::Success,
            "webhook",
            Some(webhook_id),
            request_id_value(self.request_id),
            metadata,
            now,
        )
        .await
        .map_err(|_| self.internal())
    }

    /// Checks a full set of fields, as stored.
    async fn checked(
        &self,
        transaction: &mut Transaction<'_, Any>,
        name: String,
        url: &str,
        event_types: Vec<WebhookEventType>,
        project_ids: &[String],
    ) -> Result<(String, String, Vec<WebhookEventType>, Vec<Id>), ApiError> {
        let name = name.trim().to_owned();
        if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
            return Err(self.invalid("name"));
        }
        let url =
            clean_request_url(url, self.state.development).map_err(|_| self.invalid("url"))?;
        let mut event_types = event_types;
        event_types.sort_by_key(|event_type| event_type.as_str());
        event_types.dedup();
        if event_types.is_empty() {
            return Err(self.invalid("event_types"));
        }
        let mut projects = project_ids
            .iter()
            .map(|id| id.parse::<Id>().map_err(|_| self.invalid("project_ids")))
            .collect::<Result<Vec<_>, _>>()?;
        projects.sort();
        projects.dedup();
        if projects.is_empty()
            || projects.len() > MAX_PROJECTS
            || !store::projects_exist(transaction, self.workspace_id, &projects)
                .await
                .map_err(|_| self.internal())?
        {
            return Err(self.invalid("project_ids"));
        }
        Ok((name, url, event_types, projects))
    }
}

fn not_found(instance: &str, request_id: Option<&Extension<RequestId>>) -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        "webhook_not_found",
        "Webhook not found",
        "The webhook was not found.",
        instance,
        request_id,
    )
}

/// The workspace's webhook subscriptions, oldest first.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/webhooks", params(("workspace_id" = String, Path)), responses((status = 200, body = Vec<Subscription>)))]
pub(crate) async fn list_webhooks(
    State(state): State<WebhookState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<Vec<Subscription>>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, request_id.as_ref()).await?;
    let mut transaction = call.read().await?;
    store::list(&mut transaction, call.workspace_id)
        .await
        .map(Json)
        .map_err(|_| call.internal())
}

/// Subscribes a URL to task events of some projects. The answer holds the signing secret, once.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/webhooks", params(("workspace_id" = String, Path)), request_body = CreateBody, responses((status = 201, body = Created)))]
pub(crate) async fn create_webhook(
    State(state): State<WebhookState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<CreateBody>,
) -> Result<Response, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, request_id.as_ref()).await?;
    let signing_secret = new_secret();
    let sealed = call.seal(&signing_secret)?;
    let now = TimestampMillis::now();
    let mut transaction = call.write().await?;
    let (name, url, event_types, project_ids) = call
        .checked(
            &mut transaction,
            body.name,
            &body.url,
            body.event_types,
            &body.project_ids,
        )
        .await?;
    let input = NewSubscription {
        name: &name,
        url: &url,
        event_types: &event_types,
        project_ids: &project_ids,
        enabled: body.enabled,
    };
    let id = store::insert(
        &mut transaction,
        call.workspace_id,
        call.actor_id,
        &input,
        &sealed,
        now,
    )
    .await
    .map_err(|_| call.internal())?;
    call.audit(
        &mut transaction,
        "webhook.created",
        id,
        json!({ "name": name, "event_types": event_types }),
        now,
    )
    .await?;
    let subscription = store::get(&mut transaction, call.workspace_id, id)
        .await
        .map_err(|_| call.internal())?
        .ok_or_else(|| call.internal())?;
    transaction.commit().await.map_err(|_| call.internal())?;
    Ok((
        StatusCode::CREATED,
        Json(Created {
            subscription,
            signing_secret,
        }),
    )
        .into_response())
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/webhooks/{webhook_id}", params(("workspace_id" = String, Path), ("webhook_id" = String, Path)), responses((status = 200, body = Subscription)))]
pub(crate) async fn get_webhook(
    State(state): State<WebhookState>,
    Path((workspace, webhook)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<Subscription>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, request_id.as_ref()).await?;
    let mut transaction = call.read().await?;
    call.subscription(&mut transaction, &webhook)
        .await
        .map(Json)
}

/// Changes a subscription. Turning it off cancels its pending deliveries.
#[utoipa::path(patch, path = "/api/v1/workspaces/{workspace_id}/webhooks/{webhook_id}", params(("workspace_id" = String, Path), ("webhook_id" = String, Path)), request_body = UpdateBody, responses((status = 200, body = Subscription)))]
pub(crate) async fn update_webhook(
    State(state): State<WebhookState>,
    Path((workspace, webhook)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<UpdateBody>,
) -> Result<Json<Subscription>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, request_id.as_ref()).await?;
    let now = TimestampMillis::now();
    let mut transaction = call.write().await?;
    let current = call.subscription(&mut transaction, &webhook).await?;
    let project_ids = body.project_ids.unwrap_or_else(|| {
        current
            .project_ids
            .iter()
            .map(ToString::to_string)
            .collect()
    });
    let (name, url, event_types, project_ids) = call
        .checked(
            &mut transaction,
            body.name.unwrap_or_else(|| current.name.clone()),
            body.url.as_deref().unwrap_or(&current.url),
            body.event_types
                .unwrap_or_else(|| current.event_types.clone()),
            &project_ids,
        )
        .await?;
    let enabled = body.enabled.unwrap_or(current.enabled);
    let input = NewSubscription {
        name: &name,
        url: &url,
        event_types: &event_types,
        project_ids: &project_ids,
        enabled,
    };
    store::update(&mut transaction, current.id, &input, now)
        .await
        .map_err(|_| call.internal())?;
    call.audit(
        &mut transaction,
        "webhook.updated",
        current.id,
        json!({ "name": name, "enabled": enabled, "event_types": event_types }),
        now,
    )
    .await?;
    let subscription = call.subscription(&mut transaction, &webhook).await?;
    transaction.commit().await.map_err(|_| call.internal())?;
    Ok(Json(subscription))
}

/// Deletes a subscription with its deliveries; pending ones are not sent.
#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/webhooks/{webhook_id}", params(("workspace_id" = String, Path), ("webhook_id" = String, Path)), responses((status = 204)))]
pub(crate) async fn delete_webhook(
    State(state): State<WebhookState>,
    Path((workspace, webhook)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: Option<Extension<RequestId>>,
) -> Result<StatusCode, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, request_id.as_ref()).await?;
    let now = TimestampMillis::now();
    let mut transaction = call.write().await?;
    let current = call.subscription(&mut transaction, &webhook).await?;
    store::delete(&mut transaction, current.id)
        .await
        .map_err(|_| call.internal())?;
    call.audit(
        &mut transaction,
        "webhook.deleted",
        current.id,
        json!({ "name": current.name }),
        now,
    )
    .await?;
    transaction.commit().await.map_err(|_| call.internal())?;
    Ok(StatusCode::NO_CONTENT)
}

/// Replaces the signing secret. Deliveries sent from now on, retries included, use the new one.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/webhooks/{webhook_id}/rotate-secret", params(("workspace_id" = String, Path), ("webhook_id" = String, Path)), responses((status = 200, body = SigningSecret)))]
pub(crate) async fn rotate_webhook_secret(
    State(state): State<WebhookState>,
    Path((workspace, webhook)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<SigningSecret>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, request_id.as_ref()).await?;
    let signing_secret = new_secret();
    let sealed = call.seal(&signing_secret)?;
    let now = TimestampMillis::now();
    let mut transaction = call.write().await?;
    let current = call.subscription(&mut transaction, &webhook).await?;
    store::set_secret(&mut transaction, current.id, &sealed, now)
        .await
        .map_err(|_| call.internal())?;
    call.audit(
        &mut transaction,
        "webhook.secret_rotated",
        current.id,
        json!({}),
        now,
    )
    .await?;
    transaction.commit().await.map_err(|_| call.internal())?;
    Ok(Json(SigningSecret { signing_secret }))
}

/// The subscription's newest 50 deliveries. Finished ones are kept for 30 days.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/webhooks/{webhook_id}/deliveries", params(("workspace_id" = String, Path), ("webhook_id" = String, Path)), responses((status = 200, body = Vec<Delivery>)))]
pub(crate) async fn list_webhook_deliveries(
    State(state): State<WebhookState>,
    Path((workspace, webhook)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<Vec<Delivery>>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, request_id.as_ref()).await?;
    let mut transaction = call.read().await?;
    let current = call.subscription(&mut transaction, &webhook).await?;
    store::deliveries(&mut transaction, current.id)
        .await
        .map(Json)
        .map_err(|_| call.internal())
}

/// Sends a failed or cancelled delivery again, with the same body and `webhook-id`, and a fresh
/// set of retries. 409 for a pending or delivered one.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/webhooks/{webhook_id}/deliveries/{delivery_id}/retry", params(("workspace_id" = String, Path), ("webhook_id" = String, Path), ("delivery_id" = String, Path)), responses((status = 200, body = Delivery)))]
pub(crate) async fn retry_webhook_delivery(
    State(state): State<WebhookState>,
    Path((workspace, webhook, delivery)): Path<(String, String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<Delivery>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, request_id.as_ref()).await?;
    let delivery_id = call.id(&delivery)?;
    let now = TimestampMillis::now();
    let mut transaction = call.write().await?;
    let current = call.subscription(&mut transaction, &webhook).await?;
    let found = store::delivery(&mut transaction, current.id, delivery_id)
        .await
        .map_err(|_| call.internal())?
        .ok_or_else(|| call.not_found())?;
    if !matches!(found.state.as_str(), "failed" | "cancelled") {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "webhook_delivery_not_retryable",
            "Delivery not retryable",
            "Only a failed or cancelled delivery can be sent again.",
            &call.instance,
            call.request_id,
        ));
    }
    store::reset_delivery(&mut transaction, delivery_id, now)
        .await
        .map_err(|_| call.internal())?;
    super::enqueue_in(&mut transaction, call.workspace_id, delivery_id, now)
        .await
        .map_err(|_| call.internal())?;
    let updated = store::delivery(&mut transaction, current.id, delivery_id)
        .await
        .map_err(|_| call.internal())?
        .ok_or_else(|| call.internal())?;
    transaction.commit().await.map_err(|_| call.internal())?;
    Ok(Json(updated))
}
