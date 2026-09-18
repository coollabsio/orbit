use orbit_platform::{Database, Id, TimestampMillis, generate_opaque_token};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::Row;
use thiserror::Error;
use utoipa::ToSchema;

use crate::audit::{self, AuditOutcome};

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
    Unavailable(#[from] sqlx::Error),
    #[error("api token repository contains an invalid identifier")]
    InvalidIdentifier,
}

#[derive(Clone, Debug)]
pub struct ApiTokenRepository {
    database: Database,
}

impl ApiTokenRepository {
    #[must_use]
    pub fn new(database: Database) -> Self {
        Self { database }
    }

    pub async fn list(
        &self,
        workspace_id: Id,
        actor_id: Id,
    ) -> Result<Vec<ApiTokenRecord>, ApiTokenError> {
        self.require_manager(workspace_id, actor_id).await?;
        let rows = sqlx::query("SELECT api_tokens.id, api_tokens.name, api_tokens.token_prefix, api_tokens.can_read, api_tokens.can_write, api_tokens.project_id, api_tokens.created_at, api_tokens.last_used_at, api_tokens.expires_at, api_tokens.service_account_id, service_accounts.name AS service_account_name FROM api_tokens LEFT JOIN service_accounts ON service_accounts.id = api_tokens.service_account_id WHERE api_tokens.workspace_id = ? AND api_tokens.revoked_at IS NULL ORDER BY api_tokens.created_at DESC, api_tokens.id DESC")
            .bind(workspace_id.to_string()).fetch_all(self.database.pool()).await?;
        let mut records = Vec::with_capacity(rows.len());
        for row in rows {
            let id = parse_id(&row, "id")?;
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
        let mut transaction = self.database.immediate_transaction().await?;
        require_manager_in(&mut transaction, workspace_id, actor_id).await?;
        if project_ids.is_empty() {
            return Err(ApiTokenError::NotFound);
        }
        for project_id in &project_ids {
            require_active_project(&mut transaction, workspace_id, *project_id).await?;
        }
        let id = Id::new_v7();
        let (service_account_id, service_account_name) = if service_account {
            match sqlx::query_as::<_, (String, String)>(
                "SELECT id, name FROM service_accounts WHERE workspace_id = ? AND lower(name) = lower(?) AND disabled_at IS NULL",
            )
            .bind(workspace_id.to_string())
            .bind(&name)
            .fetch_optional(&mut *transaction)
            .await?
            {
                Some((id, account_name)) => (
                    Some(id.parse().map_err(|_| ApiTokenError::InvalidIdentifier)?),
                    Some(account_name),
                ),
                None => {
                    let id = Id::new_v7();
                    sqlx::query("INSERT INTO service_accounts (id, workspace_id, name, created_by, created_at) VALUES (?, ?, ?, ?, ?)")
                        .bind(id.to_string()).bind(workspace_id.to_string()).bind(&name)
                        .bind(actor_id.to_string()).bind(now.as_millis()).execute(&mut *transaction).await?;
                    (Some(id), Some(name.clone()))
                }
            }
        } else {
            (None, None)
        };
        let token = format!("orb_{}", generate_opaque_token());
        let hash: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        let prefix: String = token.chars().take(12).collect();
        sqlx::query("INSERT INTO api_tokens (id, workspace_id, project_id, name, token_hash, token_prefix, can_read, can_write, created_by, created_at, service_account_id, expires_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(id.to_string()).bind(workspace_id.to_string()).bind(project_ids[0].to_string()).bind(&name).bind(hash.to_vec()).bind(&prefix)
            .bind(i64::from(can_read)).bind(i64::from(can_write)).bind(actor_id.to_string()).bind(now.as_millis()).bind(service_account_id.map(|id| id.to_string())).bind(expires_at.map(TimestampMillis::as_millis))
            .execute(&mut *transaction).await?;
        for project_id in &project_ids {
            sqlx::query("INSERT INTO api_token_projects (token_id, project_id) VALUES (?, ?)")
                .bind(id.to_string())
                .bind(project_id.to_string())
                .execute(&mut *transaction)
                .await?;
        }
        audit::record(
            &mut transaction,
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
        let row = sqlx::query(
            "SELECT api_tokens.id, api_tokens.workspace_id, api_tokens.created_by, api_tokens.service_account_id, api_tokens.can_read, api_tokens.can_write \
             FROM api_tokens JOIN workspaces ON workspaces.id = api_tokens.workspace_id \
             LEFT JOIN users ON users.id = api_tokens.created_by AND users.suspended_at IS NULL \
             LEFT JOIN memberships ON memberships.workspace_id = api_tokens.workspace_id AND memberships.user_id = api_tokens.created_by \
             LEFT JOIN service_accounts ON service_accounts.id = api_tokens.service_account_id AND service_accounts.workspace_id = api_tokens.workspace_id AND service_accounts.disabled_at IS NULL \
             WHERE api_tokens.token_hash = ? AND api_tokens.revoked_at IS NULL AND workspaces.deleted_at IS NULL \
             AND (api_tokens.expires_at IS NULL OR api_tokens.expires_at > ?) \
             AND ((api_tokens.service_account_id IS NOT NULL AND service_accounts.id IS NOT NULL) \
               OR (api_tokens.service_account_id IS NULL AND users.id IS NOT NULL AND memberships.id IS NOT NULL))",
        )
        .bind(hash.to_vec())
        .bind(now.as_millis())
        .fetch_optional(self.database.pool())
        .await?
        .ok_or(ApiTokenError::NotFound)?;
        let can_read = row.get::<i64, _>("can_read") != 0;
        let can_write = row.get::<i64, _>("can_write") != 0;
        let allowed = match required_scope {
            ApiTokenScope::Read => can_read,
            ApiTokenScope::Write => can_write,
        };
        if !allowed {
            return Err(ApiTokenError::Forbidden);
        }
        let token_id: Id = row
            .get::<String, _>("id")
            .parse()
            .map_err(|_| ApiTokenError::InvalidIdentifier)?;
        let workspace_id = row
            .get::<String, _>("workspace_id")
            .parse()
            .map_err(|_| ApiTokenError::InvalidIdentifier)?;
        let project_ids = self.project_ids(token_id).await?;
        if project_ids.is_empty() {
            return Err(ApiTokenError::NotFound);
        }
        let creator_id = row
            .get::<String, _>("created_by")
            .parse()
            .map_err(|_| ApiTokenError::InvalidIdentifier)?;
        let service_account_id = row
            .get::<Option<String>, _>("service_account_id")
            .map(|value| value.parse().map_err(|_| ApiTokenError::InvalidIdentifier))
            .transpose()?;
        sqlx::query("UPDATE api_tokens SET last_used_at = ? WHERE id = ?")
            .bind(now.as_millis())
            .bind(token_id.to_string())
            .execute(self.database.pool())
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
        let values = sqlx::query_scalar::<_, String>(
            "SELECT api_token_projects.project_id FROM api_token_projects JOIN projects ON projects.id = api_token_projects.project_id WHERE api_token_projects.token_id = ? AND projects.deleted_at IS NULL ORDER BY projects.created_at, projects.id",
        )
        .bind(token_id.to_string())
        .fetch_all(self.database.pool())
        .await?;
        values
            .into_iter()
            .map(|value| value.parse().map_err(|_| ApiTokenError::InvalidIdentifier))
            .collect()
    }

    pub async fn revoke(
        &self,
        workspace_id: Id,
        token_id: Id,
        actor_id: Id,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), ApiTokenError> {
        let mut transaction = self.database.immediate_transaction().await?;
        require_manager_in(&mut transaction, workspace_id, actor_id).await?;
        let result = sqlx::query("UPDATE api_tokens SET revoked_at = ? WHERE id = ? AND workspace_id = ? AND revoked_at IS NULL")
            .bind(now.as_millis()).bind(token_id.to_string()).bind(workspace_id.to_string()).execute(&mut *transaction).await?;
        if result.rows_affected() == 0 {
            return Err(ApiTokenError::NotFound);
        }
        audit::record(
            &mut transaction,
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

    async fn require_manager(&self, workspace_id: Id, actor_id: Id) -> Result<(), ApiTokenError> {
        let role = sqlx::query_scalar::<_, String>("SELECT memberships.role FROM memberships JOIN workspaces ON workspaces.id = memberships.workspace_id WHERE memberships.workspace_id = ? AND memberships.user_id = ? AND workspaces.deleted_at IS NULL")
            .bind(workspace_id.to_string()).bind(actor_id.to_string()).fetch_optional(self.database.pool()).await?.ok_or(ApiTokenError::NotFound)?;
        if role == "member" {
            Err(ApiTokenError::Forbidden)
        } else {
            Ok(())
        }
    }
}

async fn require_manager_in(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    workspace_id: Id,
    actor_id: Id,
) -> Result<(), ApiTokenError> {
    let role = sqlx::query_scalar::<_, String>("SELECT memberships.role FROM memberships JOIN workspaces ON workspaces.id = memberships.workspace_id WHERE memberships.workspace_id = ? AND memberships.user_id = ? AND workspaces.deleted_at IS NULL")
        .bind(workspace_id.to_string()).bind(actor_id.to_string()).fetch_optional(&mut **transaction).await?.ok_or(ApiTokenError::NotFound)?;
    if role == "member" {
        Err(ApiTokenError::Forbidden)
    } else {
        Ok(())
    }
}

async fn require_active_project(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    workspace_id: Id,
    project_id: Id,
) -> Result<(), ApiTokenError> {
    let exists = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM projects WHERE id = ? AND workspace_id = ? AND deleted_at IS NULL",
    )
    .bind(project_id.to_string())
    .bind(workspace_id.to_string())
    .fetch_one(&mut **transaction)
    .await?
        != 0;
    if exists {
        Ok(())
    } else {
        Err(ApiTokenError::NotFound)
    }
}

fn parse_id(row: &sqlx::sqlite::SqliteRow, column: &str) -> Result<Id, ApiTokenError> {
    row.get::<String, _>(column)
        .parse()
        .map_err(|_| ApiTokenError::InvalidIdentifier)
}

fn decode(
    row: sqlx::sqlite::SqliteRow,
    project_ids: Vec<Id>,
) -> Result<ApiTokenRecord, ApiTokenError> {
    let id = parse_id(&row, "id")?;
    let can_read = row.get::<i64, _>("can_read") != 0;
    let can_write = row.get::<i64, _>("can_write") != 0;
    Ok(ApiTokenRecord {
        id,
        name: row.get("name"),
        token_prefix: row.get("token_prefix"),
        scopes: scopes(can_read, can_write),
        project_id: parse_id(&row, "project_id")?,
        project_ids,
        service_account_id: row
            .get::<Option<String>, _>("service_account_id")
            .map(|value| value.parse().map_err(|_| ApiTokenError::InvalidIdentifier))
            .transpose()?,
        service_account_name: row.get("service_account_name"),
        expires_at: row
            .get::<Option<i64>, _>("expires_at")
            .map(TimestampMillis::from_millis),
        created_at: TimestampMillis::from_millis(row.get("created_at")),
        last_used_at: row
            .get::<Option<i64>, _>("last_used_at")
            .map(TimestampMillis::from_millis),
    })
}

fn scopes(read: bool, write: bool) -> Vec<String> {
    [(read, "read"), (write, "write")]
        .into_iter()
        .filter(|(enabled, _)| *enabled)
        .map(|(_, scope)| scope.to_owned())
        .collect()
}
