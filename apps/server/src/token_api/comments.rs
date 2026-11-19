//! Task comment edits and doc comments.
use std::collections::HashMap;

use orbit_platform::{Id, TimestampMillis};
use serde::Serialize;
use serde_json::{Value, json};

use super::{Authorized, Result, TokenApiError, hidden_page};
use crate::collab::kind_task::members;
use crate::export::markdown::{KeepLinks, blocks_to_markdown};
use crate::repositories::api_tokens::ApiTokenScope;
use crate::repositories::page_comments::{
    CommentBody, PageComment, PageCommentError, PageCommentRepository,
};
use crate::repositories::tasks::{CommentContent, CommentRecord};

/// Characters of the quoted text of a new doc thread.
const MAX_QUOTE_CHARS: usize = 1_000;

/// A member as an agent reads it.
#[derive(Clone, Debug, Serialize)]
pub struct Person {
    pub id: Id,
    pub name: String,
}

/// A doc comment thread with its comments oldest first.
#[derive(Debug, Serialize)]
pub struct DocThread {
    pub id: Id,
    /// The doc text the thread was started on.
    pub quote: String,
    pub resolved: bool,
    pub resolved_by: Option<Person>,
    pub created_by: Person,
    pub created_at: TimestampMillis,
    pub comments: Vec<DocComment>,
}

#[derive(Debug, Serialize)]
pub struct DocComment {
    pub id: Id,
    pub thread_id: Id,
    pub author: Person,
    /// Markdown; empty once deleted.
    pub body: String,
    pub mentioned_user_ids: Vec<Id>,
    pub created_at: TimestampMillis,
    pub edited_at: Option<TimestampMillis>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub deleted: bool,
}

impl Authorized<'_> {
    /// Edits a comment that the token's user wrote. A service account acts as the workspace
    /// owner, so it could edit the owner's comments: it gets `NotAvailable`.
    pub async fn update_comment(
        &self,
        task_ref: &str,
        comment_id: Id,
        expected_version: u64,
        body: String,
    ) -> Result<CommentRecord> {
        self.need(ApiTokenScope::TasksWrite)?;
        if body.trim().is_empty() || body.len() > 100_000 {
            return Err(TokenApiError::Invalid("Invalid comment body"));
        }
        self.personal()?;
        let task = self.task(task_ref).await?;
        let p = self.principal;
        self.granted()
            .update_comment(
                p.workspace_id,
                task.id,
                comment_id,
                p.actor_id,
                CommentContent::Markdown {
                    body,
                    mentioned_user_ids: Vec::new(),
                },
                expected_version,
                self.request_id,
                TimestampMillis::now(),
            )
            .await
            .map_err(comment_forbidden)
    }

    /// Deletes a comment of the token's user, or any comment with comment moderation. Not for
    /// service accounts (see [`Self::update_comment`]).
    pub async fn delete_comment(
        &self,
        task_ref: &str,
        comment_id: Id,
        expected_version: u64,
    ) -> Result<()> {
        self.need(ApiTokenScope::TasksWrite)?;
        self.personal()?;
        let task = self.task(task_ref).await?;
        let p = self.principal;
        self.granted()
            .delete_comment(
                p.workspace_id,
                task.id,
                comment_id,
                p.actor_id,
                expected_version,
                self.request_id,
                TimestampMillis::now(),
            )
            .await
            .map_err(comment_forbidden)
    }

    /// The open and resolved comment threads of a visible doc, oldest first.
    pub async fn doc_comments(&self, page_id: Id) -> Result<Vec<DocThread>> {
        let repository = self
            .doc_comment_repository(page_id, ApiTokenScope::DocsRead)
            .await?;
        let p = self.principal;
        let list = repository
            .threads(p.workspace_id, page_id, p.actor_id)
            .await
            .map_err(comment_error)?;
        let names = self.member_names().await?;
        Ok(list
            .items
            .into_iter()
            .map(|thread| DocThread {
                id: thread.id,
                quote: thread.quote,
                resolved: thread.resolved,
                resolved_by: thread.resolved_by.map(|id| person(&names, id)),
                created_by: person(&names, thread.created_by),
                created_at: thread.created_at,
                comments: thread
                    .comments
                    .into_iter()
                    .map(|comment| doc_comment(&names, comment))
                    .collect(),
            })
            .collect())
    }

    /// Replies in the thread `thread_id` of a visible doc, or starts a thread on `quote`: exactly
    /// one of them. The new thread is not anchored in the doc's text: the doc lists it with its
    /// quote.
    pub async fn create_doc_comment(
        &self,
        page_id: Id,
        thread_id: Option<Id>,
        quote: Option<String>,
        markdown: &str,
    ) -> Result<DocComment> {
        let target = match (thread_id, quote) {
            (Some(thread_id), None) => Ok(thread_id),
            (None, Some(quote)) => Err(quote),
            _ => {
                return Err(TokenApiError::Invalid(
                    "Send exactly one of thread_id (a reply) and quote (a new thread)",
                ));
            }
        };
        if markdown.trim().is_empty() || markdown.len() > 100_000 {
            return Err(TokenApiError::Invalid("Invalid comment body"));
        }
        if let Err(quote) = &target
            && (quote.trim().is_empty() || quote.chars().count() > MAX_QUOTE_CHARS)
        {
            return Err(TokenApiError::Invalid(
                "quote must be 1 to 1,000 characters of the doc's text",
            ));
        }
        let repository = self
            .doc_comment_repository(page_id, ApiTokenScope::DocsWrite)
            .await?;
        let blocks = paragraphs(&self.blocks(markdown).await?);
        let body = CommentBody::parse(Value::Array(blocks)).map_err(comment_error)?;
        let p = self.principal;
        let now = TimestampMillis::now();
        let comment = match target {
            Ok(thread_id) => repository
                .add_comment(
                    p.workspace_id,
                    page_id,
                    thread_id,
                    p.actor_id,
                    body,
                    self.request_id,
                    now,
                )
                .await
                .map_err(comment_error)?,
            Err(quote) => repository
                .create_thread(
                    p.workspace_id,
                    page_id,
                    p.actor_id,
                    body,
                    &quote,
                    self.request_id,
                    now,
                )
                .await
                .map_err(comment_error)?
                .comments
                .into_iter()
                .next()
                .ok_or_else(|| TokenApiError::Internal("new thread has no comment".into()))?,
        };
        let names = self.member_names().await?;
        Ok(doc_comment(&names, comment))
    }

    /// Doc comments act as the token's user, on a doc that `get_doc` would show.
    async fn doc_comment_repository(
        &self,
        page_id: Id,
        scope: ApiTokenScope,
    ) -> Result<PageCommentRepository> {
        let pages = self.content(scope, &self.api.pages)?;
        let p = self.principal;
        pages
            .get_page(p.workspace_id, page_id, p.actor_id)
            .await
            .map_err(hidden_page)?;
        Ok(PageCommentRepository::new(
            self.api.tasks.database().clone(),
        ))
    }

    fn personal(&self) -> Result<()> {
        match self.principal.service_account {
            Some(_) => Err(TokenApiError::NotAvailable),
            None => Ok(()),
        }
    }

    async fn member_names(&self) -> Result<HashMap<Id, String>> {
        let database = self.api.tasks.database();
        Ok(members(database.pool(), self.principal.workspace_id)
            .await?
            .into_iter()
            .collect())
    }
}

fn person(names: &HashMap<Id, String>, id: Id) -> Person {
    Person {
        id,
        name: names
            .get(&id)
            .cloned()
            .unwrap_or_else(|| "Unknown user".to_owned()),
    }
}

fn doc_comment(names: &HashMap<Id, String>, comment: PageComment) -> DocComment {
    let blocks = match comment.body {
        Value::Array(blocks) => blocks,
        _ => Vec::new(),
    };
    DocComment {
        id: comment.id,
        thread_id: comment.thread_id,
        author: person(names, comment.author_id),
        body: blocks_to_markdown(&blocks, &KeepLinks),
        mentioned_user_ids: comment.mentioned_user_ids,
        created_at: comment.created_at,
        edited_at: comment.edited_at,
        deleted: comment.deleted_at.is_some(),
    }
}

/// A comment holds only paragraphs: every block with inline content (headings, list items,
/// quotes, code, nested ones included) becomes a paragraph, so no text is dropped.
fn paragraphs(blocks: &[Value]) -> Vec<Value> {
    let mut out = Vec::new();
    for block in blocks {
        if let Some(content) = block.get("content").filter(|content| content.is_array()) {
            out.push(json!({ "type": "paragraph", "content": content }));
        }
        if let Some(children) = block.get("children").and_then(Value::as_array) {
            out.extend(paragraphs(children));
        }
    }
    out
}

/// Doc comment refusals tell no missing doc, thread or comment apart.
fn comment_error(error: PageCommentError) -> TokenApiError {
    match error {
        PageCommentError::Invalid { field: "mentions" } => {
            TokenApiError::Invalid("A comment can mention at most 50 members")
        }
        PageCommentError::Invalid { .. } => TokenApiError::Invalid(
            "Invalid comment body: 1 to 50 paragraphs, at most 10,000 characters",
        ),
        PageCommentError::Unavailable(error) => TokenApiError::Internal(error.to_string()),
        PageCommentError::Corrupt => TokenApiError::Internal("stored comment is invalid".into()),
        PageCommentError::PageNotFound
        | PageCommentError::NotFound
        | PageCommentError::Forbidden
        | PageCommentError::ThreadHasReplies => TokenApiError::NotAvailable,
    }
}

/// The task is already checked, so a refusal is the comment permission.
fn comment_forbidden(error: crate::repositories::tasks::TaskError) -> TokenApiError {
    match error {
        crate::repositories::tasks::TaskError::Forbidden => TokenApiError::Invalid(
            "Only the comment's author can edit it; only the author or a member who may moderate comments can delete it",
        ),
        error => error.into(),
    }
}
