//! The Admin mail page (instance admins, spec §8.1): the switch that lets workspaces add mail
//! domains, the health card (listener, certificate, client IP, PTR) and the test message. And a
//! workspace's failed receipts with **Retry** (owners and admins, spec §8.2).

use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode, Uri};
use orbit_platform::{Id, MailMode, RequestId, TimestampMillis};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::{Call, MailState, RequestIdExtension, problem};
use crate::mail_dns::{PtrStatus, check_ptr};
use crate::repositories::instance_settings::InstanceSettingsRepository;
use crate::repositories::mail::{
    FailedReceipt, HealthTest, HealthTestStatus, MailAuthSummary, MailError, ReceiptProblem,
    RejectReason, RejectedDelivery,
};
use crate::task_routes::{ApiError, ApiJson, authenticate_session, request_id_value};

type RequestIdRef<'a> = Option<&'a Extension<RequestId>>;

/// An authenticated instance admin: (user id, problem instance).
pub(super) async fn require_root(
    state: &MailState,
    headers: &HeaderMap,
    uri: &Uri,
    request_id: RequestIdRef<'_>,
) -> Result<(Id, String), ApiError> {
    let instance = uri.path().to_owned();
    let session = authenticate_session(
        &state.identity,
        state.cookie_mode,
        headers,
        &instance,
        request_id,
    )
    .await?;
    let admin = state
        .identity
        .is_installation_admin(session.user.id)
        .await
        .map_err(|_| {
            problem(
                MailError::Unavailable(sqlx::Error::PoolClosed),
                &instance,
                request_id,
            )
        })?;
    if !admin {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "installation_admin_required",
            "Installation administrator required",
            "Only an installation administrator may do this.",
            instance,
            request_id,
        ));
    }
    Ok((session.user.id, instance))
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MailAdminMode {
    /// A relay in front of Orbit receives the mail and checks it.
    Relay,
    /// Senders connect to Orbit and Orbit checks the mail.
    Direct,
}

/// The certificate the listener offers for `STARTTLS`.
#[derive(Serialize, ToSchema)]
pub(crate) struct MailCertView {
    /// DNS names of the certificate, lower case.
    names: Vec<String>,
    #[schema(value_type = String, format = DateTime)]
    expires_at: TimestampMillis,
    /// One of `names` matches the mail hostname.
    covers_hostname: bool,
    /// It expires within 7 days, or has expired.
    expires_soon: bool,
}

/// This server's SMTP listener. With several servers, each one reports its own.
#[derive(Serialize, ToSchema)]
pub(crate) struct MailListenerView {
    /// It accepts connections now.
    running: bool,
    /// The bound address while running.
    address: Option<String>,
    /// The client address of the last connection (after PROXY v2). A Docker gateway here means
    /// the real client address is lost.
    last_peer_ip: Option<String>,
    /// Connections that came from a Docker bridge gateway since the start.
    bridge_peers: u64,
    /// `null` without `ORBIT_MAIL_TLS_CERT`.
    cert: Option<MailCertView>,
}

/// A verified domain for the test message.
#[derive(Serialize, ToSchema)]
pub(crate) struct MailTestDomain {
    #[schema(value_type = String)]
    id: Id,
    name: String,
    #[schema(value_type = String)]
    workspace_id: Id,
}

/// The Admin mail page.
#[derive(Serialize, ToSchema)]
pub(crate) struct MailAdmin {
    /// Workspaces may add mail domains.
    domains_enabled: bool,
    /// `ORBIT_MAIL_ENABLED` on this server.
    enabled: bool,
    /// `ORBIT_MAIL_HOSTNAME`.
    hostname: Option<String>,
    mode: MailAdminMode,
    listener: MailListenerView,
    /// Looked up on each request.
    ptr: PtrStatus,
    /// The verified domains of all workspaces, by name.
    test_domains: Vec<MailTestDomain>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SetMailAdminBody {
    domains_enabled: bool,
}

async fn admin_view(
    state: &MailState,
    instance: &str,
    request_id: RequestIdRef<'_>,
) -> Result<Json<MailAdmin>, ApiError> {
    let fail = |error: MailError| problem(error, instance, request_id);
    let domains_enabled = InstanceSettingsRepository::new(state.identity.database().clone())
        .mail_domains_enabled()
        .await
        .map_err(|error| fail(MailError::Unavailable(error)))?;
    let test_domains = state.mail.test_domains().await.map_err(fail)?;
    let hostname = state.settings.hostname.clone();
    let now_secs = TimestampMillis::now().as_millis() / 1000;
    let listener = &state.listener;
    let cert = listener.cert().map(|cert| MailCertView {
        covers_hostname: hostname.as_deref().is_some_and(|host| cert.covers(host)),
        expires_soon: cert.expires_soon(now_secs),
        expires_at: TimestampMillis::from_millis(cert.not_after.saturating_mul(1000)),
        names: cert.names,
    });
    let ptr = check_ptr(state.dns.as_ref(), hostname.as_deref()).await;
    Ok(Json(MailAdmin {
        domains_enabled,
        enabled: state.settings.enabled,
        mode: match state.settings.mode {
            MailMode::Relay => MailAdminMode::Relay,
            MailMode::Direct => MailAdminMode::Direct,
        },
        listener: MailListenerView {
            running: listener.running(),
            address: listener.local_addr().map(|address| address.to_string()),
            last_peer_ip: listener.last_peer_ip().map(|ip| ip.to_string()),
            bridge_peers: listener.bridge_peers(),
            cert,
        },
        ptr,
        hostname,
        test_domains: test_domains
            .into_iter()
            .map(|domain| MailTestDomain {
                id: domain.id,
                name: domain.name,
                workspace_id: domain.workspace_id,
            })
            .collect(),
    }))
}

/// The Admin mail page: the switch, this server's mail settings and the health card.
#[utoipa::path(get, path = "/api/v1/admin/mail", responses((status = 200, body = MailAdmin)))]
pub(crate) async fn get_mail_admin(
    State(state): State<MailState>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<MailAdmin>, ApiError> {
    let request_id = request_id.as_ref();
    let (_, instance) = require_root(&state, &headers, &uri, request_id).await?;
    admin_view(&state, &instance, request_id).await
}

/// Turns on or off whether workspaces may add mail domains (audit `instance.mail_updated`).
#[utoipa::path(put, path = "/api/v1/admin/mail", request_body = SetMailAdminBody, responses((status = 200, body = MailAdmin)))]
pub(crate) async fn set_mail_admin(
    State(state): State<MailState>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<SetMailAdminBody>,
) -> Result<Json<MailAdmin>, ApiError> {
    let request_id = request_id.as_ref();
    let (actor_id, instance) = require_root(&state, &headers, &uri, request_id).await?;
    InstanceSettingsRepository::new(state.identity.database().clone())
        .set_mail_domains_enabled(
            actor_id,
            body.domains_enabled,
            request_id_value(request_id),
            TimestampMillis::now(),
        )
        .await
        .map_err(|error| problem(MailError::Unavailable(error), &instance, request_id))?;
    admin_view(&state, &instance, request_id).await
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreateMailTestBody {
    /// A verified domain of any workspace.
    domain_id: String,
}

/// A one-time address for the test message.
#[derive(Serialize, ToSchema)]
pub(crate) struct MailTestAddress {
    #[schema(value_type = String)]
    id: Id,
    /// `check-<8 random characters>@<domain>`.
    address: String,
    /// One hour after it was made.
    #[schema(value_type = String, format = DateTime)]
    expires_at: TimestampMillis,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MailTestStatus {
    Waiting,
    Received,
    /// The hour passed without a message.
    Expired,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct MailTlsView {
    /// `TLSv1.2` or `TLSv1.3`.
    version: String,
    cipher: String,
}

/// What the test message showed.
#[derive(Serialize, ToSchema)]
pub(crate) struct MailTestResult {
    #[schema(value_type = String)]
    id: Id,
    address: String,
    status: MailTestStatus,
    #[schema(value_type = String, format = DateTime)]
    expires_at: TimestampMillis,
    #[schema(value_type = Option<String>, format = DateTime)]
    received_at: Option<TimestampMillis>,
    /// The sender's address as the listener saw it (after PROXY v2).
    peer_ip: Option<String>,
    helo: Option<String>,
    mail_from: Option<String>,
    /// `null` when the message came without `STARTTLS` (or before it arrived).
    tls: Option<MailTlsView>,
    auth: Option<MailAuthSummary>,
}

fn test_result(test: HealthTest) -> MailTestResult {
    let result = test.result;
    MailTestResult {
        id: test.id,
        address: test.address,
        status: match test.status {
            HealthTestStatus::Waiting => MailTestStatus::Waiting,
            HealthTestStatus::Received => MailTestStatus::Received,
            HealthTestStatus::Expired => MailTestStatus::Expired,
        },
        expires_at: test.expires_at,
        received_at: test.received_at,
        peer_ip: result.as_ref().map(|result| result.peer_ip.clone()),
        helo: result.as_ref().map(|result| result.helo.clone()),
        mail_from: result.as_ref().and_then(|result| result.mail_from.clone()),
        tls: result.as_ref().and_then(|result| {
            result.tls.as_ref().map(|tls| MailTlsView {
                version: tls.version.clone(),
                cipher: tls.cipher.clone(),
            })
        }),
        auth: result.map(|result| MailAuthSummary::from_outcome(result.auth)),
    }
}

/// Makes a one-time test address on a verified domain, valid for one hour. Mail to it is
/// recorded for the health card and delivered to no mailbox.
#[utoipa::path(post, path = "/api/v1/admin/mail/test-address", request_body = CreateMailTestBody, responses((status = 201, body = MailTestAddress)))]
pub(crate) async fn create_mail_test_address(
    State(state): State<MailState>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<CreateMailTestBody>,
) -> Result<(StatusCode, Json<MailTestAddress>), ApiError> {
    let request_id = request_id.as_ref();
    let (_, instance) = require_root(&state, &headers, &uri, request_id).await?;
    let fail = |error: MailError| problem(error, &instance, request_id);
    let domain_id: Id = body
        .domain_id
        .parse()
        .map_err(|_| fail(MailError::Invalid { field: "domain_id" }))?;
    let test = state
        .mail
        .create_health_test(domain_id)
        .await
        .map_err(fail)?;
    Ok((
        StatusCode::CREATED,
        Json(MailTestAddress {
            id: test.id,
            address: test.address,
            expires_at: test.expires_at,
        }),
    ))
}

/// The test message's result: `waiting` until it arrives.
#[utoipa::path(get, path = "/api/v1/admin/mail/test-address/{test_id}", params(("test_id" = String, Path)), responses((status = 200, body = MailTestResult)))]
pub(crate) async fn get_mail_test_address(
    State(state): State<MailState>,
    Path(test_id): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<MailTestResult>, ApiError> {
    let request_id = request_id.as_ref();
    let (_, instance) = require_root(&state, &headers, &uri, request_id).await?;
    let fail = |error: MailError| problem(error, &instance, request_id);
    let test_id: Id = test_id.parse().map_err(|_| fail(MailError::NotFound))?;
    let test = state.mail.health_test(test_id).await.map_err(fail)?;
    Ok(Json(test_result(test)))
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MailReceiptStatus {
    /// Every ingest attempt failed; `error` says why.
    Failed,
    /// Waiting for over an hour without an ingest job (the server stopped during the last
    /// attempt).
    Stuck,
}

/// A received message that did not reach its mailboxes. The raw message is kept.
#[derive(Serialize, ToSchema)]
pub(crate) struct MailFailedReceipt {
    #[schema(value_type = String)]
    id: Id,
    status: MailReceiptStatus,
    /// The envelope sender; `null` for the null sender `<>`.
    mail_from: Option<String>,
    /// Bytes of the raw message.
    size: i64,
    /// The last ingest error.
    error: Option<String>,
    attempts: i64,
    #[schema(value_type = String, format = DateTime)]
    received_at: TimestampMillis,
    #[schema(value_type = String, format = DateTime)]
    updated_at: TimestampMillis,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct MailFailedReceipts {
    /// All failed and stuck receipts; `receipts` holds the newest 100.
    count: i64,
    receipts: Vec<MailFailedReceipt>,
    /// Mail a provider accepted that Orbit could not deliver (no body was kept), newest 100.
    rejected: Vec<MailRejectedDelivery>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MailRejectReason {
    /// No mailbox has the address.
    UnknownAddress,
    /// The domain receives through another route.
    WrongRoute,
    /// The domain is suspended, disabled or not verified.
    DomainUnavailable,
    MailboxSuspended,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct MailRejectedDelivery {
    provider_name: String,
    mail_from: Option<String>,
    recipient: String,
    reason: MailRejectReason,
    #[schema(value_type = String, format = DateTime)]
    received_at: TimestampMillis,
}

fn rejected_view(rejected: RejectedDelivery) -> MailRejectedDelivery {
    MailRejectedDelivery {
        provider_name: rejected.provider_name,
        mail_from: rejected.mail_from,
        recipient: rejected.recipient,
        reason: match rejected.reason {
            RejectReason::UnknownAddress => MailRejectReason::UnknownAddress,
            RejectReason::WrongRoute => MailRejectReason::WrongRoute,
            RejectReason::DomainUnavailable => MailRejectReason::DomainUnavailable,
            RejectReason::MailboxSuspended => MailRejectReason::MailboxSuspended,
        },
        received_at: rejected.received_at,
    }
}

fn receipt_view(receipt: FailedReceipt) -> MailFailedReceipt {
    MailFailedReceipt {
        id: receipt.id,
        status: match receipt.problem {
            ReceiptProblem::Failed => MailReceiptStatus::Failed,
            ReceiptProblem::Stuck => MailReceiptStatus::Stuck,
        },
        mail_from: receipt.mail_from,
        size: receipt.size,
        error: receipt.error,
        attempts: receipt.attempts,
        received_at: receipt.received_at,
        updated_at: receipt.updated_at,
    }
}

/// **Failed messages**: received messages that did not reach their mailboxes, newest first.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/mail/receipts", params(("workspace_id" = String, Path)), responses((status = 200, body = MailFailedReceipts)))]
pub(crate) async fn list_failed_mail_receipts(
    State(state): State<MailState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<MailFailedReceipts>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let list = state
        .mail
        .failed_receipts(call.workspace_id, call.actor_id)
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(MailFailedReceipts {
        count: list.count,
        receipts: list.receipts.into_iter().map(receipt_view).collect(),
        rejected: list.rejected.into_iter().map(rejected_view).collect(),
    }))
}

/// **Retry**: ingests a failed or stuck message again.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/mail/receipts/{receipt_id}/retry", params(("workspace_id" = String, Path), ("receipt_id" = String, Path)), responses((status = 204)))]
pub(crate) async fn retry_mail_receipt(
    State(state): State<MailState>,
    Path((workspace, receipt)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<StatusCode, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let receipt_id = call.id(&receipt)?;
    state
        .mail
        .retry_receipt(call.workspace_id, call.actor_id, receipt_id, &state.jobs)
        .await
        .map_err(|error| call.problem(error))?;
    Ok(StatusCode::NO_CONTENT)
}
