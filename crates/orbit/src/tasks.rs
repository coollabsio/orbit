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
