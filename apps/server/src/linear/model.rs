//! Tolerant serde models of the Linear GraphQL objects the importer reads, with the field
//! selections that fetch them. Every optional field defaults, so a node with fields the key
//! cannot see still decodes. Issues are stored in `linear_import_items.data_json` in this shape.

use serde::{Deserialize, Serialize};

/// A Relay connection page. After [`super::client::LinearClient::issue_page`] the nested
/// connections of an issue hold every node and `page_info.has_next_page` is false.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Connection<T> {
    #[serde(default = "Vec::new")]
    pub nodes: Vec<T>,
    #[serde(default)]
    pub page_info: PageInfo,
}

impl<T> Default for Connection<T> {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            page_info: PageInfo::default(),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageInfo {
    #[serde(default)]
    pub has_next_page: bool,
    #[serde(default)]
    pub end_cursor: Option<String>,
}

/// An object reached only for its id.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct IdRef {
    pub id: String,
}

/// An object reached only for its name (project, label group, bot).
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct NameRef {
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinearUser {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
}

pub const USER_FIELDS: &str = "id name displayName email";

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Organization {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub url_key: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinearTeam {
    pub id: String,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub archived_at: Option<String>,
    /// Archived issues included.
    #[serde(default)]
    pub issue_count: u64,
}

pub const TEAM_FIELDS: &str = "id key name archivedAt issueCount(includeArchived: true)";

/// A workflow state. `type` is `triage`, `backlog`, `unstarted`, `started`, `completed` or
/// `canceled`.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct WorkflowState {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    #[serde(default)]
    pub position: f64,
    #[serde(default)]
    pub color: Option<String>,
}

pub const STATE_FIELDS: &str = "id name type position color";

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct LinearLabel {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    /// The label group, for a label inside one.
    #[serde(default)]
    pub parent: Option<NameRef>,
}

pub const LABEL_FIELDS: &str = "id name parent { name }";

/// A relation from the issue that lists it: `blocks` (the issue blocks `related_issue`),
/// `duplicate` (the issue duplicates it), `related` or `similar`.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinearRelation {
    pub id: String,
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    #[serde(default)]
    pub related_issue: Option<IdRef>,
}

pub const RELATION_FIELDS: &str = "id type relatedIssue { id }";

/// A link attached to an issue (GitHub pull request, Slack thread, URL).
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct LinearAttachment {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}

pub const ATTACHMENT_FIELDS: &str = "id title url";

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinearComment {
    pub id: String,
    /// Markdown.
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub user: Option<LinearUser>,
    #[serde(default)]
    pub bot_actor: Option<NameRef>,
    #[serde(default)]
    pub external_user: Option<NameRef>,
    /// The comment this one replies to.
    #[serde(default)]
    pub parent: Option<IdRef>,
}

impl LinearComment {
    /// Who wrote it, as Linear shows it.
    #[must_use]
    pub fn author(&self) -> String {
        self.user
            .as_ref()
            .and_then(|user| user.display_name.clone().or_else(|| user.name.clone()))
            .or_else(|| self.bot_actor.as_ref().and_then(|bot| bot.name.clone()))
            .or_else(|| {
                self.external_user
                    .as_ref()
                    .and_then(|user| user.name.clone())
            })
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| "Linear user".to_owned())
    }
}

pub const COMMENT_FIELDS: &str = "id body createdAt user { id name displayName email } \
botActor { name } externalUser { name } parent { id }";

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct IssueRef {
    pub id: String,
    #[serde(default)]
    pub identifier: Option<String>,
}

/// A team's cycle. Issues stored by an older version have only `number` and `name`.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cycle {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub number: Option<f64>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub starts_at: Option<String>,
    #[serde(default)]
    pub ends_at: Option<String>,
}

/// A milestone of a Linear project (which can span teams).
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMilestone {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    /// Markdown.
    #[serde(default)]
    pub description: Option<String>,
    /// `YYYY-MM-DD`.
    #[serde(default)]
    pub target_date: Option<String>,
    #[serde(default)]
    pub project: Option<NameRef>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinearIssue {
    pub id: String,
    #[serde(default)]
    pub identifier: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    /// Markdown.
    #[serde(default)]
    pub description: Option<String>,
    /// 0 none, 1 urgent, 2 high, 3 medium, 4 low.
    #[serde(default)]
    pub priority: f64,
    #[serde(default)]
    pub url: Option<String>,
    /// `YYYY-MM-DD`.
    #[serde(default)]
    pub due_date: Option<String>,
    #[serde(default)]
    pub completed_at: Option<String>,
    #[serde(default)]
    pub canceled_at: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub archived_at: Option<String>,
    #[serde(default)]
    pub estimate: Option<f64>,
    #[serde(default)]
    pub sla_breaches_at: Option<String>,
    #[serde(default)]
    pub trashed: Option<bool>,
    #[serde(default)]
    pub state: Option<IdRef>,
    #[serde(default)]
    pub assignee: Option<LinearUser>,
    #[serde(default)]
    pub parent: Option<IssueRef>,
    #[serde(default)]
    pub project: Option<NameRef>,
    #[serde(default)]
    pub cycle: Option<Cycle>,
    #[serde(default)]
    pub project_milestone: Option<ProjectMilestone>,
    #[serde(default)]
    pub labels: Connection<LinearLabel>,
    #[serde(default)]
    pub relations: Connection<LinearRelation>,
    #[serde(default)]
    pub attachments: Connection<LinearAttachment>,
    #[serde(default)]
    pub comments: Connection<LinearComment>,
}

/// Nested page sizes of an issue list. With 25 issues a page the query costs about 5,000 of
/// Linear's 10,000 complexity points per query (<https://linear.app/developers/rate-limiting>);
/// issues with more nested nodes get follow-up queries. The cycle and milestone selections add
/// about 1.7 points an issue (0.1 a property, 1 an object), about 42 a page.
pub const ISSUE_PAGE: u32 = 25;
pub const NESTED_PAGE: u32 = 20;
pub const COMMENT_PAGE: u32 = 10;
/// Page size of follow-up queries for one issue's nested connection.
pub const FOLLOW_UP_PAGE: u32 = 50;

/// Everything [`LinearIssue`] reads.
#[must_use]
pub fn issue_fields() -> String {
    let page = "pageInfo { hasNextPage endCursor }";
    format!(
        "id identifier title description priority url dueDate completedAt canceledAt createdAt \
         archivedAt estimate slaBreachesAt trashed state {{ id }} assignee {{ {USER_FIELDS} }} \
         parent {{ id identifier }} project {{ name }} cycle {{ id number name startsAt endsAt }} \
         projectMilestone {{ id name description targetDate project {{ name }} }} \
         labels(first: {NESTED_PAGE}) {{ nodes {{ {LABEL_FIELDS} }} {page} }} \
         relations(first: {NESTED_PAGE}) {{ nodes {{ {RELATION_FIELDS} }} {page} }} \
         attachments(first: {NESTED_PAGE}) {{ nodes {{ {ATTACHMENT_FIELDS} }} {page} }} \
         comments(first: {COMMENT_PAGE}) {{ nodes {{ {COMMENT_FIELDS} }} {page} }}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_sparse_issues_and_names_comment_authors() {
        let issue: LinearIssue =
            serde_json::from_str(r#"{"id": "a", "labels": {"nodes": []}, "unknown": 1}"#).unwrap();
        assert_eq!(issue.id, "a");
        assert!(issue.comments.nodes.is_empty());
        assert!(!issue.labels.page_info.has_next_page);
        let comment: LinearComment =
            serde_json::from_str(r#"{"id": "c", "user": null, "botActor": {"name": "GitHub"}}"#)
                .unwrap();
        assert_eq!(comment.author(), "GitHub");
        assert_eq!(LinearComment::default().author(), "Linear user");
    }
}
