use std::error::Error;
use std::fmt;

use orbit_platform::Id;

use crate::{Membership, WorkspaceRole};

pub type Role = WorkspaceRole;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TargetRole {
    Owner,
    Admin,
    Member,
    NotApplicable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    ManageWorkspaceSettings,
    DeleteWorkspace,
    RestoreWorkspace,
    TransferOwnership,
    AddMembership,
    RemoveMembership,
    ChangeMembershipRole,
    LeaveWorkspace,
    CreateProject,
    EditProject,
    DeleteProject,
    RestoreProject,
    CreateStatus,
    EditStatus,
    DeleteStatus,
    CreateTask,
    EditTask,
    DeleteTask,
    RestoreTask,
    CreateComment,
    EditComment,
    DeleteComment,
    CreateAttachment,
    DeleteAttachment,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyError {
    Forbidden,
    OnlyOwner,
    OwnerProtected,
    TransferOwnershipRequired,
    ExactlyOneOwnerRequired,
    WorkspaceMismatch,
}

impl fmt::Display for PolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Forbidden => formatter.write_str("action is not permitted"),
            Self::OnlyOwner => {
                formatter.write_str("only the workspace owner may perform this action")
            }
            Self::OwnerProtected => formatter.write_str("the workspace owner cannot be altered"),
            Self::TransferOwnershipRequired => {
                formatter.write_str("ownership must be transferred first")
            }
            Self::ExactlyOneOwnerRequired => {
                formatter.write_str("a workspace must have exactly one owner")
            }
            Self::WorkspaceMismatch => {
                formatter.write_str("membership belongs to another workspace")
            }
        }
    }
}

impl Error for PolicyError {}

pub struct Policy;

impl Policy {
    pub fn authorize(
        role: Role,
        action: Action,
        target_role: TargetRole,
    ) -> Result<(), PolicyError> {
        if action == Action::LeaveWorkspace && role == Role::Owner {
            return Err(PolicyError::TransferOwnershipRequired);
        }

        if target_role == TargetRole::Owner
            && matches!(
                action,
                Action::RemoveMembership | Action::ChangeMembershipRole
            )
        {
            return if role == Role::Owner {
                Err(PolicyError::TransferOwnershipRequired)
            } else {
                Err(PolicyError::OwnerProtected)
            };
        }

        if target_role == TargetRole::Owner && action == Action::AddMembership {
            return Err(PolicyError::OwnerProtected);
        }

        if matches!(
            action,
            Action::DeleteWorkspace | Action::RestoreWorkspace | Action::TransferOwnership
        ) {
            return if role == Role::Owner {
                Ok(())
            } else {
                Err(PolicyError::OnlyOwner)
            };
        }

        if Self::is_content_action(action) {
            return Ok(());
        }

        match role {
            Role::Owner | Role::Admin => Ok(()),
            Role::Member if action == Action::LeaveWorkspace => Ok(()),
            Role::Member => Err(PolicyError::Forbidden),
        }
    }

    pub fn authorize_in_workspace(
        membership: &Membership,
        workspace_id: Id,
        action: Action,
        target_role: TargetRole,
    ) -> Result<(), PolicyError> {
        if membership.workspace_id != workspace_id {
            return Err(PolicyError::WorkspaceMismatch);
        }
        Self::authorize(membership.role, action, target_role)
    }

    /// Validates one workspace from a broader membership collection.
    /// Memberships belonging to other workspaces are ignored.
    pub fn validate_owner_count(
        workspace_id: Id,
        memberships: &[Membership],
    ) -> Result<(), PolicyError> {
        let owners = memberships
            .iter()
            .filter(|membership| {
                membership.workspace_id == workspace_id && membership.role == WorkspaceRole::Owner
            })
            .count();
        if owners == 1 {
            Ok(())
        } else {
            Err(PolicyError::ExactlyOneOwnerRequired)
        }
    }

    const fn is_content_action(action: Action) -> bool {
        matches!(
            action,
            Action::CreateProject
                | Action::EditProject
                | Action::DeleteProject
                | Action::RestoreProject
                | Action::CreateStatus
                | Action::EditStatus
                | Action::DeleteStatus
                | Action::CreateTask
                | Action::EditTask
                | Action::DeleteTask
                | Action::RestoreTask
                | Action::CreateComment
                | Action::EditComment
                | Action::DeleteComment
                | Action::CreateAttachment
                | Action::DeleteAttachment
        )
    }
}
