//! Push subscriptions and notification preferences of the signed-in user.

use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use orbit_platform::{RequestId, TimestampMillis};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::{ApiError, ApiJson, AuthState, ProblemBody, authenticate};
use crate::push::{
    Notice, NotificationPrefs, PushService, PushSubscriptionRecord, allowed_endpoint, webpush,
};

#[derive(Serialize, ToSchema)]
pub(super) struct PushKey {
    /// The `applicationServerKey` a browser subscribes with.
    public_key: String,
}

#[utoipa::path(get, path = "/api/v1/push/key", responses((status = 200, body = PushKey), (status = 401, description = "authentication_required", body = ProblemBody, content_type = "application/problem+json")))]
pub(super) async fn get_push_key(
    State(state): State<AuthState>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<PushKey>, ApiError> {
    let instance = "/api/v1/push/key";
    authenticate(&state, &headers, instance, request_id.as_ref()).await?;
    let public_key = PushService::of(state.repository.database())
        .public_key()
        .await
        .map_err(|_| ApiError::internal(instance, request_id.as_ref()))?;
    Ok(Json(PushKey { public_key }))
}

#[utoipa::path(get, path = "/api/v1/push/subscriptions", responses((status = 200, body = Vec<PushSubscriptionRecord>), (status = 401, description = "authentication_required", body = ProblemBody, content_type = "application/problem+json")))]
pub(super) async fn list_push_subscriptions(
    State(state): State<AuthState>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<Vec<PushSubscriptionRecord>>, ApiError> {
    let instance = "/api/v1/push/subscriptions";
    let session = authenticate(&state, &headers, instance, request_id.as_ref()).await?;
    PushService::of(state.repository.database())
        .subscriptions(session.user.id)
        .await
        .map(Json)
        .map_err(|_| ApiError::internal(instance, request_id.as_ref()))
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct PushSubscriptionBody {
    /// The URL of the browser's `PushSubscription`.
    endpoint: String,
    /// Its `p256dh` key, base64url.
    p256dh: String,
    /// Its `auth` secret, base64url.
    auth: String,
    /// What the list of devices shows, e.g. "Firefox on Linux".
    label: String,
}

#[utoipa::path(post, path = "/api/v1/push/subscriptions", request_body = PushSubscriptionBody, responses((status = 200, body = PushSubscriptionRecord), (status = 401, description = "authentication_required", body = ProblemBody, content_type = "application/problem+json"), (status = 422, description = "invalid_subscription", body = ProblemBody, content_type = "application/problem+json")))]
pub(super) async fn create_push_subscription(
    State(state): State<AuthState>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<PushSubscriptionBody>,
) -> Result<Json<PushSubscriptionRecord>, ApiError> {
    let instance = "/api/v1/push/subscriptions";
    let session = authenticate(&state, &headers, instance, request_id.as_ref()).await?;
    let label: String = body.label.trim().chars().take(120).collect();
    let keys_fit = webpush::decode(&body.p256dh).is_some_and(|key| key.len() == 65)
        && webpush::decode(&body.auth).is_some_and(|auth| auth.len() == 16);
    if body.endpoint.len() > 2048 || !allowed_endpoint(&body.endpoint) || !keys_fit {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_subscription",
            "Invalid subscription",
            "The subscription is not from the push service of a supported browser.",
            instance,
            request_id.as_ref(),
        ));
    }
    PushService::of(state.repository.database())
        .subscribe(
            session.user.id,
            session.id,
            &body.endpoint,
            &body.p256dh,
            &body.auth,
            &label,
            TimestampMillis::now(),
        )
        .await
        .map(Json)
        .map_err(|_| ApiError::internal(instance, request_id.as_ref()))
}

#[utoipa::path(delete, path = "/api/v1/push/subscriptions/{id}", params(("id" = String, Path)), responses((status = 204), (status = 401, description = "authentication_required", body = ProblemBody, content_type = "application/problem+json")))]
pub(super) async fn delete_push_subscription(
    State(state): State<AuthState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<StatusCode, ApiError> {
    let instance = "/api/v1/push/subscriptions/{id}";
    let session = authenticate(&state, &headers, instance, request_id.as_ref()).await?;
    PushService::of(state.repository.database())
        .unsubscribe(session.user.id, &id)
        .await
        .map_err(|_| ApiError::internal(instance, request_id.as_ref()))?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, ToSchema)]
pub(super) struct TestPushResult {
    /// How many of the user's browsers took the push.
    sent: usize,
}

#[utoipa::path(post, path = "/api/v1/push/test", responses((status = 200, body = TestPushResult), (status = 401, description = "authentication_required", body = ProblemBody, content_type = "application/problem+json")))]
pub(super) async fn send_test_push(
    State(state): State<AuthState>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<TestPushResult>, ApiError> {
    let instance = "/api/v1/push/test";
    let session = authenticate(&state, &headers, instance, request_id.as_ref()).await?;
    let notice = Notice {
        title: "Orbit".to_owned(),
        body: "Push notifications work on this device.".to_owned(),
        url: "/profile/notifications".to_owned(),
        tag: "test".to_owned(),
        sound: "message",
        icon: None,
    };
    let sent = PushService::of(state.repository.database())
        .push(session.user.id, &notice)
        .await
        .map_err(|_| ApiError::internal(instance, request_id.as_ref()))?;
    Ok(Json(TestPushResult { sent }))
}

#[utoipa::path(get, path = "/api/v1/notification-preferences", responses((status = 200, body = NotificationPrefs), (status = 401, description = "authentication_required", body = ProblemBody, content_type = "application/problem+json")))]
pub(super) async fn get_notification_preferences(
    State(state): State<AuthState>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<NotificationPrefs>, ApiError> {
    let instance = "/api/v1/notification-preferences";
    let session = authenticate(&state, &headers, instance, request_id.as_ref()).await?;
    PushService::of(state.repository.database())
        .prefs(session.user.id)
        .await
        .map(Json)
        .map_err(|_| ApiError::internal(instance, request_id.as_ref()))
}

#[utoipa::path(put, path = "/api/v1/notification-preferences", request_body = NotificationPrefs, responses((status = 200, body = NotificationPrefs), (status = 401, description = "authentication_required", body = ProblemBody, content_type = "application/problem+json")))]
pub(super) async fn put_notification_preferences(
    State(state): State<AuthState>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<NotificationPrefs>,
) -> Result<Json<NotificationPrefs>, ApiError> {
    let instance = "/api/v1/notification-preferences";
    let session = authenticate(&state, &headers, instance, request_id.as_ref()).await?;
    PushService::of(state.repository.database())
        .set_prefs(session.user.id, &body, TimestampMillis::now())
        .await
        .map_err(|_| ApiError::internal(instance, request_id.as_ref()))?;
    Ok(Json(body))
}
