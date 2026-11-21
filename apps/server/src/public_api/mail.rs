//! Mail (`mail:read`, `mail:write`): the token's creator's mailboxes, threads, drafts, sending,
//! labels, contacts, signatures, snippets, settings and vacation replies, with the web app's
//! access rules. Not for service-account tokens (`404`). Mail administration (domains, send
//! routes, providers, suppressions, mailbox members and addresses) stays in the web app.
use axum::Json;
use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{DefaultBodyLimit, Path, Query, Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use orbit_platform::{BlobStoreError, Id, Problem, TimestampMillis, UploadError};
use serde::Deserialize;
use utoipa::{IntoParams, ToSchema};

use super::{Ctx, List, PublicApiState, Responder, problem_response, with_cause};
use crate::attachment_routes::{StageFailure, stage_single_file};
use crate::export::names::{ascii_file_name, rfc5987};
use crate::repositories::api_tokens::ApiTokenScope;
use crate::repositories::mail::{
    DraftChange, DraftMode, MailCounts, MailError, MailFolder, MailRepository, MailThreadDetail,
    MailThreadSummary, NewDraft, ThreadChange, ThreadListInput, VacationChange,
};
use crate::token_api::{
    MailContactView, MailDraftCheck, MailDraftFileView, MailDraftView, MailFromAddressView,
    MailLabelView, MailOutboundItemView, MailSent, MailSettingsView, MailSignatureView,
    MailSnippetView, MailVacationView, MailboxView, TokenApiError, mail_refusal,
};

const WORKSPACE: &str = "The token's workspace, from `GET /me`.";

impl PublicApiState {
    #[must_use]
    pub fn with_mail(mut self, mail: std::sync::Arc<MailRepository>) -> Self {
        self.api = self.api.with_mail(mail);
        self
    }
}

/// The mail routes, under `/workspaces/{workspace_id}`.
pub(super) fn routes(state: &PublicApiState) -> Router<PublicApiState> {
    Router::new()
        .route("/mail/mailboxes", get(mailboxes))
        .route(
            "/mail/mailboxes/{mailbox_id}/vacation",
            get(vacation).put(put_vacation),
        )
        .route("/mail/threads", get(threads))
        .route("/mail/threads/bulk", post(update_threads))
        .route(
            "/mail/threads/{thread_id}",
            get(thread).patch(update_thread),
        )
        .route("/mail/search", get(search))
        .route("/mail/counts", get(counts))
        .route("/mail/attachments/{attachment_id}", get(attachment))
        .route("/mail/drafts", get(drafts).post(create_draft))
        .route(
            "/mail/drafts/{draft_id}",
            get(draft).patch(update_draft).delete(delete_draft),
        )
        .route("/mail/drafts/{draft_id}/check", post(check_draft))
        .route("/mail/drafts/{draft_id}/send", post(send_draft))
        .route(
            "/mail/drafts/{draft_id}/files",
            // Multipart streams; this bounds boundaries, headers and ignored fields too.
            post(upload_draft_file).layer(DefaultBodyLimit::max(
                state.api.mail_upload_limit().unwrap_or(0),
            )),
        )
        .route(
            "/mail/drafts/{draft_id}/files/{file_id}",
            patch(update_draft_file).delete(delete_draft_file),
        )
        .route("/mail/outbound", get(outbound))
        .route("/mail/outbound/{outbound_id}/cancel", post(cancel_outbound))
        .route("/mail/labels", get(labels).post(create_label))
        .route(
            "/mail/labels/{label_id}",
            patch(update_label).delete(delete_label),
        )
        .route("/mail/contacts", get(contacts))
        .route("/mail/from-addresses", get(from_addresses))
        .route(
            "/mail/signatures/{address_id}",
            get(signature).put(put_signature).delete(delete_signature),
        )
        .route("/mail/snippets", get(snippets).post(create_snippet))
        .route(
            "/mail/snippets/{snippet_id}",
            patch(update_snippet).delete(delete_snippet),
        )
        .route("/mail/settings", get(settings).put(put_settings))
}

/// The problem of a mail refusal, with the web app's stable codes.
pub(super) fn mail_problem(out: &Responder, error: &MailError) -> Problem {
    let (status, code, detail) = mail_refusal(error);
    if let MailError::Invalid { field } = error {
        return Problem::validation(
            &out.request_id,
            &out.instance,
            [(*field, "The value is invalid.")],
        );
    }
    let status = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let title = status.canonical_reason().unwrap_or("Error");
    out.problem(status, code, title, &detail)
}

fn no_content(ctx: &Ctx, result: Result<(), TokenApiError>) -> Response {
    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => ctx.error(error),
    }
}

fn ids(raw: &[String], invalid: &'static str) -> Result<Vec<Id>, TokenApiError> {
    raw.iter()
        .map(|id| id.parse().map_err(|_| TokenApiError::Invalid(invalid)))
        .collect()
}

fn instant(
    raw: Option<&str>,
    invalid: &'static str,
) -> Result<Option<TimestampMillis>, TokenApiError> {
    raw.map(|raw| {
        crate::task_import::parse_instant_millis(raw)
            .map(TimestampMillis::from_millis)
            .ok_or(TokenApiError::Invalid(invalid))
    })
    .transpose()
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
pub(super) struct ThreadsQuery {
    /// One mailbox from `GET /mail/mailboxes`; every mailbox you read when unset.
    #[param(value_type = Option<String>)]
    mailbox_id: Option<Id>,
    /// Without a folder: the inbox, or with `label_id` or `starred`, every folder but spam and
    /// trash.
    folder: Option<MailFolder>,
    /// Only the threads with this label, from `GET /mail/labels`.
    #[param(value_type = Option<String>)]
    label_id: Option<Id>,
    /// Only your starred threads.
    #[serde(default)]
    starred: bool,
    /// Only the threads you have not read.
    #[serde(default)]
    unread: bool,
    /// Sent: threads with mail sent from the mailbox, newest sent first, in every folder but
    /// trash. Not together with `folder`.
    #[serde(default)]
    sent: bool,
    /// The `next_cursor` of the previous page.
    cursor: Option<String>,
    /// 1 to 100; 50 when unset.
    #[param(minimum = 1, maximum = 100)]
    limit: Option<usize>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
pub(super) struct SearchQuery {
    /// Words (each must match, the last as a prefix) and the operators `from:`, `to:`,
    /// `subject:`, `label:`, `in:`, `is:unread`, `is:starred`, `has:attachment`, `before:` and
    /// `after:` (`YYYY-MM-DD`). Without `in:`, spam and trash are left out.
    #[serde(default)]
    q: String,
    /// The `next_cursor` of the previous page.
    cursor: Option<String>,
    /// 1 to 100; 50 when unset.
    #[param(minimum = 1, maximum = 100)]
    limit: Option<usize>,
}

/// A missing field keeps its value.
#[derive(Deserialize, ToSchema)]
#[schema(as = PublicUpdateMailThreadBody)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateThreadBody {
    /// Move the thread: `inbox`, `archive`, `spam` or `trash`. Trash keeps the folder to
    /// restore; threads in the trash are deleted after 30 days.
    folder: Option<MailFolder>,
    /// Move a thread in the trash back to the folder it came from. Not together with `folder`.
    #[serde(default)]
    restore: bool,
    /// Your read state.
    read: Option<bool>,
    /// Your star.
    starred: Option<bool>,
    /// Label ids from `GET /mail/labels`.
    #[serde(default)]
    add_labels: Vec<String>,
    #[serde(default)]
    remove_labels: Vec<String>,
}

#[derive(Deserialize, ToSchema)]
#[schema(as = PublicUpdateMailThreadsBody)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateThreadsBody {
    /// 1 to 100 thread ids. Threads you do not read are left alone.
    ids: Vec<String>,
    /// As in `PATCH /mail/threads/{thread_id}`.
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

impl UpdateThreadBody {
    fn change(self) -> Result<ThreadChange, TokenApiError> {
        Ok(ThreadChange {
            folder: self.folder,
            restore: self.restore,
            read: self.read,
            starred: self.starred,
            add_labels: ids(&self.add_labels, "add_labels must be label ids")?,
            remove_labels: ids(&self.remove_labels, "remove_labels must be label ids")?,
        })
    }
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
pub(super) struct DraftsQuery {
    /// Only the drafts of this mailbox.
    #[param(value_type = Option<String>)]
    mailbox_id: Option<Id>,
}

#[derive(Deserialize, ToSchema)]
#[schema(as = PublicCreateMailDraftBody)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateDraftBody {
    /// `new` (needs `from_address_id`), or `reply`, `reply_all`, `forward` or
    /// `forward_attached` (need `thread_id`). A reply gets its recipients, subject and quote
    /// from the message; a forward copies its attachments.
    #[schema(value_type = String)]
    mode: DraftMode,
    #[schema(value_type = Option<String>)]
    thread_id: Option<Id>,
    /// The message answered or forwarded; the newest of the thread when unset.
    #[schema(value_type = Option<String>)]
    parent_message_id: Option<Id>,
    /// An address from `GET /mail/from-addresses`. A reply defaults to the address that
    /// received the message.
    #[schema(value_type = Option<String>)]
    from_address_id: Option<Id>,
    to: Option<Vec<String>>,
    cc: Option<Vec<String>>,
    bcc: Option<Vec<String>>,
    subject: Option<String>,
    /// HTML, sanitized on save.
    body_html: Option<String>,
}

/// A missing field keeps its value.
#[derive(Deserialize, ToSchema)]
#[schema(as = PublicUpdateMailDraftBody)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateDraftBody {
    /// The draft's `version` as read. A stale one is a `409 draft_conflict`.
    version: i64,
    #[schema(value_type = Option<String>)]
    from_address_id: Option<Id>,
    /// Between `reply`, `reply_all`, `forward` and `forward_attached` of a thread's draft.
    #[schema(value_type = Option<String>)]
    mode: Option<DraftMode>,
    to: Option<Vec<String>>,
    cc: Option<Vec<String>>,
    bcc: Option<Vec<String>>,
    subject: Option<String>,
    /// HTML, sanitized on save.
    body_html: Option<String>,
}

#[derive(Deserialize, ToSchema, Default)]
#[schema(as = PublicSendMailDraftBody)]
#[serde(deny_unknown_fields)]
pub(super) struct SendDraftBody {
    /// Send at this RFC 3339 time instead of after your undo-send time. Must be in the future.
    scheduled_at: Option<String>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
pub(super) struct UploadQuery {
    /// Put the image in the body: the file gets a `content_id` to use as
    /// `<img src="cid:...">`. Only a PNG, JPEG, GIF or WebP image (by its bytes).
    #[serde(default)]
    inline: bool,
}

#[derive(ToSchema)]
#[schema(as = PublicMailDraftFileUpload)]
#[allow(dead_code)]
pub(super) struct DraftFileUpload {
    #[schema(value_type = String, format = Binary)]
    file: Vec<u8>,
}

#[derive(Deserialize, ToSchema)]
#[schema(as = PublicUpdateMailDraftFileBody)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateDraftFileBody {
    /// Send the file as a download link instead of an attachment.
    as_link: bool,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
pub(super) struct OutboundQuery {
    /// `scheduled` (held for a scheduled time) or `outbox` (held for the undo time or a send
    /// limit, and going).
    view: String,
    /// Only the messages sent from this mailbox.
    #[param(value_type = Option<String>)]
    mailbox_id: Option<Id>,
}

#[derive(Deserialize, ToSchema)]
#[schema(as = PublicCreateMailLabelBody)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateLabelBody {
    /// 1 to 80 characters, unique in the workspace whatever the case.
    name: String,
    /// `#rrggbb`.
    color: String,
}

/// A missing field keeps its value.
#[derive(Deserialize, ToSchema)]
#[schema(as = PublicUpdateMailLabelBody)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateLabelBody {
    name: Option<String>,
    color: Option<String>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
pub(super) struct ContactsQuery {
    /// Part of a name or an address, any case. Empty: the members.
    #[serde(default)]
    q: String,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
pub(super) struct FromAddressesQuery {
    /// A reply or forward: only the addresses of this thread's mailbox.
    #[param(value_type = Option<String>)]
    thread_id: Option<Id>,
}

#[derive(Deserialize, ToSchema)]
#[schema(as = PublicPutMailSignatureBody)]
#[serde(deny_unknown_fields)]
pub(super) struct PutSignatureBody {
    /// HTML, sanitized on save; up to 20,000 bytes.
    body_html: String,
}

#[derive(Deserialize, ToSchema)]
#[schema(as = PublicCreateMailSnippetBody)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateSnippetBody {
    /// 1 to 80 characters.
    name: String,
    /// HTML, sanitized on save; up to 100,000 bytes.
    #[serde(default)]
    body_html: String,
}

/// A missing field keeps its value.
#[derive(Deserialize, ToSchema)]
#[schema(as = PublicUpdateMailSnippetBody)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateSnippetBody {
    name: Option<String>,
    body_html: Option<String>,
}

#[derive(Deserialize, ToSchema)]
#[schema(as = PublicPutMailSettingsBody)]
#[serde(deny_unknown_fields)]
pub(super) struct PutSettingsBody {
    /// 0, 5, 10, 20 or 30.
    undo_send_seconds: i64,
}

#[derive(Deserialize, ToSchema)]
#[schema(as = PublicPutMailVacationBody)]
#[serde(deny_unknown_fields)]
pub(super) struct PutVacationBody {
    enabled: bool,
    /// An RFC 3339 time, a date (`2026-11-20`) or a date and time without an offset
    /// (`2026-11-20T09:00`) in the owner's time zone. Inclusive. Unset or empty: no start.
    starts_at: Option<String>,
    /// As `starts_at`. Exclusive, and after `starts_at`. Unset or empty: no end.
    ends_at: Option<String>,
    /// Up to 200 characters. Empty: `Re:` and the subject of the message.
    #[serde(default)]
    subject: String,
    /// HTML, sanitized on save; up to 20,000 bytes; required when `enabled`.
    #[serde(default)]
    body_html: String,
    /// Reply only to people the mailbox's members wrote to.
    #[serde(default)]
    contacts_only: bool,
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/mail/mailboxes",
    operation_id = "public_list_mailboxes",
    tag = "public-api",
    summary = "The mailboxes you are a member of",
    description = "By name, with the unread threads of each inbox. Scope: `mail:read`.",
    params(("workspace_id" = String, Path, description = WORKSPACE)),
    responses((status = 200, description = "Every mailbox; `next_cursor` is `null`", body = List<MailboxView>)),
    security(("bearerAuth" = []))
)]
pub(super) async fn mailboxes(State(s): State<PublicApiState>, ctx: Ctx) -> Response {
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::MailRead, &ctx.request_id)
            .await?;
        Ok(super::list::<_, MailboxView>(api.mailboxes().await?, None))
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/mail/threads",
    operation_id = "public_list_mail_threads",
    tag = "public-api",
    summary = "Mail threads, newest first",
    description = "The threads of one mailbox or of every mailbox you read. `unread` and `starred` are your own. Scope: `mail:read`.",
    params(("workspace_id" = String, Path, description = WORKSPACE), ThreadsQuery),
    responses((status = 200, description = "A page of threads", body = List<MailThreadSummary>)),
    security(("bearerAuth" = []))
)]
pub(super) async fn threads(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    query: Result<Query<ThreadsQuery>, QueryRejection>,
) -> Response {
    let query = input!(ctx, ApiTokenScope::MailRead, query);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::MailRead, &ctx.request_id)
            .await?;
        let page = api
            .mail_threads(ThreadListInput {
                mailbox_id: query.mailbox_id,
                folder: query.folder,
                label_id: query.label_id,
                starred: query.starred,
                unread: query.unread,
                sent: query.sent,
                cursor: query.cursor,
                limit: query.limit,
            })
            .await?;
        Ok(super::list::<_, MailThreadSummary>(page.items, page.cursor))
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/mail/threads/{thread_id}",
    operation_id = "public_get_mail_thread",
    tag = "public-api",
    summary = "A mail thread with its messages, oldest first",
    description = "Each message has its sender, recipients, plain-text body (`body_text`, also for HTML mail), attachments' metadata (download them with `GET /mail/attachments/{attachment_id}`), authentication results and, for mail you sent, its delivery per recipient. Reading does not mark the thread read. Scope: `mail:read`.",
    params(("workspace_id" = String, Path, description = WORKSPACE), ("thread_id" = String, Path)),
    responses((status = 200, description = "The thread", body = MailThreadDetail)),
    security(("bearerAuth" = []))
)]
pub(super) async fn thread(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, thread)): Path<(String, String)>,
) -> Response {
    let thread = path_id!(ctx, thread);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::MailRead, &ctx.request_id)
            .await?;
        api.mail_thread(thread).await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    patch,
    path = "/api/public/v1/workspaces/{workspace_id}/mail/threads/{thread_id}",
    operation_id = "public_update_mail_thread",
    tag = "public-api",
    summary = "Move, mark, star or label a thread",
    description = "Scope: `mail:write`.",
    params(("workspace_id" = String, Path, description = WORKSPACE), ("thread_id" = String, Path)),
    request_body = UpdateThreadBody,
    responses((status = 200, description = "The thread", body = MailThreadSummary)),
    security(("bearerAuth" = []))
)]
pub(super) async fn update_thread(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, thread)): Path<(String, String)>,
    body: Result<Json<UpdateThreadBody>, JsonRejection>,
) -> Response {
    let thread = path_id!(ctx, thread);
    let body = input!(ctx, ApiTokenScope::MailWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::MailWrite, &ctx.request_id)
            .await?;
        api.update_mail_thread(thread, &body.change()?).await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/mail/threads/bulk",
    operation_id = "public_update_mail_threads",
    tag = "public-api",
    summary = "The same change for up to 100 threads",
    description = "Scope: `mail:write`.",
    params(("workspace_id" = String, Path, description = WORKSPACE)),
    request_body = UpdateThreadsBody,
    responses((status = 200, description = "The changed threads; `next_cursor` is `null`", body = List<MailThreadSummary>)),
    security(("bearerAuth" = []))
)]
pub(super) async fn update_threads(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    body: Result<Json<UpdateThreadsBody>, JsonRejection>,
) -> Response {
    let body = input!(ctx, ApiTokenScope::MailWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::MailWrite, &ctx.request_id)
            .await?;
        let thread_ids = ids(&body.ids, "ids must be thread ids")?;
        let change = UpdateThreadBody {
            folder: body.folder,
            restore: body.restore,
            read: body.read,
            starred: body.starred,
            add_labels: body.add_labels,
            remove_labels: body.remove_labels,
        }
        .change()?;
        let items = api.update_mail_threads(&thread_ids, &change).await?;
        Ok(super::list::<_, MailThreadSummary>(items, None))
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/mail/search",
    operation_id = "public_search_mail",
    tag = "public-api",
    summary = "Search the threads of your mailboxes, newest first",
    description = "Empty when `q` has nothing to look for. Scope: `mail:read`.",
    params(("workspace_id" = String, Path, description = WORKSPACE), SearchQuery),
    responses((status = 200, description = "A page of threads", body = List<MailThreadSummary>)),
    security(("bearerAuth" = []))
)]
pub(super) async fn search(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    query: Result<Query<SearchQuery>, QueryRejection>,
) -> Response {
    let query = input!(ctx, ApiTokenScope::MailRead, query);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::MailRead, &ctx.request_id)
            .await?;
        let page = api
            .search_mail(&query.q, query.cursor.as_deref(), query.limit)
            .await?;
        Ok(super::list::<_, MailThreadSummary>(page.items, page.cursor))
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/mail/counts",
    operation_id = "public_get_mail_counts",
    tag = "public-api",
    summary = "Unread threads in each folder of each mailbox you read",
    description = "Scope: `mail:read`.",
    params(("workspace_id" = String, Path, description = WORKSPACE)),
    responses((status = 200, description = "The counts", body = MailCounts)),
    security(("bearerAuth" = []))
)]
pub(super) async fn counts(State(s): State<PublicApiState>, ctx: Ctx) -> Response {
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::MailRead, &ctx.request_id)
            .await?;
        api.mail_counts().await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/mail/attachments/{attachment_id}",
    operation_id = "public_download_mail_attachment",
    tag = "public-api",
    summary = "Download an attachment of a message you read",
    description = "Always a download (`Content-Disposition: attachment`, `nosniff`); the type is an image type when the bytes are a PNG, JPEG, GIF, WebP or AVIF image, else `application/octet-stream`. Scope: `mail:read`.",
    params(("workspace_id" = String, Path, description = WORKSPACE), ("attachment_id" = String, Path, description = "An `attachments[].id` of a message, from `GET /mail/threads/{thread_id}`.")),
    responses((status = 200, description = "The file", content_type = "application/octet-stream")),
    security(("bearerAuth" = []))
)]
pub(super) async fn attachment(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, attachment)): Path<(String, String)>,
) -> Response {
    let attachment = path_id!(ctx, attachment);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::MailRead, &ctx.request_id)
            .await?;
        api.mail_attachment(attachment).await
    }
    .await;
    let file = match result {
        Ok(file) => file,
        Err(error) => return ctx.error(error),
    };
    let disposition = format!(
        "attachment; filename=\"{}\"; filename*={}",
        ascii_file_name(&file.filename),
        rfc5987(&file.filename)
    );
    let mut response = Response::new(Body::from(file.bytes));
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(file.image_type.unwrap_or("application/octet-stream")),
    );
    if let Ok(value) = HeaderValue::from_str(&disposition) {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    response
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/mail/drafts",
    operation_id = "public_list_mail_drafts",
    tag = "public-api",
    summary = "Your drafts, the most recently changed first",
    description = "Scope: `mail:read`.",
    params(("workspace_id" = String, Path, description = WORKSPACE), DraftsQuery),
    responses((status = 200, description = "Every draft; `next_cursor` is `null`", body = List<MailDraftView>)),
    security(("bearerAuth" = []))
)]
pub(super) async fn drafts(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    query: Result<Query<DraftsQuery>, QueryRejection>,
) -> Response {
    let query = input!(ctx, ApiTokenScope::MailRead, query);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::MailRead, &ctx.request_id)
            .await?;
        Ok(super::list::<_, MailDraftView>(
            api.mail_drafts(query.mailbox_id).await?,
            None,
        ))
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/mail/drafts",
    operation_id = "public_create_mail_draft",
    tag = "public-api",
    summary = "Start a draft: a new message, a reply or a forward",
    description = "You have one draft for each thread: when there is one, it is returned with `200` (a different `mode` changes its mode); a new one is `201`. Send it with `POST /mail/drafts/{draft_id}/send`. Scope: `mail:write`.",
    params(("workspace_id" = String, Path, description = WORKSPACE)),
    request_body = CreateDraftBody,
    responses((status = 201, description = "The new draft", body = MailDraftView), (status = 200, description = "Your draft of the thread", body = MailDraftView)),
    security(("bearerAuth" = []))
)]
pub(super) async fn create_draft(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    body: Result<Json<CreateDraftBody>, JsonRejection>,
) -> Response {
    let body = input!(ctx, ApiTokenScope::MailWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::MailWrite, &ctx.request_id)
            .await?;
        api.create_mail_draft(NewDraft {
            mode: body.mode,
            thread_id: body.thread_id,
            parent_message_id: body.parent_message_id,
            from_address_id: body.from_address_id,
            to: body.to,
            cc: body.cc,
            bcc: body.bcc,
            subject: body.subject,
            body_html: body.body_html,
        })
        .await
    }
    .await;
    match result {
        Ok((draft, created)) => {
            let status = if created {
                StatusCode::CREATED
            } else {
                StatusCode::OK
            };
            (status, Json(draft)).into_response()
        }
        Err(error) => ctx.error(error),
    }
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/mail/drafts/{draft_id}",
    operation_id = "public_get_mail_draft",
    tag = "public-api",
    summary = "A draft of yours",
    description = "Scope: `mail:read`.",
    params(("workspace_id" = String, Path, description = WORKSPACE), ("draft_id" = String, Path)),
    responses((status = 200, description = "The draft", body = MailDraftView)),
    security(("bearerAuth" = []))
)]
pub(super) async fn draft(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, draft)): Path<(String, String)>,
) -> Response {
    let draft = path_id!(ctx, draft);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::MailRead, &ctx.request_id)
            .await?;
        api.mail_draft(draft).await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    patch,
    path = "/api/public/v1/workspaces/{workspace_id}/mail/drafts/{draft_id}",
    operation_id = "public_update_mail_draft",
    tag = "public-api",
    summary = "Change a draft of yours",
    description = "A stale `version` is a `409 draft_conflict`: read the draft again. A draft that is being sent is a `409 draft_sending`: cancel the send first. Scope: `mail:write`.",
    params(("workspace_id" = String, Path, description = WORKSPACE), ("draft_id" = String, Path)),
    request_body = UpdateDraftBody,
    responses((status = 200, description = "The draft with its new version", body = MailDraftView)),
    security(("bearerAuth" = []))
)]
pub(super) async fn update_draft(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, draft)): Path<(String, String)>,
    body: Result<Json<UpdateDraftBody>, JsonRejection>,
) -> Response {
    let draft = path_id!(ctx, draft);
    let body = input!(ctx, ApiTokenScope::MailWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::MailWrite, &ctx.request_id)
            .await?;
        api.update_mail_draft(
            draft,
            DraftChange {
                version: body.version,
                from_address_id: body.from_address_id,
                mode: body.mode,
                to: body.to,
                cc: body.cc,
                bcc: body.bcc,
                subject: body.subject,
                body_html: body.body_html,
            },
        )
        .await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    delete,
    path = "/api/public/v1/workspaces/{workspace_id}/mail/drafts/{draft_id}",
    operation_id = "public_delete_mail_draft",
    tag = "public-api",
    summary = "Discard a draft of yours and its files",
    description = "Scope: `mail:write`.",
    params(("workspace_id" = String, Path, description = WORKSPACE), ("draft_id" = String, Path)),
    responses((status = 204, description = "The draft is deleted")),
    security(("bearerAuth" = []))
)]
pub(super) async fn delete_draft(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, draft)): Path<(String, String)>,
) -> Response {
    let draft = path_id!(ctx, draft);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::MailWrite, &ctx.request_id)
            .await?;
        api.delete_mail_draft(draft).await
    }
    .await;
    no_content(&ctx, result)
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/mail/drafts/{draft_id}/check",
    operation_id = "public_check_mail_draft",
    tag = "public-api",
    summary = "Which recipients of a draft a send would skip",
    description = "The recipients on the workspace's suppression list (bounces, complaints). It tells nothing of the rest of the list. Scope: `mail:read`.",
    params(("workspace_id" = String, Path, description = WORKSPACE), ("draft_id" = String, Path)),
    responses((status = 200, description = "The suppressed recipients", body = MailDraftCheck)),
    security(("bearerAuth" = []))
)]
pub(super) async fn check_draft(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, draft)): Path<(String, String)>,
) -> Response {
    let draft = path_id!(ctx, draft);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::MailRead, &ctx.request_id)
            .await?;
        api.check_mail_draft(draft).await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/mail/drafts/{draft_id}/send",
    operation_id = "public_send_mail_draft",
    tag = "public-api",
    summary = "Send a draft of yours",
    description = "The message is held for your undo-send time (`GET /mail/settings`), or until `scheduled_at`, and goes then; `POST /mail/outbound/{outbound_id}/cancel` undoes it before. A message over a send limit waits in the Outbox. The workspace's send policy applies as in the web app (`403 sending_paused`, `403 send_as_denied`, `409 send_not_ready`, `422 no_recipients`, `422 message_too_large`). A second call returns the same message. Scope: `mail:write`.",
    params(("workspace_id" = String, Path, description = WORKSPACE), ("draft_id" = String, Path)),
    request_body(content = SendDraftBody, description = "Optional"),
    responses((status = 200, description = "The held message and the recipients it skips", body = MailSent)),
    security(("bearerAuth" = []))
)]
pub(super) async fn send_draft(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, draft)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    let draft = path_id!(ctx, draft);
    // The body is optional: an empty one sends now.
    let body = if body.iter().all(u8::is_ascii_whitespace) {
        Ok(Json(SendDraftBody::default()))
    } else {
        serde_json::from_slice(&body).map(Json)
    };
    let body = input!(ctx, ApiTokenScope::MailWrite, body);
    let result = async {
        let scheduled_at = instant(
            body.scheduled_at.as_deref(),
            "scheduled_at must be an RFC 3339 time",
        )?;
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::MailWrite, &ctx.request_id)
            .await?;
        api.send_mail_draft(draft, scheduled_at).await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/mail/drafts/{draft_id}/files",
    operation_id = "public_upload_mail_draft_file",
    tag = "public-api",
    summary = "Attach a file to a draft of yours",
    description = "Multipart with one `file` field, up to the server's upload limit; a draft has up to 50 files. With `inline=true` the file must be an image and gets a `content_id` for the body. Scope: `mail:write`.",
    params(("workspace_id" = String, Path, description = WORKSPACE), ("draft_id" = String, Path), UploadQuery),
    request_body(content = DraftFileUpload, content_type = "multipart/form-data"),
    responses((status = 201, description = "The file", body = MailDraftFileView)),
    security(("bearerAuth" = []))
)]
pub(super) async fn upload_draft_file(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, draft)): Path<(String, String)>,
    query: Result<Query<UploadQuery>, QueryRejection>,
    request: Request,
) -> Response {
    let draft = path_id!(ctx, draft);
    let query = input!(ctx, ApiTokenScope::MailWrite, query);
    let api = match s
        .api
        .authorize(&ctx.principal, ApiTokenScope::MailWrite, &ctx.request_id)
        .await
    {
        Ok(api) => api,
        Err(error) => return ctx.error(error),
    };
    // Authorize before the body is read.
    let uploads = match api.mail_draft_uploads(draft).await {
        Ok(uploads) => uploads,
        Err(error) => return ctx.error(error),
    };
    let p = &ctx.principal;
    let mut staged = match stage_single_file(uploads, request, p.workspace_id, p.actor_id).await {
        Ok(staged) => staged,
        Err(failure) => return upload_failure(&ctx, failure),
    };
    let upload = staged.upload().clone();
    match api.add_mail_draft_file(draft, &upload, query.inline).await {
        Ok(file) => {
            staged.complete();
            (StatusCode::CREATED, Json(file)).into_response()
        }
        Err(error) => {
            staged.discard().await;
            ctx.error(error)
        }
    }
}

fn upload_failure(ctx: &Ctx, failure: StageFailure) -> Response {
    let too_large = || {
        problem_response(ctx.problem(
            StatusCode::PAYLOAD_TOO_LARGE,
            "upload_too_large",
            "Upload too large",
            "The upload exceeds the configured size limit.",
        ))
    };
    let invalid =
        || ctx.invalid("The request must be multipart with exactly one file field named \"file\".");
    match failure {
        StageFailure::TooLarge => too_large(),
        StageFailure::InvalidMultipart => invalid(),
        StageFailure::Upload(
            UploadError::FileTooLarge { .. } | UploadError::RequestTooLarge { .. },
        ) => too_large(),
        StageFailure::Upload(UploadError::StorageQuotaExceeded { .. }) => {
            problem_response(mail_problem(ctx, &MailError::QuotaExceeded))
        }
        StageFailure::Upload(UploadError::InvalidDisplayName) => {
            ctx.invalid("The file name is invalid.")
        }
        StageFailure::Upload(UploadError::BlobStore(BlobStoreError::Io { source, .. }))
            if source.kind() == std::io::ErrorKind::FileTooLarge =>
        {
            too_large()
        }
        StageFailure::Upload(UploadError::BlobStore(BlobStoreError::Io { source, .. }))
            if source.kind() == std::io::ErrorKind::InvalidData =>
        {
            invalid()
        }
        StageFailure::Upload(error) => with_cause(
            problem_response(Problem::internal(&ctx.request_id, &ctx.instance)),
            Some(error.to_string()),
        ),
    }
}

#[utoipa::path(
    patch,
    path = "/api/public/v1/workspaces/{workspace_id}/mail/drafts/{draft_id}/files/{file_id}",
    operation_id = "public_update_mail_draft_file",
    tag = "public-api",
    summary = "Send a draft's file as a download link, or as an attachment again",
    description = "Scope: `mail:write`.",
    params(("workspace_id" = String, Path, description = WORKSPACE), ("draft_id" = String, Path), ("file_id" = String, Path)),
    request_body = UpdateDraftFileBody,
    responses((status = 200, description = "The file", body = MailDraftFileView)),
    security(("bearerAuth" = []))
)]
pub(super) async fn update_draft_file(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, draft, file)): Path<(String, String, String)>,
    body: Result<Json<UpdateDraftFileBody>, JsonRejection>,
) -> Response {
    let draft = path_id!(ctx, draft);
    let file = path_id!(ctx, file);
    let body = input!(ctx, ApiTokenScope::MailWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::MailWrite, &ctx.request_id)
            .await?;
        api.set_mail_draft_file_link(draft, file, body.as_link)
            .await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    delete,
    path = "/api/public/v1/workspaces/{workspace_id}/mail/drafts/{draft_id}/files/{file_id}",
    operation_id = "public_delete_mail_draft_file",
    tag = "public-api",
    summary = "Remove a file from a draft of yours",
    description = "Scope: `mail:write`.",
    params(("workspace_id" = String, Path, description = WORKSPACE), ("draft_id" = String, Path), ("file_id" = String, Path)),
    responses((status = 204, description = "The file is removed")),
    security(("bearerAuth" = []))
)]
pub(super) async fn delete_draft_file(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, draft, file)): Path<(String, String, String)>,
) -> Response {
    let draft = path_id!(ctx, draft);
    let file = path_id!(ctx, file);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::MailWrite, &ctx.request_id)
            .await?;
        api.delete_mail_draft_file(draft, file).await
    }
    .await;
    no_content(&ctx, result)
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/mail/outbound",
    operation_id = "public_list_mail_outbound",
    tag = "public-api",
    summary = "Your scheduled messages, or your Outbox",
    description = "Scope: `mail:read`.",
    params(("workspace_id" = String, Path, description = WORKSPACE), OutboundQuery),
    responses((status = 200, description = "Every message; `next_cursor` is `null`", body = List<MailOutboundItemView>)),
    security(("bearerAuth" = []))
)]
pub(super) async fn outbound(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    query: Result<Query<OutboundQuery>, QueryRejection>,
) -> Response {
    let query = input!(ctx, ApiTokenScope::MailRead, query);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::MailRead, &ctx.request_id)
            .await?;
        Ok(super::list::<_, MailOutboundItemView>(
            api.mail_outbound(&query.view, query.mailbox_id).await?,
            None,
        ))
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/mail/outbound/{outbound_id}/cancel",
    operation_id = "public_cancel_mail_outbound",
    tag = "public-api",
    summary = "Undo a send, or cancel a scheduled one, before it goes",
    description = "The draft comes back. A message that went already is a `409 already_sent`. Scope: `mail:write`.",
    params(("workspace_id" = String, Path, description = WORKSPACE), ("outbound_id" = String, Path, description = "The `outbound.id` of a send.")),
    responses((status = 200, description = "The draft", body = MailDraftView)),
    security(("bearerAuth" = []))
)]
pub(super) async fn cancel_outbound(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, outbound)): Path<(String, String)>,
) -> Response {
    let outbound = path_id!(ctx, outbound);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::MailWrite, &ctx.request_id)
            .await?;
        api.cancel_mail_outbound(outbound).await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/mail/labels",
    operation_id = "public_list_mail_labels",
    tag = "public-api",
    summary = "The mail labels of the workspace, by name",
    description = "Scope: `mail:read`.",
    params(("workspace_id" = String, Path, description = WORKSPACE)),
    responses((status = 200, description = "Every label; `next_cursor` is `null`", body = List<MailLabelView>)),
    security(("bearerAuth" = []))
)]
pub(super) async fn labels(State(s): State<PublicApiState>, ctx: Ctx) -> Response {
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::MailRead, &ctx.request_id)
            .await?;
        Ok(super::list::<_, MailLabelView>(
            api.mail_labels().await?,
            None,
        ))
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/mail/labels",
    operation_id = "public_create_mail_label",
    tag = "public-api",
    summary = "Add a mail label",
    description = "Only workspace owners and admins manage labels (`403 mail_forbidden`). A name the workspace has already is a `409 label_exists`. Scope: `mail:write`.",
    params(("workspace_id" = String, Path, description = WORKSPACE)),
    request_body = CreateLabelBody,
    responses((status = 201, description = "The new label", body = MailLabelView)),
    security(("bearerAuth" = []))
)]
pub(super) async fn create_label(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    body: Result<Json<CreateLabelBody>, JsonRejection>,
) -> Response {
    let body = input!(ctx, ApiTokenScope::MailWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::MailWrite, &ctx.request_id)
            .await?;
        api.create_mail_label(&body.name, &body.color).await
    }
    .await;
    ctx.reply(StatusCode::CREATED, result)
}

#[utoipa::path(
    patch,
    path = "/api/public/v1/workspaces/{workspace_id}/mail/labels/{label_id}",
    operation_id = "public_update_mail_label",
    tag = "public-api",
    summary = "Rename or recolor a mail label",
    description = "Owners and admins only. Scope: `mail:write`.",
    params(("workspace_id" = String, Path, description = WORKSPACE), ("label_id" = String, Path)),
    request_body = UpdateLabelBody,
    responses((status = 200, description = "The label", body = MailLabelView)),
    security(("bearerAuth" = []))
)]
pub(super) async fn update_label(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, label)): Path<(String, String)>,
    body: Result<Json<UpdateLabelBody>, JsonRejection>,
) -> Response {
    let label = path_id!(ctx, label);
    let body = input!(ctx, ApiTokenScope::MailWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::MailWrite, &ctx.request_id)
            .await?;
        api.update_mail_label(label, body.name.as_deref(), body.color.as_deref())
            .await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    delete,
    path = "/api/public/v1/workspaces/{workspace_id}/mail/labels/{label_id}",
    operation_id = "public_delete_mail_label",
    tag = "public-api",
    summary = "Delete a mail label; threads lose it",
    description = "Owners and admins only. Scope: `mail:write`.",
    params(("workspace_id" = String, Path, description = WORKSPACE), ("label_id" = String, Path)),
    responses((status = 204, description = "The label is deleted")),
    security(("bearerAuth" = []))
)]
pub(super) async fn delete_label(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, label)): Path<(String, String)>,
) -> Response {
    let label = path_id!(ctx, label);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::MailWrite, &ctx.request_id)
            .await?;
        api.delete_mail_label(label).await
    }
    .await;
    no_content(&ctx, result)
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/mail/contacts",
    operation_id = "public_list_mail_contacts",
    tag = "public-api",
    summary = "Address suggestions",
    description = "At most 8: workspace members first, then the people you wrote to (by how often and how lately), then senders of mail in your mailboxes. Scope: `mail:read`.",
    params(("workspace_id" = String, Path, description = WORKSPACE), ContactsQuery),
    responses((status = 200, description = "The suggestions; `next_cursor` is `null`", body = List<MailContactView>)),
    security(("bearerAuth" = []))
)]
pub(super) async fn contacts(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    query: Result<Query<ContactsQuery>, QueryRejection>,
) -> Response {
    let query = input!(ctx, ApiTokenScope::MailRead, query);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::MailRead, &ctx.request_id)
            .await?;
        Ok(super::list::<_, MailContactView>(
            api.mail_contacts(&query.q).await?,
            None,
        ))
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/mail/from-addresses",
    operation_id = "public_list_mail_from_addresses",
    tag = "public-api",
    summary = "The addresses you can send as",
    description = "For a new message, every one; with `thread_id`, only those of the thread's mailbox. Each says whether it can send now (`ready`). Scope: `mail:read`.",
    params(("workspace_id" = String, Path, description = WORKSPACE), FromAddressesQuery),
    responses((status = 200, description = "The addresses; `next_cursor` is `null`", body = List<MailFromAddressView>)),
    security(("bearerAuth" = []))
)]
pub(super) async fn from_addresses(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    query: Result<Query<FromAddressesQuery>, QueryRejection>,
) -> Response {
    let query = input!(ctx, ApiTokenScope::MailRead, query);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::MailRead, &ctx.request_id)
            .await?;
        Ok(super::list::<_, MailFromAddressView>(
            api.mail_from_addresses(query.thread_id).await?,
            None,
        ))
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/mail/signatures/{address_id}",
    operation_id = "public_get_mail_signature",
    tag = "public-api",
    summary = "Your signature for an address you can send as",
    description = "Scope: `mail:read`.",
    params(("workspace_id" = String, Path, description = WORKSPACE), ("address_id" = String, Path, description = "An `address_id` from `GET /mail/from-addresses`.")),
    responses((status = 200, description = "The signature", body = MailSignatureView)),
    security(("bearerAuth" = []))
)]
pub(super) async fn signature(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, address)): Path<(String, String)>,
) -> Response {
    let address = path_id!(ctx, address);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::MailRead, &ctx.request_id)
            .await?;
        api.mail_signature(address).await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    put,
    path = "/api/public/v1/workspaces/{workspace_id}/mail/signatures/{address_id}",
    operation_id = "public_put_mail_signature",
    tag = "public-api",
    summary = "Set your signature for an address you can send as",
    description = "Scope: `mail:write`.",
    params(("workspace_id" = String, Path, description = WORKSPACE), ("address_id" = String, Path)),
    request_body = PutSignatureBody,
    responses((status = 200, description = "The signature as stored", body = MailSignatureView)),
    security(("bearerAuth" = []))
)]
pub(super) async fn put_signature(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, address)): Path<(String, String)>,
    body: Result<Json<PutSignatureBody>, JsonRejection>,
) -> Response {
    let address = path_id!(ctx, address);
    let body = input!(ctx, ApiTokenScope::MailWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::MailWrite, &ctx.request_id)
            .await?;
        api.put_mail_signature(address, &body.body_html).await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    delete,
    path = "/api/public/v1/workspaces/{workspace_id}/mail/signatures/{address_id}",
    operation_id = "public_delete_mail_signature",
    tag = "public-api",
    summary = "Remove your signature for an address",
    description = "Scope: `mail:write`.",
    params(("workspace_id" = String, Path, description = WORKSPACE), ("address_id" = String, Path)),
    responses((status = 204, description = "The signature is removed")),
    security(("bearerAuth" = []))
)]
pub(super) async fn delete_signature(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, address)): Path<(String, String)>,
) -> Response {
    let address = path_id!(ctx, address);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::MailWrite, &ctx.request_id)
            .await?;
        api.delete_mail_signature(address).await
    }
    .await;
    no_content(&ctx, result)
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/mail/snippets",
    operation_id = "public_list_mail_snippets",
    tag = "public-api",
    summary = "Your snippets (saved texts for the composer), by name",
    description = "Scope: `mail:read`.",
    params(("workspace_id" = String, Path, description = WORKSPACE)),
    responses((status = 200, description = "Every snippet; `next_cursor` is `null`", body = List<MailSnippetView>)),
    security(("bearerAuth" = []))
)]
pub(super) async fn snippets(State(s): State<PublicApiState>, ctx: Ctx) -> Response {
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::MailRead, &ctx.request_id)
            .await?;
        Ok(super::list::<_, MailSnippetView>(
            api.mail_snippets().await?,
            None,
        ))
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/mail/snippets",
    operation_id = "public_create_mail_snippet",
    tag = "public-api",
    summary = "Add a snippet",
    description = "Scope: `mail:write`.",
    params(("workspace_id" = String, Path, description = WORKSPACE)),
    request_body = CreateSnippetBody,
    responses((status = 201, description = "The new snippet", body = MailSnippetView)),
    security(("bearerAuth" = []))
)]
pub(super) async fn create_snippet(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    body: Result<Json<CreateSnippetBody>, JsonRejection>,
) -> Response {
    let body = input!(ctx, ApiTokenScope::MailWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::MailWrite, &ctx.request_id)
            .await?;
        api.create_mail_snippet(&body.name, &body.body_html).await
    }
    .await;
    ctx.reply(StatusCode::CREATED, result)
}

#[utoipa::path(
    patch,
    path = "/api/public/v1/workspaces/{workspace_id}/mail/snippets/{snippet_id}",
    operation_id = "public_update_mail_snippet",
    tag = "public-api",
    summary = "Change a snippet of yours",
    description = "Scope: `mail:write`.",
    params(("workspace_id" = String, Path, description = WORKSPACE), ("snippet_id" = String, Path)),
    request_body = UpdateSnippetBody,
    responses((status = 200, description = "The snippet", body = MailSnippetView)),
    security(("bearerAuth" = []))
)]
pub(super) async fn update_snippet(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, snippet)): Path<(String, String)>,
    body: Result<Json<UpdateSnippetBody>, JsonRejection>,
) -> Response {
    let snippet = path_id!(ctx, snippet);
    let body = input!(ctx, ApiTokenScope::MailWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::MailWrite, &ctx.request_id)
            .await?;
        api.update_mail_snippet(snippet, body.name.as_deref(), body.body_html.as_deref())
            .await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    delete,
    path = "/api/public/v1/workspaces/{workspace_id}/mail/snippets/{snippet_id}",
    operation_id = "public_delete_mail_snippet",
    tag = "public-api",
    summary = "Delete a snippet of yours",
    description = "Scope: `mail:write`.",
    params(("workspace_id" = String, Path, description = WORKSPACE), ("snippet_id" = String, Path)),
    responses((status = 204, description = "The snippet is deleted")),
    security(("bearerAuth" = []))
)]
pub(super) async fn delete_snippet(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, snippet)): Path<(String, String)>,
) -> Response {
    let snippet = path_id!(ctx, snippet);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::MailWrite, &ctx.request_id)
            .await?;
        api.delete_mail_snippet(snippet).await
    }
    .await;
    no_content(&ctx, result)
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/mail/settings",
    operation_id = "public_get_mail_settings",
    tag = "public-api",
    summary = "Your mail settings",
    description = "Scope: `mail:read`.",
    params(("workspace_id" = String, Path, description = WORKSPACE)),
    responses((status = 200, description = "The settings", body = MailSettingsView)),
    security(("bearerAuth" = []))
)]
pub(super) async fn settings(State(s): State<PublicApiState>, ctx: Ctx) -> Response {
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::MailRead, &ctx.request_id)
            .await?;
        api.mail_settings().await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    put,
    path = "/api/public/v1/workspaces/{workspace_id}/mail/settings",
    operation_id = "public_put_mail_settings",
    tag = "public-api",
    summary = "Set your mail settings",
    description = "Scope: `mail:write`.",
    params(("workspace_id" = String, Path, description = WORKSPACE)),
    request_body = PutSettingsBody,
    responses((status = 200, description = "The settings", body = MailSettingsView)),
    security(("bearerAuth" = []))
)]
pub(super) async fn put_settings(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    body: Result<Json<PutSettingsBody>, JsonRejection>,
) -> Response {
    let body = input!(ctx, ApiTokenScope::MailWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::MailWrite, &ctx.request_id)
            .await?;
        api.put_mail_settings(body.undo_send_seconds).await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/mail/mailboxes/{mailbox_id}/vacation",
    operation_id = "public_get_mail_vacation",
    tag = "public-api",
    summary = "The vacation reply of a mailbox",
    description = "The owner of a personal mailbox, and the members of a shared one, read it. Scope: `mail:read`.",
    params(("workspace_id" = String, Path, description = WORKSPACE), ("mailbox_id" = String, Path)),
    responses((status = 200, description = "The vacation reply", body = MailVacationView)),
    security(("bearerAuth" = []))
)]
pub(super) async fn vacation(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, mailbox)): Path<(String, String)>,
) -> Response {
    let mailbox = path_id!(ctx, mailbox);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::MailRead, &ctx.request_id)
            .await?;
        api.mail_vacation(mailbox).await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    put,
    path = "/api/public/v1/workspaces/{workspace_id}/mail/mailboxes/{mailbox_id}/vacation",
    operation_id = "public_put_mail_vacation",
    tag = "public-api",
    summary = "Set the vacation reply of a mailbox",
    description = "The owner of a personal mailbox, or an owner or admin for a shared one. Scope: `mail:write`.",
    params(("workspace_id" = String, Path, description = WORKSPACE), ("mailbox_id" = String, Path)),
    request_body = PutVacationBody,
    responses((status = 200, description = "The vacation reply as stored", body = MailVacationView)),
    security(("bearerAuth" = []))
)]
pub(super) async fn put_vacation(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, mailbox)): Path<(String, String)>,
    body: Result<Json<PutVacationBody>, JsonRejection>,
) -> Response {
    let mailbox = path_id!(ctx, mailbox);
    let body = input!(ctx, ApiTokenScope::MailWrite, body);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::MailWrite, &ctx.request_id)
            .await?;
        api.put_mail_vacation(
            mailbox,
            VacationChange {
                enabled: body.enabled,
                starts_at: body.starts_at,
                ends_at: body.ends_at,
                subject: body.subject,
                body_html: body.body_html,
                contacts_only: body.contacts_only,
            },
        )
        .await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}
