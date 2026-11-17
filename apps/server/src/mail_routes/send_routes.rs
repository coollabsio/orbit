//! Send routes (spec §5.5, §7): the root user's instance routes under
//! `/api/v1/admin/mail/send-routes`, and a workspace's own under
//! `…/workspaces/{id}/mail/send-routes` (owners and admins, while the instance switch allows
//! them). A route's secrets are written, never read: the views show only which are set.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode, Uri};
use orbit_platform::{Id, RequestId, TimestampMillis};
use rand_core::RngCore as _;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::admin::require_root;
use super::{Call, MailState, RequestIdExtension, problem};
use crate::repositories::instance_settings::InstanceSettingsRepository;
use crate::repositories::mail::{
    MailError, NewRoute, ProviderRouteInput, ProviderScope, RouteChange, RouteSecrets,
    RouteSettings, RouteStep, SendRoute, SendRouteKind,
};
use crate::task_routes::{ApiError, ApiJson, request_id_value};

type RequestIdRef<'a> = Option<&'a Extension<RequestId>>;

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MailSendRouteScope {
    /// Set up by the root user; every workspace may pick it for a domain.
    Instance,
    Workspace,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct MailSendRouteView {
    #[schema(value_type = String)]
    id: Id,
    scope: MailSendRouteScope,
    /// `direct`, `smtp`, `postshiba`, `postmark`, `resend`, `mailgun`, `ses` or `cloudflare`.
    #[schema(value_type = String)]
    kind: SendRouteKind,
    name: String,
    /// The receiving provider this route was made from.
    #[schema(value_type = Option<String>)]
    provider_id: Option<Id>,
    /// `null` for an instance route listed to a workspace.
    settings: Option<RouteSettings>,
    /// The names of the secrets that are set (never their values).
    secrets_set: Vec<String>,
    /// Domains that use it as their own route.
    domain_count: i64,
    #[schema(value_type = Option<String>, format = DateTime)]
    last_sent_at: Option<TimestampMillis>,
    /// The last failed send, cleared by the next good one.
    last_error: Option<String>,
    /// Domains without a route of their own use this route.
    is_default: bool,
    #[schema(value_type = String, format = DateTime)]
    created_at: TimestampMillis,
    /// Where the provider posts bounce, complaint and delivery events; `null` for kinds without
    /// events (`direct`, `smtp`) and for an instance route listed to a workspace.
    events_url: Option<String>,
    /// Secrets Orbit made for this route, shown once (in the answer that created it).
    generated: Option<BTreeMap<String, String>>,
    /// A Cloudflare route made from a provider keeps the provider's signing secret as its event
    /// secret; the Worker already has it.
    event_secret_from_provider: bool,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct MailSendRouteList {
    routes: Vec<MailSendRouteView>,
    /// Whether workspaces may add their own routes (the instance switch).
    workspace_providers_enabled: bool,
    /// `ORBIT_MAIL_HOSTNAME` is set, so the `direct` kind can be created.
    direct_available: bool,
    #[schema(value_type = Option<String>)]
    default_route_id: Option<Id>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreateMailSendRouteBody {
    #[schema(value_type = String)]
    kind: SendRouteKind,
    name: String,
    /// The kind's secrets by name: `smtp` username and password (both or neither),
    /// `mailgun` api_key and webhook_signing_key, `ses` access_key_id and secret_access_key,
    /// `cloudflare` api_token and event_secret, `postmark` server_token, webhook_user and
    /// webhook_password, `resend` api_key and webhook_secret, `postshiba` smtp_username,
    /// smtp_password and webhook_secret. `direct` has none.
    #[serde(default)]
    secrets: BTreeMap<String, String>,
    #[serde(default)]
    settings: RouteSettings,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct UpdateMailSendRouteBody {
    #[serde(default)]
    name: Option<String>,
    /// Changes the named secrets; the ones left out keep their values.
    #[serde(default)]
    secrets: Option<BTreeMap<String, String>>,
    #[serde(default)]
    settings: Option<RouteSettings>,
    /// Removes the username and password of an `smtp` route (a relay without login).
    #[serde(default)]
    clear_login: bool,
}

/// **Also send through this provider**: only what the provider lacks.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreateProviderSendRouteBody {
    /// The provider's name when absent.
    #[serde(default)]
    name: Option<String>,
    /// For example the SMTP credentials; names as for a new route.
    #[serde(default)]
    secrets: BTreeMap<String, String>,
    #[serde(default)]
    settings: Option<RouteSettings>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct TestMailSendRouteBody {
    /// The address the admin types.
    to: String,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct MailSendRouteTest {
    /// The result of each step.
    steps: Vec<RouteStep>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SetMailSendDefaultBody {
    /// An instance route; `null` clears the default.
    route_id: Option<String>,
}

fn view(state: &MailState, route: SendRoute, own: bool, default: Option<Id>) -> MailSendRouteView {
    let events_url = (own && !matches!(route.kind, SendRouteKind::Direct | SendRouteKind::Smtp))
        .then(|| {
            format!(
                "{}/api/v1/mail/events/{}/{}",
                state.public_origin,
                route.kind.as_str(),
                route.id
            )
        });
    let is_default = default == Some(route.id);
    // an instance route listed to a workspace: its name and kind only
    MailSendRouteView {
        id: route.id,
        scope: if route.workspace_id.is_some() {
            MailSendRouteScope::Workspace
        } else {
            MailSendRouteScope::Instance
        },
        kind: route.kind,
        name: route.name,
        provider_id: route.provider_id.filter(|_| own),
        settings: own.then_some(route.settings),
        secrets_set: if own {
            route
                .secrets_set
                .iter()
                .map(|name| (*name).to_owned())
                .collect()
        } else {
            Vec::new()
        },
        domain_count: if own { route.domain_count } else { 0 },
        last_sent_at: route.last_sent_at.filter(|_| own),
        last_error: route.last_error.filter(|_| own),
        is_default,
        created_at: route.created_at,
        events_url,
        generated: None,
        event_secret_from_provider: false,
    }
}

async fn default_route(
    state: &MailState,
    instance: &str,
    request_id: RequestIdRef<'_>,
) -> Result<Option<Id>, ApiError> {
    state
        .mail
        .default_send_route()
        .await
        .map_err(|error| problem(error, instance, request_id))
}

async fn switch_on(
    state: &MailState,
    instance: &str,
    request_id: RequestIdRef<'_>,
) -> Result<bool, ApiError> {
    InstanceSettingsRepository::new(state.identity.database().clone())
        .mail_workspace_providers_enabled()
        .await
        .map_err(|error| problem(MailError::Unavailable(error), instance, request_id))
}

fn secrets_of(
    kind: SendRouteKind,
    given: &BTreeMap<String, String>,
) -> Result<RouteSecrets, MailError> {
    RouteSecrets::from_fields(kind, given).ok_or(MailError::Invalid { field: "secrets" })
}

fn random_hex(bytes: usize) -> String {
    let mut buffer = vec![0_u8; bytes];
    rand_core::OsRng.fill_bytes(&mut buffer);
    buffer.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Makes the event credentials the body left out: Postmark's `webhook_user` and
/// `webhook_password`, Cloudflare's `event_secret`. Returns what it made (shown once).
fn generate_missing(
    kind: SendRouteKind,
    given: &mut BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let wanted: &[(&str, bool)] = match kind {
        SendRouteKind::Postmark => &[("webhook_user", true), ("webhook_password", false)],
        SendRouteKind::Cloudflare => &[("event_secret", false)],
        _ => &[],
    };
    let mut made = BTreeMap::new();
    for (name, is_user) in wanted {
        if given
            .get(*name)
            .is_some_and(|value| !value.trim().is_empty())
        {
            continue;
        }
        let value = if *is_user {
            format!("orbit-{}", random_hex(6))
        } else {
            random_hex(32)
        };
        given.insert((*name).to_owned(), value.clone());
        made.insert((*name).to_owned(), value);
    }
    made
}

fn address(to: &str) -> Result<&str, MailError> {
    let to = to.trim();
    let valid = to.len() <= 254
        && to.matches('@').count() == 1
        && !to.starts_with('@')
        && !to.ends_with('@')
        && !to.chars().any(|c| c.is_whitespace() || c.is_control());
    if valid {
        Ok(to)
    } else {
        Err(MailError::Invalid { field: "to" })
    }
}

async fn list(
    state: &MailState,
    scope: ProviderScope,
    actor_id: Id,
    instance: &str,
    request_id: RequestIdRef<'_>,
) -> Result<Json<MailSendRouteList>, ApiError> {
    let fail = |error| problem(error, instance, request_id);
    let routes = state
        .mail
        .list_routes(scope, actor_id)
        .await
        .map_err(fail)?;
    let default = default_route(state, instance, request_id).await?;
    Ok(Json(MailSendRouteList {
        routes: routes
            .into_iter()
            .map(|route| {
                let own = match scope {
                    ProviderScope::Instance => true,
                    ProviderScope::Workspace(_) => route.workspace_id.is_some(),
                };
                view(state, route, own, default)
            })
            .collect(),
        workspace_providers_enabled: switch_on(state, instance, request_id).await?,
        direct_available: state
            .settings
            .hostname
            .as_deref()
            .is_some_and(|host| !host.trim().is_empty()),
        default_route_id: default,
    }))
}

async fn create(
    state: &MailState,
    scope: ProviderScope,
    actor_id: Id,
    body: CreateMailSendRouteBody,
    instance: &str,
    request_id: RequestIdRef<'_>,
) -> Result<(StatusCode, Json<MailSendRouteView>), ApiError> {
    let fail = |error| problem(error, instance, request_id);
    let mut given = body.secrets;
    let generated = generate_missing(body.kind, &mut given);
    let secrets = secrets_of(body.kind, &given).map_err(fail)?;
    let route = state
        .mail
        .create_route(
            scope,
            actor_id,
            NewRoute {
                kind: body.kind,
                name: body.name,
                provider_id: None,
                secrets,
                settings: body.settings,
            },
            state.settings.hostname.as_deref(),
            request_id_value(request_id),
        )
        .await
        .map_err(fail)?;
    let default = default_route(state, instance, request_id).await?;
    let mut created = view(state, route, true, default);
    created.generated = (!generated.is_empty()).then_some(generated);
    Ok((StatusCode::CREATED, Json(created)))
}

async fn create_for_provider(
    state: &MailState,
    scope: ProviderScope,
    actor_id: Id,
    provider: &str,
    body: CreateProviderSendRouteBody,
    instance: &str,
    request_id: RequestIdRef<'_>,
) -> Result<(StatusCode, Json<MailSendRouteView>), ApiError> {
    let fail = |error| problem(error, instance, request_id);
    let provider_id: Id = provider.parse().map_err(|_| fail(MailError::NotFound))?;
    let own_event_secret = body
        .secrets
        .get("event_secret")
        .is_some_and(|value| !value.trim().is_empty());
    let route = state
        .mail
        .create_route_for_provider(
            scope,
            actor_id,
            provider_id,
            ProviderRouteInput {
                name: body.name,
                secrets: body.secrets,
                settings: body.settings,
            },
            state.settings.hostname.as_deref(),
            request_id_value(request_id),
        )
        .await
        .map_err(fail)?;
    let default = default_route(state, instance, request_id).await?;
    let from_provider = route.kind == SendRouteKind::Cloudflare && !own_event_secret;
    let mut created = view(state, route, true, default);
    created.event_secret_from_provider = from_provider;
    Ok((StatusCode::CREATED, Json(created)))
}

async fn update(
    state: &MailState,
    scope: ProviderScope,
    actor_id: Id,
    route: &str,
    body: UpdateMailSendRouteBody,
    instance: &str,
    request_id: RequestIdRef<'_>,
) -> Result<Json<MailSendRouteView>, ApiError> {
    let fail = |error| problem(error, instance, request_id);
    let route_id: Id = route.parse().map_err(|_| fail(MailError::NotFound))?;
    let changed = state
        .mail
        .update_route(
            scope,
            actor_id,
            route_id,
            RouteChange {
                name: body.name,
                secrets: body.secrets,
                settings: body.settings,
                clear_login: body.clear_login,
            },
            state.settings.hostname.as_deref(),
            request_id_value(request_id),
        )
        .await
        .map_err(fail)?;
    let default = default_route(state, instance, request_id).await?;
    Ok(Json(view(state, changed, true, default)))
}

async fn delete(
    state: &MailState,
    scope: ProviderScope,
    actor_id: Id,
    route: &str,
    instance: &str,
    request_id: RequestIdRef<'_>,
) -> Result<StatusCode, ApiError> {
    let fail = |error| problem(error, instance, request_id);
    let route_id: Id = route.parse().map_err(|_| fail(MailError::NotFound))?;
    state
        .mail
        .delete_route(scope, actor_id, route_id, request_id_value(request_id))
        .await
        .map_err(fail)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn test(
    state: &MailState,
    scope: ProviderScope,
    actor_id: Id,
    route: &str,
    body: TestMailSendRouteBody,
    instance: &str,
    request_id: RequestIdRef<'_>,
) -> Result<Json<MailSendRouteTest>, ApiError> {
    let fail = |error| problem(error, instance, request_id);
    let route_id: Id = route.parse().map_err(|_| fail(MailError::NotFound))?;
    let to = address(&body.to).map_err(fail)?;
    let steps = state
        .mail
        .check_route(
            scope,
            actor_id,
            route_id,
            to,
            state.settings.hostname.as_deref(),
        )
        .await
        .map_err(fail)?;
    Ok(Json(MailSendRouteTest { steps }))
}

/// The instance send routes (root user).
#[utoipa::path(get, path = "/api/v1/admin/mail/send-routes", responses((status = 200, body = MailSendRouteList)))]
pub(crate) async fn list_instance_mail_send_routes(
    State(state): State<MailState>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<MailSendRouteList>, ApiError> {
    let request_id = request_id.as_ref();
    let (actor_id, instance) = require_root(&state, &headers, &uri, request_id).await?;
    list(
        &state,
        ProviderScope::Instance,
        actor_id,
        &instance,
        request_id,
    )
    .await
}

/// Adds an instance send route (root user).
#[utoipa::path(post, path = "/api/v1/admin/mail/send-routes", request_body = CreateMailSendRouteBody, responses((status = 201, body = MailSendRouteView)))]
pub(crate) async fn create_instance_mail_send_route(
    State(state): State<MailState>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<CreateMailSendRouteBody>,
) -> Result<(StatusCode, Json<MailSendRouteView>), ApiError> {
    let request_id = request_id.as_ref();
    let (actor_id, instance) = require_root(&state, &headers, &uri, request_id).await?;
    create(
        &state,
        ProviderScope::Instance,
        actor_id,
        body,
        &instance,
        request_id,
    )
    .await
}

#[utoipa::path(patch, path = "/api/v1/admin/mail/send-routes/{route_id}", params(("route_id" = String, Path)), request_body = UpdateMailSendRouteBody, responses((status = 200, body = MailSendRouteView)))]
pub(crate) async fn update_instance_mail_send_route(
    State(state): State<MailState>,
    Path(route): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<UpdateMailSendRouteBody>,
) -> Result<Json<MailSendRouteView>, ApiError> {
    let request_id = request_id.as_ref();
    let (actor_id, instance) = require_root(&state, &headers, &uri, request_id).await?;
    update(
        &state,
        ProviderScope::Instance,
        actor_id,
        &route,
        body,
        &instance,
        request_id,
    )
    .await
}

/// Removes an instance send route that no domain uses and that is not the default route.
#[utoipa::path(delete, path = "/api/v1/admin/mail/send-routes/{route_id}", params(("route_id" = String, Path)), responses((status = 204)))]
pub(crate) async fn delete_instance_mail_send_route(
    State(state): State<MailState>,
    Path(route): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<StatusCode, ApiError> {
    let request_id = request_id.as_ref();
    let (actor_id, instance) = require_root(&state, &headers, &uri, request_id).await?;
    delete(
        &state,
        ProviderScope::Instance,
        actor_id,
        &route,
        &instance,
        request_id,
    )
    .await
}

/// **Send test** of an instance route: the result of each step.
#[utoipa::path(post, path = "/api/v1/admin/mail/send-routes/{route_id}/test", params(("route_id" = String, Path)), request_body = TestMailSendRouteBody, responses((status = 200, body = MailSendRouteTest)))]
pub(crate) async fn test_instance_mail_send_route(
    State(state): State<MailState>,
    Path(route): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<TestMailSendRouteBody>,
) -> Result<Json<MailSendRouteTest>, ApiError> {
    let request_id = request_id.as_ref();
    let (actor_id, instance) = require_root(&state, &headers, &uri, request_id).await?;
    test(
        &state,
        ProviderScope::Instance,
        actor_id,
        &route,
        body,
        &instance,
        request_id,
    )
    .await
}

/// Sets the route that domains without one use (an instance route), or clears it.
#[utoipa::path(put, path = "/api/v1/admin/mail/send-default", request_body = SetMailSendDefaultBody, responses((status = 200, body = MailSendRouteList)))]
pub(crate) async fn set_mail_send_default(
    State(state): State<MailState>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<SetMailSendDefaultBody>,
) -> Result<Json<MailSendRouteList>, ApiError> {
    let request_id = request_id.as_ref();
    let (actor_id, instance) = require_root(&state, &headers, &uri, request_id).await?;
    let fail = |error| problem(error, &instance, request_id);
    let route_id = body
        .route_id
        .as_deref()
        .map(|id| {
            id.parse::<Id>()
                .map_err(|_| fail(MailError::Invalid { field: "route_id" }))
        })
        .transpose()?;
    state
        .mail
        .set_default_send_route(actor_id, route_id, request_id_value(request_id))
        .await
        .map_err(fail)?;
    list(
        &state,
        ProviderScope::Instance,
        actor_id,
        &instance,
        request_id,
    )
    .await
}

/// **Also send through this provider** (instance provider, root user): a route with the
/// provider's account, asking only for what is missing.
#[utoipa::path(post, path = "/api/v1/admin/mail/providers/{provider_id}/send-route", params(("provider_id" = String, Path)), request_body = CreateProviderSendRouteBody, responses((status = 201, body = MailSendRouteView)))]
pub(crate) async fn create_instance_mail_provider_send_route(
    State(state): State<MailState>,
    Path(provider): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<CreateProviderSendRouteBody>,
) -> Result<(StatusCode, Json<MailSendRouteView>), ApiError> {
    let request_id = request_id.as_ref();
    let (actor_id, instance) = require_root(&state, &headers, &uri, request_id).await?;
    create_for_provider(
        &state,
        ProviderScope::Instance,
        actor_id,
        &provider,
        body,
        &instance,
        request_id,
    )
    .await
}

/// The routes a workspace can use for its domains: the instance routes (read-only) and its own.
/// Owners and admins.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/mail/send-routes", params(("workspace_id" = String, Path)), responses((status = 200, body = MailSendRouteList)))]
pub(crate) async fn list_workspace_mail_send_routes(
    State(state): State<MailState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<MailSendRouteList>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    list(
        &state,
        ProviderScope::Workspace(call.workspace_id),
        call.actor_id,
        &call.instance,
        call.request_id,
    )
    .await
}

/// Adds a send route of the workspace.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/mail/send-routes", params(("workspace_id" = String, Path)), request_body = CreateMailSendRouteBody, responses((status = 201, body = MailSendRouteView)))]
pub(crate) async fn create_workspace_mail_send_route(
    State(state): State<MailState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<CreateMailSendRouteBody>,
) -> Result<(StatusCode, Json<MailSendRouteView>), ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    create(
        &state,
        ProviderScope::Workspace(call.workspace_id),
        call.actor_id,
        body,
        &call.instance,
        call.request_id,
    )
    .await
}

#[utoipa::path(patch, path = "/api/v1/workspaces/{workspace_id}/mail/send-routes/{route_id}", params(("workspace_id" = String, Path), ("route_id" = String, Path)), request_body = UpdateMailSendRouteBody, responses((status = 200, body = MailSendRouteView)))]
pub(crate) async fn update_workspace_mail_send_route(
    State(state): State<MailState>,
    Path((workspace, route)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<UpdateMailSendRouteBody>,
) -> Result<Json<MailSendRouteView>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    update(
        &state,
        ProviderScope::Workspace(call.workspace_id),
        call.actor_id,
        &route,
        body,
        &call.instance,
        call.request_id,
    )
    .await
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/mail/send-routes/{route_id}", params(("workspace_id" = String, Path), ("route_id" = String, Path)), responses((status = 204)))]
pub(crate) async fn delete_workspace_mail_send_route(
    State(state): State<MailState>,
    Path((workspace, route)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<StatusCode, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    delete(
        &state,
        ProviderScope::Workspace(call.workspace_id),
        call.actor_id,
        &route,
        &call.instance,
        call.request_id,
    )
    .await
}

/// **Send test** of a workspace route: the result of each step.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/mail/send-routes/{route_id}/test", params(("workspace_id" = String, Path), ("route_id" = String, Path)), request_body = TestMailSendRouteBody, responses((status = 200, body = MailSendRouteTest)))]
pub(crate) async fn test_workspace_mail_send_route(
    State(state): State<MailState>,
    Path((workspace, route)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<TestMailSendRouteBody>,
) -> Result<Json<MailSendRouteTest>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    test(
        &state,
        ProviderScope::Workspace(call.workspace_id),
        call.actor_id,
        &route,
        body,
        &call.instance,
        call.request_id,
    )
    .await
}

/// **Also send through this provider** (a provider of the workspace).
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/mail/providers/{provider_id}/send-route", params(("workspace_id" = String, Path), ("provider_id" = String, Path)), request_body = CreateProviderSendRouteBody, responses((status = 201, body = MailSendRouteView)))]
pub(crate) async fn create_workspace_mail_provider_send_route(
    State(state): State<MailState>,
    Path((workspace, provider)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<CreateProviderSendRouteBody>,
) -> Result<(StatusCode, Json<MailSendRouteView>), ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    create_for_provider(
        &state,
        ProviderScope::Workspace(call.workspace_id),
        call.actor_id,
        &provider,
        body,
        &call.instance,
        call.request_id,
    )
    .await
}
