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
    /// Manage chat categories and channel order, edit and archive channels made by other
    /// members, and delete other members' chat messages.
    #[serde(rename = "chat.manage")]
    ChatManage,
}

impl Permission {
    pub const ALL: [Self; 12] = [
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
        Self::ChatManage,
    ];
}

/// A member acting in one workspace. Every rule about what a member may do is a method here:
/// `can` for rules that depend only on the role, the rest for rules that also depend on a record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Actor {
    pub user_id: Id,
    pub role: WorkspaceRole,
}

/// Why a membership change is refused.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MembershipDenied {
    /// The actor's role does not allow the change.
    RoleForbidden,
    /// The owner's membership, and the owner role, are out of reach.
    OwnerProtected,
    /// The owner role only moves through an ownership transfer.
    TransferRequired,
}

impl Actor {
    #[must_use]
    pub const fn can(self, permission: Permission) -> bool {
        Policy::can(self.role, permission)
    }

    /// Only the author rewrites a task comment.
    #[must_use]
    pub fn can_edit_comment(self, author_id: Id) -> bool {
        self.user_id == author_id
    }

    #[must_use]
    pub fn can_delete_comment(self, author_id: Id) -> bool {
        self.user_id == author_id || self.can(Permission::CommentsModerate)
    }

    /// `shared` is workspace visibility; a personal view is only ever its owner's.
    #[must_use]
    pub fn can_edit_view(self, owner_id: Id, shared: bool) -> bool {
        self.user_id == owner_id || (shared && self.can(Permission::ViewsManageShared))
    }

    #[must_use]
    pub fn can_change_view_visibility(self, owner_id: Id) -> bool {
        self.user_id == owner_id
    }

    /// A channel's creator and chat managers edit it, archive it and manage its members.
    #[must_use]
    pub fn can_manage_chat_channel(self, created_by: Id) -> bool {
        self.user_id == created_by || self.can(Permission::ChatManage)
    }

    /// Only the author rewrites a chat message; `can_delete_chat_message` also lets managers in.
    #[must_use]
    pub fn can_edit_chat_message(self, author_id: Id) -> bool {
        self.user_id == author_id
    }

    #[must_use]
    pub fn can_delete_chat_message(self, author_id: Id) -> bool {
        self.user_id == author_id || self.can(Permission::ChatManage)
    }

    /// A private page is visible to its owner alone, so whoever sees one may delete it forever.
    #[must_use]
    pub const fn can_purge_page(self, private: bool) -> bool {
        private || self.can(Permission::PagesPurge)
    }

    pub fn invite(self, role: WorkspaceRole) -> Result<(), MembershipDenied> {
        if !self.can(Permission::MembersManage) {
            return Err(MembershipDenied::RoleForbidden);
        }
        if role == WorkspaceRole::Owner {
            return Err(MembershipDenied::OwnerProtected);
        }
        Ok(())
    }

    pub fn change_member_role(
        self,
        target_role: WorkspaceRole,
        new_role: WorkspaceRole,
    ) -> Result<(), MembershipDenied> {
        if new_role == WorkspaceRole::Owner {
            return Err(MembershipDenied::TransferRequired);
        }
        if !self.can(Permission::MembersManage) {
            return Err(MembershipDenied::RoleForbidden);
        }
        self.owner_protected(target_role)
    }

    /// Any member may remove themselves (leave); removing others needs `MembersManage`.
    pub fn remove_member(
        self,
        target_user_id: Id,
        target_role: WorkspaceRole,
    ) -> Result<(), MembershipDenied> {
        self.owner_protected(target_role)?;
        if self.user_id != target_user_id && !self.can(Permission::MembersManage) {
            return Err(MembershipDenied::RoleForbidden);
        }
        Ok(())
    }

    #[must_use]
    pub fn can_transfer_ownership_to(self, target_user_id: Id) -> bool {
        self.can(Permission::WorkspaceTransfer) && self.user_id != target_user_id
    }

    /// The owner's membership never changes directly, not even by the owner.
    fn owner_protected(self, target_role: WorkspaceRole) -> Result<(), MembershipDenied> {
        if target_role != WorkspaceRole::Owner {
            Ok(())
        } else if self.role == WorkspaceRole::Owner {
            Err(MembershipDenied::TransferRequired)
        } else {
            Err(MembershipDenied::OwnerProtected)
        }
    }
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
            | Permission::CommentsModerate
            | Permission::ChatManage => {
                matches!(role, WorkspaceRole::Owner | WorkspaceRole::Admin)
            }
        }
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
