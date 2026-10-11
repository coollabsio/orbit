//! SeaORM entities (spike). Only the tables and columns that `repositories::api_tokens` uses.
//! Identifiers stay `String` and timestamps stay epoch milliseconds (`i64`), as in the SQLite
//! schema; flags are `i64` 0/1, as both schemas store them (INTEGER on SQLite, BIGINT on
//! Postgres).

pub mod workspaces {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "workspaces")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub name: String,
        pub owner_membership_id: Option<String>,
        pub deleted_at: Option<i64>,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod users {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "users")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub email: String,
        pub normalized_email: String,
        pub display_name: String,
        pub password_hash: String,
        pub suspended_at: Option<i64>,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod memberships {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "memberships")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub workspace_id: String,
        pub user_id: String,
        pub role: String,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(
            belongs_to = "super::workspaces::Entity",
            from = "Column::WorkspaceId",
            to = "super::workspaces::Column::Id"
        )]
        Workspace,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod projects {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "projects")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub workspace_id: String,
        pub name: String,
        pub project_key: String,
        pub color: String,
        pub deleted_at: Option<i64>,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod service_accounts {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "service_accounts")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub workspace_id: String,
        pub name: String,
        pub created_by: String,
        pub created_at: i64,
        pub disabled_at: Option<i64>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod api_tokens {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "api_tokens")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub workspace_id: String,
        pub project_id: Option<String>,
        pub name: String,
        pub token_hash: Vec<u8>,
        pub token_prefix: String,
        pub can_read: i64,
        pub can_write: i64,
        pub created_by: String,
        pub created_at: i64,
        pub last_used_at: Option<i64>,
        pub revoked_at: Option<i64>,
        pub service_account_id: Option<String>,
        pub expires_at: Option<i64>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(
            belongs_to = "super::workspaces::Entity",
            from = "Column::WorkspaceId",
            to = "super::workspaces::Column::Id"
        )]
        Workspace,
        #[sea_orm(
            belongs_to = "super::users::Entity",
            from = "Column::CreatedBy",
            to = "super::users::Column::Id"
        )]
        Creator,
        #[sea_orm(
            belongs_to = "super::memberships::Entity",
            from = "(Column::WorkspaceId, Column::CreatedBy)",
            to = "(super::memberships::Column::WorkspaceId, super::memberships::Column::UserId)"
        )]
        CreatorMembership,
        #[sea_orm(
            belongs_to = "super::service_accounts::Entity",
            from = "Column::ServiceAccountId",
            to = "super::service_accounts::Column::Id"
        )]
        ServiceAccount,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod api_token_projects {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "api_token_projects")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub token_id: String,
        #[sea_orm(primary_key, auto_increment = false)]
        pub project_id: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(
            belongs_to = "super::projects::Entity",
            from = "Column::ProjectId",
            to = "super::projects::Column::Id"
        )]
        Project,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod audit_events {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "audit_events")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub workspace_id: Option<String>,
        pub actor_id: Option<String>,
        pub action: String,
        pub outcome: String,
        pub resource_type: String,
        pub resource_id: Option<String>,
        pub request_id: String,
        pub metadata_json: String,
        pub occurred_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
