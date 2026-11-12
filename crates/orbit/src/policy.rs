use std::error::Error;
use std::fmt;

use orbit_platform::Id;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{Membership, WorkspaceRole};

/// A workspace capability that depends on the member's role. Actions open to
/// every member (tasks, projects, attachments, docs) are not listed here.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, ToSchema)]
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
    /// Manage the workspace's mail: domains, mailboxes and their addresses and members, labels
    /// and failed receipts.
    #[serde(rename = "mail.manage")]
    MailManage,
    /// Subscribe and manage the workspace's Orbit Cloud plan.
    #[serde(rename = "billing.manage")]
    BillingManage,
    /// Connect the workspace's own S3 bucket for its files.
    #[serde(rename = "storage.manage")]
    StorageManage,
    /// Export the whole workspace as a ZIP archive and download its exports.
    #[serde(rename = "workspace.export")]
    WorkspaceExport,
    /// Export one's own private pages, private channels and DMs, and create an API token whose
    /// only scope is `workspace:export:personal`.
    #[serde(rename = "workspace.export_personal")]
    WorkspaceExportPersonal,
    /// Create, change and delete the workspace's custom roles, and give them to members. Never
    /// grantable by a custom role, so a custom role cannot hand out more than it holds.
    #[serde(rename = "roles.manage")]
    RolesManage,
}

impl Permission {
    pub const ALL: [Self; 18] = [
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
        Self::MailManage,
        Self::BillingManage,
        Self::StorageManage,
        Self::WorkspaceExport,
        Self::WorkspaceExportPersonal,
        Self::RolesManage,
    ];

    /// The permission's wire name, as in the API.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WorkspaceUpdate => "workspace.update",
            Self::WorkspaceDelete => "workspace.delete",
            Self::WorkspaceTransfer => "workspace.transfer",
            Self::MembersManage => "members.manage",
            Self::AuditView => "audit.view",
            Self::ApiTokensManage => "api_tokens.manage",
            Self::IntegrationsManage => "integrations.manage",
            Self::TeamspacesDelete => "teamspaces.delete",
            Self::PagesPurge => "pages.purge",
            Self::ViewsManageShared => "views.manage_shared",
            Self::CommentsModerate => "comments.moderate",
            Self::ChatManage => "chat.manage",
            Self::MailManage => "mail.manage",
            Self::BillingManage => "billing.manage",
            Self::StorageManage => "storage.manage",
            Self::WorkspaceExport => "workspace.export",
            Self::WorkspaceExportPersonal => "workspace.export_personal",
            Self::RolesManage => "roles.manage",
        }
    }

    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|permission| permission.as_str() == name)
    }

    /// Whether a custom role may carry the permission. The owner's own powers (deleting or
    /// handing over the workspace, billing), role management, and what every member holds
    /// already stay out.
    #[must_use]
    pub const fn is_grantable(self) -> bool {
        !matches!(
            self,
            Self::WorkspaceDelete
                | Self::WorkspaceTransfer
                | Self::BillingManage
                | Self::RolesManage
                | Self::WorkspaceExportPersonal
        )
    }

    /// The permissions a custom role may carry, in display order.
    #[must_use]
    pub fn grantable() -> Vec<Self> {
        Self::ALL
            .into_iter()
            .filter(|permission| permission.is_grantable())
            .collect()
    }

    const fn bit(self) -> u32 {
        1 << self as u32
    }
}

/// The permissions a custom role adds to an ordinary member. Only grantable permissions are
/// ever in a set.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PermissionSet(u32);

impl PermissionSet {
    pub const EMPTY: Self = Self(0);

    /// Drops the permissions that a custom role may not carry.
    #[must_use]
    pub fn of(permissions: impl IntoIterator<Item = Permission>) -> Self {
        Self(
            permissions
                .into_iter()
                .filter(|permission| permission.is_grantable())
                .fold(0, |bits, permission| bits | permission.bit()),
        )
    }

    #[must_use]
    pub const fn contains(self, permission: Permission) -> bool {
        self.0 & permission.bit() != 0
    }

    #[must_use]
    pub fn to_vec(self) -> Vec<Permission> {
        Permission::ALL
            .into_iter()
            .filter(|permission| self.contains(*permission))
            .collect()
    }
}

/// A member acting in one workspace. Every rule about what a member may do is a method here:
/// `can` for rules that depend only on the role, the rest for rules that also depend on a record.
/// A member may also hold a custom role, which adds permissions to the member role (`granted`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Actor {
    pub user_id: Id,
    pub role: WorkspaceRole,
    granted: PermissionSet,
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
    /// A member never becomes a guest; remove them and invite them again.
    GuestDowngrade,
}

impl Actor {
    #[must_use]
    pub const fn new(user_id: Id, role: WorkspaceRole) -> Self {
        Self {
            user_id,
            role,
            granted: PermissionSet::EMPTY,
        }
    }

    /// Adds the permissions of the member's custom role. Only an ordinary member holds one:
    /// owners and admins hold every permission already, and guests none.
    #[must_use]
    pub const fn with_custom_role(mut self, granted: PermissionSet) -> Self {
        if matches!(self.role, WorkspaceRole::Member) {
            self.granted = granted;
        }
        self
    }

    #[must_use]
    pub const fn can(self, permission: Permission) -> bool {
        Policy::can(self.role, permission) || self.granted.contains(permission)
    }

    /// Everything the actor may do, from the role and the custom role.
    #[must_use]
    pub fn permissions(self) -> Vec<Permission> {
        Policy::permissions_with(self.role, self.granted)
    }

    /// Guests only see the private chat channels and DMs they belong to.
    #[must_use]
    pub const fn is_guest(self) -> bool {
        matches!(self.role, WorkspaceRole::Guest)
    }

    /// Owners and admins see every project of the workspace, private ones too.
    #[must_use]
    pub const fn sees_all_projects(self) -> bool {
        Policy::can_see_project(self.role, true, false)
    }

    /// See [`Policy::can_see_project`]. Whoever sees a project may do everything in it, also
    /// change who sees it.
    #[must_use]
    pub const fn can_see_project(self, private: bool, member: bool) -> bool {
        Policy::can_see_project(self.role, private, member)
    }

    /// Only the author rewrites a task comment. `author_id` is `None` for a comment that a
    /// service account wrote: no member is its author.
    #[must_use]
    pub fn can_edit_comment(self, author_id: Option<Id>) -> bool {
        author_id == Some(self.user_id)
    }

    #[must_use]
    pub fn can_delete_comment(self, author_id: Option<Id>) -> bool {
        author_id == Some(self.user_id) || self.can(Permission::CommentsModerate)
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

    /// Tasks are shared like teamspace pages, so deleting one forever takes the same role.
    #[must_use]
    pub const fn can_purge_task(self) -> bool {
        self.can(Permission::PagesPurge)
    }

    /// Moving teamspace pages into a private space hides them from everyone else and lets the
    /// new owner delete them forever, so it takes [`Permission::PagesPurge`] unless the actor
    /// created every page that moves.
    #[must_use]
    pub const fn can_move_pages_to_private(self, created_all: bool) -> bool {
        created_all || self.can(Permission::PagesPurge)
    }

    pub fn invite(self, role: WorkspaceRole) -> Result<(), MembershipDenied> {
        if !self.can(Permission::MembersManage) {
            return Err(MembershipDenied::RoleForbidden);
        }
        if role == WorkspaceRole::Owner {
            return Err(MembershipDenied::OwnerProtected);
        }
        self.may_assign(role)
    }

    /// Only owners and admins make admins, so a custom role that manages members cannot lift
    /// anyone, itself included, above its own level.
    fn may_assign(self, role: WorkspaceRole) -> Result<(), MembershipDenied> {
        if role == WorkspaceRole::Admin
            && !matches!(self.role, WorkspaceRole::Owner | WorkspaceRole::Admin)
        {
            Err(MembershipDenied::RoleForbidden)
        } else {
            Ok(())
        }
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
        self.owner_protected(target_role)?;
        if new_role == WorkspaceRole::Guest {
            return Err(MembershipDenied::GuestDowngrade);
        }
        self.admin_protected(target_role)?;
        self.may_assign(new_role)
    }

    /// Any member may remove themselves (leave); removing others needs `MembersManage`, and
    /// removing an admin needs the owner.
    pub fn remove_member(
        self,
        target_user_id: Id,
        target_role: WorkspaceRole,
    ) -> Result<(), MembershipDenied> {
        self.owner_protected(target_role)?;
        if self.user_id == target_user_id {
            return Ok(());
        }
        if !self.can(Permission::MembersManage) {
            return Err(MembershipDenied::RoleForbidden);
        }
        self.admin_protected(target_role)
    }

    /// Only the owner changes or removes an admin's membership.
    fn admin_protected(self, target_role: WorkspaceRole) -> Result<(), MembershipDenied> {
        if target_role == WorkspaceRole::Admin && self.role != WorkspaceRole::Owner {
            Err(MembershipDenied::RoleForbidden)
        } else {
            Ok(())
        }
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
    /// What a member of `role` may do, including the permissions of their custom role. Only an
    /// ordinary member holds a custom role.
    #[must_use]
    pub fn permissions_with(role: WorkspaceRole, custom_role: PermissionSet) -> Vec<Permission> {
        let granted = if role == WorkspaceRole::Member {
            custom_role
        } else {
            PermissionSet::EMPTY
        };
        Permission::ALL
            .into_iter()
            .filter(|permission| Self::can(role, *permission) || granted.contains(*permission))
            .collect()
    }

    /// The only place that maps a role to what it may do. Guests hold no permission.
    #[must_use]
    pub const fn can(role: WorkspaceRole, permission: Permission) -> bool {
        match permission {
            Permission::WorkspaceDelete
            | Permission::WorkspaceTransfer
            | Permission::BillingManage => {
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
            | Permission::ChatManage
            | Permission::MailManage
            | Permission::StorageManage
            | Permission::WorkspaceExport
            | Permission::RolesManage => {
                matches!(role, WorkspaceRole::Owner | WorkspaceRole::Admin)
            }
            Permission::WorkspaceExportPersonal => !matches!(role, WorkspaceRole::Guest),
        }
    }

    /// The one rule for project visibility: a workspace-wide project is open to every member, a
    /// private one to the people on its member list (`member`) and to owners and admins. Guests
    /// see no project.
    #[must_use]
    pub const fn can_see_project(role: WorkspaceRole, private: bool, member: bool) -> bool {
        match role {
            WorkspaceRole::Owner | WorkspaceRole::Admin => true,
            WorkspaceRole::Member => !private || member,
            WorkspaceRole::Guest => false,
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
