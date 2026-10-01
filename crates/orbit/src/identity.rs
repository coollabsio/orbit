use orbit_platform::{Id, TimestampMillis};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct User {
    pub id: Id,
    pub email: String,
    pub display_name: String,
    pub version: u64,
    pub suspended_at: Option<TimestampMillis>,
}

impl User {
    #[must_use]
    pub fn new(email: impl Into<String>, display_name: impl Into<String>) -> Self {
        Self {
            id: Id::new_v7(),
            email: email.into(),
            display_name: display_name.into(),
            version: 0,
            suspended_at: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceRole {
    Owner,
    Admin,
    Member,
}

impl WorkspaceRole {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Admin => "admin",
            Self::Member => "member",
        }
    }

    #[must_use]
    pub fn parse(role: &str) -> Option<Self> {
        match role {
            "owner" => Some(Self::Owner),
            "admin" => Some(Self::Admin),
            "member" => Some(Self::Member),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Membership {
    pub id: Id,
    pub workspace_id: Id,
    pub user_id: Id,
    pub role: WorkspaceRole,
    pub version: u64,
}

impl Membership {
    #[must_use]
    pub fn new(workspace_id: Id, user_id: Id, role: WorkspaceRole) -> Self {
        Self {
            id: Id::new_v7(),
            workspace_id,
            user_id,
            role,
            version: 0,
        }
    }
}
