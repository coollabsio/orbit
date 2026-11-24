//! Mailboxes: list, create, get, rename, delete, suspend and resume, members, aliases, and
//! **Create a mailbox for each member** (preview, then create).

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, Uri};
use orbit_platform::{Id, TimestampMillis};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::{Call, MailState, RequestIdExtension};
use crate::repositories::mail::{
    AddressKind, MailAddress, Mailbox, MailboxKind, MailboxStatus, NamePattern, NewMailbox,
    Proposal, Vacation, VacationChange,
};
use crate::task_routes::{ApiError, ApiJson};

#[derive(Serialize, ToSchema)]
pub(crate) struct MailboxAddress {
    #[schema(value_type = String)]
    id: Id,
    #[schema(value_type = String)]
    domain_id: Id,
    /// Lower case, without a `+tag`; mail to `local+anything@` arrives here too.
    local_part: String,
    /// `local_part@domain`.
    address: String,
    kind: AddressKind,
    /// The name recipients see on mail from this address; `null`: the mailbox name.
    #[schema(required = true)]
    sender_name: Option<String>,
}

/// A mailbox with its addresses (primary first) and members.
#[derive(Serialize, ToSchema)]
pub(crate) struct MailMailbox {
    #[schema(value_type = String)]
    id: Id,
    kind: MailboxKind,
    name: String,
    /// Personal mailboxes: the owner's membership. `null` once the owner left the workspace;
    /// such a mailbox stays suspended and only owners and admins see it.
    #[schema(value_type = Option<String>)]
    owner_membership_id: Option<Id>,
    /// A suspended mailbox refuses mail.
    status: MailboxStatus,
    addresses: Vec<MailboxAddress>,
    /// Memberships that read the mailbox.
    #[schema(value_type = Vec<String>)]
    member_ids: Vec<Id>,
    #[schema(value_type = String, format = DateTime)]
    created_at: TimestampMillis,
    #[schema(value_type = String, format = DateTime)]
    updated_at: TimestampMillis,
}

fn address_view(address: MailAddress) -> MailboxAddress {
    MailboxAddress {
        address: address.address(),
        id: address.id,
        domain_id: address.domain_id,
        local_part: address.local_part,
        kind: address.kind,
        sender_name: address.sender_name,
    }
}

fn view(mailbox: Mailbox) -> MailMailbox {
    MailMailbox {
        id: mailbox.id,
        kind: mailbox.kind,
        name: mailbox.name,
        owner_membership_id: mailbox.owner_membership_id,
        status: mailbox.status,
        addresses: mailbox.addresses.into_iter().map(address_view).collect(),
        member_ids: mailbox.member_ids,
        created_at: mailbox.created_at,
        updated_at: mailbox.updated_at,
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreateMailboxBody {
    kind: MailboxKind,
    name: String,
    /// The domain of the primary address, a domain of the workspace (any status).
    domain_id: String,
    /// The primary address is `local_part@<domain>`; no `+`.
    local_part: String,
    /// Required for `personal` (a membership of the workspace), absent for `shared`.
    owner_membership_id: Option<String>,
    /// `shared`: the memberships that read it. `personal`: empty or the owner.
    #[serde(default)]
    member_ids: Vec<String>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct UpdateMailboxBody {
    name: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct MailboxStatusBody {
    status: MailboxStatus,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct MailboxMemberBody {
    membership_id: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct MailboxAliasBody {
    /// Any domain of the workspace.
    domain_id: String,
    local_part: String,
    /// The name recipients see on mail from this alias; absent, `null` or blank: the mailbox name.
    #[serde(default)]
    sender_name: Option<String>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct MailboxAddressBody {
    /// The name recipients see on mail from this address; `null` or blank: the mailbox name.
    #[schema(required = true)]
    sender_name: Option<String>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct PreviewPersonalMailboxesBody {
    domain_id: String,
    pattern: NamePattern,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct PersonalMailboxOverride {
    membership_id: String,
    local_part: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreatePersonalMailboxesBody {
    domain_id: String,
    pattern: NamePattern,
    /// The admin's edits of the proposed local parts.
    #[serde(default)]
    overrides: Vec<PersonalMailboxOverride>,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct PersonalMailboxesResult {
    created: Vec<MailMailbox>,
    /// Members whose address still conflicts; nothing was created for them.
    conflicts: Vec<Proposal>,
}

/// Owners and admins: every mailbox of the workspace. Other members: their mailboxes.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/mail/mailboxes", params(("workspace_id" = String, Path)), responses((status = 200, body = Vec<MailMailbox>)))]
pub(crate) async fn list_mailboxes(
    State(state): State<MailState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<Vec<MailMailbox>>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let mailboxes = state
        .mail
        .list_mailboxes(call.workspace_id, call.actor_id)
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(mailboxes.into_iter().map(view).collect()))
}

/// Creates a personal or shared mailbox with its primary address.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/mail/mailboxes", params(("workspace_id" = String, Path)), request_body = CreateMailboxBody, responses((status = 201, body = MailMailbox)))]
pub(crate) async fn create_mailbox(
    State(state): State<MailState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<CreateMailboxBody>,
) -> Result<(StatusCode, Json<MailMailbox>), ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let new = NewMailbox {
        kind: body.kind,
        name: body.name,
        domain_id: call.body_id(&body.domain_id, "domain_id")?,
        local_part: body.local_part,
        owner_membership_id: body
            .owner_membership_id
            .as_deref()
            .map(|id| call.body_id(id, "owner_membership_id"))
            .transpose()?,
        member_ids: body
            .member_ids
            .iter()
            .map(|id| call.body_id(id, "member_ids"))
            .collect::<Result<_, _>>()?,
    };
    let mailbox = state
        .mail
        .create_mailbox(call.workspace_id, call.actor_id, new, call.request_id())
        .await
        .map_err(|error| call.problem(error))?;
    Ok((StatusCode::CREATED, Json(view(mailbox))))
}

/// A mailbox, for its members and for owners and admins.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/mail/mailboxes/{mailbox_id}", params(("workspace_id" = String, Path), ("mailbox_id" = String, Path)), responses((status = 200, body = MailMailbox)))]
pub(crate) async fn get_mailbox(
    State(state): State<MailState>,
    Path((workspace, mailbox)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<MailMailbox>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let mailbox_id = call.id(&mailbox)?;
    let mailbox = state
        .mail
        .get_mailbox(call.workspace_id, call.actor_id, mailbox_id)
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(view(mailbox)))
}

/// Renames a mailbox.
#[utoipa::path(patch, path = "/api/v1/workspaces/{workspace_id}/mail/mailboxes/{mailbox_id}", params(("workspace_id" = String, Path), ("mailbox_id" = String, Path)), request_body = UpdateMailboxBody, responses((status = 200, body = MailMailbox)))]
pub(crate) async fn update_mailbox(
    State(state): State<MailState>,
    Path((workspace, mailbox)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<UpdateMailboxBody>,
) -> Result<Json<MailMailbox>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let mailbox_id = call.id(&mailbox)?;
    let mailbox = state
        .mail
        .update_mailbox(
            call.workspace_id,
            call.actor_id,
            mailbox_id,
            &body.name,
            call.request_id(),
        )
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(view(mailbox)))
}

/// Deletes a mailbox with its addresses and threads.
#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/mail/mailboxes/{mailbox_id}", params(("workspace_id" = String, Path), ("mailbox_id" = String, Path)), responses((status = 204)))]
pub(crate) async fn delete_mailbox(
    State(state): State<MailState>,
    Path((workspace, mailbox)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<StatusCode, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let mailbox_id = call.id(&mailbox)?;
    state
        .mail
        .delete_mailbox(
            call.workspace_id,
            call.actor_id,
            mailbox_id,
            call.request_id(),
        )
        .await
        .map_err(|error| call.problem(error))?;
    Ok(StatusCode::NO_CONTENT)
}

/// Suspends a mailbox (its mail is refused) or resumes it.
#[utoipa::path(put, path = "/api/v1/workspaces/{workspace_id}/mail/mailboxes/{mailbox_id}/status", params(("workspace_id" = String, Path), ("mailbox_id" = String, Path)), request_body = MailboxStatusBody, responses((status = 200, body = MailMailbox)))]
pub(crate) async fn set_mailbox_status(
    State(state): State<MailState>,
    Path((workspace, mailbox)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<MailboxStatusBody>,
) -> Result<Json<MailMailbox>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let mailbox_id = call.id(&mailbox)?;
    let mailbox = state
        .mail
        .set_mailbox_status(
            call.workspace_id,
            call.actor_id,
            mailbox_id,
            body.status,
            call.request_id(),
        )
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(view(mailbox)))
}

/// Adds a member to a shared mailbox.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/mail/mailboxes/{mailbox_id}/members", params(("workspace_id" = String, Path), ("mailbox_id" = String, Path)), request_body = MailboxMemberBody, responses((status = 200, body = MailMailbox)))]
pub(crate) async fn add_mailbox_member(
    State(state): State<MailState>,
    Path((workspace, mailbox)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<MailboxMemberBody>,
) -> Result<Json<MailMailbox>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let mailbox_id = call.id(&mailbox)?;
    let membership_id = call.body_id(&body.membership_id, "membership_id")?;
    let mailbox = state
        .mail
        .add_member(
            call.workspace_id,
            call.actor_id,
            mailbox_id,
            membership_id,
            call.request_id(),
        )
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(view(mailbox)))
}

/// Removes a member from a shared mailbox.
#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/mail/mailboxes/{mailbox_id}/members/{membership_id}", params(("workspace_id" = String, Path), ("mailbox_id" = String, Path), ("membership_id" = String, Path)), responses((status = 200, body = MailMailbox)))]
pub(crate) async fn remove_mailbox_member(
    State(state): State<MailState>,
    Path((workspace, mailbox, membership)): Path<(String, String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<MailMailbox>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let mailbox_id = call.id(&mailbox)?;
    let membership_id = call.id(&membership)?;
    let mailbox = state
        .mail
        .remove_member(
            call.workspace_id,
            call.actor_id,
            mailbox_id,
            membership_id,
            call.request_id(),
        )
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(view(mailbox)))
}

/// Adds an alias on any domain of the workspace.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/mail/mailboxes/{mailbox_id}/addresses", params(("workspace_id" = String, Path), ("mailbox_id" = String, Path)), request_body = MailboxAliasBody, responses((status = 201, body = MailMailbox)))]
pub(crate) async fn add_mailbox_alias(
    State(state): State<MailState>,
    Path((workspace, mailbox)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<MailboxAliasBody>,
) -> Result<(StatusCode, Json<MailMailbox>), ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let mailbox_id = call.id(&mailbox)?;
    let domain_id = call.body_id(&body.domain_id, "domain_id")?;
    let mailbox = state
        .mail
        .add_alias(
            call.workspace_id,
            call.actor_id,
            mailbox_id,
            domain_id,
            &body.local_part,
            body.sender_name.as_deref(),
            call.request_id(),
        )
        .await
        .map_err(|error| call.problem(error))?;
    Ok((StatusCode::CREATED, Json(view(mailbox))))
}

/// Sets the sender name of one address.
#[utoipa::path(patch, path = "/api/v1/workspaces/{workspace_id}/mail/mailboxes/{mailbox_id}/addresses/{address_id}", params(("workspace_id" = String, Path), ("mailbox_id" = String, Path), ("address_id" = String, Path)), request_body = MailboxAddressBody, responses((status = 200, body = MailMailbox)))]
pub(crate) async fn update_mailbox_address(
    State(state): State<MailState>,
    Path((workspace, mailbox, address)): Path<(String, String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<MailboxAddressBody>,
) -> Result<Json<MailMailbox>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let mailbox_id = call.id(&mailbox)?;
    let address_id = call.id(&address)?;
    let mailbox = state
        .mail
        .set_address_sender_name(
            call.workspace_id,
            call.actor_id,
            mailbox_id,
            address_id,
            body.sender_name.as_deref(),
            call.request_id(),
        )
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(view(mailbox)))
}

/// Removes an alias. The primary address stays.
#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/mail/mailboxes/{mailbox_id}/addresses/{address_id}", params(("workspace_id" = String, Path), ("mailbox_id" = String, Path), ("address_id" = String, Path)), responses((status = 200, body = MailMailbox)))]
pub(crate) async fn remove_mailbox_address(
    State(state): State<MailState>,
    Path((workspace, mailbox, address)): Path<(String, String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<MailMailbox>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let mailbox_id = call.id(&mailbox)?;
    let address_id = call.id(&address)?;
    let mailbox = state
        .mail
        .remove_address(
            call.workspace_id,
            call.actor_id,
            mailbox_id,
            address_id,
            call.request_id(),
        )
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(view(mailbox)))
}

/// The personal mailboxes a pattern gives the members without one. Names are folded to ASCII;
/// taken and repeated addresses are marked as conflicts. Nothing is written.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/mail/mailboxes/bulk-personal/preview", params(("workspace_id" = String, Path)), request_body = PreviewPersonalMailboxesBody, responses((status = 200, body = Vec<Proposal>)))]
pub(crate) async fn preview_personal_mailboxes(
    State(state): State<MailState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<PreviewPersonalMailboxesBody>,
) -> Result<Json<Vec<Proposal>>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let domain_id = call.body_id(&body.domain_id, "domain_id")?;
    let proposals = state
        .mail
        .preview_bulk_personal(call.workspace_id, call.actor_id, domain_id, body.pattern)
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(proposals))
}

/// **Create a mailbox for each member**: creates the proposals of the preview with the admin's
/// edits. Conflicting ones are returned and not created.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/mail/mailboxes/bulk-personal", params(("workspace_id" = String, Path)), request_body = CreatePersonalMailboxesBody, responses((status = 200, body = PersonalMailboxesResult)))]
pub(crate) async fn create_personal_mailboxes(
    State(state): State<MailState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<CreatePersonalMailboxesBody>,
) -> Result<Json<PersonalMailboxesResult>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let domain_id = call.body_id(&body.domain_id, "domain_id")?;
    let overrides = body
        .overrides
        .into_iter()
        .map(|entry| {
            Ok((
                call.body_id(&entry.membership_id, "overrides")?,
                entry.local_part,
            ))
        })
        .collect::<Result<Vec<_>, ApiError>>()?;
    let result = state
        .mail
        .bulk_personal(
            call.workspace_id,
            call.actor_id,
            domain_id,
            body.pattern,
            &overrides,
            call.request_id(),
        )
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(PersonalMailboxesResult {
        created: result.created.into_iter().map(view).collect(),
        conflicts: result.conflicts,
    }))
}

#[derive(Serialize, ToSchema)]
pub(crate) struct MailVacation {
    enabled: bool,
    /// The first moment a reply is sent (inclusive); `null` is open.
    #[schema(value_type = Option<String>, format = DateTime)]
    starts_at: Option<TimestampMillis>,
    /// The first moment no reply is sent (exclusive); `null` is open.
    #[schema(value_type = Option<String>, format = DateTime)]
    ends_at: Option<TimestampMillis>,
    /// Empty: `Re:` and the subject of the message.
    subject: String,
    body_html: String,
    /// Reply only to people the mailbox's members wrote to.
    contacts_only: bool,
    /// The time zone that dates without an offset are read in.
    timezone: String,
}

fn vacation_view(vacation: Vacation) -> MailVacation {
    MailVacation {
        enabled: vacation.enabled,
        starts_at: vacation.starts_at,
        ends_at: vacation.ends_at,
        subject: vacation.subject,
        body_html: vacation.body_html,
        contacts_only: vacation.contacts_only,
        timezone: vacation.timezone,
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct PutMailVacationBody {
    enabled: bool,
    /// An RFC 3339 time, a date (`2026-11-20`) or a date and time without an offset
    /// (`2026-11-20T09:00`) in the owner's time zone. Inclusive. Empty: no start.
    starts_at: Option<String>,
    /// As `starts_at`. Exclusive, and after `starts_at`. Empty: no end.
    ends_at: Option<String>,
    /// Up to 200 characters. Empty: `Re:` and the subject of the message.
    #[serde(default)]
    subject: String,
    /// Sanitized on save; up to 20,000 bytes; required when `enabled`.
    #[serde(default)]
    body_html: String,
    #[serde(default)]
    contacts_only: bool,
}

/// The vacation reply of a mailbox: the owner of a personal mailbox, and the members of a shared
/// one, read it.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/mail/mailboxes/{mailbox_id}/vacation", params(("workspace_id" = String, Path), ("mailbox_id" = String, Path)), responses((status = 200, body = MailVacation)))]
pub(crate) async fn get_mailbox_vacation(
    State(state): State<MailState>,
    Path((workspace, mailbox)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<MailVacation>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let mailbox_id = call.id(&mailbox)?;
    let vacation = state
        .mail
        .vacation(call.workspace_id, call.actor_id, mailbox_id)
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(vacation_view(vacation)))
}

/// Sets the vacation reply: the owner of a personal mailbox, or a member with `mail.manage`
/// for a shared one.
#[utoipa::path(put, path = "/api/v1/workspaces/{workspace_id}/mail/mailboxes/{mailbox_id}/vacation", params(("workspace_id" = String, Path), ("mailbox_id" = String, Path)), request_body = PutMailVacationBody, responses((status = 200, body = MailVacation)))]
pub(crate) async fn put_mailbox_vacation(
    State(state): State<MailState>,
    Path((workspace, mailbox)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<PutMailVacationBody>,
) -> Result<Json<MailVacation>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let mailbox_id = call.id(&mailbox)?;
    let vacation = state
        .mail
        .put_vacation(
            call.workspace_id,
            call.actor_id,
            mailbox_id,
            VacationChange {
                enabled: body.enabled,
                starts_at: body.starts_at,
                ends_at: body.ends_at,
                subject: body.subject,
                body_html: body.body_html,
                contacts_only: body.contacts_only,
            },
            TimestampMillis::now(),
        )
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(vacation_view(vacation)))
}
