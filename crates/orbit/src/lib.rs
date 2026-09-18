//! Orbit domain models and policies.

mod identity;
mod policy;
mod ports;
pub mod rich_text;
mod tasks;
mod workspace;

pub use identity::{Membership, User, WorkspaceRole};
pub use orbit_platform as platform;
pub use policy::{Action, Policy, PolicyError, Role, TargetRole};
pub use ports::{
    AttachmentRepository, AttachmentStorageService, CommentRepository, PortFuture,
    ProjectRepository, RepositoryError, TaskRepository, TaskStatusRepository, UserRepository,
    WorkspaceRepository,
};
pub use tasks::{AttachmentRef, Comment, MAX_PARENT_DEPTH, Task, TaskPriority, check_parent_edge};
pub use workspace::{
    DomainError, ExpectedVersion, Project, RestoreAvailability, StatusCategory, TaskStatus,
    Workspace, WorkspaceDefaults,
};
