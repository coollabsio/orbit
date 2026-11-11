//! The JSON records of a workspace export archive (`workspace_export/archive.rs` writes them;
//! the layout is in its `README`). Unknown fields are ignored and missing ones default, so a
//! newer or older minor variant of the format still reads.

use serde::Deserialize;
use serde_json::Value;

use crate::workspace_export::ExportMode;

pub const FORMAT: &str = "orbit-workspace-export";
pub const VERSION: u64 = 1;

#[derive(Debug, Deserialize)]
pub struct Manifest {
    #[serde(default)]
    pub format: String,
    #[serde(default)]
    pub version: u64,
    pub mode: ExportMode,
    #[serde(default)]
    pub workspace_id: String,
    #[serde(default)]
    pub workspace_name: String,
    #[serde(default)]
    pub exported_by: ExportedBy,
}

#[derive(Debug, Default, Deserialize)]
pub struct ExportedBy {
    pub email: Option<String>,
}

/// `{"id", "name"}` of a member, as every record names people.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Person {
    #[serde(default)]
    pub id: String,
    pub name: Option<String>,
}

impl Person {
    #[must_use]
    pub fn display(&self) -> String {
        self.name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or("Unknown member")
            .to_owned()
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct Workspace {
    #[serde(default)]
    pub members: Vec<Member>,
    #[serde(default)]
    pub teamspaces: Vec<Teamspace>,
    #[serde(default)]
    pub projects: Vec<Project>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Member {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub email: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Teamspace {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub icon: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Project {
    pub id: String,
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default)]
    pub statuses: Vec<Status>,
    #[serde(default)]
    pub labels: Vec<Label>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Status {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub category: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Label {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub color: String,
}

/// `tasks/<KEY>-<n>.json`.
#[derive(Debug, Default, Deserialize)]
pub struct Task {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub status: TaskStatus,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub priority: String,
    pub parent_task_id: Option<String>,
    pub creator: Option<Person>,
    pub source_url: Option<String>,
    pub due_start_at: Option<String>,
    pub due_at: Option<String>,
    pub completed_at: Option<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default)]
    pub assignees: Vec<Person>,
    #[serde(default)]
    pub labels: Vec<Named>,
    #[serde(default)]
    pub relations: Vec<Relation>,
    #[serde(default)]
    pub attachments: Vec<File>,
    #[serde(default)]
    pub comments: Vec<Comment>,
}

#[derive(Debug, Default, Deserialize)]
pub struct TaskStatus {
    #[serde(default)]
    pub id: String,
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Named {
    #[serde(default)]
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub struct Relation {
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub task_id: String,
    #[serde(default)]
    pub related_task_id: String,
}

/// A task, comment, page or chat file. `path` is the archive entry, `None` when the file was
/// missing from the source's storage.
#[derive(Clone, Debug, Deserialize)]
pub struct File {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub path: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Comment {
    #[serde(default)]
    pub id: String,
    pub parent_id: Option<String>,
    pub author: Option<Person>,
    pub author_service_account: Option<Person>,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub attachments: Vec<File>,
}

/// `pages/.../<Page>.blocknote.json`.
#[derive(Debug, Default, Deserialize)]
pub struct Page {
    #[serde(default)]
    pub id: String,
    pub parent_id: Option<String>,
    pub teamspace_id: Option<String>,
    #[serde(default)]
    pub title: String,
    pub icon: Option<String>,
    pub cover_url: Option<String>,
    #[serde(default)]
    pub position: i64,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub content: Vec<Value>,
    #[serde(default)]
    pub files: Vec<File>,
}

/// One entry of `pages/comments.json`.
#[derive(Debug, Deserialize)]
pub struct PageComments {
    #[serde(default)]
    pub page_id: String,
    #[serde(default)]
    pub threads: Vec<Thread>,
}

#[derive(Debug, Deserialize)]
pub struct Thread {
    #[serde(default)]
    pub id: String,
    pub quote: Option<String>,
    #[serde(default)]
    pub created_at: String,
    pub resolved_at: Option<String>,
    #[serde(default)]
    pub comments: Vec<PageComment>,
}

#[derive(Debug, Deserialize)]
pub struct PageComment {
    #[serde(default)]
    pub id: String,
    pub author: Option<Person>,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub deleted: bool,
}

/// `chat/<conversation>.json`.
#[derive(Debug, Default, Deserialize)]
pub struct Conversation {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub name: String,
    pub topic: Option<String>,
    #[serde(default)]
    pub members: Vec<Person>,
    #[serde(default)]
    pub messages: Vec<Message>,
}

#[derive(Debug, Default, Deserialize)]
pub struct Message {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub kind: String,
    pub author: Option<Person>,
    pub bot_name: Option<String>,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub created_at: String,
    pub edited_at: Option<String>,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default)]
    pub pinned: bool,
    pub reply_to_id: Option<String>,
    pub forward_of_id: Option<String>,
    #[serde(default)]
    pub reactions: Vec<Reaction>,
    #[serde(default)]
    pub files: Vec<File>,
    #[serde(default)]
    pub replies: Vec<Message>,
}

#[derive(Debug, Deserialize)]
pub struct Reaction {
    #[serde(default)]
    pub emoji: String,
    #[serde(default)]
    pub users: Vec<Person>,
}
