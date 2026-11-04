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
    Proposal,
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
            call.request_id(),
        )
        .await
        .map_err(|error| call.problem(error))?;
    Ok((StatusCode::CREATED, Json(view(mailbox))))
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
