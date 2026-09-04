use orbit_platform::{Id, TimestampMillis};

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceRole {
    Owner,
    Admin,
    Member,
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
