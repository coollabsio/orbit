use orbit_domain::Permission;
use orbit_platform::{Database, Id, TimestampMillis, generate_opaque_token, orm_write_transaction};
use sea_orm::ActiveValue::{NotSet, Set};
use sea_orm::sea_query::{Expr, Func};
use sea_orm::{
    ColumnTrait, Condition, ConnectionTrait, DatabaseConnection, DbErr, EntityTrait, ExprTrait,
    FromQueryResult, JoinType, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, RelationTrait,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use thiserror::Error;
use utoipa::ToSchema;

use super::membership;
use crate::audit::{self, AuditOutcome};
use crate::entities::{
    api_token_projects, api_tokens, memberships, projects, service_accounts, users, workspaces,
};

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ApiTokenRecord {
    #[schema(value_type = String)]
    pub id: Id,
    pub name: String,
    pub token_prefix: String,
    pub scopes: Vec<String>,
    #[schema(value_type = String)]
    pub project_id: Id,
    #[schema(value_type = Vec<String>)]
    pub project_ids: Vec<Id>,
    #[schema(value_type = Option<String>)]
    pub service_account_id: Option<Id>,
    pub service_account_name: Option<String>,
    #[schema(value_type = Option<String>, format = DateTime)]
    pub expires_at: Option<TimestampMillis>,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
    #[schema(value_type = Option<String>, format = DateTime)]
    pub last_used_at: Option<TimestampMillis>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct IssuedApiToken {
    #[serde(flatten)]
    pub api_token: ApiTokenRecord,
    pub token: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiTokenScope {
    Read,
    Write,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApiTokenPrincipal {
    pub token_id: Id,
    pub workspace_id: Id,
    pub project_ids: Vec<Id>,
    pub creator_id: Id,
    pub service_account_id: Option<Id>,
    pub scopes: Vec<ApiTokenScope>,
}

#[derive(Debug, Error)]
pub enum ApiTokenError {
    #[error("workspace resource was not found")]
    NotFound,
    #[error("workspace administrator access is required")]
    Forbidden,
    #[error("api token repository is unavailable")]
    Unavailable(#[from] DbErr),
    #[error("api token repository contains an invalid identifier")]
    InvalidIdentifier,
}

#[derive(Clone, Debug)]
pub struct ApiTokenRepository {
    connection: DatabaseConnection,
}

/// One row of `list`: the token and the name of its service account.
#[derive(Debug, FromQueryResult)]
struct ListRow {
    id: String,
    name: String,
    token_prefix: String,
    can_read: i64,
    can_write: i64,
    project_id: Option<String>,
    created_at: i64,
    last_used_at: Option<i64>,
    expires_at: Option<i64>,
    service_account_id: Option<String>,
    service_account_name: Option<String>,
}

impl ApiTokenRepository {
    #[must_use]
    pub fn new(database: Database) -> Self {
        Self::with_connection(database.orm().clone())
    }

    /// A repository on any SeaORM connection, e.g. Postgres.
    #[must_use]
    pub fn with_connection(connection: DatabaseConnection) -> Self {
        Self { connection }
    }

    pub async fn list(
        &self,
        workspace_id: Id,
        actor_id: Id,
    ) -> Result<Vec<ApiTokenRecord>, ApiTokenError> {
        require_manager(&self.connection, workspace_id, actor_id).await?;
        let rows = api_tokens::Entity::find()
            .select_only()
            .columns([
                api_tokens::Column::Id,
                api_tokens::Column::Name,
                api_tokens::Column::TokenPrefix,
                api_tokens::Column::CanRead,
                api_tokens::Column::CanWrite,
                api_tokens::Column::ProjectId,
                api_tokens::Column::CreatedAt,
                api_tokens::Column::LastUsedAt,
                api_tokens::Column::ExpiresAt,
                api_tokens::Column::ServiceAccountId,
            ])
            .column_as(service_accounts::Column::Name, "service_account_name")
            .join(
                JoinType::LeftJoin,
                api_tokens::Relation::ServiceAccount.def(),
            )
            .filter(api_tokens::Column::WorkspaceId.eq(workspace_id.to_string()))
            .filter(api_tokens::Column::RevokedAt.is_null())
            .order_by_desc(api_tokens::Column::CreatedAt)
            .order_by_desc(api_tokens::Column::Id)
            .into_model::<ListRow>()
            .all(&self.connection)
            .await?;
        let mut records = Vec::with_capacity(rows.len());
        for row in rows {
            let id = parse_id(&row.id)?;
            records.push(decode(row, self.project_ids(id).await?)?);
        }
        Ok(records)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        &self,
        workspace_id: Id,
        actor_id: Id,
        name: String,
        project_ids: Vec<Id>,
        can_read: bool,
        can_write: bool,
        service_account: bool,
        expires_at: Option<TimestampMillis>,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<IssuedApiToken, ApiTokenError> {
        let transaction = orm_write_transaction(&self.connection).await?;
        require_manager(&transaction, workspace_id, actor_id).await?;
        if project_ids.is_empty() {
            return Err(ApiTokenError::NotFound);
        }
        for project_id in &project_ids {
            require_active_project(&transaction, workspace_id, *project_id).await?;
        }
        let id = Id::new_v7();
        let (service_account_id, service_account_name) = if service_account {
            let existing: Option<(String, String)> = service_accounts::Entity::find()
                .select_only()
                .columns([service_accounts::Column::Id, service_accounts::Column::Name])
                .filter(service_accounts::Column::WorkspaceId.eq(workspace_id.to_string()))
                .filter(
                    Func::lower(Expr::col(service_accounts::Column::Name))
                        .eq(Func::lower(Expr::val(&name))),
                )
                .filter(service_accounts::Column::DisabledAt.is_null())
                .into_tuple()
                .one(&transaction)
                .await?;
            match existing {
                Some((id, account_name)) => (Some(parse_id(&id)?), Some(account_name)),
                None => {
                    let id = Id::new_v7();
                    service_accounts::Entity::insert(service_accounts::ActiveModel {
                        id: Set(id.to_string()),
                        workspace_id: Set(workspace_id.to_string()),
                        name: Set(name.clone()),
                        created_by: Set(actor_id.to_string()),
                        created_at: Set(now.as_millis()),
                        disabled_at: NotSet,
                    })
                    .exec_without_returning(&transaction)
                    .await?;
                    (Some(id), Some(name.clone()))
                }
            }
        } else {
            (None, None)
        };
        let token = format!("orb_{}", generate_opaque_token());
        let hash: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        let prefix: String = token.chars().take(12).collect();
        api_tokens::Entity::insert(api_tokens::ActiveModel {
            id: Set(id.to_string()),
            workspace_id: Set(workspace_id.to_string()),
            project_id: Set(Some(project_ids[0].to_string())),
            name: Set(name.clone()),
            token_hash: Set(hash.to_vec()),
            token_prefix: Set(prefix.clone()),
            can_read: Set(i64::from(can_read)),
            can_write: Set(i64::from(can_write)),
            created_by: Set(actor_id.to_string()),
            created_at: Set(now.as_millis()),
            last_used_at: NotSet,
            revoked_at: NotSet,
            service_account_id: Set(service_account_id.map(|id| id.to_string())),
            expires_at: Set(expires_at.map(TimestampMillis::as_millis)),
        })
        .exec_without_returning(&transaction)
        .await?;
        api_token_projects::Entity::insert_many(project_ids.iter().map(|project_id| {
            api_token_projects::ActiveModel {
                token_id: Set(id.to_string()),
                project_id: Set(project_id.to_string()),
            }
        }))
        .exec_without_returning(&transaction)
        .await?;
        audit::record_orm(
            &transaction,
            workspace_id,
            Some(actor_id),
            "api_token.create",
            AuditOutcome::Success,
            "api_token",
            Some(id),
            request_id,
            serde_json::json!({"scopes": scopes(can_read, can_write), "project_ids": project_ids}),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(IssuedApiToken {
            api_token: ApiTokenRecord {
                id,
                name: name.clone(),
                token_prefix: prefix,
                scopes: scopes(can_read, can_write),
                project_id: project_ids[0],
                project_ids,
                service_account_id,
                service_account_name,
                expires_at,
                created_at: now,
                last_used_at: None,
            },
            token,
        })
    }

    pub async fn authenticate(
        &self,
        token: &str,
        required_scope: ApiTokenScope,
        now: TimestampMillis,
    ) -> Result<ApiTokenPrincipal, ApiTokenError> {
        let hash: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        let (id, workspace_id, created_by, service_account_id, can_read, can_write): (
            String,
            String,
            String,
            Option<String>,
            i64,
            i64,
        ) = api_tokens::Entity::find()
            .select_only()
            .columns([
                api_tokens::Column::Id,
                api_tokens::Column::WorkspaceId,
                api_tokens::Column::CreatedBy,
                api_tokens::Column::ServiceAccountId,
                api_tokens::Column::CanRead,
                api_tokens::Column::CanWrite,
            ])
            .join(JoinType::InnerJoin, api_tokens::Relation::Workspace.def())
            .join(
                JoinType::LeftJoin,
                api_tokens::Relation::Creator.def().on_condition(|_, _| {
                    Condition::all().add(users::Column::SuspendedAt.is_null())
                }),
            )
            .join(
                JoinType::LeftJoin,
                api_tokens::Relation::CreatorMembership.def(),
            )
            .join(
                JoinType::LeftJoin,
                api_tokens::Relation::ServiceAccount
                    .def()
                    .on_condition(|_, _| {
                        Condition::all()
                            .add(
                                Expr::col((
                                    service_accounts::Entity,
                                    service_accounts::Column::WorkspaceId,
                                ))
                                .equals((api_tokens::Entity, api_tokens::Column::WorkspaceId)),
                            )
                            .add(service_accounts::Column::DisabledAt.is_null())
                    }),
            )
            .filter(api_tokens::Column::TokenHash.eq(hash.to_vec()))
            .filter(api_tokens::Column::RevokedAt.is_null())
            .filter(workspaces::Column::DeletedAt.is_null())
            .filter(
                Condition::any()
                    .add(api_tokens::Column::ExpiresAt.is_null())
                    .add(api_tokens::Column::ExpiresAt.gt(now.as_millis())),
            )
            .filter(
                Condition::any()
                    .add(
                        Condition::all()
                            .add(api_tokens::Column::ServiceAccountId.is_not_null())
                            .add(service_accounts::Column::Id.is_not_null()),
                    )
                    .add(
                        Condition::all()
                            .add(api_tokens::Column::ServiceAccountId.is_null())
                            .add(users::Column::Id.is_not_null())
                            .add(memberships::Column::Id.is_not_null()),
                    ),
            )
            .into_tuple()
            .one(&self.connection)
            .await?
            .ok_or(ApiTokenError::NotFound)?;
        let (can_read, can_write) = (can_read != 0, can_write != 0);
        let allowed = match required_scope {
            ApiTokenScope::Read => can_read,
            ApiTokenScope::Write => can_write,
        };
        if !allowed {
            return Err(ApiTokenError::Forbidden);
        }
        let token_id = parse_id(&id)?;
        let workspace_id = parse_id(&workspace_id)?;
        let project_ids = self.project_ids(token_id).await?;
        if project_ids.is_empty() {
            return Err(ApiTokenError::NotFound);
        }
        let creator_id = parse_id(&created_by)?;
        let service_account_id = service_account_id.as_deref().map(parse_id).transpose()?;
        api_tokens::Entity::update_many()
            .col_expr(api_tokens::Column::LastUsedAt, Expr::val(now.as_millis()))
            .filter(api_tokens::Column::Id.eq(token_id.to_string()))
            .exec(&self.connection)
            .await?;
        Ok(ApiTokenPrincipal {
            token_id,
            workspace_id,
            project_ids,
            creator_id,
            service_account_id,
            scopes: [
                (can_read, ApiTokenScope::Read),
                (can_write, ApiTokenScope::Write),
            ]
            .into_iter()
            .filter(|(enabled, _)| *enabled)
            .map(|(_, scope)| scope)
            .collect(),
        })
    }

    async fn project_ids(&self, token_id: Id) -> Result<Vec<Id>, ApiTokenError> {
        let values: Vec<String> = api_token_projects::Entity::find()
            .select_only()
            .column(api_token_projects::Column::ProjectId)
            .join(
                JoinType::InnerJoin,
                api_token_projects::Relation::Project.def(),
            )
            .filter(api_token_projects::Column::TokenId.eq(token_id.to_string()))
            .filter(projects::Column::DeletedAt.is_null())
            .order_by_asc(projects::Column::CreatedAt)
            .order_by_asc(projects::Column::Id)
            .into_tuple()
            .all(&self.connection)
            .await?;
        values.iter().map(|value| parse_id(value)).collect()
    }

    pub async fn revoke(
        &self,
        workspace_id: Id,
        token_id: Id,
        actor_id: Id,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), ApiTokenError> {
        let transaction = orm_write_transaction(&self.connection).await?;
        require_manager(&transaction, workspace_id, actor_id).await?;
        let result = api_tokens::Entity::update_many()
            .col_expr(api_tokens::Column::RevokedAt, Expr::val(now.as_millis()))
            .filter(api_tokens::Column::Id.eq(token_id.to_string()))
            .filter(api_tokens::Column::WorkspaceId.eq(workspace_id.to_string()))
            .filter(api_tokens::Column::RevokedAt.is_null())
            .exec(&transaction)
            .await?;
        if result.rows_affected == 0 {
            return Err(ApiTokenError::NotFound);
        }
        audit::record_orm(
            &transaction,
            workspace_id,
            Some(actor_id),
            "api_token.revoke",
            AuditOutcome::Success,
            "api_token",
            Some(token_id),
            request_id,
            serde_json::json!({}),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }
}

async fn require_manager(
    connection: &impl ConnectionTrait,
    workspace_id: Id,
    actor_id: Id,
) -> Result<(), ApiTokenError> {
    let actor = membership::actor_orm(connection, workspace_id, actor_id).await?;
    if actor
        .ok_or(ApiTokenError::NotFound)?
        .can(Permission::ApiTokensManage)
    {
        Ok(())
    } else {
        Err(ApiTokenError::Forbidden)
    }
}

async fn require_active_project(
    connection: &impl ConnectionTrait,
    workspace_id: Id,
    project_id: Id,
) -> Result<(), ApiTokenError> {
    let exists = projects::Entity::find()
        .filter(projects::Column::Id.eq(project_id.to_string()))
        .filter(projects::Column::WorkspaceId.eq(workspace_id.to_string()))
        .filter(projects::Column::DeletedAt.is_null())
        .count(connection)
        .await?
        != 0;
    if exists {
        Ok(())
    } else {
        Err(ApiTokenError::NotFound)
    }
}

fn parse_id(value: &str) -> Result<Id, ApiTokenError> {
    value.parse().map_err(|_| ApiTokenError::InvalidIdentifier)
}

fn decode(row: ListRow, project_ids: Vec<Id>) -> Result<ApiTokenRecord, ApiTokenError> {
    Ok(ApiTokenRecord {
        id: parse_id(&row.id)?,
        name: row.name,
        token_prefix: row.token_prefix,
        scopes: scopes(row.can_read != 0, row.can_write != 0),
        project_id: parse_id(row.project_id.as_deref().unwrap_or_default())?,
        project_ids,
        service_account_id: row
            .service_account_id
            .as_deref()
            .map(parse_id)
            .transpose()?,
        service_account_name: row.service_account_name,
        expires_at: row.expires_at.map(TimestampMillis::from_millis),
        created_at: TimestampMillis::from_millis(row.created_at),
        last_used_at: row.last_used_at.map(TimestampMillis::from_millis),
    })
}

fn scopes(read: bool, write: bool) -> Vec<String> {
    [(read, "read"), (write, "write")]
        .into_iter()
        .filter(|(enabled, _)| *enabled)
        .map(|(_, scope)| scope.to_owned())
        .collect()
}
