use orbit_domain::{Actor, Permission};
use orbit_platform::{Database, Id, Row, TimestampMillis, generate_opaque_token};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use utoipa::ToSchema;

use super::membership;
use crate::audit::{self, AuditOutcome};

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ApiTokenRecord {
    #[schema(value_type = String)]
    pub id: Id,
    pub name: String,
    pub token_prefix: String,
    pub scopes: Vec<ApiTokenScope>,
    /// The projects whose tasks the token may use; empty when it has no task scope.
    #[schema(value_type = Vec<String>)]
    pub project_ids: Vec<Id>,
    /// Whether the task scope covers every live project of the workspace, including projects
    /// created later. `project_ids` then lists the projects of today.
    pub all_projects: bool,
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

/// What a credential may do. API tokens and OAuth grants use the same names; `tasks:write`
/// includes `tasks:read`, and `docs:write` includes `docs:read`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize, ToSchema)]
pub enum ApiTokenScope {
    #[serde(rename = "tasks:read")]
    TasksRead,
    #[serde(rename = "tasks:write")]
    TasksWrite,
    #[serde(rename = "docs:read")]
    DocsRead,
    #[serde(rename = "docs:write")]
    DocsWrite,
    #[serde(rename = "chat:read")]
    ChatRead,
}

impl ApiTokenScope {
    pub const ALL: [Self; 5] = [
        Self::TasksRead,
        Self::TasksWrite,
        Self::DocsRead,
        Self::DocsWrite,
        Self::ChatRead,
    ];

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TasksRead => "tasks:read",
            Self::TasksWrite => "tasks:write",
            Self::DocsRead => "docs:read",
            Self::DocsWrite => "docs:write",
            Self::ChatRead => "chat:read",
        }
    }

    /// The scopes of a stored space-separated list, in canonical order; unknown names are dropped.
    #[must_use]
    pub fn parse_list(value: &str) -> Vec<Self> {
        Self::ALL
            .into_iter()
            .filter(|scope| value.split_whitespace().any(|name| name == scope.as_str()))
            .collect()
    }

    /// Whether the scopes give task access, which is limited to the approved projects.
    #[must_use]
    pub fn grants_tasks(scopes: &[Self]) -> bool {
        scopes
            .iter()
            .any(|scope| matches!(scope, Self::TasksRead | Self::TasksWrite))
    }

    #[must_use]
    pub fn format_list(scopes: &[Self]) -> String {
        Self::ALL
            .into_iter()
            .filter(|scope| scopes.contains(scope))
            .map(Self::as_str)
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// The projects whose tasks a new token may use.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TokenProjects {
    /// Every live project of the workspace, including projects created later.
    All,
    Only(Vec<Id>),
}

impl From<Vec<Id>> for TokenProjects {
    fn from(ids: Vec<Id>) -> Self {
        Self::Only(ids)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApiTokenPrincipal {
    pub token_id: Id,
    pub workspace_id: Id,
    /// The approved projects, read again on each request, so an all-projects credential also
    /// covers projects created after it.
    pub project_ids: Vec<Id>,
    /// Whether the credential covers every live project, including projects created later.
    pub all_projects: bool,
    pub creator_id: Id,
    /// The member whose access a request is checked with: the creator of a personal token, the
    /// workspace owner for a service account (which does not depend on its creator).
    pub actor_id: Id,
    pub service_account: Option<ServiceAccount>,
    pub scopes: Vec<ApiTokenScope>,
}

/// The non-login identity a token acts as; reads still use the creator's membership.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceAccount {
    pub id: Id,
    pub name: String,
}

impl ApiTokenPrincipal {
    #[must_use]
    pub fn allows(&self, scope: ApiTokenScope) -> bool {
        let write = match scope {
            ApiTokenScope::TasksRead => Some(ApiTokenScope::TasksWrite),
            ApiTokenScope::DocsRead => Some(ApiTokenScope::DocsWrite),
            _ => None,
        };
        self.scopes.contains(&scope) || write.is_some_and(|write| self.scopes.contains(&write))
    }
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
        let rows = sqlx::query("SELECT api_tokens.id, api_tokens.name, api_tokens.token_prefix, api_tokens.scopes, api_tokens.all_projects, api_tokens.created_at, api_tokens.last_used_at, api_tokens.expires_at, api_tokens.service_account_id, service_accounts.name AS service_account_name FROM api_tokens LEFT JOIN service_accounts ON service_accounts.id = api_tokens.service_account_id WHERE api_tokens.workspace_id = $1 AND api_tokens.revoked_at IS NULL ORDER BY api_tokens.created_at DESC, api_tokens.id DESC")
            .bind(workspace_id.to_string()).fetch_all(self.database.pool()).await?;
        let mut records = Vec::with_capacity(rows.len());
        for row in rows {
            let id = parse_id(&row, "id")?;
            let scopes = ApiTokenScope::parse_list(&row.get::<String, _>("scopes"));
            let all_projects: bool = row.get("all_projects");
            let project_ids = self
                .project_ids(workspace_id, id, &scopes, all_projects)
                .await?;
            records.push(decode(row, project_ids)?);
        }
        Ok(records)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        &self,
        workspace_id: Id,
        actor_id: Id,
        name: String,
        projects: impl Into<TokenProjects>,
        scopes: &[ApiTokenScope],
        service_account: bool,
        expires_at: Option<TimestampMillis>,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<IssuedApiToken, ApiTokenError> {
        let scopes = ApiTokenScope::parse_list(&ApiTokenScope::format_list(scopes));
        // A service account acts as the workspace owner, so it gets no docs or chat access.
        let content_scope = scopes.iter().any(|scope| {
            matches!(
                scope,
                ApiTokenScope::DocsRead | ApiTokenScope::DocsWrite | ApiTokenScope::ChatRead
            )
        });
        // Projects only limit task access, so a token without a task scope stores none, and an
        // all-projects token reads them on each request instead.
        let grants_tasks = ApiTokenScope::grants_tasks(&scopes);
        let (all_projects, mut project_ids) = match projects.into() {
            _ if !grants_tasks => (false, Vec::new()),
            TokenProjects::All => (true, Vec::new()),
            TokenProjects::Only(ids) => (false, ids),
        };
        if scopes.is_empty() || (service_account && content_scope) {
            return Err(ApiTokenError::NotFound);
        }
        let mut transaction = self.database.immediate_transaction().await?;
        require_manager_in(&mut transaction, workspace_id, actor_id).await?;
        if grants_tasks && !all_projects && project_ids.is_empty() {
            return Err(ApiTokenError::NotFound);
        }
        for project_id in &project_ids {
            require_active_project(&mut transaction, workspace_id, *project_id).await?;
        }
        let id = Id::new_v7();
        let (service_account_id, service_account_name) = if service_account {
            // A disabled account of the same name is enabled again, so the name keeps one
            // identity and its past work stays linked.
            match sqlx::query_as::<_, (String, String, Option<i64>)>(
                "SELECT id, name, disabled_at FROM service_accounts WHERE workspace_id = $1 AND lower(name) = lower($2)",
            )
            .bind(workspace_id.to_string())
            .bind(&name)
            .fetch_optional(&mut *transaction)
            .await?
            {
                Some((account_id, account_name, disabled_at)) => {
                    let account_id: Id = account_id
                        .parse()
                        .map_err(|_| ApiTokenError::InvalidIdentifier)?;
                    if disabled_at.is_some() {
                        sqlx::query("UPDATE service_accounts SET disabled_at = NULL WHERE id = $1")
                            .bind(account_id.to_string())
                            .execute(&mut *transaction)
                            .await?;
                        audit::record(
                            &mut transaction,
                            workspace_id,
                            Some(actor_id),
                            "service_account.enable",
                            AuditOutcome::Success,
                            "service_account",
                            Some(account_id),
                            request_id,
                            serde_json::json!({"api_token_id": id}),
                            now,
                        )
                        .await?;
                    }
                    (Some(account_id), Some(account_name))
                }
                None => {
                    let id = Id::new_v7();
                    sqlx::query("INSERT INTO service_accounts (id, workspace_id, name, created_by, created_at) VALUES ($1, $2, $3, $4, $5)")
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
        sqlx::query("INSERT INTO api_tokens (id, workspace_id, project_id, name, token_hash, token_prefix, scopes, created_by, created_at, service_account_id, expires_at, all_projects) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)")
            .bind(id.to_string()).bind(workspace_id.to_string()).bind(project_ids.first().map(Id::to_string)).bind(&name).bind(hash.to_vec()).bind(&prefix)
            .bind(ApiTokenScope::format_list(&scopes)).bind(actor_id.to_string()).bind(now.as_millis()).bind(service_account_id.map(|id| id.to_string())).bind(expires_at.map(TimestampMillis::as_millis))
            .bind(i64::from(all_projects)).execute(&mut *transaction).await?;
        for project_id in &project_ids {
            sqlx::query("INSERT INTO api_token_projects (token_id, project_id) VALUES ($1, $2)")
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
            serde_json::json!({"scopes": scopes, "project_ids": project_ids, "all_projects": all_projects}),
            now,
        )
        .await?;
        if all_projects {
            project_ids = live_projects(&mut *transaction, workspace_id).await?;
        }
        transaction.commit().await?;
        Ok(IssuedApiToken {
            api_token: ApiTokenRecord {
                id,
                name: name.clone(),
                token_prefix: prefix,
                scopes,
                project_ids,
                all_projects,
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
        required: Option<ApiTokenScope>,
        now: TimestampMillis,
    ) -> Result<ApiTokenPrincipal, ApiTokenError> {
        let hash: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        let row = sqlx::query(
            "SELECT api_tokens.id, api_tokens.workspace_id, api_tokens.created_by, api_tokens.service_account_id, service_accounts.name AS service_account_name, api_tokens.scopes, api_tokens.all_projects \
             FROM api_tokens JOIN workspaces ON workspaces.id = api_tokens.workspace_id \
             LEFT JOIN users ON users.id = api_tokens.created_by AND users.suspended_at IS NULL \
             LEFT JOIN memberships ON memberships.workspace_id = api_tokens.workspace_id AND memberships.user_id = api_tokens.created_by AND memberships.role <> 'guest' \
             LEFT JOIN service_accounts ON service_accounts.id = api_tokens.service_account_id AND service_accounts.workspace_id = api_tokens.workspace_id AND service_accounts.disabled_at IS NULL \
             WHERE api_tokens.token_hash = $1 AND api_tokens.revoked_at IS NULL AND workspaces.deleted_at IS NULL \
             AND (api_tokens.expires_at IS NULL OR api_tokens.expires_at > $2) \
             AND ((api_tokens.service_account_id IS NOT NULL AND service_accounts.id IS NOT NULL) \
               OR (api_tokens.service_account_id IS NULL AND users.id IS NOT NULL AND memberships.id IS NOT NULL))",
        )
        .bind(hash.to_vec())
        .bind(now.as_millis())
        .fetch_optional(self.database.pool())
        .await?
        .ok_or(ApiTokenError::NotFound)?;
        let scopes = ApiTokenScope::parse_list(&row.get::<String, _>("scopes"));
        if scopes.is_empty() {
            return Err(ApiTokenError::NotFound);
        }
        let token_id: Id = row
            .get::<String, _>("id")
            .parse()
            .map_err(|_| ApiTokenError::InvalidIdentifier)?;
        let workspace_id: Id = row
            .get::<String, _>("workspace_id")
            .parse()
            .map_err(|_| ApiTokenError::InvalidIdentifier)?;
        let all_projects: bool = row.get("all_projects");
        let project_ids = self
            .project_ids(workspace_id, token_id, &scopes, all_projects)
            .await?;
        // A task token whose chosen projects were all deleted no longer works at all. An
        // all-projects token stays valid in a workspace without projects.
        if ApiTokenScope::grants_tasks(&scopes) && !all_projects && project_ids.is_empty() {
            return Err(ApiTokenError::NotFound);
        }
        let creator_id = row
            .get::<String, _>("created_by")
            .parse()
            .map_err(|_| ApiTokenError::InvalidIdentifier)?;
        let service_account = row
            .get::<Option<String>, _>("service_account_id")
            .map(|value| {
                Ok::<_, ApiTokenError>(ServiceAccount {
                    id: value
                        .parse()
                        .map_err(|_| ApiTokenError::InvalidIdentifier)?,
                    name: row
                        .get::<Option<String>, _>("service_account_name")
                        .unwrap_or_default(),
                })
            })
            .transpose()?;
        let actor_id = match &service_account {
            None => creator_id,
            Some(_) => sqlx::query_scalar::<_, String>(
                "SELECT user_id FROM memberships WHERE workspace_id = $1 AND role = 'owner'",
            )
            .bind(workspace_id.to_string())
            .fetch_optional(self.database.pool())
            .await?
            .ok_or(ApiTokenError::NotFound)?
            .parse()
            .map_err(|_| ApiTokenError::InvalidIdentifier)?,
        };
        let principal = ApiTokenPrincipal {
            token_id,
            workspace_id,
            project_ids,
            all_projects,
            creator_id,
            actor_id,
            service_account,
            scopes,
        };
        if required.is_some_and(|scope| !principal.allows(scope)) {
            return Err(ApiTokenError::Forbidden);
        }
        sqlx::query("UPDATE api_tokens SET last_used_at = $1 WHERE id = $2")
            .bind(now.as_millis())
            .bind(token_id.to_string())
            .execute(self.database.pool())
            .await?;
        Ok(principal)
    }

    /// The live projects of a token with a task scope: every project of the workspace for an
    /// all-projects token. Older tokens without a task scope may still have stored projects,
    /// which give them nothing.
    async fn project_ids(
        &self,
        workspace_id: Id,
        token_id: Id,
        scopes: &[ApiTokenScope],
        all_projects: bool,
    ) -> Result<Vec<Id>, ApiTokenError> {
        if !ApiTokenScope::grants_tasks(scopes) {
            return Ok(Vec::new());
        }
        if all_projects {
            return live_projects(self.database.pool(), workspace_id).await;
        }
        let values = sqlx::query_scalar::<_, String>(
            "SELECT api_token_projects.project_id FROM api_token_projects JOIN projects ON projects.id = api_token_projects.project_id WHERE api_token_projects.token_id = $1 AND projects.deleted_at IS NULL ORDER BY projects.created_at, projects.id",
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
        let service_account_id = sqlx::query_scalar::<_, Option<String>>("UPDATE api_tokens SET revoked_at = $1 WHERE id = $2 AND workspace_id = $3 AND revoked_at IS NULL RETURNING service_account_id")
            .bind(now.as_millis()).bind(token_id.to_string()).bind(workspace_id.to_string()).fetch_optional(&mut *transaction).await?
            .ok_or(ApiTokenError::NotFound)?;
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
        if let Some(account_id) = service_account_id {
            disable_unused_service_account(
                &mut transaction,
                workspace_id,
                account_id,
                actor_id,
                token_id,
                request_id,
                now,
            )
            .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    async fn require_manager(&self, workspace_id: Id, actor_id: Id) -> Result<(), ApiTokenError> {
        require_token_manager(
            membership::actor(self.database.pool(), workspace_id, actor_id).await?,
        )
    }
}

/// Disables a service account once its last token is revoked. The GitHub integration's account
/// is kept: `github_service_account_in_tx` finds it by its name, it acts without any token, and
/// tokens named "GitHub" share it.
async fn disable_unused_service_account(
    transaction: &mut sqlx::Transaction<'_, sqlx::Any>,
    workspace_id: Id,
    account_id: String,
    actor_id: Id,
    token_id: Id,
    request_id: &str,
    now: TimestampMillis,
) -> Result<(), ApiTokenError> {
    let disabled = sqlx::query(
        "UPDATE service_accounts SET disabled_at = $1 WHERE id = $2 AND workspace_id = $3 \
         AND disabled_at IS NULL AND lower(name) <> 'github' \
         AND NOT EXISTS (SELECT 1 FROM api_tokens WHERE service_account_id = $4 AND revoked_at IS NULL \
           AND (expires_at IS NULL OR expires_at > $1))",
    )
    .bind(now.as_millis())
    .bind(&account_id)
    .bind(workspace_id.to_string())
    .bind(&account_id)
    .execute(&mut **transaction)
    .await?;
    if disabled.rows_affected() == 0 {
        return Ok(());
    }
    audit::record(
        transaction,
        workspace_id,
        Some(actor_id),
        "service_account.disable",
        AuditOutcome::Success,
        "service_account",
        Some(
            account_id
                .parse()
                .map_err(|_| ApiTokenError::InvalidIdentifier)?,
        ),
        request_id,
        serde_json::json!({"api_token_id": token_id}),
        now,
    )
    .await?;
    Ok(())
}

async fn require_manager_in(
    transaction: &mut sqlx::Transaction<'_, sqlx::Any>,
    workspace_id: Id,
    actor_id: Id,
) -> Result<(), ApiTokenError> {
    require_token_manager(membership::actor(&mut **transaction, workspace_id, actor_id).await?)
}

fn require_token_manager(actor: Option<Actor>) -> Result<(), ApiTokenError> {
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
    transaction: &mut sqlx::Transaction<'_, sqlx::Any>,
    workspace_id: Id,
    project_id: Id,
) -> Result<(), ApiTokenError> {
    let exists = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM projects WHERE id = $1 AND workspace_id = $2 AND deleted_at IS NULL",
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

/// Every live project of a workspace, in creation order. An all-projects token or OAuth grant
/// uses these, so a project created later is covered and a deleted one is not.
pub(crate) async fn live_projects<'e, E>(
    executor: E,
    workspace_id: Id,
) -> Result<Vec<Id>, ApiTokenError>
where
    E: sqlx::Executor<'e, Database = sqlx::Any>,
{
    sqlx::query_scalar::<_, String>(
        "SELECT id FROM projects WHERE workspace_id = $1 AND deleted_at IS NULL ORDER BY created_at, id",
    )
    .bind(workspace_id.to_string())
    .fetch_all(executor)
    .await?
    .into_iter()
    .map(|value| value.parse().map_err(|_| ApiTokenError::InvalidIdentifier))
    .collect()
}

fn parse_id(row: &sqlx::any::AnyRow, column: &str) -> Result<Id, ApiTokenError> {
    row.get::<String, _>(column)
        .parse()
        .map_err(|_| ApiTokenError::InvalidIdentifier)
}

fn decode(row: sqlx::any::AnyRow, project_ids: Vec<Id>) -> Result<ApiTokenRecord, ApiTokenError> {
    let id = parse_id(&row, "id")?;
    Ok(ApiTokenRecord {
        id,
        name: row.get("name"),
        token_prefix: row.get("token_prefix"),
        scopes: ApiTokenScope::parse_list(&row.get::<String, _>("scopes")),
        project_ids,
        all_projects: row.get("all_projects"),
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
