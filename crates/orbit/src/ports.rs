use std::error::Error;
use std::fmt;
use std::future::Future;
use std::pin::Pin;

use orbit_platform::Id;

use crate::{
    AttachmentRef, Comment, DomainError, ExpectedVersion, Membership, Project, Task, TaskStatus,
    User, Workspace, WorkspaceDefaults,
};

pub type PortFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, RepositoryError>> + Send + 'a>>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RepositoryError {
    NotFound,
    Conflict(DomainError),
    Duplicate { field: &'static str },
    Unavailable,
}

impl fmt::Display for RepositoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => formatter.write_str("record was not found"),
            Self::Conflict(error) => fmt::Display::fmt(error, formatter),
            Self::Duplicate { field } => write!(formatter, "{field} is already in use"),
            Self::Unavailable => formatter.write_str("repository is unavailable"),
        }
    }
}

impl Error for RepositoryError {}

pub trait UserRepository: Send + Sync {
    fn get<'a>(&'a self, user_id: Id) -> PortFuture<'a, User>;
    fn save<'a>(&'a self, user: &'a User, expected: ExpectedVersion) -> PortFuture<'a, User>;
}

pub trait WorkspaceRepository: Send + Sync {
    fn create<'a>(&'a self, defaults: &'a WorkspaceDefaults) -> PortFuture<'a, ()>;
    fn get<'a>(&'a self, workspace_id: Id) -> PortFuture<'a, Workspace>;
    fn save<'a>(
        &'a self,
        workspace_id: Id,
        workspace: &'a Workspace,
        expected: ExpectedVersion,
    ) -> PortFuture<'a, Workspace>;
    fn memberships<'a>(&'a self, workspace_id: Id) -> PortFuture<'a, Vec<Membership>>;
    fn add_membership<'a>(
        &'a self,
        workspace_id: Id,
        membership: &'a Membership,
    ) -> PortFuture<'a, Membership>;
    fn save_membership<'a>(
        &'a self,
        workspace_id: Id,
        membership: &'a Membership,
        expected: ExpectedVersion,
    ) -> PortFuture<'a, Membership>;
}

pub trait ProjectRepository: Send + Sync {
    fn list<'a>(&'a self, workspace_id: Id) -> PortFuture<'a, Vec<Project>>;
    fn get<'a>(&'a self, workspace_id: Id, project_id: Id) -> PortFuture<'a, Project>;
    fn create<'a>(&'a self, workspace_id: Id, project: &'a Project) -> PortFuture<'a, Project>;
    fn save<'a>(
        &'a self,
        workspace_id: Id,
        project: &'a Project,
        expected: ExpectedVersion,
    ) -> PortFuture<'a, Project>;
}

pub trait TaskStatusRepository: Send + Sync {
    fn list<'a>(&'a self, workspace_id: Id, project_id: Id) -> PortFuture<'a, Vec<TaskStatus>>;
    fn create<'a>(&'a self, workspace_id: Id, status: &'a TaskStatus)
    -> PortFuture<'a, TaskStatus>;
    fn save<'a>(
        &'a self,
        workspace_id: Id,
        status: &'a TaskStatus,
        expected: ExpectedVersion,
    ) -> PortFuture<'a, TaskStatus>;
}

pub trait TaskRepository: Send + Sync {
    fn list<'a>(&'a self, workspace_id: Id, project_id: Id) -> PortFuture<'a, Vec<Task>>;
    fn get<'a>(&'a self, workspace_id: Id, task_id: Id) -> PortFuture<'a, Task>;
    fn create<'a>(&'a self, workspace_id: Id, task: &'a Task) -> PortFuture<'a, Task>;
    fn save<'a>(
        &'a self,
        workspace_id: Id,
        task: &'a Task,
        expected: ExpectedVersion,
    ) -> PortFuture<'a, Task>;
}

pub trait CommentRepository: Send + Sync {
    fn list<'a>(&'a self, workspace_id: Id, task_id: Id) -> PortFuture<'a, Vec<Comment>>;
    fn create<'a>(&'a self, workspace_id: Id, comment: &'a Comment) -> PortFuture<'a, Comment>;
    fn save<'a>(
        &'a self,
        workspace_id: Id,
        comment: &'a Comment,
        expected: ExpectedVersion,
    ) -> PortFuture<'a, Comment>;
    fn delete<'a>(&'a self, workspace_id: Id, comment_id: Id) -> PortFuture<'a, ()>;
}

pub trait AttachmentRepository: Send + Sync {
    fn list<'a>(&'a self, workspace_id: Id, task_id: Id) -> PortFuture<'a, Vec<AttachmentRef>>;
    fn create<'a>(
        &'a self,
        workspace_id: Id,
        attachment: &'a AttachmentRef,
    ) -> PortFuture<'a, AttachmentRef>;
    fn delete<'a>(&'a self, workspace_id: Id, attachment_id: Id) -> PortFuture<'a, ()>;
}

pub trait AttachmentStorageService: Send + Sync {
    fn delete<'a>(&'a self, workspace_id: Id, storage_key: &'a str) -> PortFuture<'a, ()>;
}
