use orbit_platform::{Id, TimestampMillis};

use crate::workspace::{DomainError, ExpectedVersion, Project, RestoreAvailability, check_version};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskPriority {
    None,
    Low,
    Medium,
    High,
    Urgent,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Task {
    pub id: Id,
    pub workspace_id: Id,
    pub project_id: Id,
    pub status_id: Id,
    pub title: String,
    pub description: String,
    pub priority: TaskPriority,
    pub position: i64,
    pub creator_id: Id,
    pub assignee_ids: Vec<Id>,
    pub version: u64,
    pub deleted_at: Option<TimestampMillis>,
}

impl Task {
    #[must_use]
    pub fn new(
        workspace_id: Id,
        project_id: Id,
        status_id: Id,
        title: impl Into<String>,
        creator_id: Id,
    ) -> Self {
        Self {
            id: Id::new_v7(),
            workspace_id,
            project_id,
            status_id,
            title: title.into(),
            description: String::new(),
            priority: TaskPriority::None,
            position: 0,
            creator_id,
            assignee_ids: Vec::new(),
            version: 0,
            deleted_at: None,
        }
    }

    pub fn rename(
        &mut self,
        title: impl Into<String>,
        expected: ExpectedVersion,
    ) -> Result<(), DomainError> {
        check_version(self.version, expected)?;
        self.title = title.into();
        self.version += 1;
        Ok(())
    }

    pub fn delete(
        &mut self,
        expected: ExpectedVersion,
        deleted_at: TimestampMillis,
    ) -> Result<(), DomainError> {
        check_version(self.version, expected)?;
        self.deleted_at = Some(deleted_at);
        self.version += 1;
        Ok(())
    }

    pub fn restore(
        &mut self,
        expected: ExpectedVersion,
        availability: RestoreAvailability,
    ) -> Result<(), DomainError> {
        check_version(self.version, expected)?;
        if self.deleted_at.is_none() {
            return Err(DomainError::NotDeleted);
        }
        if availability == RestoreAvailability::NameConflict {
            return Err(DomainError::RestoreConflict { field: "title" });
        }
        self.deleted_at = None;
        self.version += 1;
        Ok(())
    }

    pub fn is_visible(&self, project: &Project) -> Result<bool, DomainError> {
        if self.workspace_id != project.workspace_id {
            return Err(DomainError::WorkspaceMismatch);
        }
        if self.project_id != project.id {
            return Err(DomainError::ParentMismatch);
        }
        Ok(self.deleted_at.is_none() && project.deleted_at.is_none())
    }
}

/// How far the parent walk is allowed to climb before the edge is refused.
///
/// Acyclicity can only be proved inside this bound, so an ancestor chain that
/// still has not terminated after `MAX_PARENT_DEPTH` hops is treated as unsafe.
pub const MAX_PARENT_DEPTH: usize = 10;

/// Decide whether `child` may be re-parented under `parent`.
///
/// `ancestors` is the parent chain produced by a bounded walk, closest first:
/// `ancestors[0]` is `parent` itself, `ancestors[1]` its parent, and so on, with
/// at most `MAX_PARENT_DEPTH` entries. The caller owns the SQL; this function
/// owns the rule.
///
/// # Errors
///
/// `ParentCycle` when the edge would close a loop, `ParentDepthExceeded` when
/// the walk could not prove the chain terminates within `MAX_PARENT_DEPTH`.
pub fn check_parent_edge(child: Id, parent: Id, ancestors: &[Id]) -> Result<(), DomainError> {
    if child == parent || ancestors.contains(&child) {
        return Err(DomainError::ParentCycle);
    }
    if ancestors.len() >= MAX_PARENT_DEPTH {
        return Err(DomainError::ParentDepthExceeded {
            limit: MAX_PARENT_DEPTH,
        });
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Comment {
    pub id: Id,
    pub workspace_id: Id,
    pub task_id: Id,
    pub author_id: Id,
    pub parent_id: Option<Id>,
    pub body: String,
    pub version: u64,
}

impl Comment {
    #[must_use]
    pub fn new(workspace_id: Id, task_id: Id, author_id: Id, body: impl Into<String>) -> Self {
        Self {
            id: Id::new_v7(),
            workspace_id,
            task_id,
            author_id,
            parent_id: None,
            body: body.into(),
            version: 0,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttachmentRef {
    pub id: Id,
    pub workspace_id: Id,
    pub task_id: Id,
    pub comment_id: Option<Id>,
    pub file_name: String,
    pub media_type: String,
    pub size_bytes: u64,
    pub storage_key: String,
    pub version: u64,
}

impl AttachmentRef {
    #[must_use]
    pub fn new(
        workspace_id: Id,
        task_id: Id,
        file_name: impl Into<String>,
        media_type: impl Into<String>,
        size_bytes: u64,
        storage_key: impl Into<String>,
    ) -> Self {
        Self {
            id: Id::new_v7(),
            workspace_id,
            task_id,
            comment_id: None,
            file_name: file_name.into(),
            media_type: media_type.into(),
            size_bytes,
            storage_key: storage_key.into(),
            version: 0,
        }
    }
}
