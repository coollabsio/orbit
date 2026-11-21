//! Doc search, recents, favorites, copies, locks, version restores, permanent deletes, teamspace
//! writes and doc comment thread edits for API tokens and MCP. Every call acts as the token's
//! creator; the page, teamspace and comment repositories decide what that person may see and do.
use orbit_platform::{Id, TimestampMillis};
use serde_json::json;

use super::comments::{comment_error, doc_comment, doc_thread, paragraphs};
use super::{
    Authorized, DocComment, DocThread, DocView, Result, TeamspaceView, TokenApiError, hidden_page,
};
use crate::repositories::api_tokens::ApiTokenScope;
use crate::repositories::page_comments::{CommentBody, PageCommentError};
use crate::repositories::pages::{
    PageError, PageFavorite, PageFavoriteList, PageList, PageSearch, PageTrashEmptied,
    RECENT_DEFAULT_LIMIT, RECENT_MAX_LIMIT, RecentPageList, revision,
};
use crate::repositories::tasks::TaskError;
use crate::repositories::teamspaces::{
    TeamspaceChanges, TeamspaceError, TeamspaceRecord, TeamspaceRepository,
};

const NOT_TRASHED: &str =
    "Only a doc in the trash, trashed itself (not with its parent doc), can be deleted forever";
const PURGE_FORBIDDEN: &str = "Deleting teamspace docs forever needs a workspace owner or admin; members delete their own private docs";

/// A teamspace change; `None` keeps the value, `Some(None)` clears it.
#[derive(Debug, Default)]
pub struct TeamspacePatch {
    pub name: Option<String>,
    pub icon: Option<Option<String>>,
    pub project_id: Option<Option<Id>>,
}

impl Authorized<'_> {
    /// Docs whose title or text match `query`, best first.
    pub async fn search_docs(&self, query: &str) -> Result<PageSearch> {
        let pages = self.content(ApiTokenScope::DocsRead, &self.api.pages)?;
        if query.trim().is_empty() || query.chars().count() > 200 || query.len() > 800 {
            return Err(TokenApiError::Invalid(
                "A search query has 1 to 200 characters",
            ));
        }
        let p = self.principal;
        pages
            .search_pages(p.workspace_id, p.actor_id, query)
            .await
            .map_err(hidden_page)
    }

    /// The docs the token's creator opened last, most recent first.
    pub async fn recent_docs(&self, limit: Option<usize>) -> Result<RecentPageList> {
        let pages = self.content(ApiTokenScope::DocsRead, &self.api.pages)?;
        let limit = limit.unwrap_or(RECENT_DEFAULT_LIMIT);
        if !(1..=RECENT_MAX_LIMIT).contains(&limit) {
            return Err(TokenApiError::Invalid("limit must be 1 to 50"));
        }
        let p = self.principal;
        pages
            .recent_pages(p.workspace_id, p.actor_id, limit)
            .await
            .map_err(hidden_page)
    }

    /// The token creator's favorite docs, in their order.
    pub async fn favorite_docs(&self) -> Result<PageFavoriteList> {
        let pages = self.content(ApiTokenScope::DocsRead, &self.api.pages)?;
        let p = self.principal;
        pages
            .favorites(p.workspace_id, p.actor_id)
            .await
            .map_err(hidden_page)
    }

    /// Adds the doc as the last favorite; a favorite stays where it is.
    pub async fn add_favorite_doc(&self, page_id: Id) -> Result<PageFavorite> {
        let pages = self.content(ApiTokenScope::DocsWrite, &self.api.pages)?;
        let p = self.principal;
        pages
            .add_favorite(p.workspace_id, page_id, p.actor_id, TimestampMillis::now())
            .await
            .map_err(hidden_page)
    }

    pub async fn remove_favorite_doc(&self, page_id: Id) -> Result<()> {
        let pages = self.content(ApiTokenScope::DocsWrite, &self.api.pages)?;
        let p = self.principal;
        pages
            .remove_favorite(p.workspace_id, page_id, p.actor_id)
            .await
            .map_err(hidden_page)
    }

    /// Moves a favorite to `position` among the favorites; past the end puts it last.
    pub async fn move_favorite_doc(&self, page_id: Id, position: i64) -> Result<PageFavoriteList> {
        let pages = self.content(ApiTokenScope::DocsWrite, &self.api.pages)?;
        if position < 0 {
            return Err(TokenApiError::Invalid("position must be 0 or more"));
        }
        let p = self.principal;
        pages
            .move_favorite(p.workspace_id, page_id, p.actor_id, position)
            .await
            .map_err(hidden_page)
    }

    /// Every live sub-doc of a doc, at any depth; `version` is each one's revision.
    pub async fn doc_descendants(&self, page_id: Id) -> Result<PageList> {
        let pages = self.content(ApiTokenScope::DocsRead, &self.api.pages)?;
        let p = self.principal;
        let mut list = pages
            .descendants(p.workspace_id, page_id, p.actor_id)
            .await
            .map_err(hidden_page)?;
        for page in &mut list.items {
            page.version = revision(page.version, page.updated_at);
        }
        Ok(list)
    }

    /// Copies a doc (with `include_children`, its live sub-docs too) right after it.
    pub async fn duplicate_doc(&self, page_id: Id, include_children: bool) -> Result<DocView> {
        let pages = self.content(ApiTokenScope::DocsWrite, &self.api.pages)?;
        let p = self.principal;
        let copy = pages
            .duplicate_page(
                p.workspace_id,
                page_id,
                p.actor_id,
                include_children,
                self.request_id,
                TimestampMillis::now(),
            )
            .await?;
        self.doc_view(pages, copy).await
    }

    /// Locks or unlocks a doc: a locked doc's title, icon and content cannot change. Any member
    /// who sees the doc may do both, as in the web app.
    pub async fn set_doc_lock(&self, page_id: Id, locked: bool) -> Result<DocView> {
        let pages = self.content(ApiTokenScope::DocsWrite, &self.api.pages)?;
        let p = self.principal;
        let page = pages
            .set_lock(
                p.workspace_id,
                page_id,
                p.actor_id,
                locked,
                self.request_id,
                TimestampMillis::now(),
            )
            .await?;
        self.doc_view(pages, page).await
    }

    /// Replaces a doc's content with a saved version's. `expected_version` is the doc's
    /// `version` as read (its [`revision`]).
    pub async fn restore_doc_version(
        &self,
        page_id: Id,
        version_id: Id,
        expected_version: u64,
    ) -> Result<DocView> {
        let pages = self.content(ApiTokenScope::DocsWrite, &self.api.pages)?;
        let p = self.principal;
        let mut current = pages
            .get_page(p.workspace_id, page_id, p.actor_id)
            .await
            .map_err(hidden_page)?;
        let current_revision = revision(current.version, current.updated_at);
        if current_revision != expected_version {
            current.version = current_revision;
            return Err(PageError::VersionConflict {
                current: Box::new(json!(current)),
            }
            .into());
        }
        let page = pages
            .restore_page_version(
                p.workspace_id,
                page_id,
                version_id,
                p.actor_id,
                current.version,
                self.request_id,
                TimestampMillis::now(),
            )
            .await
            .map_err(|error| match error {
                PageError::VersionNotFound => TokenApiError::NotAvailable,
                error => error.into(),
            })?;
        self.doc_view(pages, page).await
    }

    /// Deletes a trashed doc and its sub-docs forever. `expected_version` is the doc's `version`
    /// in the trash list (its [`revision`]).
    pub async fn purge_doc(&self, page_id: Id, expected_version: u64) -> Result<()> {
        let pages = self.content(ApiTokenScope::DocsWrite, &self.api.pages)?;
        let p = self.principal;
        let now = TimestampMillis::now();
        let stored = match pages
            .trashed_page(p.workspace_id, page_id, p.actor_id, now)
            .await
        {
            Ok(trashed) => {
                let mut current = trashed.page;
                let current_revision = revision(current.version, current.updated_at);
                if current_revision != expected_version {
                    current.version = current_revision;
                    return Err(PageError::VersionConflict {
                        current: Box::new(json!(current)),
                    }
                    .into());
                }
                current.version
            }
            // A live doc: the purge below answers "not in the trash" or "not found".
            Err(PageError::NotFound) => 0,
            Err(error) => return Err(hidden_page(error)),
        };
        pages
            .purge_page(
                p.workspace_id,
                page_id,
                p.actor_id,
                stored,
                self.request_id,
                now,
            )
            .await
            .map_err(purge_error)
    }

    /// Deletes forever every trashed doc that the token's creator may purge: their own private
    /// docs, and teamspace docs for owners and admins.
    pub async fn empty_doc_trash(&self) -> Result<PageTrashEmptied> {
        let pages = self.content(ApiTokenScope::DocsWrite, &self.api.pages)?;
        let p = self.principal;
        pages
            .empty_trash(
                p.workspace_id,
                p.actor_id,
                self.request_id,
                TimestampMillis::now(),
            )
            .await
            .map_err(purge_error)
    }

    /// Adds a teamspace after the others.
    pub async fn create_teamspace(
        &self,
        name: &str,
        icon: Option<String>,
    ) -> Result<TeamspaceView> {
        self.content(ApiTokenScope::DocsWrite, &self.api.pages)?;
        let name = teamspace_name(name)?;
        let icon = icon.map(teamspace_icon).transpose()?;
        let p = self.principal;
        let record = self
            .teamspaces()
            .create_teamspace(
                p.workspace_id,
                p.actor_id,
                name,
                icon,
                self.request_id,
                TimestampMillis::now(),
            )
            .await
            .map_err(teamspace_error)?;
        Ok(self.teamspace_view(record))
    }

    /// Renames a teamspace, changes its icon, or links it to a project. A project must be
    /// approved for the token (which needs a task scope for that); `Some(None)` unlinks.
    pub async fn update_teamspace(
        &self,
        teamspace_id: Id,
        expected_version: u64,
        patch: TeamspacePatch,
    ) -> Result<TeamspaceView> {
        self.content(ApiTokenScope::DocsWrite, &self.api.pages)?;
        if let Some(Some(project_id)) = patch.project_id
            && !self.approved(project_id)
        {
            return Err(TokenApiError::Invalid(
                "project_id must be a project approved for this token (see list_projects)",
            ));
        }
        let changes = TeamspaceChanges {
            name: patch.name.as_deref().map(teamspace_name).transpose()?,
            icon: patch
                .icon
                .map(|icon| icon.map(teamspace_icon).transpose())
                .transpose()?,
            project_id: patch.project_id,
        };
        let p = self.principal;
        let record = self
            .teamspaces()
            .update_teamspace(
                p.workspace_id,
                teamspace_id,
                p.actor_id,
                expected_version,
                changes,
                self.request_id,
                TimestampMillis::now(),
            )
            .await
            .map_err(teamspace_error)?;
        Ok(self.teamspace_view(record))
    }

    /// Moves a teamspace to `position` (0 makes it the default); returns them all in order.
    pub async fn move_teamspace(
        &self,
        teamspace_id: Id,
        expected_version: u64,
        position: i64,
    ) -> Result<Vec<TeamspaceView>> {
        self.content(ApiTokenScope::DocsWrite, &self.api.pages)?;
        if position < 0 {
            return Err(TokenApiError::Invalid("position must be 0 or more"));
        }
        let p = self.principal;
        let list = self
            .teamspaces()
            .move_teamspace(
                p.workspace_id,
                teamspace_id,
                p.actor_id,
                expected_version,
                position,
                self.request_id,
                TimestampMillis::now(),
            )
            .await
            .map_err(teamspace_error)?;
        Ok(list
            .items
            .into_iter()
            .map(|record| self.teamspace_view(record))
            .collect())
    }

    /// Deletes an empty teamspace; owners and admins only, and never the last one.
    pub async fn delete_teamspace(&self, teamspace_id: Id, expected_version: u64) -> Result<()> {
        self.content(ApiTokenScope::DocsWrite, &self.api.pages)?;
        let p = self.principal;
        self.teamspaces()
            .delete_teamspace(
                p.workspace_id,
                teamspace_id,
                p.actor_id,
                expected_version,
                self.request_id,
                TimestampMillis::now(),
            )
            .await
            .map_err(teamspace_error)
    }

    /// Edits a doc comment that the token's creator wrote.
    pub async fn update_doc_comment(
        &self,
        page_id: Id,
        thread_id: Id,
        comment_id: Id,
        markdown: &str,
    ) -> Result<DocComment> {
        if markdown.trim().is_empty() || markdown.len() > 100_000 {
            return Err(TokenApiError::Invalid("Invalid comment body"));
        }
        let repository = self
            .doc_comment_repository(page_id, ApiTokenScope::DocsWrite)
            .await?;
        let blocks = paragraphs(&self.blocks(markdown).await?);
        let body = CommentBody::parse(serde_json::Value::Array(blocks)).map_err(comment_error)?;
        let p = self.principal;
        let comment = repository
            .update_comment(
                p.workspace_id,
                page_id,
                thread_id,
                comment_id,
                p.actor_id,
                body,
                self.request_id,
                TimestampMillis::now(),
            )
            .await
            .map_err(thread_error)?;
        let names = self.member_names().await?;
        Ok(doc_comment(&names, comment))
    }

    /// Deletes a doc comment that the token's creator wrote; it keeps its place, empty.
    pub async fn delete_doc_comment(
        &self,
        page_id: Id,
        thread_id: Id,
        comment_id: Id,
    ) -> Result<()> {
        let repository = self
            .doc_comment_repository(page_id, ApiTokenScope::DocsWrite)
            .await?;
        let p = self.principal;
        repository
            .delete_comment(
                p.workspace_id,
                page_id,
                thread_id,
                comment_id,
                p.actor_id,
                self.request_id,
                TimestampMillis::now(),
            )
            .await
            .map_err(thread_error)
    }

    /// Resolves (`true`) or reopens (`false`) a doc comment thread; the same state again is a
    /// no-op.
    pub async fn set_doc_thread_resolved(
        &self,
        page_id: Id,
        thread_id: Id,
        resolved: bool,
    ) -> Result<DocThread> {
        let repository = self
            .doc_comment_repository(page_id, ApiTokenScope::DocsWrite)
            .await?;
        let p = self.principal;
        let thread = repository
            .set_resolved(
                p.workspace_id,
                page_id,
                thread_id,
                p.actor_id,
                resolved,
                self.request_id,
                TimestampMillis::now(),
            )
            .await
            .map_err(thread_error)?;
        let names = self.member_names().await?;
        Ok(doc_thread(&names, thread))
    }

    /// Deletes a thread with its comments: its creator while nobody else commented, or a member
    /// who may moderate comments.
    pub async fn delete_doc_thread(&self, page_id: Id, thread_id: Id) -> Result<()> {
        let repository = self
            .doc_comment_repository(page_id, ApiTokenScope::DocsWrite)
            .await?;
        let p = self.principal;
        repository
            .delete_thread(
                p.workspace_id,
                page_id,
                thread_id,
                p.actor_id,
                self.request_id,
                TimestampMillis::now(),
            )
            .await
            .map_err(thread_error)
    }

    fn teamspaces(&self) -> TeamspaceRepository {
        TeamspaceRepository::new(self.api.tasks.database().clone())
    }

    /// As `list_teamspaces` shows it: a project outside the grant is `null`.
    fn teamspace_view(&self, record: TeamspaceRecord) -> TeamspaceView {
        TeamspaceView {
            id: record.id,
            name: record.name,
            icon: record.icon,
            is_default: record.is_default,
            project_id: record.project_id.filter(|id| self.approved(*id)),
            global: record.project_id.is_none(),
            version: record.version,
        }
    }
}

/// A teamspace name: 1 to 100 characters after trimming.
fn teamspace_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 100 || name.len() > 400 {
        return Err(TokenApiError::Invalid(
            "A teamspace name has 1 to 100 characters",
        ));
    }
    Ok(name.to_owned())
}

/// An emoji or a short emoji code: not blank, at most 64 characters.
fn teamspace_icon(icon: String) -> Result<String> {
    if icon.trim().is_empty() || icon.chars().count() > 64 || icon.len() > 256 {
        return Err(TokenApiError::Invalid(
            "A teamspace icon is an emoji: 1 to 64 characters, or null to clear it",
        ));
    }
    Ok(icon)
}

fn purge_error(error: PageError) -> TokenApiError {
    match error {
        PageError::NotTrashed => TokenApiError::Conflict(NOT_TRASHED),
        PageError::Forbidden => TokenApiError::Forbidden(PURGE_FORBIDDEN),
        error => error.into(),
    }
}

fn teamspace_error(error: TeamspaceError) -> TokenApiError {
    match error {
        TeamspaceError::NotFound => TokenApiError::NotAvailable,
        TeamspaceError::Forbidden => {
            TokenApiError::Forbidden("Only workspace owners and admins may delete teamspaces")
        }
        TeamspaceError::NotEmpty => TokenApiError::Conflict(
            "The teamspace still has docs: move or delete them first (see list_docs)",
        ),
        TeamspaceError::LastTeamspace => {
            TokenApiError::Conflict("A workspace keeps at least one teamspace")
        }
        TeamspaceError::ProjectTaken => {
            TokenApiError::Conflict("Another teamspace already holds this project's docs")
        }
        TeamspaceError::ProjectNotFound => TokenApiError::Invalid(
            "project_id must be a project approved for this token (see list_projects)",
        ),
        TeamspaceError::VersionConflict { current } => {
            TokenApiError::Task(TaskError::VersionConflict { current })
        }
        error => TokenApiError::Internal(error.to_string()),
    }
}

/// [`comment_error`], but telling the refusals of a visible thread apart.
fn thread_error(error: PageCommentError) -> TokenApiError {
    match error {
        PageCommentError::Forbidden => TokenApiError::Forbidden(
            "Only the comment's author may edit or delete it; a thread is deleted by its creator or a member who may moderate comments",
        ),
        PageCommentError::ThreadHasReplies => TokenApiError::Conflict(
            "Others commented in this thread: only a member who may moderate comments can delete it",
        ),
        error => comment_error(error),
    }
}
