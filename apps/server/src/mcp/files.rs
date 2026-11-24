//! Task and comment attachments, and doc files. Uploads carry the bytes as base64 in the tool
//! arguments (at most [`MAX_UPLOAD_BYTES`]); downloads return them as base64 up to
//! [`MAX_INLINE_DOWNLOAD_BYTES`], and always the public REST path that streams any size.
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use rmcp::ErrorData;
use rmcp::model::{CallToolRequestParams, Tool};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{arguments, definition, parse_id, write_definition};
use crate::repositories::api_tokens::ApiTokenScope;
use crate::token_api::{Authorized, FileDownload, MAX_INLINE_DOWNLOAD_BYTES, TokenApiError};

/// The largest file an upload tool takes. Its base64 fits in the MCP request body limit
/// ([`super::MAX_REQUEST_BODY_BYTES`]); larger files go through the REST upload.
pub(super) const MAX_UPLOAD_BYTES: usize = 1024 * 1024;
const _: () =
    assert!(MAX_UPLOAD_BYTES.div_ceil(3) * 4 + 64 * 1024 <= super::MAX_REQUEST_BODY_BYTES);

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ListAttachmentsArgs {
    /// Task id, or its identifier such as ENG-12.
    task_id: String,
    /// List the files of this comment of the task (from list_comments) instead of the task's own.
    comment_id: Option<String>,
    /// The next_cursor of the previous page.
    cursor: Option<String>,
    /// Files per page, from 1 to 100. Default: 50.
    limit: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct UploadAttachmentArgs {
    /// Task id, or its identifier such as ENG-12.
    task_id: String,
    /// Attach the file to this comment of the task instead of the task itself.
    comment_id: Option<String>,
    /// The file name with its extension, 1 to 255 bytes, such as `report.csv`.
    filename: String,
    /// The file's bytes, base64 (standard alphabet, with padding); at most 1 MiB decoded. The
    /// type is detected from the bytes.
    content_base64: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AttachmentArgs {
    /// Task id, or its identifier such as ENG-12.
    task_id: String,
    /// The comment that holds the file, for a comment's file.
    comment_id: Option<String>,
    /// A file id, from list_task_attachments.
    attachment_id: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct UploadDocFileArgs {
    /// A doc id, from list_docs.
    page_id: String,
    /// The file name with its extension, 1 to 255 bytes, such as `diagram.png`.
    filename: String,
    /// The file's bytes, base64 (standard alphabet, with padding); at most 1 MiB decoded.
    content_base64: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct DocFileArgs {
    page_id: String,
    /// The file id: the last segment of a doc file url (`/api/v1/workspaces/…/pages/{page_id}/files/{file_id}`).
    file_id: String,
}

pub(super) fn scope(name: &str) -> Option<ApiTokenScope> {
    match name {
        "list_task_attachments" | "download_task_attachment" => Some(ApiTokenScope::TasksRead),
        "upload_task_attachment" | "delete_task_attachment" => Some(ApiTokenScope::TasksWrite),
        "download_doc_file" => Some(ApiTokenScope::DocsRead),
        "upload_doc_file" => Some(ApiTokenScope::DocsWrite),
        _ => None,
    }
}

pub(super) fn tools() -> Vec<Tool> {
    vec![
        definition::<ListAttachmentsArgs>(
            "list_task_attachments",
            "The files attached to a task of an approved project, or to one of its comments (comment_id), oldest first: name, detected media type, size and download_path.",
        ),
        write_definition::<UploadAttachmentArgs>(
            "upload_task_attachment",
            "Attach a file to a task of an approved project, or to one of its comments (comment_id; its author or an admin only). Send the bytes as content_base64, at most 1 MiB; for larger files use the REST upload (multipart) of the public API. The workspace's storage quota applies. Not for service-account tokens.",
            false,
        ),
        write_definition::<AttachmentArgs>(
            "delete_task_attachment",
            "Delete a file of a task, or of one of its comments (comment_id; its author or an admin only). Not for service-account tokens.",
            true,
        ),
        definition::<AttachmentArgs>(
            "download_task_attachment",
            "Read a file of a task or comment. Files up to 64 KiB come back as content_base64; larger ones as null, with download_path to fetch through the public REST API (GET with the same token).",
        ),
        write_definition::<UploadDocFileArgs>(
            "upload_doc_file",
            "Upload a file (an image, a PDF…) to a doc that is not locked, at most 1 MiB as content_base64. Then put the returned url in the doc's Markdown with update_doc: ![name](url) shows an image, [name](url) links a file.",
            false,
        ),
        definition::<DocFileArgs>(
            "download_doc_file",
            "Read a file of a doc (an image or file url in its Markdown). Files up to 64 KiB come back as content_base64; larger ones as null, with download_path for the public REST API.",
        ),
    ]
}

/// Runs a tool of this module; `None` for another tool's name. Each tool's future is boxed: an
/// upload inline in the MCP dispatch overflowed a worker thread's stack (debug build).
pub(super) async fn call(
    api: &Authorized<'_>,
    request: &CallToolRequestParams,
) -> Option<Result<Result<Value, TokenApiError>, ErrorData>> {
    let result = match request.name.as_ref() {
        "list_task_attachments" => {
            let run = async {
                let a: ListAttachmentsArgs = arguments(request)?;
                let comment_id = a.comment_id.as_deref().map(parse_id).transpose()?;
                Ok(api
                    .attachments(&a.task_id, comment_id, a.cursor.as_deref(), a.limit)
                    .await
                    .map(|page| json!(page)))
            };
            Box::pin(run).await
        }
        "upload_task_attachment" => {
            let run = async {
                let a: UploadAttachmentArgs = arguments(request)?;
                let comment_id = a.comment_id.as_deref().map(parse_id).transpose()?;
                let bytes = decode(&a.content_base64)?;
                Ok(api
                    .upload_attachment(&a.task_id, comment_id, &a.filename, &bytes)
                    .await
                    .map(|file| json!(file)))
            };
            Box::pin(run).await
        }
        "delete_task_attachment" => {
            let run = async {
                let a: AttachmentArgs = arguments(request)?;
                let comment_id = a.comment_id.as_deref().map(parse_id).transpose()?;
                Ok(api
                    .delete_attachment(&a.task_id, comment_id, parse_id(&a.attachment_id)?)
                    .await
                    .map(|()| json!({ "deleted": true })))
            };
            Box::pin(run).await
        }
        "download_task_attachment" => {
            let run = async {
                let a: AttachmentArgs = arguments(request)?;
                let comment_id = a.comment_id.as_deref().map(parse_id).transpose()?;
                let download = api
                    .download_attachment(&a.task_id, comment_id, parse_id(&a.attachment_id)?)
                    .await;
                Ok(inline(download).await)
            };
            Box::pin(run).await
        }
        "upload_doc_file" => {
            let run = async {
                let a: UploadDocFileArgs = arguments(request)?;
                let bytes = decode(&a.content_base64)?;
                Ok(api
                    .upload_doc_file(parse_id(&a.page_id)?, &a.filename, &bytes)
                    .await
                    .map(|file| json!(file)))
            };
            Box::pin(run).await
        }
        "download_doc_file" => {
            let run = async {
                let a: DocFileArgs = arguments(request)?;
                let download = api
                    .download_doc_file(parse_id(&a.page_id)?, parse_id(&a.file_id)?)
                    .await;
                Ok(inline(download).await)
            };
            Box::pin(run).await
        }
        _ => return None,
    };
    Some(result)
}

fn decode(raw: &str) -> Result<Vec<u8>, ErrorData> {
    let too_large = || {
        ErrorData::invalid_params(
            "content_base64 holds more than 1 MiB; upload larger files through the public REST API (multipart)",
            None,
        )
    };
    if raw.len() > MAX_UPLOAD_BYTES.div_ceil(3) * 4 {
        return Err(too_large());
    }
    let bytes = STANDARD
        .decode(raw.trim())
        .map_err(|_| ErrorData::invalid_params("content_base64 is not valid base64", None))?;
    if bytes.len() > MAX_UPLOAD_BYTES {
        return Err(too_large());
    }
    Ok(bytes)
}

/// The file's type and path, and its bytes when small enough.
async fn inline(download: Result<FileDownload, TokenApiError>) -> Result<Value, TokenApiError> {
    let download = download?;
    let content_type = download.blob.metadata.content_type.clone();
    let deleted = download.blob.deleted;
    let download_path = download.download_path.clone();
    if deleted {
        return Ok(json!({
            "deleted": true,
            "content_type": null,
            "size_bytes": null,
            "content_base64": null,
            "download_path": download_path,
        }));
    }
    let bytes = download.read_capped(MAX_INLINE_DOWNLOAD_BYTES).await?;
    Ok(json!({
        "deleted": false,
        "content_type": content_type,
        "size_bytes": bytes.as_ref().map(Vec::len),
        "content_base64": bytes.map(|bytes| STANDARD.encode(bytes)),
        "download_path": download_path,
        "inline_limit_bytes": MAX_INLINE_DOWNLOAD_BYTES,
    }))
}
