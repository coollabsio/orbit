//! What composing needs from the member (spec §7): the addresses to send from, signatures,
//! personal snippets and the undo-send delay. Everything is the caller's own.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, Uri};
use orbit_platform::{Id, TimestampMillis};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use super::{Call, MailState, RequestIdExtension};
use crate::repositories::mail::{CONTACT_LIMIT, ContactKind, FromAddress, SendReadiness, Snippet};
use crate::task_routes::{ApiError, ApiJson, ApiQuery};

#[derive(Serialize, ToSchema)]
pub(crate) struct MailFromAddress {
    #[schema(value_type = String)]
    address_id: Id,
    #[schema(value_type = String)]
    mailbox_id: Id,
    address: String,
    /// The sender name on mail from this address: its own, else the mailbox name.
    display_name: String,
    signature_html: Option<String>,
    /// `ready` or `not_ready`.
    ready: String,
    /// Why it is `not_ready`: `domain_unverified`, `inbound_not_live`, `dkim_inactive`,
    /// `no_route` or `mailbox_suspended`.
    not_ready_reason: Option<String>,
    /// The largest message, in bytes as sent, the route of this address's domain takes; `null`
    /// without a route.
    max_message_bytes: Option<i64>,
}

fn from_view(address: FromAddress) -> MailFromAddress {
    let (ready, reason) = match address.ready {
        SendReadiness::Ready => ("ready".to_owned(), None),
        SendReadiness::NotReady(reason) => (
            "not_ready".to_owned(),
            serde_json::to_value(reason)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned)),
        ),
    };
    MailFromAddress {
        address_id: address.address_id,
        mailbox_id: address.mailbox_id,
        address: address.address,
        display_name: address.display_name,
        signature_html: address.signature_html,
        ready,
        not_ready_reason: reason,
        max_message_bytes: address.max_message_bytes,
    }
}

#[derive(Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub(crate) struct FromAddressQuery {
    /// A reply or a forward: only the addresses of this thread's mailbox.
    thread_id: Option<String>,
}

/// The addresses the caller can send as. For a new message, every one; with `thread_id`, only
/// those of the thread's mailbox.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/mail/from-addresses", params(("workspace_id" = String, Path), FromAddressQuery), responses((status = 200, body = Vec<MailFromAddress>)))]
pub(crate) async fn list_from_addresses(
    State(state): State<MailState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiQuery(query): ApiQuery<FromAddressQuery>,
) -> Result<Json<Vec<MailFromAddress>>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let membership = state
        .mail
        .mail_membership(call.workspace_id, call.actor_id)
        .await
        .map_err(|error| call.problem(error))?;
    let mailbox = match query.thread_id {
        Some(thread) => {
            let thread = call.id(&thread)?;
            Some(
                state
                    .mail
                    .thread_mailbox(call.workspace_id, membership, thread)
                    .await
                    .map_err(|error| call.problem(error))?,
            )
        }
        None => None,
    };
    let addresses = state
        .mail
        .from_addresses(membership, call.workspace_id, mailbox)
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(addresses.into_iter().map(from_view).collect()))
}

#[derive(Serialize, ToSchema)]
pub(crate) struct MailSignature {
    #[schema(value_type = String)]
    address_id: Id,
    /// `null` when there is no signature.
    body_html: Option<String>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct PutMailSignatureBody {
    /// Sanitized on save; up to 20,000 bytes.
    body_html: String,
}

/// The caller's signature for an address they can send as.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/mail/signatures/{address_id}", params(("workspace_id" = String, Path), ("address_id" = String, Path)), responses((status = 200, body = MailSignature)))]
pub(crate) async fn get_mail_signature(
    State(state): State<MailState>,
    Path((workspace, address)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<MailSignature>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let address_id = call.id(&address)?;
    let membership = state
        .mail
        .mail_membership(call.workspace_id, call.actor_id)
        .await
        .map_err(|error| call.problem(error))?;
    let body_html = state
        .mail
        .signature(membership, call.workspace_id, address_id)
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(MailSignature {
        address_id,
        body_html,
    }))
}

/// Sets the caller's signature for an address they can send as.
#[utoipa::path(put, path = "/api/v1/workspaces/{workspace_id}/mail/signatures/{address_id}", params(("workspace_id" = String, Path), ("address_id" = String, Path)), request_body = PutMailSignatureBody, responses((status = 200, body = MailSignature)))]
pub(crate) async fn put_mail_signature(
    State(state): State<MailState>,
    Path((workspace, address)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<PutMailSignatureBody>,
) -> Result<Json<MailSignature>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let address_id = call.id(&address)?;
    let membership = state
        .mail
        .mail_membership(call.workspace_id, call.actor_id)
        .await
        .map_err(|error| call.problem(error))?;
    let stored = state
        .mail
        .put_signature(membership, call.workspace_id, address_id, &body.body_html)
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(MailSignature {
        address_id,
        body_html: Some(stored),
    }))
}

/// Removes the caller's signature for an address.
#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/mail/signatures/{address_id}", params(("workspace_id" = String, Path), ("address_id" = String, Path)), responses((status = 204)))]
pub(crate) async fn delete_mail_signature(
    State(state): State<MailState>,
    Path((workspace, address)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<StatusCode, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let address_id = call.id(&address)?;
    let membership = state
        .mail
        .mail_membership(call.workspace_id, call.actor_id)
        .await
        .map_err(|error| call.problem(error))?;
    state
        .mail
        .delete_signature(membership, call.workspace_id, address_id)
        .await
        .map_err(|error| call.problem(error))?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, ToSchema)]
pub(crate) struct MailSnippet {
    #[schema(value_type = String)]
    id: Id,
    name: String,
    body_html: String,
    #[schema(value_type = String, format = DateTime)]
    created_at: TimestampMillis,
    #[schema(value_type = String, format = DateTime)]
    updated_at: TimestampMillis,
}

fn snippet_view(snippet: Snippet) -> MailSnippet {
    MailSnippet {
        id: snippet.id,
        name: snippet.name,
        body_html: snippet.body_html,
        created_at: snippet.created_at,
        updated_at: snippet.updated_at,
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreateMailSnippetBody {
    /// 1 to 80 characters.
    name: String,
    /// Sanitized on save; up to 100,000 bytes.
    #[serde(default)]
    body_html: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct UpdateMailSnippetBody {
    name: Option<String>,
    body_html: Option<String>,
}

/// The caller's snippets by name.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/mail/snippets", params(("workspace_id" = String, Path)), responses((status = 200, body = Vec<MailSnippet>)))]
pub(crate) async fn list_mail_snippets(
    State(state): State<MailState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<Vec<MailSnippet>>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let membership = state
        .mail
        .mail_membership(call.workspace_id, call.actor_id)
        .await
        .map_err(|error| call.problem(error))?;
    let snippets = state
        .mail
        .list_snippets(membership)
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(snippets.into_iter().map(snippet_view).collect()))
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/mail/snippets", params(("workspace_id" = String, Path)), request_body = CreateMailSnippetBody, responses((status = 201, body = MailSnippet)))]
pub(crate) async fn create_mail_snippet(
    State(state): State<MailState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<CreateMailSnippetBody>,
) -> Result<(StatusCode, Json<MailSnippet>), ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let membership = state
        .mail
        .mail_membership(call.workspace_id, call.actor_id)
        .await
        .map_err(|error| call.problem(error))?;
    let snippet = state
        .mail
        .create_snippet(membership, call.workspace_id, &body.name, &body.body_html)
        .await
        .map_err(|error| call.problem(error))?;
    Ok((StatusCode::CREATED, Json(snippet_view(snippet))))
}

/// Changes a snippet of the caller; absent fields keep their value.
#[utoipa::path(patch, path = "/api/v1/workspaces/{workspace_id}/mail/snippets/{snippet_id}", params(("workspace_id" = String, Path), ("snippet_id" = String, Path)), request_body = UpdateMailSnippetBody, responses((status = 200, body = MailSnippet)))]
pub(crate) async fn update_mail_snippet(
    State(state): State<MailState>,
    Path((workspace, snippet)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<UpdateMailSnippetBody>,
) -> Result<Json<MailSnippet>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let snippet_id = call.id(&snippet)?;
    let membership = state
        .mail
        .mail_membership(call.workspace_id, call.actor_id)
        .await
        .map_err(|error| call.problem(error))?;
    let snippet = state
        .mail
        .update_snippet(
            membership,
            snippet_id,
            body.name.as_deref(),
            body.body_html.as_deref(),
        )
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(snippet_view(snippet)))
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/mail/snippets/{snippet_id}", params(("workspace_id" = String, Path), ("snippet_id" = String, Path)), responses((status = 204)))]
pub(crate) async fn delete_mail_snippet(
    State(state): State<MailState>,
    Path((workspace, snippet)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<StatusCode, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let snippet_id = call.id(&snippet)?;
    let membership = state
        .mail
        .mail_membership(call.workspace_id, call.actor_id)
        .await
        .map_err(|error| call.problem(error))?;
    state
        .mail
        .delete_snippet(membership, snippet_id)
        .await
        .map_err(|error| call.problem(error))?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, ToSchema)]
pub(crate) struct MailMemberSettings {
    /// 0, 5, 10, 20 or 30; default 10.
    undo_send_seconds: i64,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct PutMailSettingsBody {
    /// 0, 5, 10, 20 or 30.
    undo_send_seconds: i64,
}

/// The caller's mail settings.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/mail/settings/me", params(("workspace_id" = String, Path)), responses((status = 200, body = MailMemberSettings)))]
pub(crate) async fn get_mail_settings(
    State(state): State<MailState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<MailMemberSettings>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let membership = state
        .mail
        .mail_membership(call.workspace_id, call.actor_id)
        .await
        .map_err(|error| call.problem(error))?;
    let undo_send_seconds = state
        .mail
        .member_settings(membership)
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(MailMemberSettings { undo_send_seconds }))
}

/// Sets the caller's mail settings.
#[utoipa::path(put, path = "/api/v1/workspaces/{workspace_id}/mail/settings/me", params(("workspace_id" = String, Path)), request_body = PutMailSettingsBody, responses((status = 200, body = MailMemberSettings)))]
pub(crate) async fn put_mail_settings(
    State(state): State<MailState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<PutMailSettingsBody>,
) -> Result<Json<MailMemberSettings>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let membership = state
        .mail
        .mail_membership(call.workspace_id, call.actor_id)
        .await
        .map_err(|error| call.problem(error))?;
    let undo_send_seconds = state
        .mail
        .put_member_settings(membership, body.undo_send_seconds)
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(MailMemberSettings { undo_send_seconds }))
}

#[derive(Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub(crate) struct ContactQuery {
    /// Part of a name or an address, any case. Empty: the members.
    q: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct MailContact {
    address: String,
    name: Option<String>,
    /// `member`, `contact` or `seen`.
    kind: ContactKind,
}

/// Address suggestions for the composer (at most 8): workspace members first, then the people
/// the caller wrote to by how often and how lately, then senders of mail in the caller's own
/// mailboxes.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/mail/contacts", params(("workspace_id" = String, Path), ContactQuery), responses((status = 200, body = Vec<MailContact>)))]
pub(crate) async fn search_mail_contacts(
    State(state): State<MailState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiQuery(query): ApiQuery<ContactQuery>,
) -> Result<Json<Vec<MailContact>>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let membership = state
        .mail
        .mail_membership(call.workspace_id, call.actor_id)
        .await
        .map_err(|error| call.problem(error))?;
    let hits = state
        .mail
        .search_contacts(
            call.workspace_id,
            membership,
            query.q.as_deref().unwrap_or(""),
            CONTACT_LIMIT,
            TimestampMillis::now(),
        )
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(
        hits.into_iter()
            .map(|hit| MailContact {
                address: hit.address,
                name: hit.name,
                kind: hit.kind,
            })
            .collect(),
    ))
}

/// Removes an address from the caller's contacts and suggestions.
#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/mail/contacts/{address}", params(("workspace_id" = String, Path), ("address" = String, Path)), responses((status = 204)))]
pub(crate) async fn delete_mail_contact(
    State(state): State<MailState>,
    Path((workspace, address)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<StatusCode, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let membership = state
        .mail
        .mail_membership(call.workspace_id, call.actor_id)
        .await
        .map_err(|error| call.problem(error))?;
    state
        .mail
        .hide_contact(
            call.workspace_id,
            membership,
            &address,
            TimestampMillis::now(),
        )
        .await
        .map_err(|error| call.problem(error))?;
    Ok(StatusCode::NO_CONTENT)
}
