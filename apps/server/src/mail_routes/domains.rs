//! Mail domains: list, add, get, remove, **Check now**, catch-all, disable and enable, and the
//! instance admin's move to another workspace.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, Uri};
use orbit_platform::{Id, TimestampMillis};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::{Call, MailState, RequestIdExtension, problem};
use crate::mail_dns::{RecordKind, expected_records, txt_strings};
use crate::repositories::mail::{Domain, DomainStatus, RecordStatus};
use crate::task_routes::{ApiError, ApiJson, request_id_value};

/// A mail domain with the records to publish and the result of its last DNS check.
#[derive(Serialize, ToSchema)]
pub(crate) struct MailDomain {
    #[schema(value_type = String)]
    id: Id,
    /// Normalized: lower case, IDNA A-labels (`xn--…`), no trailing dot.
    name: String,
    status: DomainStatus,
    #[schema(value_type = Option<String>, format = DateTime)]
    verified_at: Option<TimestampMillis>,
    #[schema(value_type = Option<String>, format = DateTime)]
    last_verified_at: Option<TimestampMillis>,
    /// When the TXT record of a verified domain was found missing.
    #[schema(value_type = Option<String>, format = DateTime)]
    suspended_at: Option<TimestampMillis>,
    /// Suspended domains only: mail is refused after this.
    #[schema(value_type = Option<String>, format = DateTime)]
    grace_ends_at: Option<TimestampMillis>,
    /// The last check that reached the DNS; `null` before the first one.
    #[schema(value_type = Option<String>, format = DateTime)]
    checked_at: Option<TimestampMillis>,
    #[schema(value_type = Option<String>)]
    catch_all_mailbox_id: Option<Id>,
    /// Addresses on the domain. A domain is removed only without any.
    address_count: i64,
    /// The `p=` of the domain's DMARC record at the last check.
    dmarc_policy: Option<String>,
    /// The records to publish, each with its status at the last check.
    records: Vec<MailDnsRecord>,
    #[schema(value_type = String, format = DateTime)]
    created_at: TimestampMillis,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MailDnsRecordKind {
    /// TXT `_orbit-verify.<domain>`: proves the domain belongs to the workspace.
    Verify,
    /// Mail for the domain comes to Orbit (direct mode) or to the relay in front of it.
    Mx,
    Dkim,
    Spf,
    Dmarc,
}

#[derive(Serialize, ToSchema)]
pub(crate) enum MailDnsRecordType {
    #[serde(rename = "TXT")]
    Txt,
    #[serde(rename = "MX")]
    Mx,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct MailDnsRecord {
    kind: MailDnsRecordKind,
    #[serde(rename = "type")]
    record_type: MailDnsRecordType,
    /// The full name, without a trailing dot.
    name: String,
    /// The TXT text, or the MX target host.
    value: String,
    /// TXT only: `value` cut into strings of at most 255 bytes. A DKIM key is longer than one
    /// string; zone files write them as `"…" "…"`, most DNS panels take `value` as it is.
    txt_strings: Vec<String>,
    /// MX only.
    priority: Option<u16>,
    /// `unknown` before the first check, or when the last lookup timed out.
    status: RecordStatus,
}

fn view(domain: Domain, hostname: Option<&str>) -> MailDomain {
    let report = domain.report.clone();
    let records = expected_records(&domain, hostname)
        .into_iter()
        .map(|record| {
            let status =
                report
                    .as_ref()
                    .map_or(RecordStatus::Unknown, |report| match record.kind {
                        RecordKind::Verify => report.verify.clone(),
                        RecordKind::Mx => report.mx.clone(),
                        RecordKind::Dkim => report.dkim.clone(),
                        RecordKind::Spf => report.spf.clone(),
                        RecordKind::Dmarc => report.dmarc.clone(),
                    });
            let txt = record.record_type == "TXT";
            MailDnsRecord {
                kind: match record.kind {
                    RecordKind::Verify => MailDnsRecordKind::Verify,
                    RecordKind::Mx => MailDnsRecordKind::Mx,
                    RecordKind::Dkim => MailDnsRecordKind::Dkim,
                    RecordKind::Spf => MailDnsRecordKind::Spf,
                    RecordKind::Dmarc => MailDnsRecordKind::Dmarc,
                },
                record_type: if txt {
                    MailDnsRecordType::Txt
                } else {
                    MailDnsRecordType::Mx
                },
                txt_strings: if txt {
                    txt_strings(&record.value)
                } else {
                    Vec::new()
                },
                name: record.name,
                value: record.value,
                priority: record.priority,
                status,
            }
        })
        .collect();
    MailDomain {
        grace_ends_at: domain.grace_ends_at(),
        id: domain.id,
        name: domain.name,
        status: domain.status,
        verified_at: domain.verified_at,
        last_verified_at: domain.last_verified_at,
        suspended_at: domain.suspended_at,
        checked_at: domain.checked_at,
        catch_all_mailbox_id: domain.catch_all_mailbox_id,
        address_count: domain.address_count,
        dmarc_policy: report.and_then(|report| report.dmarc_policy),
        records,
        created_at: domain.created_at,
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreateMailDomainBody {
    /// A domain such as `example.com`; Unicode names are stored as A-labels.
    name: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct MailDomainCatchAllBody {
    /// A mailbox of the workspace, or `null` to refuse unknown addresses.
    mailbox_id: Option<String>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct MailDomainEnabledBody {
    /// `false` refuses all mail for the domain.
    enabled: bool,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct MoveMailDomainBody {
    workspace_id: String,
}

/// The mail domains of the workspace. Owners and admins.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/mail/domains", params(("workspace_id" = String, Path)), responses((status = 200, body = Vec<MailDomain>)))]
pub(crate) async fn list_mail_domains(
    State(state): State<MailState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<Vec<MailDomain>>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let domains = state
        .mail
        .list_domains(call.workspace_id, call.actor_id)
        .await
        .map_err(|error| call.problem(error))?;
    let hostname = state.settings.hostname.as_deref();
    Ok(Json(
        domains
            .into_iter()
            .map(|domain| view(domain, hostname))
            .collect(),
    ))
}

/// Adds a domain: Orbit makes a verification token and a DKIM key, and checks the DNS every
/// 10 minutes for 72 hours, then once a day. Refused when the domain, a parent or a subdomain
/// belongs to another workspace, or when the instance does not allow mail domains.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/mail/domains", params(("workspace_id" = String, Path)), request_body = CreateMailDomainBody, responses((status = 201, body = MailDomain)))]
pub(crate) async fn create_mail_domain(
    State(state): State<MailState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<CreateMailDomainBody>,
) -> Result<(StatusCode, Json<MailDomain>), ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let domain = state
        .mail
        .create_domain(
            call.workspace_id,
            call.actor_id,
            &body.name,
            call.request_id(),
        )
        .await
        .map_err(|error| call.problem(error))?;
    Ok((
        StatusCode::CREATED,
        Json(view(domain, state.settings.hostname.as_deref())),
    ))
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/mail/domains/{domain_id}", params(("workspace_id" = String, Path), ("domain_id" = String, Path)), responses((status = 200, body = MailDomain)))]
pub(crate) async fn get_mail_domain(
    State(state): State<MailState>,
    Path((workspace, domain)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<MailDomain>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let domain_id = call.id(&domain)?;
    let domain = state
        .mail
        .get_domain(call.workspace_id, call.actor_id, domain_id)
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(view(domain, state.settings.hostname.as_deref())))
}

/// Removes a domain that has no addresses. Mail it received stays in its mailboxes.
#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/mail/domains/{domain_id}", params(("workspace_id" = String, Path), ("domain_id" = String, Path)), responses((status = 204)))]
pub(crate) async fn delete_mail_domain(
    State(state): State<MailState>,
    Path((workspace, domain)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<StatusCode, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let domain_id = call.id(&domain)?;
    state
        .mail
        .delete_domain(
            call.workspace_id,
            call.actor_id,
            domain_id,
            call.request_id(),
        )
        .await
        .map_err(|error| call.problem(error))?;
    Ok(StatusCode::NO_CONTENT)
}

/// **Check now**: queues a DNS check of the domain. A check that is waiting already is not
/// queued twice. Read the domain again for the result.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/mail/domains/{domain_id}/check", params(("workspace_id" = String, Path), ("domain_id" = String, Path)), responses((status = 202)))]
pub(crate) async fn check_mail_domain(
    State(state): State<MailState>,
    Path((workspace, domain)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<StatusCode, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let domain_id = call.id(&domain)?;
    state
        .mail
        .request_dns_check(call.workspace_id, call.actor_id, domain_id, &state.jobs)
        .await
        .map_err(|error| call.problem(error))?;
    Ok(StatusCode::ACCEPTED)
}

/// Sets the mailbox that receives mail for unknown addresses of the domain, or clears it.
#[utoipa::path(put, path = "/api/v1/workspaces/{workspace_id}/mail/domains/{domain_id}/catch-all", params(("workspace_id" = String, Path), ("domain_id" = String, Path)), request_body = MailDomainCatchAllBody, responses((status = 200, body = MailDomain)))]
pub(crate) async fn set_mail_domain_catch_all(
    State(state): State<MailState>,
    Path((workspace, domain)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<MailDomainCatchAllBody>,
) -> Result<Json<MailDomain>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let domain_id = call.id(&domain)?;
    let mailbox_id = body
        .mailbox_id
        .as_deref()
        .map(|id| call.body_id(id, "mailbox_id"))
        .transpose()?;
    let domain = state
        .mail
        .set_catch_all(
            call.workspace_id,
            call.actor_id,
            domain_id,
            mailbox_id,
            call.request_id(),
        )
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(view(domain, state.settings.hostname.as_deref())))
}

/// Disables the domain (all its mail is refused) or enables it again.
#[utoipa::path(put, path = "/api/v1/workspaces/{workspace_id}/mail/domains/{domain_id}/enabled", params(("workspace_id" = String, Path), ("domain_id" = String, Path)), request_body = MailDomainEnabledBody, responses((status = 200, body = MailDomain)))]
pub(crate) async fn set_mail_domain_enabled(
    State(state): State<MailState>,
    Path((workspace, domain)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<MailDomainEnabledBody>,
) -> Result<Json<MailDomain>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let domain_id = call.id(&domain)?;
    let domain = state
        .mail
        .set_domain_enabled(
            call.workspace_id,
            call.actor_id,
            domain_id,
            body.enabled,
            call.request_id(),
        )
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(view(domain, state.settings.hostname.as_deref())))
}

/// Moves a domain to another workspace. Instance admins only, and only for a domain without
/// addresses: its mailboxes stay in the old workspace. The catch-all is cleared.
#[utoipa::path(put, path = "/api/v1/admin/mail/domains/{name}/workspace", params(("name" = String, Path)), request_body = MoveMailDomainBody, responses((status = 204)))]
pub(crate) async fn move_mail_domain(
    State(state): State<MailState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<MoveMailDomainBody>,
) -> Result<StatusCode, ApiError> {
    let request_id = request_id.as_ref();
    let (actor_id, instance) =
        super::admin::require_root(&state, &headers, &uri, request_id).await?;
    let workspace_id: Id = body.workspace_id.parse().map_err(|_| {
        problem(
            crate::repositories::mail::MailError::Invalid {
                field: "workspace_id",
            },
            &instance,
            request_id,
        )
    })?;
    state
        .mail
        .move_domain(&name, workspace_id, actor_id, request_id_value(request_id))
        .await
        .map_err(|error| problem(error, &instance, request_id))?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repositories::mail::{DkimPublic, DnsReport};

    #[test]
    fn the_dkim_record_comes_in_255_byte_strings() {
        let domain = Domain {
            id: Id::new_v7(),
            workspace_id: Id::new_v7(),
            name: "example.com".to_owned(),
            status: DomainStatus::Pending,
            verify_token: "token".to_owned(),
            verified_at: None,
            last_verified_at: None,
            suspended_at: None,
            catch_all_mailbox_id: None,
            report: Some(DnsReport::unknown()),
            checked_at: None,
            created_at: TimestampMillis::from_millis(0),
            updated_at: TimestampMillis::from_millis(0),
            dkim: Some(DkimPublic {
                selector: "o2026a".to_owned(),
                public_key: "A".repeat(392),
                status: "pending".to_owned(),
            }),
            address_count: 0,
        };
        let view = view(domain, Some("mx.orbit.test"));
        let dkim = view
            .records
            .iter()
            .find(|record| matches!(record.kind, MailDnsRecordKind::Dkim))
            .unwrap();
        assert_eq!(dkim.name, "o2026a._domainkey.example.com");
        assert!(dkim.value.len() > 255);
        assert_eq!(dkim.txt_strings.len(), 2);
        assert!(dkim.txt_strings.iter().all(|part| part.len() <= 255));
        assert_eq!(dkim.txt_strings.concat(), dkim.value);
        let mx = view
            .records
            .iter()
            .find(|record| matches!(record.kind, MailDnsRecordKind::Mx))
            .unwrap();
        assert_eq!(mx.priority, Some(10));
        assert!(mx.txt_strings.is_empty());
    }
}
