//! Drafts (spec §7): the caller's own unsent messages and their files. Another member's draft is
//! `mail_not_found`. A change names the `version` it was made from; a stale one is a 409
//! `draft_conflict` that carries the current draft in `conflict.current`.

use std::io;

use axum::Json;
use axum::extract::{Path, Request, State};
use axum::http::{HeaderMap, StatusCode, Uri};
use orbit_platform::{BlobStoreError, Id, TimestampMillis, UploadError};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use super::{Call, MailState, RequestIdExtension};
use crate::attachment_routes::{StageFailure, stage_single_file, storage_quota_exceeded};
use crate::repositories::mail::{Draft, DraftChange, DraftFile, DraftMode, MailError, NewDraft};
use crate::task_routes::{ApiError, ApiJson, ApiQuery};

#[derive(Serialize, ToSchema)]
pub(crate) struct MailDraftFile {
    #[schema(value_type = String)]
    id: Id,
    filename: String,
    content_type: String,
    size: i64,
    content_id: Option<String>,
    /// Sent as a download link instead of an attachment.
    as_link: bool,
    /// The received attachment a forward copied this file from.
    #[schema(value_type = Option<String>)]
    source_attachment_id: Option<Id>,
}

fn file_view(file: DraftFile) -> MailDraftFile {
    MailDraftFile {
        id: file.id,
        filename: file.filename,
        content_type: file.content_type,
        size: file.size,
        content_id: file.content_id,
        as_link: file.as_link,
        source_attachment_id: file.source_attachment_id,
    }
}

#[derive(Serialize, ToSchema)]
pub(crate) struct MailDraft {
    #[schema(value_type = String)]
    id: Id,
    #[schema(value_type = String)]
    mailbox_id: Id,
    #[schema(value_type = String)]
    from_address_id: Id,
    #[schema(value_type = Option<String>)]
    thread_id: Option<Id>,
    #[schema(value_type = Option<String>)]
    parent_message_id: Option<Id>,
    /// `new`, `reply`, `reply_all`, `forward` or `forward_attached`.
    #[schema(value_type = String)]
    mode: DraftMode,
    to: Vec<String>,
    cc: Vec<String>,
    bcc: Vec<String>,
    subject: String,
    body_html: String,
    /// Send it back in the next change.
    version: i64,
    files: Vec<MailDraftFile>,
    #[schema(value_type = String, format = DateTime)]
    created_at: TimestampMillis,
    #[schema(value_type = String, format = DateTime)]
    updated_at: TimestampMillis,
}

fn view(draft: Draft) -> MailDraft {
    MailDraft {
        id: draft.id,
        mailbox_id: draft.mailbox_id,
        from_address_id: draft.from_address_id,
        thread_id: draft.thread_id,
        parent_message_id: draft.parent_message_id,
        mode: draft.mode,
        to: draft.to,
        cc: draft.cc,
        bcc: draft.bcc,
        subject: draft.subject,
        body_html: draft.body_html,
        version: draft.version,
        files: draft.files.into_iter().map(file_view).collect(),
        created_at: draft.created_at,
        updated_at: draft.updated_at,
    }
}

#[derive(Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub(crate) struct ListMailDraftsQuery {
    /// Only the drafts of this mailbox.
    mailbox_id: Option<String>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreateMailDraftBody {
    /// `new` (needs `from_address_id`), or `reply`, `reply_all`, `forward`, `forward_attached`
    /// (need `thread_id`).
    #[schema(value_type = String)]
    mode: DraftMode,
    thread_id: Option<String>,
    /// The message answered; the newest of the thread when absent.
    parent_message_id: Option<String>,
    /// Must be an address the caller can send as. A reply defaults to the address that received
    /// the message.
    from_address_id: Option<String>,
    to: Option<Vec<String>>,
    cc: Option<Vec<String>>,
    bcc: Option<Vec<String>>,
    subject: Option<String>,
    /// Sanitized on save.
    body_html: Option<String>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct UpdateMailDraftBody {
    /// The version the change was made from.
    version: i64,
    from_address_id: Option<String>,
    /// Between `reply`, `reply_all`, `forward` and `forward_attached` of a thread draft.
    #[schema(value_type = Option<String>)]
    mode: Option<DraftMode>,
    to: Option<Vec<String>>,
    cc: Option<Vec<String>>,
    bcc: Option<Vec<String>>,
    subject: Option<String>,
    /// Sanitized on save.
    body_html: Option<String>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct UpdateMailDraftFileBody {
    as_link: bool,
}

/// The caller's drafts, the most recently changed first.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/mail/drafts", params(("workspace_id" = String, Path), ListMailDraftsQuery), responses((status = 200, body = Vec<MailDraft>)))]
pub(crate) async fn list_mail_drafts(
    State(state): State<MailState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiQuery(query): ApiQuery<ListMailDraftsQuery>,
) -> Result<Json<Vec<MailDraft>>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let membership = membership(&state, &call).await?;
    let mailbox = query
        .mailbox_id
        .as_deref()
        .map(|id| call.body_id(id, "mailbox_id"))
        .transpose()?;
    let drafts = state
        .mail
        .list_drafts(membership, mailbox)
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(drafts.into_iter().map(view).collect()))
}

async fn membership(state: &MailState, call: &Call<'_>) -> Result<Id, ApiError> {
    state
        .mail
        .mail_membership(call.workspace_id, call.actor_id)
        .await
        .map_err(|error| call.problem(error))
}

/// Starts a draft. A member has one draft for each thread: when there is one, it is returned
/// (and a different `mode` changes its mode).
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/mail/drafts", params(("workspace_id" = String, Path)), request_body = CreateMailDraftBody, responses((status = 201, body = MailDraft)))]
pub(crate) async fn create_mail_draft(
    State(state): State<MailState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<CreateMailDraftBody>,
) -> Result<(StatusCode, Json<MailDraft>), ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let membership = membership(&state, &call).await?;
    let new = NewDraft {
        mode: body.mode,
        thread_id: body
            .thread_id
            .as_deref()
            .map(|id| call.body_id(id, "thread_id"))
            .transpose()?,
        parent_message_id: body
            .parent_message_id
            .as_deref()
            .map(|id| call.body_id(id, "parent_message_id"))
            .transpose()?,
        from_address_id: body
            .from_address_id
            .as_deref()
            .map(|id| call.body_id(id, "from_address_id"))
            .transpose()?,
        to: body.to,
        cc: body.cc,
        bcc: body.bcc,
        subject: body.subject,
        body_html: body.body_html,
    };
    let draft = state
        .mail
        .create_draft(membership, call.workspace_id, new)
        .await
        .map_err(|error| call.problem(error))?;
    Ok((StatusCode::CREATED, Json(view(draft))))
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/mail/drafts/{draft_id}", params(("workspace_id" = String, Path), ("draft_id" = String, Path)), responses((status = 200, body = MailDraft)))]
pub(crate) async fn get_mail_draft(
    State(state): State<MailState>,
    Path((workspace, draft)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<MailDraft>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let draft_id = call.id(&draft)?;
    let membership = membership(&state, &call).await?;
    let draft = state
        .mail
        .get_draft(membership, draft_id)
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(view(draft)))
}

/// Changes a draft; absent fields keep their value. A stale `version` is a 409 with the current
/// draft in `conflict.current`.
#[utoipa::path(patch, path = "/api/v1/workspaces/{workspace_id}/mail/drafts/{draft_id}", params(("workspace_id" = String, Path), ("draft_id" = String, Path)), request_body = UpdateMailDraftBody, responses((status = 200, body = MailDraft)))]
pub(crate) async fn update_mail_draft(
    State(state): State<MailState>,
    Path((workspace, draft)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<UpdateMailDraftBody>,
) -> Result<Json<MailDraft>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let draft_id = call.id(&draft)?;
    let membership = membership(&state, &call).await?;
    let change = DraftChange {
        version: body.version,
        from_address_id: body
            .from_address_id
            .as_deref()
            .map(|id| call.body_id(id, "from_address_id"))
            .transpose()?,
        mode: body.mode,
        to: body.to,
        cc: body.cc,
        bcc: body.bcc,
        subject: body.subject,
        body_html: body.body_html,
    };
    match state.mail.update_draft(membership, draft_id, change).await {
        Ok(draft) => Ok(Json(view(draft))),
        Err(MailError::Conflict("draft_conflict")) => {
            let error = call.problem(MailError::Conflict("draft_conflict"));
            let current = state
                .mail
                .get_draft(membership, draft_id)
                .await
                .map_err(|error| call.problem(error))?;
            Err(error.with_current(
                serde_json::to_value(view(current)).unwrap_or(serde_json::Value::Null),
            ))
        }
        Err(error) => Err(call.problem(error)),
    }
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/mail/drafts/{draft_id}", params(("workspace_id" = String, Path), ("draft_id" = String, Path)), responses((status = 204)))]
pub(crate) async fn delete_mail_draft(
    State(state): State<MailState>,
    Path((workspace, draft)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<StatusCode, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let draft_id = call.id(&draft)?;
    let membership = membership(&state, &call).await?;
    state
        .mail
        .delete_draft(membership, draft_id)
        .await
        .map_err(|error| call.problem(error))?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(ToSchema)]
#[allow(dead_code)]
pub(crate) struct MailDraftFileUpload {
    #[schema(value_type = String, format = Binary)]
    file: Vec<u8>,
}

/// Attaches one file (multipart, one `file` field) to a draft of the caller, up to the upload
/// size limit.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/mail/drafts/{draft_id}/files", params(("workspace_id" = String, Path), ("draft_id" = String, Path)), request_body(content = MailDraftFileUpload, content_type = "multipart/form-data"), responses((status = 201, body = MailDraftFile)))]
pub(crate) async fn upload_mail_draft_file(
    State(state): State<MailState>,
    Path((workspace, draft)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    request: Request,
) -> Result<(StatusCode, Json<MailDraftFile>), ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let draft_id = call.id(&draft)?;
    let membership = membership(&state, &call).await?;
    // Authorize before the body is read.
    state
        .mail
        .authorize_draft_file(membership, draft_id)
        .await
        .map_err(|error| call.problem(error))?;
    let mut staged = stage_single_file(
        state.mail.uploads(),
        request,
        call.workspace_id,
        call.actor_id,
    )
    .await
    .map_err(|failure| match failure {
        StageFailure::TooLarge => upload_too_large(&call),
        StageFailure::InvalidMultipart => ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_multipart",
            "Invalid multipart upload",
            "The request must contain exactly one file field named \"file\".",
            call.instance.clone(),
            call.request_id,
        ),
        StageFailure::Upload(error) => upload_problem(&call, error),
    })?;
    let upload = staged.upload().clone();
    match state
        .mail
        .add_draft_file(membership, call.workspace_id, draft_id, &upload)
        .await
    {
        Ok(file) => {
            staged.complete();
            Ok((StatusCode::CREATED, Json(file_view(file))))
        }
        Err(error) => {
            staged.discard().await;
            Err(call.problem(error))
        }
    }
}

fn upload_too_large(call: &Call<'_>) -> ApiError {
    ApiError::new(
        StatusCode::PAYLOAD_TOO_LARGE,
        "upload_too_large",
        "Upload too large",
        "The upload exceeds the configured size limit.",
        call.instance.clone(),
        call.request_id,
    )
}

fn upload_problem(call: &Call<'_>, error: UploadError) -> ApiError {
    match error {
        UploadError::FileTooLarge { .. } | UploadError::RequestTooLarge { .. } => {
            upload_too_large(call)
        }
        UploadError::StorageQuotaExceeded { .. } => {
            storage_quota_exceeded(&call.instance, call.request_id)
        }
        UploadError::InvalidDisplayName => call.problem(MailError::Invalid { field: "file" }),
        UploadError::BlobStore(BlobStoreError::Io { source, .. })
            if source.kind() == io::ErrorKind::FileTooLarge =>
        {
            upload_too_large(call)
        }
        UploadError::BlobStore(BlobStoreError::Io { source, .. })
            if source.kind() == io::ErrorKind::InvalidData =>
        {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_multipart",
                "Invalid multipart upload",
                "The request must contain exactly one file field named \"file\".",
                call.instance.clone(),
                call.request_id,
            )
        }
        other => call.problem(MailError::Upload(other)),
    }
}

/// Sends a file as a download link instead of an attachment, or back.
#[utoipa::path(patch, path = "/api/v1/workspaces/{workspace_id}/mail/drafts/{draft_id}/files/{file_id}", params(("workspace_id" = String, Path), ("draft_id" = String, Path), ("file_id" = String, Path)), request_body = UpdateMailDraftFileBody, responses((status = 200, body = MailDraftFile)))]
pub(crate) async fn update_mail_draft_file(
    State(state): State<MailState>,
    Path((workspace, draft, file)): Path<(String, String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<UpdateMailDraftFileBody>,
) -> Result<Json<MailDraftFile>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let draft_id = call.id(&draft)?;
    let file_id = call.id(&file)?;
    let membership = membership(&state, &call).await?;
    let file = state
        .mail
        .set_file_as_link(membership, draft_id, file_id, body.as_link)
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(file_view(file)))
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/mail/drafts/{draft_id}/files/{file_id}", params(("workspace_id" = String, Path), ("draft_id" = String, Path), ("file_id" = String, Path)), responses((status = 204)))]
pub(crate) async fn delete_mail_draft_file(
    State(state): State<MailState>,
    Path((workspace, draft, file)): Path<(String, String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<StatusCode, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let draft_id = call.id(&draft)?;
    let file_id = call.id(&file)?;
    let membership = membership(&state, &call).await?;
    state
        .mail
        .remove_draft_file(membership, draft_id, file_id)
        .await
        .map_err(|error| call.problem(error))?;
    Ok(StatusCode::NO_CONTENT)
}
