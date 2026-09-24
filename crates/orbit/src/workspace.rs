use std::error::Error;
use std::fmt;

use orbit_platform::{Id, TimestampMillis};

use crate::{Membership, WorkspaceRole};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExpectedVersion(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RestoreAvailability {
    Available,
    NameConflict,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DomainError {
    VersionConflict { expected: u64, current: u64 },
    RestoreConflict { field: &'static str },
    WorkspaceMismatch,
    ParentMismatch,
    NotDeleted,
}

impl fmt::Display for DomainError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::VersionConflict { expected, current } => {
                write!(
                    formatter,
                    "expected version {expected}, current version is {current}"
                )
            }
            Self::RestoreConflict { field } => {
                write!(
                    formatter,
                    "cannot restore because {field} is already in use"
                )
            }
            Self::WorkspaceMismatch => formatter.write_str("resource belongs to another workspace"),
            Self::ParentMismatch => formatter.write_str("resource does not belong to that parent"),
            Self::NotDeleted => formatter.write_str("resource is not deleted"),
        }
    }
}

impl Error for DomainError {}

pub(crate) fn check_version(
    current: u64,
    ExpectedVersion(expected): ExpectedVersion,
) -> Result<(), DomainError> {
    if current == expected {
        Ok(())
    } else {
        Err(DomainError::VersionConflict { expected, current })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Workspace {
    pub id: Id,
    pub name: String,
    pub version: u64,
    pub deleted_at: Option<TimestampMillis>,
}

impl Workspace {
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: Id::new_v7(),
            name: name.into(),
            version: 0,
            deleted_at: None,
        }
    }

    pub fn rename(
        &mut self,
        name: impl Into<String>,
        expected: ExpectedVersion,
    ) -> Result<(), DomainError> {
        check_version(self.version, expected)?;
        self.name = name.into();
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
        restore(
            &mut self.deleted_at,
            &mut self.version,
            expected,
            availability,
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Project {
    pub id: Id,
    pub workspace_id: Id,
    pub name: String,
    pub key: String,
    pub color: String,
    pub version: u64,
    pub deleted_at: Option<TimestampMillis>,
}

impl Project {
    #[must_use]
    pub fn new(
        workspace_id: Id,
        name: impl Into<String>,
        key: impl Into<String>,
        color: impl Into<String>,
    ) -> Self {
        Self {
            id: Id::new_v7(),
            workspace_id,
            name: name.into(),
            key: key.into(),
            color: color.into(),
            version: 0,
            deleted_at: None,
        }
    }

    pub fn rename(
        &mut self,
        name: impl Into<String>,
        expected: ExpectedVersion,
    ) -> Result<(), DomainError> {
        check_version(self.version, expected)?;
        self.name = name.into();
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
        restore(
            &mut self.deleted_at,
            &mut self.version,
            expected,
            availability,
        )
    }
}

fn restore(
    deleted_at: &mut Option<TimestampMillis>,
    version: &mut u64,
    expected: ExpectedVersion,
    availability: RestoreAvailability,
) -> Result<(), DomainError> {
    check_version(*version, expected)?;
    if deleted_at.is_none() {
        return Err(DomainError::NotDeleted);
    }
    if availability == RestoreAvailability::NameConflict {
        return Err(DomainError::RestoreConflict { field: "name" });
    }
    *deleted_at = None;
    *version += 1;
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatusCategory {
    Unstarted,
    Started,
    Completed,
    Cancelled,
    /// System-managed: a task enters it only by being marked as a duplicate of another task.
    Duplicate,
}

impl StatusCategory {
    /// The value stored in `task_statuses.category` and sent over the API.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unstarted => "unstarted",
            Self::Started => "started",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Duplicate => "duplicate",
        }
    }
}

/// The statuses every new project starts with, in position order.
pub const DEFAULT_STATUSES: [(&str, &str, StatusCategory); 6] = [
    ("Backlog", "#8b8f98", StatusCategory::Unstarted),
    ("Todo", "#8b8f98", StatusCategory::Unstarted),
    ("In Progress", "#f2c94c", StatusCategory::Started),
    ("Done", "#4cb782", StatusCategory::Completed),
    ("Cancelled", "#8b8f98", StatusCategory::Cancelled),
    ("Duplicate", "#8b8f98", StatusCategory::Duplicate),
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskStatus {
    pub id: Id,
    pub workspace_id: Id,
    pub project_id: Id,
    pub name: String,
    pub description: String,
    pub color: String,
    pub category: StatusCategory,
    pub position: i64,
    pub version: u64,
}

impl TaskStatus {
    #[must_use]
    pub fn new(
        workspace_id: Id,
        project_id: Id,
        name: impl Into<String>,
        color: impl Into<String>,
        category: StatusCategory,
        position: i64,
    ) -> Self {
        Self {
            id: Id::new_v7(),
            workspace_id,
            project_id,
            name: name.into(),
            description: String::new(),
            color: color.into(),
            category,
            position,
            version: 0,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceDefaults {
    pub workspace: Workspace,
    pub owner: Membership,
    pub project: Project,
    pub statuses: Vec<TaskStatus>,
}

impl WorkspaceDefaults {
    #[must_use]
    pub fn new(
        owner_user_id: Id,
        workspace_name: impl Into<String>,
        project_name: impl Into<String>,
    ) -> Self {
        let workspace = Workspace::new(workspace_name);
        let owner = Membership::new(workspace.id, owner_user_id, WorkspaceRole::Owner);
        let project = Project::new(workspace.id, project_name, "GEN", "#5e6ad2");
        let statuses = DEFAULT_STATUSES
            .into_iter()
            .enumerate()
            .map(|(position, (name, color, category))| {
                TaskStatus::new(
                    workspace.id,
                    project.id,
                    name,
                    color,
                    category,
                    position as i64,
                )
            })
            .collect();

        Self {
            workspace,
            owner,
            project,
            statuses,
        }
    }
}
