use std::error::Error;
use std::fmt;

use orbit_platform::Id;
use serde::Serialize;
use utoipa::ToSchema;

use crate::{Membership, WorkspaceRole};

/// A workspace capability that depends on the member's role. Actions open to
/// every member (tasks, projects, attachments, docs) are not listed here.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ToSchema)]
pub enum Permission {
    #[serde(rename = "workspace.update")]
    WorkspaceUpdate,
    #[serde(rename = "workspace.delete")]
    WorkspaceDelete,
    #[serde(rename = "workspace.transfer")]
    WorkspaceTransfer,
    /// Invite, revoke invitations, change roles and remove other members.
    #[serde(rename = "members.manage")]
    MembersManage,
    #[serde(rename = "audit.view")]
    AuditView,
    #[serde(rename = "api_tokens.manage")]
    ApiTokensManage,
    #[serde(rename = "integrations.manage")]
    IntegrationsManage,
    #[serde(rename = "teamspaces.delete")]
    TeamspacesDelete,
    /// Permanently delete teamspace pages.
    #[serde(rename = "pages.purge")]
    PagesPurge,
    /// Edit and delete workspace-visible views owned by other members.
    #[serde(rename = "views.manage_shared")]
    ViewsManageShared,
    /// Delete task comments written by other members.
    #[serde(rename = "comments.moderate")]
    CommentsModerate,
}

impl Permission {
    pub const ALL: [Self; 11] = [
        Self::WorkspaceUpdate,
        Self::WorkspaceDelete,
        Self::WorkspaceTransfer,
        Self::MembersManage,
        Self::AuditView,
        Self::ApiTokensManage,
        Self::IntegrationsManage,
        Self::TeamspacesDelete,
        Self::PagesPurge,
        Self::ViewsManageShared,
        Self::CommentsModerate,
    ];
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyError {
    ExactlyOneOwnerRequired,
}

impl fmt::Display for PolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExactlyOneOwnerRequired => {
                formatter.write_str("a workspace must have exactly one owner")
            }
        }
    }
}

impl Error for PolicyError {}

pub struct Policy;

impl Policy {
    /// The only place that maps a role to what it may do.
    #[must_use]
    pub const fn can(role: WorkspaceRole, permission: Permission) -> bool {
        match permission {
            Permission::WorkspaceDelete | Permission::WorkspaceTransfer => {
                matches!(role, WorkspaceRole::Owner)
            }
            Permission::WorkspaceUpdate
            | Permission::MembersManage
            | Permission::AuditView
            | Permission::ApiTokensManage
            | Permission::IntegrationsManage
            | Permission::TeamspacesDelete
            | Permission::PagesPurge
            | Permission::ViewsManageShared
            | Permission::CommentsModerate => {
                matches!(role, WorkspaceRole::Owner | WorkspaceRole::Admin)
            }
        }
    }

    /// As `can`, for a role name read from storage. An unknown name may do nothing.
    #[must_use]
    pub fn stored_role_can(role: &str, permission: Permission) -> bool {
        WorkspaceRole::parse(role).is_some_and(|role| Self::can(role, permission))
    }

    #[must_use]
    pub fn permissions(role: WorkspaceRole) -> Vec<Permission> {
        Permission::ALL
            .into_iter()
            .filter(|permission| Self::can(role, *permission))
            .collect()
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
}
