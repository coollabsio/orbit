//! The listener's certificate in Admin → Mail (root only), and the public HTTP-01 challenge answer.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use orbit_mail::{CertInfo, pem_cert_info};
use orbit_platform::{RequestId, TimestampMillis};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::admin::require_root;
use super::{MailState, RequestIdExtension, problem};

use crate::mail_certificate::{CertificateError, CertificateSource, MailCertificates, renew_after};
use crate::repositories::mail::{ChallengeMethod, IssuedCert, MailError, PendingOrder};
use crate::task_routes::{ApiError, ApiJson, request_id_value};

type RequestIdRef<'a> = Option<&'a Extension<RequestId>>;

#[derive(Clone, Copy, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MailCertificateMethod {
    /// HTTP-01: the CA fetches a file from `http://<hostname>/.well-known/acme-challenge/`.
    Http,
    /// DNS-01: a TXT record at `_acme-challenge.<hostname>`.
    Dns,
}

impl From<ChallengeMethod> for MailCertificateMethod {
    fn from(method: ChallengeMethod) -> Self {
        match method {
            ChallengeMethod::Http => Self::Http,
            ChallengeMethod::Dns => Self::Dns,
        }
    }
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MailCertificateSourceView {
    /// `ORBIT_MAIL_TLS_CERT` / `ORBIT_MAIL_TLS_KEY`: read only here.
    Environment,
    /// Orbit got it from the ACME CA.
    Orbit,
    None,
}

/// The certificate in use.
#[derive(Serialize, ToSchema)]
pub(crate) struct MailCertificateActive {
    /// DNS names, lower case.
    names: Vec<String>,
    #[schema(value_type = String, format = DateTime)]
    not_after: TimestampMillis,
    /// One of `names` matches `ORBIT_MAIL_HOSTNAME`.
    covers_hostname: bool,
    /// `null` for a certificate from files.
    #[schema(value_type = Option<String>, format = DateTime)]
    issued_at: Option<TimestampMillis>,
    method: Option<MailCertificateMethod>,
}

/// An order that waits for its challenge.
#[derive(Serialize, ToSchema)]
pub(crate) struct MailCertificatePending {
    method: MailCertificateMethod,
    /// DNS-01: the TXT record to add.
    dns_name: Option<String>,
    dns_value: Option<String>,
    #[schema(value_type = String, format = DateTime)]
    started_at: TimestampMillis,
}

/// The SMTP listener's certificate.
#[derive(Serialize, ToSchema)]
pub(crate) struct MailCertificate {
    source: MailCertificateSourceView,
    /// `ORBIT_MAIL_HOSTNAME`: the name the certificate is for.
    hostname: Option<String>,
    active: Option<MailCertificateActive>,
    pending: Option<MailCertificatePending>,
    /// Why the last request or renewal failed; cleared by the next success.
    last_error: Option<String>,
    #[schema(value_type = Option<String>, format = DateTime)]
    last_attempt_at: Option<TimestampMillis>,
    /// When Orbit renews it (HTTP-01, daily check). `null` unless Orbit got it.
    #[schema(value_type = Option<String>, format = DateTime)]
    renew_after: Option<TimestampMillis>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct StartMailCertificateBody {
    method: MailCertificateMethod,
}

fn seconds(secs: i64) -> TimestampMillis {
    TimestampMillis::from_millis(secs.saturating_mul(1000))
}

fn active_view(
    info: CertInfo,
    hostname: Option<&str>,
    issued: Option<&IssuedCert>,
) -> MailCertificateActive {
    MailCertificateActive {
        covers_hostname: hostname.is_some_and(|host| info.covers(host)),
        not_after: seconds(info.not_after),
        names: info.names,
        issued_at: issued.map(|issued| issued.issued_at),
        method: issued.map(|issued| issued.method.into()),
    }
}

fn certificates<'a>(
    state: &'a MailState,
    instance: &str,
    request_id: RequestIdRef<'_>,
) -> Result<&'a Arc<MailCertificates>, ApiError> {
    state
        .certificates
        .as_ref()
        .ok_or_else(|| problem(MailError::NotFound, instance, request_id))
}

async fn view(
    state: &MailState,
    instance: &str,
    request_id: RequestIdRef<'_>,
) -> Result<Json<MailCertificate>, ApiError> {
    let certificates = certificates(state, instance, request_id)?;
    let row = certificates
        .state()
        .await
        .map_err(|error| problem(error, instance, request_id))?;
    let hostname = certificates.hostname().map(str::to_owned);
    let source = certificates.source(&row);
    let active = match source {
        CertificateSource::Environment => state
            .listener
            .cert()
            .map(|info| active_view(info, hostname.as_deref(), None)),
        CertificateSource::Orbit | CertificateSource::None => {
            row.issued.as_ref().and_then(|issued| {
                pem_cert_info(issued.cert_pem.as_bytes())
                    .ok()
                    .map(|info| active_view(info, hostname.as_deref(), Some(issued)))
            })
        }
    };
    let pending = row.pending.as_ref().map(|order| {
        let (dns_name, dns_value) = match order {
            PendingOrder::Dns { name, value, .. } => (Some(name.clone()), Some(value.clone())),
            PendingOrder::Http { .. } => (None, None),
        };
        MailCertificatePending {
            method: order.method().into(),
            dns_name,
            dns_value,
            started_at: order.started_at(),
        }
    });
    Ok(Json(MailCertificate {
        source: match source {
            CertificateSource::Environment => MailCertificateSourceView::Environment,
            CertificateSource::Orbit => MailCertificateSourceView::Orbit,
            CertificateSource::None => MailCertificateSourceView::None,
        },
        renew_after: match source {
            CertificateSource::Orbit => row
                .issued
                .as_ref()
                .map(|issued| seconds(renew_after(issued.not_before, issued.not_after))),
            _ => None,
        },
        hostname,
        active,
        pending,
        last_error: row.last_error,
        last_attempt_at: row.last_attempt_at,
    }))
}

fn refused(error: CertificateError, instance: &str, request_id: RequestIdRef<'_>) -> ApiError {
    let (status, code, detail) = match error {
        CertificateError::Mail(error) => return problem(error, instance, request_id),
        CertificateError::FromEnvironment => (
            StatusCode::CONFLICT,
            "certificate_from_environment",
            "The certificate comes from ORBIT_MAIL_TLS_CERT and ORBIT_MAIL_TLS_KEY. Remove them to let Orbit get one.",
        ),
        CertificateError::HostnameRequired => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "hostname_required",
            "A certificate needs the mail server's host name (ORBIT_MAIL_HOSTNAME).",
        ),
        CertificateError::NoDnsOrder => (
            StatusCode::CONFLICT,
            "no_dns_order",
            "No DNS check waits. Start one with the DNS method first.",
        ),
        CertificateError::DnsRecordNotFound => (
            StatusCode::CONFLICT,
            "dns_record_not_found",
            "The TXT record is not visible yet. Add it, wait a few minutes for DNS, then verify again.",
        ),
        CertificateError::Busy => (
            StatusCode::CONFLICT,
            "certificate_busy",
            "A certificate request runs already. Wait for it to end.",
        ),
    };
    ApiError::new(
        status,
        code,
        "Certificate request refused",
        detail,
        instance,
        request_id,
    )
}

/// The listener's certificate: where it comes from, the pending order and the last result.
#[utoipa::path(get, path = "/api/v1/admin/mail/certificate", responses((status = 200, body = MailCertificate)))]
pub(crate) async fn get_mail_certificate(
    State(state): State<MailState>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<MailCertificate>, ApiError> {
    let request_id = request_id.as_ref();
    let (_, instance) = require_root(&state, &headers, &uri, request_id).await?;
    view(&state, &instance, request_id).await
}

/// Gets a certificate for `ORBIT_MAIL_HOSTNAME` from the ACME CA. `http` validates and issues in this request (up
/// to about a minute); `dns` answers with the TXT record to add, then call verify. A CA failure is in `last_error`.
#[utoipa::path(post, path = "/api/v1/admin/mail/certificate", request_body = StartMailCertificateBody, responses((status = 200, body = MailCertificate), (status = 409, description = "certificate_from_environment or certificate_busy"), (status = 422, description = "hostname_required")))]
pub(crate) async fn start_mail_certificate(
    State(state): State<MailState>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<StartMailCertificateBody>,
) -> Result<Json<MailCertificate>, ApiError> {
    let request_id = request_id.as_ref();
    let (actor_id, instance) = require_root(&state, &headers, &uri, request_id).await?;
    let method = match body.method {
        MailCertificateMethod::Http => ChallengeMethod::Http,
        MailCertificateMethod::Dns => ChallengeMethod::Dns,
    };
    certificates(&state, &instance, request_id)?
        .start(method, Some(actor_id), request_id_value(request_id))
        .await
        .map_err(|error| refused(error, &instance, request_id))?;
    view(&state, &instance, request_id).await
}

/// Checks the TXT record of the pending DNS order, then lets the CA validate it and stores the certificate.
#[utoipa::path(post, path = "/api/v1/admin/mail/certificate/verify", responses((status = 200, body = MailCertificate), (status = 409, description = "dns_record_not_found, no_dns_order or certificate_busy")))]
pub(crate) async fn verify_mail_certificate(
    State(state): State<MailState>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<MailCertificate>, ApiError> {
    let request_id = request_id.as_ref();
    let (actor_id, instance) = require_root(&state, &headers, &uri, request_id).await?;
    certificates(&state, &instance, request_id)?
        .verify_dns(Some(actor_id), request_id_value(request_id))
        .await
        .map_err(|error| refused(error, &instance, request_id))?;
    view(&state, &instance, request_id).await
}

/// Cancels the pending order.
#[utoipa::path(delete, path = "/api/v1/admin/mail/certificate/order", responses((status = 200, body = MailCertificate)))]
pub(crate) async fn cancel_mail_certificate_order(
    State(state): State<MailState>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<MailCertificate>, ApiError> {
    let request_id = request_id.as_ref();
    let (_, instance) = require_root(&state, &headers, &uri, request_id).await?;
    certificates(&state, &instance, request_id)?
        .cancel()
        .await
        .map_err(|error| refused(error, &instance, request_id))?;
    view(&state, &instance, request_id).await
}

/// The HTTP-01 answer for the pending order's token; 404 for any other token. Public, over plain HTTP.
pub(crate) async fn acme_http_challenge(
    State(state): State<MailState>,
    Path(token): Path<String>,
) -> Response {
    let Some(certificates) = state.certificates.as_deref() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match certificates.http_challenge(&token).await {
        Ok(Some(key_authorization)) => (
            [(
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/plain; charset=utf-8"),
            )],
            key_authorization,
        )
            .into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => {
            tracing::warn!(%error, "ACME challenge lookup failed");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}
