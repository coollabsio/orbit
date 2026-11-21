//! The token user's inbox: list, mark one read, mark all read.
//!
//! The inbox belongs to a person, so a service-account token has none. A notification shows only
//! when the token may read what it points to: a task notification needs the task's project in
//! the grant, a doc mention `docs:read`, a chat mention or report `chat:read`, and a GitHub
//! notice a repository connected to an approved project. Other kinds stay hidden. Listing needs
//! `tasks:read`; marking read needs `tasks:write` and only touches the notifications the token
//! can list.
use std::collections::{HashMap, HashSet};

use orbit_platform::{Id, Row, SqlBuilder, TimestampMillis};
use serde::Serialize;
use utoipa::ToSchema;

use super::{Authorized, Result, TokenApiError, page_limit};
use crate::repositories::api_tokens::ApiTokenScope;
use crate::repositories::tasks::{NotificationRecord, Page};

/// Most repository pages read for one listing that the token's filter leaves empty.
const MAX_ROUNDS: usize = 10;
/// Most notifications one "mark all read" marks (about 50 pages of 100).
const MAX_MARK_ALL: usize = 5_000;
/// Notifications per `UPDATE` of "mark all read".
const MARK_BATCH: usize = 200;

pub(super) const SERVICE_ACCOUNT_INBOX: &str = "A service-account token has no inbox: notifications belong to a person. Use a personal API token.";

/// A notification, without the recipient and workspace (the token's own).
#[derive(Clone, Debug, Serialize, ToSchema)]
#[schema(as = PublicNotification)]
pub struct NotificationView {
    #[schema(value_type = String)]
    pub id: Id,
    /// `task_assigned`, `comment_mentioned` (task comment), `description_mentioned` (task
    /// description), `page_comment_mentioned`, `page_mentioned` (a doc), `chat_mentioned`,
    /// `chat_reported`, `import_finished`, `github_review_requested`, `github_review` or
    /// `github_merged`.
    pub kind: String,
    /// The member whose action made the notification.
    #[schema(value_type = String)]
    pub actor_user_id: Id,
    #[schema(value_type = Option<String>, required = true)]
    pub task_id: Option<Id>,
    /// The task comment of a `comment_mentioned` notification.
    #[schema(value_type = Option<String>, required = true)]
    pub comment_id: Option<Id>,
    /// The doc of a `page_*` notification.
    #[schema(value_type = Option<String>, required = true)]
    pub page_id: Option<Id>,
    #[schema(value_type = Option<String>, required = true)]
    pub page_thread_id: Option<Id>,
    #[schema(value_type = Option<String>, required = true)]
    pub page_comment_id: Option<Id>,
    #[schema(value_type = Option<String>, required = true)]
    pub chat_conversation_id: Option<Id>,
    #[schema(value_type = Option<String>, required = true)]
    pub chat_message_id: Option<Id>,
    /// `asana`, `linear`, `trello`, `notion` or `obsidian` for `import_finished`.
    #[schema(required = true)]
    pub import_source: Option<String>,
    /// `completed` or `failed` for `import_finished`.
    #[schema(required = true)]
    pub import_status: Option<String>,
    /// `owner/repo` of a GitHub notice.
    #[schema(required = true)]
    pub github_repository: Option<String>,
    #[schema(required = true)]
    pub github_pull_number: Option<i64>,
    #[schema(required = true)]
    pub github_actor_login: Option<String>,
    #[schema(required = true)]
    pub github_review_state: Option<String>,
    /// `null` while unread.
    #[schema(value_type = Option<String>, format = DateTime, required = true)]
    pub read_at: Option<TimestampMillis>,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
}

impl From<NotificationRecord> for NotificationView {
    fn from(n: NotificationRecord) -> Self {
        Self {
            id: n.id,
            kind: n.kind,
            actor_user_id: n.actor_user_id,
            task_id: n.task_id,
            comment_id: n.comment_id,
            page_id: n.page_id,
            page_thread_id: n.page_thread_id,
            page_comment_id: n.page_comment_id,
            chat_conversation_id: n.chat_conversation_id,
            chat_message_id: n.chat_message_id,
            import_source: n.import_source,
            import_status: n.import_status,
            github_repository: n.github_repository,
            github_pull_number: n.github_pull_number,
            github_actor_login: n.github_actor_login,
            github_review_state: n.github_review_state,
            read_at: n.read_at,
            created_at: n.created_at,
        }
    }
}

/// What a notification points to, for [`Authorized::visible_notifications`].
struct Target<'n> {
    kind: &'n str,
    task_id: Option<Id>,
    github_repository: Option<&'n str>,
}

impl<'n> From<&'n NotificationRecord> for Target<'n> {
    fn from(n: &'n NotificationRecord) -> Self {
        Self {
            kind: &n.kind,
            task_id: n.task_id,
            github_repository: n.github_repository.as_deref(),
        }
    }
}

impl Authorized<'_> {
    /// The token user's notifications, newest first. A page can hold fewer than `limit` items
    /// (the token cannot see some); continue while `next_cursor` is set.
    pub async fn notifications(
        &self,
        unread_only: bool,
        cursor: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Page<NotificationView>> {
        self.need(ApiTokenScope::TasksRead)?;
        self.inbox()?;
        let limit = page_limit(cursor, limit)?;
        let mut cursor = cursor.map(str::to_owned);
        for _ in 0..MAX_ROUNDS {
            let page = self
                .inbox_page(unread_only, cursor.as_deref(), limit)
                .await?;
            let items = self.visible_notifications(page.items).await?;
            cursor = page.next_cursor;
            if !items.is_empty() || cursor.is_none() {
                return Ok(Page {
                    items: items.into_iter().map(NotificationView::from).collect(),
                    next_cursor: cursor,
                });
            }
        }
        Ok(Page {
            items: Vec::new(),
            next_cursor: cursor,
        })
    }

    /// Marks one notification that the token can list as read; marking it again keeps the first
    /// `read_at`.
    pub async fn mark_notification_read(&self, notification_id: Id) -> Result<NotificationView> {
        self.need(ApiTokenScope::TasksWrite)?;
        self.inbox()?;
        let p = self.principal;
        let row = sqlx::query(
            "SELECT kind, task_id, github_repository FROM notifications \
             WHERE id = $1 AND workspace_id = $2 AND recipient_user_id = $3",
        )
        .bind(notification_id.to_string())
        .bind(p.workspace_id.to_string())
        .bind(p.actor_id.to_string())
        .fetch_optional(self.api.tasks.database().pool())
        .await?
        .ok_or(TokenApiError::NotAvailable)?;
        let kind: String = row.get("kind");
        let task_id = row
            .get::<Option<String>, _>("task_id")
            .map(|id| id.parse::<Id>())
            .transpose()
            .map_err(|_| TokenApiError::Internal("invalid notification task_id".to_owned()))?;
        let github_repository: Option<String> = row.get("github_repository");
        let target = Target {
            kind: &kind,
            task_id,
            github_repository: github_repository.as_deref(),
        };
        if self.visible_targets(&[target]).await?.first() != Some(&true) {
            return Err(TokenApiError::NotAvailable);
        }
        let record = self
            .api
            .tasks
            .mark_notification_read(
                p.workspace_id,
                p.actor_id,
                notification_id,
                TimestampMillis::now(),
            )
            .await?;
        Ok(record.into())
    }

    /// Marks every unread notification that the token can list as read (at most 5,000 per
    /// call); returns how many changed. Notifications the token cannot see stay unread.
    pub async fn mark_notifications_read(&self) -> Result<u64> {
        self.need(ApiTokenScope::TasksWrite)?;
        self.inbox()?;
        let mut ids = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let page = self.inbox_page(true, cursor.as_deref(), 100).await?;
            ids.extend(
                self.visible_notifications(page.items)
                    .await?
                    .into_iter()
                    .map(|n| n.id),
            );
            cursor = page.next_cursor;
            if cursor.is_none() || ids.len() >= MAX_MARK_ALL {
                break;
            }
        }
        ids.truncate(MAX_MARK_ALL);
        let p = self.principal;
        let now = TimestampMillis::now().as_millis();
        let mut updated = 0;
        // One statement per batch, each committed on its own: no write transaction stays open.
        for batch in ids.chunks(MARK_BATCH) {
            let mut query = SqlBuilder::new("UPDATE notifications SET read_at = ");
            query
                .push_bind(now)
                .push(" WHERE workspace_id = ")
                .push_bind(p.workspace_id.to_string())
                .push(" AND recipient_user_id = ")
                .push_bind(p.actor_id.to_string())
                .push(" AND read_at IS NULL AND id IN (");
            let mut list = query.separated(", ");
            for id in batch {
                list.push_bind(id.to_string());
            }
            query.push(")");
            updated += query
                .build()
                .execute(self.api.tasks.database().pool())
                .await?
                .rows_affected();
        }
        Ok(updated)
    }

    /// The inbox is a person's: a service-account token has none.
    fn inbox(&self) -> Result<()> {
        match self.principal.service_account {
            Some(_) => Err(TokenApiError::Forbidden(SERVICE_ACCOUNT_INBOX)),
            None => Ok(()),
        }
    }

    async fn inbox_page(
        &self,
        unread_only: bool,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<Page<NotificationRecord>> {
        let p = self.principal;
        Ok(self
            .api
            .tasks
            .notifications(p.workspace_id, p.actor_id, unread_only, cursor, limit)
            .await?)
    }

    async fn visible_notifications(
        &self,
        items: Vec<NotificationRecord>,
    ) -> Result<Vec<NotificationRecord>> {
        let targets: Vec<Target> = items.iter().map(Target::from).collect();
        let visible = self.visible_targets(&targets).await?;
        Ok(items
            .into_iter()
            .zip(visible)
            .filter_map(|(item, visible)| visible.then_some(item))
            .collect())
    }

    /// Whether the token may see each target (see the module docs).
    async fn visible_targets(&self, targets: &[Target<'_>]) -> Result<Vec<bool>> {
        let p = self.principal;
        let pool = self.api.tasks.database().pool();
        let task_ids: HashSet<Id> = targets.iter().filter_map(|t| t.task_id).collect();
        let mut task_projects: HashMap<Id, Id> = HashMap::new();
        if !task_ids.is_empty() {
            let mut query =
                SqlBuilder::new("SELECT id, project_id FROM tasks WHERE workspace_id = ");
            query
                .push_bind(p.workspace_id.to_string())
                .push(" AND id IN (");
            let mut list = query.separated(", ");
            for id in &task_ids {
                list.push_bind(id.to_string());
            }
            query.push(")");
            for row in query.build().fetch_all(pool).await? {
                let (Ok(id), Ok(project)) = (
                    row.get::<String, _>("id").parse::<Id>(),
                    row.get::<String, _>("project_id").parse::<Id>(),
                ) else {
                    continue;
                };
                task_projects.insert(id, project);
            }
        }
        let mut repositories: HashSet<String> = HashSet::new();
        if targets.iter().any(|t| t.github_repository.is_some()) && !p.project_ids.is_empty() {
            let mut query = SqlBuilder::new(
                "SELECT lower(repository) AS repository FROM github_project_connections WHERE workspace_id = ",
            );
            query
                .push_bind(p.workspace_id.to_string())
                .push(" AND project_id IN (");
            let mut list = query.separated(", ");
            for id in &p.project_ids {
                list.push_bind(id.to_string());
            }
            query.push(")");
            for row in query.build().fetch_all(pool).await? {
                repositories.insert(row.get("repository"));
            }
        }
        let docs = p.allows(ApiTokenScope::DocsRead);
        let chat = p.allows(ApiTokenScope::ChatRead);
        Ok(targets
            .iter()
            .map(|target| match target.kind {
                "task_assigned" | "comment_mentioned" | "description_mentioned" => target
                    .task_id
                    .and_then(|id| task_projects.get(&id))
                    .is_some_and(|project| self.approved(*project)),
                "page_comment_mentioned" | "page_mentioned" => docs,
                "chat_mentioned" | "chat_reported" => chat,
                "import_finished" => true,
                "github_review_requested" | "github_review" | "github_merged" => target
                    .github_repository
                    .is_some_and(|repo| repositories.contains(&repo.to_lowercase())),
                _ => false,
            })
            .collect())
    }
}
