//! Reading mail: thread lists, a thread with its messages, organising threads (one or many),
//! unread counts, search and the mail app's start data. Mailbox members only: anyone else gets
//! `mail_not_found`.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Uri};
use orbit_platform::Id;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use super::labels::{MailLabel, view as label_view};
use super::{Call, MailState, RequestIdExtension};
use crate::repositories::mail::{
    AddressKind, MailAddress, MailCounts, MailFolder, MailThreadDetail, MailThreadPage,
    MailThreadSummary, MailboxKind, MailboxStatus, ThreadChange, ThreadListInput, parse_query,
};
use crate::task_routes::{ApiError, ApiJson, ApiQuery};

#[derive(Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub(crate) struct MailThreadQuery {
    /// One mailbox the caller reads; without it, every mailbox the caller reads.
    mailbox_id: Option<String>,
    /// Without a folder: the inbox, or with `label_id` or `starred` every folder but spam and
    /// trash.
    folder: Option<MailFolder>,
    label_id: Option<String>,
    /// Only the caller's starred threads.
    #[serde(default)]
    starred: bool,
    /// The `cursor` of the page before.
    cursor: Option<String>,
    /// 1–100; default 50.
    limit: Option<usize>,
}

#[derive(Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub(crate) struct MailSearchQuery {
    /// Words (every one must match, the last as a prefix) and the operators `from:`, `to:`,
    /// `subject:`, `label:`, `in:`, `is:unread`, `is:starred`, `has:attachment`, `before:` and
    /// `after:` (`YYYY-MM-DD`). Without `in:`, spam and trash are left out.
    #[serde(default)]
    q: String,
    /// The `cursor` of the page before.
    cursor: Option<String>,
    /// 1–100; default 50.
    limit: Option<usize>,
}

/// A change to a thread; absent fields keep their value.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct UpdateMailThreadBody {
    /// Moving to trash keeps the folder to restore; moving out of trash restores it there.
    folder: Option<MailFolder>,
    /// Moves a trashed thread back to the folder it came from (the inbox when unknown). Not
    /// together with `folder`.
    #[serde(default)]
    restore: bool,
    /// The caller's read state.
    read: Option<bool>,
    /// The caller's star.
    starred: Option<bool>,
    /// Label ids of the workspace.
    #[serde(default)]
    add_labels: Vec<String>,
    #[serde(default)]
    remove_labels: Vec<String>,
}

/// The same change for many threads.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct UpdateMailThreadsBody {
    /// 1–100 thread ids. Threads the caller does not read are left alone.
    ids: Vec<String>,
    folder: Option<MailFolder>,
    #[serde(default)]
    restore: bool,
    read: Option<bool>,
    starred: Option<bool>,
    #[serde(default)]
    add_labels: Vec<String>,
    #[serde(default)]
    remove_labels: Vec<String>,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct MailThreadList {
    /// The changed threads, newest first.
    items: Vec<MailThreadSummary>,
}

/// A mailbox the caller reads.
#[derive(Serialize, ToSchema)]
pub(crate) struct MailMeMailbox {
    #[schema(value_type = String)]
    id: Id,
    name: String,
    kind: MailboxKind,
    /// A suspended mailbox refuses new mail; its threads stay readable.
    status: MailboxStatus,
    primary_address: Option<String>,
    /// Unread threads in the inbox.
    unread: i64,
}

/// What the mail app needs at start.
#[derive(Serialize, ToSchema)]
pub(crate) struct MailMe {
    /// The mailboxes the caller is a member of, by name.
    mailboxes: Vec<MailMeMailbox>,
    labels: Vec<MailLabel>,
    /// The caller manages mail (domains, mailboxes, labels).
    can_manage: bool,
}

fn ids(call: &Call<'_>, values: &[String], field: &'static str) -> Result<Vec<Id>, ApiError> {
    values
        .iter()
        .map(|value| call.body_id(value, field))
        .collect()
}

fn optional_id(
    call: &Call<'_>,
    value: Option<&str>,
    field: &'static str,
) -> Result<Option<Id>, ApiError> {
    value.map(|value| call.body_id(value, field)).transpose()
}

/// The threads of one mailbox or all the caller reads, newest first.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/mail/threads", params(MailThreadQuery, ("workspace_id" = String, Path)), responses((status = 200, body = MailThreadPage)))]
pub(crate) async fn list_mail_threads(
    State(state): State<MailState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiQuery(query): ApiQuery<MailThreadQuery>,
) -> Result<Json<MailThreadPage>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let input = ThreadListInput {
        mailbox_id: optional_id(&call, query.mailbox_id.as_deref(), "mailbox_id")?,
        folder: query.folder,
        label_id: optional_id(&call, query.label_id.as_deref(), "label_id")?,
        starred: query.starred,
        cursor: query.cursor,
        limit: query.limit,
    };
    let page = state
        .mail
        .list_threads(call.workspace_id, call.actor_id, input)
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(page))
}

/// A thread with its messages, oldest first.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/mail/threads/{thread_id}", params(("workspace_id" = String, Path), ("thread_id" = String, Path)), responses((status = 200, body = MailThreadDetail)))]
pub(crate) async fn get_mail_thread(
    State(state): State<MailState>,
    Path((workspace, thread)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<MailThreadDetail>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let thread_id = call.id(&thread)?;
    let detail = state
        .mail
        .get_thread(call.workspace_id, call.actor_id, thread_id)
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(detail))
}

/// Moves a thread, marks it read or unread, stars it, or changes its labels.
#[utoipa::path(patch, path = "/api/v1/workspaces/{workspace_id}/mail/threads/{thread_id}", params(("workspace_id" = String, Path), ("thread_id" = String, Path)), request_body = UpdateMailThreadBody, responses((status = 200, body = MailThreadSummary)))]
pub(crate) async fn update_mail_thread(
    State(state): State<MailState>,
    Path((workspace, thread)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<UpdateMailThreadBody>,
) -> Result<Json<MailThreadSummary>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let thread_id = call.id(&thread)?;
    let change = ThreadChange {
        folder: body.folder,
        restore: body.restore,
        read: body.read,
        starred: body.starred,
        add_labels: ids(&call, &body.add_labels, "add_labels")?,
        remove_labels: ids(&call, &body.remove_labels, "remove_labels")?,
    };
    let summary = state
        .mail
        .update_thread(call.workspace_id, call.actor_id, thread_id, &change)
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(summary))
}

/// The same change for up to 100 threads (selection with `x`).
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/mail/threads/bulk", params(("workspace_id" = String, Path)), request_body = UpdateMailThreadsBody, responses((status = 200, body = MailThreadList)))]
pub(crate) async fn update_mail_threads(
    State(state): State<MailState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<UpdateMailThreadsBody>,
) -> Result<Json<MailThreadList>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let thread_ids = ids(&call, &body.ids, "ids")?;
    let change = ThreadChange {
        folder: body.folder,
        restore: body.restore,
        read: body.read,
        starred: body.starred,
        add_labels: ids(&call, &body.add_labels, "add_labels")?,
        remove_labels: ids(&call, &body.remove_labels, "remove_labels")?,
    };
    let items = state
        .mail
        .update_threads(call.workspace_id, call.actor_id, &thread_ids, &change)
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(MailThreadList { items }))
}

/// Unread threads in each folder of each mailbox the caller reads.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/mail/counts", params(("workspace_id" = String, Path)), responses((status = 200, body = MailCounts)))]
pub(crate) async fn get_mail_counts(
    State(state): State<MailState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<MailCounts>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let counts = state
        .mail
        .counts(call.workspace_id, call.actor_id)
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(counts))
}

/// Threads of the mailboxes the caller reads that match `q`, newest first. Empty when `q` has
/// nothing to look for.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/mail/search", params(MailSearchQuery, ("workspace_id" = String, Path)), responses((status = 200, body = MailThreadPage)))]
pub(crate) async fn search_mail(
    State(state): State<MailState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiQuery(query): ApiQuery<MailSearchQuery>,
) -> Result<Json<MailThreadPage>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let page = state
        .mail
        .search_threads(
            call.workspace_id,
            call.actor_id,
            parse_query(&query.q),
            query.cursor.as_deref(),
            query.limit,
        )
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(page))
}

/// The caller's mailboxes with their unread counts, the labels, and whether the caller manages
/// mail.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/mail/me", params(("workspace_id" = String, Path)), responses((status = 200, body = MailMe)))]
pub(crate) async fn get_mail_me(
    State(state): State<MailState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<MailMe>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let overview = state
        .mail
        .overview(call.workspace_id, call.actor_id)
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(MailMe {
        mailboxes: overview
            .mailboxes
            .into_iter()
            .map(|(mailbox, unread)| MailMeMailbox {
                primary_address: mailbox
                    .addresses
                    .iter()
                    .find(|address| address.kind == AddressKind::Primary)
                    .map(MailAddress::address),
                id: mailbox.id,
                name: mailbox.name,
                kind: mailbox.kind,
                status: mailbox.status,
                unread,
            })
            .collect(),
        labels: overview.labels.into_iter().map(label_view).collect(),
        can_manage: overview.can_manage,
    }))
}
