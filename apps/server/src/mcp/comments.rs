//! Task comment edits and doc comments.
use rmcp::ErrorData;
use rmcp::model::{CallToolRequestParams, Tool};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{arguments, definition, parse_id, write_definition};
use crate::public_api::CommentOut;
use crate::repositories::api_tokens::ApiTokenScope;
use crate::token_api::{Authorized, TokenApiError};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct UpdateCommentArgs {
    /// Task id, or its identifier such as ENG-12.
    task_id: String,
    /// The comment's id, from list_comments.
    comment_id: String,
    /// The comment's `version` as read.
    expected_version: u64,
    /// The new comment as Markdown; `@Name` mentions a member.
    body: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct DeleteCommentArgs {
    /// Task id, or its identifier such as ENG-12.
    task_id: String,
    /// The comment's id, from list_comments.
    comment_id: String,
    /// The comment's `version` as read.
    expected_version: u64,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct DocCommentsArgs {
    /// The doc's id, from list_docs.
    page_id: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CreateDocCommentArgs {
    /// The doc's id, from list_docs.
    page_id: String,
    /// The comment as Markdown: paragraphs, bold, italic, code, links and `@Name` mentions.
    /// Headings and list items become plain paragraphs.
    body: String,
    /// Reply in this thread, from list_doc_comments. Send this or `quote`.
    thread_id: Option<String>,
    /// Start a new thread on this text of the doc, up to 1,000 characters. Send this or
    /// `thread_id`.
    quote: Option<String>,
}

pub(super) fn scope(name: &str) -> Option<ApiTokenScope> {
    match name {
        "update_comment" | "delete_comment" => Some(ApiTokenScope::TasksWrite),
        "list_doc_comments" => Some(ApiTokenScope::DocsRead),
        "create_doc_comment" => Some(ApiTokenScope::DocsWrite),
        _ => None,
    }
}

pub(super) fn tools() -> Vec<Tool> {
    vec![
        write_definition::<UpdateCommentArgs>(
            "update_comment",
            "Replace the Markdown body of a task comment you wrote. Returns the comment with its new version.",
            true,
        ),
        write_definition::<DeleteCommentArgs>(
            "delete_comment",
            "Delete a task comment: your own, or any comment if you may moderate comments in the project. This cannot be undone.",
            true,
        ),
        definition::<DocCommentsArgs>(
            "list_doc_comments",
            "Read the comment threads of a visible doc, oldest first: each thread's quoted text, resolved state and comments as Markdown, with author names.",
        ),
        write_definition::<CreateDocCommentArgs>(
            "create_doc_comment",
            "Comment on a visible doc as Markdown: reply in a thread (thread_id), or start a thread on a quoted text of the doc (quote). Returns the new comment with its thread_id.",
            false,
        ),
    ]
}

/// Runs a tool of this module; `None` for another tool's name.
pub(super) async fn call(
    api: &Authorized<'_>,
    request: &CallToolRequestParams,
) -> Option<Result<Result<Value, TokenApiError>, ErrorData>> {
    Some(match request.name.as_ref() {
        "update_comment" => {
            let run = async {
                let a: UpdateCommentArgs = arguments(request)?;
                Ok(api
                    .update_comment(
                        &a.task_id,
                        parse_id(&a.comment_id)?,
                        a.expected_version,
                        a.body,
                    )
                    .await
                    .map(|comment| json!(CommentOut::from(comment))))
            };
            run.await
        }
        "delete_comment" => {
            let run = async {
                let a: DeleteCommentArgs = arguments(request)?;
                Ok(api
                    .delete_comment(&a.task_id, parse_id(&a.comment_id)?, a.expected_version)
                    .await
                    .map(|()| json!({ "deleted": true })))
            };
            run.await
        }
        "list_doc_comments" => {
            let run = async {
                let a: DocCommentsArgs = arguments(request)?;
                Ok(api
                    .doc_comments(parse_id(&a.page_id)?)
                    .await
                    .map(|threads| json!({ "threads": threads })))
            };
            run.await
        }
        "create_doc_comment" => {
            let run = async {
                let a: CreateDocCommentArgs = arguments(request)?;
                let thread_id = a.thread_id.as_deref().map(parse_id).transpose()?;
                Ok(api
                    .create_doc_comment(parse_id(&a.page_id)?, thread_id, a.quote, &a.body)
                    .await
                    .map(|comment| json!(comment)))
            };
            run.await
        }
        _ => return None,
    })
}
