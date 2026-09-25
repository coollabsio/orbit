use orbit_domain::WorkspaceDefaults;
use orbit_platform::{
    AuthenticatedUser, Database, Id, IssuedSession, IssuedToken, SessionRecord, TimestampMillis,
    generate_opaque_token, normalize_email,
};
use sha2::{Digest, Sha256};
use sqlx::{Row, Sqlite, Transaction};
use thiserror::Error;

use super::teamspaces::insert_default_teamspace;
use crate::audit::{self, AuditOutcome};

#[derive(Clone)]
pub struct SetupRequest {
    pub token: String,
    pub email: String,
    pub display_name: String,
    pub password_hash: String,
    pub workspace_name: String,
    pub project_name: String,
}

#[derive(Clone, Eq, PartialEq)]
pub struct StoredIdentity {
    pub id: Id,
    pub email: String,
    pub normalized_email: String,
    pub display_name: String,
    pub password_hash: String,
    pub suspended: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetupResult {
    pub user_id: Id,
    pub workspace_id: Id,
    pub project_id: Id,
    pub session: IssuedSession,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthenticatedSession {
    pub id: Id,
    pub user: AuthenticatedUser,
    pub absolute_expires_at: TimestampMillis,
}

#[derive(Debug, Error)]
pub enum SetupError {
    #[error("setup has already been completed")]
    AlreadyComplete,
    #[error("setup token is invalid or expired")]
    InvalidToken,
    #[error("identity repository is unavailable")]
    Unavailable(#[source] sqlx::Error),
}

#[derive(Debug, Error)]
pub enum IdentityError {
    #[error("identity repository is unavailable")]
    Unavailable(#[from] sqlx::Error),
    #[error("identity repository contains an invalid identifier")]
    InvalidIdentifier,
    #[error("session or token is invalid or expired")]
    InvalidCredential,
}

#[derive(Debug, Error)]
pub enum SuspensionError {
    #[error("only an installation administrator may suspend accounts")]
    Forbidden,
    #[error("global user was not found")]
    NotFound,
    #[error("identity repository is unavailable")]
    Unavailable(#[from] sqlx::Error),
}

#[derive(Clone, Debug)]
pub struct IdentityRepository {
    database: Database,
}

impl IdentityRepository {
    const SETUP_TOKEN_LIFETIME_MILLIS: i64 = 30 * 60 * 1_000;

    #[must_use]
    pub fn new(database: Database) -> Self {
        Self { database }
    }

    #[must_use]
    pub fn database(&self) -> &Database {
        &self.database
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_security_event(
        &self,
        actor_id: Option<Id>,
        action: &str,
        outcome: AuditOutcome,
        resource_type: &str,
        resource_id: Option<Id>,
        request_id: &str,
        metadata: serde_json::Value,
        now: TimestampMillis,
    ) -> Result<(), IdentityError> {
        let mut transaction = self.database.immediate_transaction().await?;
        audit::record_global(
            &mut transaction,
            actor_id,
            action,
            outcome,
            resource_type,
            resource_id,
            request_id,
            metadata,
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn initialize_setup_token(
        &self,
        now: TimestampMillis,
    ) -> Result<Option<IssuedToken>, IdentityError> {
        let mut transaction = self.database.immediate_transaction().await?;
        sqlx::query("INSERT OR IGNORE INTO installation_state (id, initialized) VALUES (1, 0)")
            .execute(&mut *transaction)
            .await?;
        let initialized =
            sqlx::query_scalar::<_, i64>("SELECT initialized FROM installation_state WHERE id = 1")
                .fetch_one(&mut *transaction)
                .await?;
        if initialized != 0 {
            return Ok(None);
        }
        sqlx::query("DELETE FROM setup_tokens WHERE expires_at <= ?")
            .bind(now.as_millis())
            .execute(&mut *transaction)
            .await?;
        let exists = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM setup_tokens")
            .fetch_one(&mut *transaction)
            .await?
            != 0;
        if exists {
            transaction.commit().await?;
            return Ok(None);
        }
        let issued = issue_setup_token(now);
        insert_setup_token(&mut transaction, &issued, now).await?;
        transaction.commit().await?;
        Ok(Some(issued))
    }

    pub async fn rotate_setup_token(
        &self,
        now: TimestampMillis,
    ) -> Result<IssuedToken, IdentityError> {
        let mut transaction = self.database.immediate_transaction().await?;
        sqlx::query("INSERT OR IGNORE INTO installation_state (id, initialized) VALUES (1, 0)")
            .execute(&mut *transaction)
            .await?;
        let initialized =
            sqlx::query_scalar::<_, i64>("SELECT initialized FROM installation_state WHERE id = 1")
                .fetch_one(&mut *transaction)
                .await?;
        if initialized != 0 {
            return Err(IdentityError::InvalidCredential);
        }
        sqlx::query("DELETE FROM setup_tokens")
            .execute(&mut *transaction)
            .await?;
        let issued = issue_setup_token(now);
        insert_setup_token(&mut transaction, &issued, now).await?;
        transaction.commit().await?;
        Ok(issued)
    }

    pub async fn setup_complete(&self) -> Result<bool, IdentityError> {
        let initialized = sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE((SELECT initialized FROM installation_state WHERE id = 1), 0)",
        )
        .fetch_one(self.database.pool())
        .await?;
        Ok(initialized != 0)
    }

    pub async fn setup_token_valid(
        &self,
        token: &str,
        now: TimestampMillis,
    ) -> Result<bool, IdentityError> {
        if self.setup_complete().await? {
            return Ok(false);
        }
        let supplied = token_hash(token);
        let row =
            sqlx::query("SELECT token_hash, expires_at FROM setup_tokens WHERE token_hash = ?")
                .bind(supplied.to_vec())
                .fetch_optional(self.database.pool())
                .await?;
        Ok(row.is_some_and(|row| {
            row.get::<i64, _>("expires_at") > now.as_millis()
                && constant_time_eq(&row.get::<Vec<u8>, _>("token_hash"), &supplied)
        }))
    }

    pub async fn store_setup_token(
        &self,
        token: &str,
        expires_at: TimestampMillis,
    ) -> Result<(), IdentityError> {
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        sqlx::query("INSERT OR IGNORE INTO installation_state (id, initialized) VALUES (1, 0)")
            .execute(&mut *transaction)
            .await?;
        let initialized =
            sqlx::query_scalar::<_, i64>("SELECT initialized FROM installation_state WHERE id = 1")
                .fetch_one(&mut *transaction)
                .await?;
        if initialized == 0 {
            sqlx::query("DELETE FROM setup_tokens")
                .execute(&mut *transaction)
                .await?;
            sqlx::query(
                "INSERT INTO setup_tokens (token_hash, expires_at, created_at) VALUES (?, ?, ?)",
            )
            .bind(token_hash(token).to_vec())
            .bind(expires_at.as_millis())
            .bind(now.as_millis())
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    pub async fn complete_setup(
        &self,
        request: SetupRequest,
        now: TimestampMillis,
    ) -> Result<SetupResult, SetupError> {
        self.complete_setup_inner(request, now, None).await
    }

    pub async fn complete_setup_audited(
        &self,
        request: SetupRequest,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<SetupResult, SetupError> {
        self.complete_setup_inner(request, now, Some(request_id))
            .await
    }

    async fn complete_setup_inner(
        &self,
        request: SetupRequest,
        now: TimestampMillis,
        request_id: Option<&str>,
    ) -> Result<SetupResult, SetupError> {
        let mut transaction = self
            .database
            .immediate_transaction()
            .await
            .map_err(SetupError::Unavailable)?;
        sqlx::query("INSERT OR IGNORE INTO installation_state (id, initialized) VALUES (1, 0)")
            .execute(&mut *transaction)
            .await
            .map_err(SetupError::Unavailable)?;
        let initialized =
            sqlx::query_scalar::<_, i64>("SELECT initialized FROM installation_state WHERE id = 1")
                .fetch_one(&mut *transaction)
                .await
                .map_err(SetupError::Unavailable)?;
        if initialized != 0 {
            if let Some(request_id) = request_id {
                audit::record_global(
                    &mut transaction,
                    None,
                    "setup.complete",
                    AuditOutcome::Failure,
                    "installation",
                    None,
                    request_id,
                    serde_json::json!({"reason":"already_complete"}),
                    now,
                )
                .await
                .map_err(SetupError::Unavailable)?;
                transaction
                    .commit()
                    .await
                    .map_err(SetupError::Unavailable)?;
            }
            return Err(SetupError::AlreadyComplete);
        }

        let supplied_hash = token_hash(&request.token);
        let stored = sqlx::query("SELECT token_hash, expires_at FROM setup_tokens")
            .fetch_optional(&mut *transaction)
            .await
            .map_err(SetupError::Unavailable)?;
        let valid_token = stored.is_some_and(|row| {
            let hash = row.get::<Vec<u8>, _>("token_hash");
            row.get::<i64, _>("expires_at") > now.as_millis()
                && constant_time_eq(&hash, &supplied_hash)
        });
        if !valid_token {
            if let Some(request_id) = request_id {
                audit::record_global(
                    &mut transaction,
                    None,
                    "setup.complete",
                    AuditOutcome::Failure,
                    "installation",
                    None,
                    request_id,
                    serde_json::json!({"reason":"invalid_token"}),
                    now,
                )
                .await
                .map_err(SetupError::Unavailable)?;
                transaction
                    .commit()
                    .await
                    .map_err(SetupError::Unavailable)?;
            }
            return Err(SetupError::InvalidToken);
        }

        let email = request.email.trim().to_owned();
        let normalized_email = normalize_email(&email);
        let user_id = Id::new_v7();
        let session_id = Id::new_v7();
        let session_token = generate_opaque_token();
        let defaults =
            WorkspaceDefaults::new(user_id, request.workspace_name, request.project_name);
        let timestamp = now.as_millis();
        sqlx::query(
            "INSERT INTO users (id, email, normalized_email, display_name, password_hash, \
             installation_admin, created_at, updated_at) VALUES (?, ?, ?, ?, ?, 1, ?, ?)",
        )
        .bind(user_id.to_string())
        .bind(email)
        .bind(normalized_email)
        .bind(request.display_name)
        .bind(request.password_hash)
        .bind(timestamp)
        .bind(timestamp)
        .execute(&mut *transaction)
        .await
        .map_err(SetupError::Unavailable)?;
        sqlx::query(
            "INSERT INTO workspaces (id, name, version, owner_membership_id, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(defaults.workspace.id.to_string())
        .bind(&defaults.workspace.name)
        .bind(defaults.workspace.version as i64)
        .bind(defaults.owner.id.to_string())
        .bind(timestamp)
        .bind(timestamp)
        .execute(&mut *transaction)
        .await
        .map_err(SetupError::Unavailable)?;
        sqlx::query(
            "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at) \
             VALUES (?, ?, ?, 'owner', ?, ?, ?)",
        )
        .bind(defaults.owner.id.to_string())
        .bind(defaults.workspace.id.to_string())
        .bind(user_id.to_string())
        .bind(defaults.owner.version as i64)
        .bind(timestamp)
        .bind(timestamp)
        .execute(&mut *transaction)
        .await
        .map_err(SetupError::Unavailable)?;
        sqlx::query(
            "INSERT INTO projects (id, workspace_id, name, project_key, color, version, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(defaults.project.id.to_string())
        .bind(defaults.workspace.id.to_string())
        .bind(&defaults.project.name)
        .bind(&defaults.project.key)
        .bind(&defaults.project.color)
        .bind(defaults.project.version as i64)
        .bind(timestamp)
        .bind(timestamp)
        .execute(&mut *transaction)
        .await
        .map_err(SetupError::Unavailable)?;
        for status in &defaults.statuses {
            sqlx::query(
                "INSERT INTO task_statuses (id, workspace_id, project_id, name, description, color, \
                 category, position, version, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(status.id.to_string())
            .bind(status.workspace_id.to_string())
            .bind(status.project_id.to_string())
            .bind(&status.name)
            .bind(&status.description)
            .bind(&status.color)
            .bind(status.category.as_str())
            .bind(status.position)
            .bind(status.version as i64)
            .bind(timestamp)
            .bind(timestamp)
            .execute(&mut *transaction)
            .await
            .map_err(SetupError::Unavailable)?;
        }
        insert_default_teamspace(&mut transaction, defaults.workspace.id, user_id, now)
            .await
            .map_err(SetupError::Unavailable)?;
        let session = insert_session(&mut transaction, session_id, &session_token, user_id, now)
            .await
            .map_err(SetupError::Unavailable)?;
        sqlx::query(
            "UPDATE installation_state SET initialized = 1, initialized_at = ? WHERE id = 1",
        )
        .bind(timestamp)
        .execute(&mut *transaction)
        .await
        .map_err(SetupError::Unavailable)?;
        sqlx::query("DELETE FROM setup_tokens")
            .execute(&mut *transaction)
            .await
            .map_err(SetupError::Unavailable)?;
        if let Some(request_id) = request_id {
            audit::record_global(
                &mut transaction,
                Some(user_id),
                "setup.complete",
                AuditOutcome::Success,
                "installation",
                None,
                request_id,
                serde_json::json!({}),
                now,
            )
            .await
            .map_err(SetupError::Unavailable)?;
        }
        transaction
            .commit()
            .await
            .map_err(SetupError::Unavailable)?;

        Ok(SetupResult {
            user_id,
            workspace_id: defaults.workspace.id,
            project_id: defaults.project.id,
            session,
        })
    }

    pub async fn find_by_email(
        &self,
        email: &str,
    ) -> Result<Option<StoredIdentity>, IdentityError> {
        let row = sqlx::query(
            "SELECT id, email, normalized_email, display_name, password_hash, suspended_at \
             FROM users WHERE normalized_email = ?",
        )
        .bind(normalize_email(email))
        .fetch_optional(self.database.pool())
        .await?;
        row.map(decode_identity).transpose()
    }

    pub async fn is_installation_admin(&self, user_id: Id) -> Result<bool, IdentityError> {
        Ok(sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM users WHERE id = ? AND installation_admin = 1 AND suspended_at IS NULL",
        )
        .bind(user_id.to_string())
        .fetch_one(self.database.pool())
        .await?
            != 0)
    }

    pub async fn set_suspended(
        &self,
        actor_id: Id,
        user_id: Id,
        suspended: bool,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), SuspensionError> {
        let mut transaction = self.database.immediate_transaction().await?;
        let installation_admin = sqlx::query_scalar::<_, i64>(
            "SELECT installation_admin FROM users WHERE id = ? AND suspended_at IS NULL",
        )
        .bind(actor_id.to_string())
        .fetch_optional(&mut *transaction)
        .await?
        .unwrap_or(0);
        if installation_admin == 0 {
            audit::record_global(
                &mut transaction,
                Some(actor_id),
                "account.suspension_denied",
                AuditOutcome::Failure,
                "user",
                Some(user_id),
                request_id,
                serde_json::json!({"reason": "installation_admin_required"}),
                now,
            )
            .await?;
            transaction.commit().await?;
            return Err(SuspensionError::Forbidden);
        }
        let changed = if suspended {
            sqlx::query(
                "UPDATE users SET suspended_at = COALESCE(suspended_at, ?), version = version + 1, \
                 updated_at = ? WHERE id = ?",
            )
            .bind(now.as_millis())
            .bind(now.as_millis())
            .bind(user_id.to_string())
            .execute(&mut *transaction)
            .await?
            .rows_affected()
        } else {
            sqlx::query(
                "UPDATE users SET suspended_at = NULL, version = version + 1, updated_at = ? WHERE id = ?",
            )
            .bind(now.as_millis())
            .bind(user_id.to_string())
            .execute(&mut *transaction)
            .await?
            .rows_affected()
        };
        if changed == 0 {
            return Err(SuspensionError::NotFound);
        }
        if suspended {
            sqlx::query(
                "UPDATE sessions SET revoked_at = ? WHERE user_id = ? AND revoked_at IS NULL",
            )
            .bind(now.as_millis())
            .bind(user_id.to_string())
            .execute(&mut *transaction)
            .await?;
        }
        audit::record_global(
            &mut transaction,
            Some(actor_id),
            if suspended {
                "account.suspended"
            } else {
                "account.reinstated"
            },
            AuditOutcome::Success,
            "user",
            Some(user_id),
            request_id,
            serde_json::json!({}),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn update_password_hash(
        &self,
        user_id: Id,
        password_hash: &str,
        now: TimestampMillis,
    ) -> Result<(), IdentityError> {
        sqlx::query(
            "UPDATE users SET password_hash = ?, updated_at = ?, version = version + 1 WHERE id = ?",
        )
        .bind(password_hash)
        .bind(now.as_millis())
        .bind(user_id.to_string())
        .execute(self.database.pool())
        .await?;
        Ok(())
    }

    pub async fn update_display_name_audited(
        &self,
        user_id: Id,
        display_name: &str,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), IdentityError> {
        let mut transaction = self.database.immediate_transaction().await?;
        let changed = sqlx::query(
            "UPDATE users SET display_name = ?, updated_at = ?, version = version + 1 WHERE id = ?",
        )
        .bind(display_name)
        .bind(now.as_millis())
        .bind(user_id.to_string())
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if changed == 0 {
            return Err(IdentityError::InvalidCredential);
        }
        audit::record_global(
            &mut transaction,
            Some(user_id),
            "account.updated",
            AuditOutcome::Success,
            "user",
            Some(user_id),
            request_id,
            serde_json::json!({"fields":["display_name"]}),
            now,
        )
        .await?;
        let workspaces: Vec<String> =
            sqlx::query_scalar("SELECT workspace_id FROM memberships WHERE user_id = ?")
                .bind(user_id.to_string())
                .fetch_all(&mut *transaction)
                .await?;
        for workspace in workspaces {
            audit::record(
                &mut transaction,
                workspace
                    .parse()
                    .map_err(|_| IdentityError::InvalidCredential)?,
                Some(user_id),
                "member.profile_updated",
                AuditOutcome::Success,
                "user",
                Some(user_id),
                request_id,
                serde_json::json!({"fields":["display_name"]}),
                now,
            )
            .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    pub async fn change_password_keeping_session(
        &self,
        user_id: Id,
        keep_session_id: Id,
        password_hash: &str,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), IdentityError> {
        let mut transaction = self.database.immediate_transaction().await?;
        let changed = sqlx::query(
            "UPDATE users SET password_hash = ?, updated_at = ?, version = version + 1 WHERE id = ?",
        )
        .bind(password_hash)
        .bind(now.as_millis())
        .bind(user_id.to_string())
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if changed == 0 {
            return Err(IdentityError::InvalidCredential);
        }
        sqlx::query(
            "UPDATE sessions SET revoked_at = ? WHERE user_id = ? AND id != ? AND revoked_at IS NULL",
        )
        .bind(now.as_millis())
        .bind(user_id.to_string())
        .bind(keep_session_id.to_string())
        .execute(&mut *transaction)
        .await?;
        audit::record_global(
            &mut transaction,
            Some(user_id),
            "account.password_changed",
            AuditOutcome::Success,
            "user",
            Some(user_id),
            request_id,
            serde_json::json!({"other_sessions_revoked": true}),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn store_recovery_token(
        &self,
        user_id: &str,
        token: &str,
        expires_at: TimestampMillis,
    ) -> Result<(), IdentityError> {
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        sqlx::query("DELETE FROM recovery_tokens WHERE expires_at <= ? OR user_id = ?")
            .bind(now.as_millis())
            .bind(user_id)
            .execute(&mut *transaction)
            .await?;
        sqlx::query(
            "INSERT INTO recovery_tokens (token_hash, user_id, expires_at, created_at) \
             VALUES (?, ?, ?, ?)",
        )
        .bind(token_hash(token).to_vec())
        .bind(user_id)
        .bind(expires_at.as_millis())
        .bind(now.as_millis())
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn store_recovery_token_audited(
        &self,
        user_id: Id,
        token: &str,
        expires_at: TimestampMillis,
        request_id: &str,
    ) -> Result<(), IdentityError> {
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        sqlx::query("DELETE FROM recovery_tokens WHERE expires_at <= ? OR user_id = ?")
            .bind(now.as_millis())
            .bind(user_id.to_string())
            .execute(&mut *transaction)
            .await?;
        sqlx::query(
            "INSERT INTO recovery_tokens (token_hash, user_id, expires_at, created_at) \
             VALUES (?, ?, ?, ?)",
        )
        .bind(token_hash(token).to_vec())
        .bind(user_id.to_string())
        .bind(expires_at.as_millis())
        .bind(now.as_millis())
        .execute(&mut *transaction)
        .await?;
        audit::record_global(
            &mut transaction,
            Some(user_id),
            "recovery.requested",
            AuditOutcome::Success,
            "user",
            Some(user_id),
            request_id,
            serde_json::json!({}),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn recovery_token_valid(
        &self,
        token: &str,
        now: TimestampMillis,
    ) -> Result<bool, IdentityError> {
        let supplied = token_hash(token);
        let row =
            sqlx::query("SELECT token_hash, expires_at FROM recovery_tokens WHERE token_hash = ?")
                .bind(supplied.to_vec())
                .fetch_optional(self.database.pool())
                .await?;
        Ok(row.is_some_and(|row| {
            row.get::<i64, _>("expires_at") > now.as_millis()
                && constant_time_eq(&row.get::<Vec<u8>, _>("token_hash"), &supplied)
        }))
    }

    pub async fn complete_recovery(
        &self,
        token: &str,
        password_hash: &str,
        now: TimestampMillis,
    ) -> Result<Id, IdentityError> {
        let supplied = token_hash(token);
        let mut transaction = self.database.immediate_transaction().await?;
        sqlx::query("DELETE FROM recovery_tokens WHERE expires_at <= ?")
            .bind(now.as_millis())
            .execute(&mut *transaction)
            .await?;
        let row =
            sqlx::query("SELECT token_hash, user_id FROM recovery_tokens WHERE token_hash = ?")
                .bind(supplied.to_vec())
                .fetch_optional(&mut *transaction)
                .await?
                .ok_or(IdentityError::InvalidCredential)?;
        let stored = row.get::<Vec<u8>, _>("token_hash");
        if !constant_time_eq(&stored, &supplied) {
            return Err(IdentityError::InvalidCredential);
        }
        let user_id = row
            .get::<String, _>("user_id")
            .parse::<Id>()
            .map_err(|_| IdentityError::InvalidIdentifier)?;
        sqlx::query(
            "UPDATE users SET password_hash = ?, updated_at = ?, version = version + 1 WHERE id = ?",
        )
        .bind(password_hash)
        .bind(now.as_millis())
        .bind(user_id.to_string())
        .execute(&mut *transaction)
        .await?;
        sqlx::query("UPDATE sessions SET revoked_at = ? WHERE user_id = ? AND revoked_at IS NULL")
            .bind(now.as_millis())
            .bind(user_id.to_string())
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DELETE FROM recovery_tokens WHERE token_hash = ?")
            .bind(stored)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(user_id)
    }

    pub async fn complete_recovery_audited(
        &self,
        token: &str,
        password_hash: &str,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<Id, IdentityError> {
        let supplied = token_hash(token);
        let mut transaction = self.database.immediate_transaction().await?;
        sqlx::query("DELETE FROM recovery_tokens WHERE expires_at <= ?")
            .bind(now.as_millis())
            .execute(&mut *transaction)
            .await?;
        let row =
            sqlx::query("SELECT token_hash, user_id FROM recovery_tokens WHERE token_hash = ?")
                .bind(supplied.to_vec())
                .fetch_optional(&mut *transaction)
                .await?;
        let Some(row) = row else {
            audit::record_global(
                &mut transaction,
                None,
                "recovery.completed",
                AuditOutcome::Failure,
                "user",
                None,
                request_id,
                serde_json::json!({"reason":"invalid_token"}),
                now,
            )
            .await?;
            transaction.commit().await?;
            return Err(IdentityError::InvalidCredential);
        };
        let stored = row.get::<Vec<u8>, _>("token_hash");
        if !constant_time_eq(&stored, &supplied) {
            return Err(IdentityError::InvalidCredential);
        }
        let user_id = row
            .get::<String, _>("user_id")
            .parse::<Id>()
            .map_err(|_| IdentityError::InvalidIdentifier)?;
        sqlx::query("UPDATE users SET password_hash = ?, updated_at = ?, version = version + 1 WHERE id = ?")
            .bind(password_hash)
            .bind(now.as_millis())
            .bind(user_id.to_string())
            .execute(&mut *transaction)
            .await?;
        sqlx::query("UPDATE sessions SET revoked_at = ? WHERE user_id = ? AND revoked_at IS NULL")
            .bind(now.as_millis())
            .bind(user_id.to_string())
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DELETE FROM recovery_tokens WHERE token_hash = ?")
            .bind(stored)
            .execute(&mut *transaction)
            .await?;
        audit::record_global(
            &mut transaction,
            Some(user_id),
            "recovery.completed",
            AuditOutcome::Success,
            "user",
            Some(user_id),
            request_id,
            serde_json::json!({"sessions_revoked":true}),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(user_id)
    }

    pub async fn create_session(
        &self,
        user: &AuthenticatedUser,
        now: TimestampMillis,
    ) -> Result<IssuedSession, IdentityError> {
        const DAY: i64 = 24 * 60 * 60 * 1_000;
        let id = Id::new_v7();
        let token = generate_opaque_token();
        let idle_expires_at = TimestampMillis::from_millis(now.as_millis() + 30 * DAY);
        let absolute_expires_at = TimestampMillis::from_millis(now.as_millis() + 90 * DAY);
        sqlx::query(
            "INSERT INTO sessions (id, token_hash, user_id, created_at, last_activity_at, \
             idle_expires_at, absolute_expires_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id.to_string())
        .bind(token_hash(&token).to_vec())
        .bind(user.id.to_string())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .bind(idle_expires_at.as_millis())
        .bind(absolute_expires_at.as_millis())
        .execute(self.database.pool())
        .await?;
        Ok(IssuedSession {
            id,
            token,
            idle_expires_at,
            absolute_expires_at,
        })
    }

    pub async fn create_session_audited(
        &self,
        user: &AuthenticatedUser,
        observed_password_hash: &str,
        replacement_hash: Option<&str>,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<IssuedSession, IdentityError> {
        let id = Id::new_v7();
        let token = generate_opaque_token();
        let mut transaction = self.database.immediate_transaction().await?;
        let credential_unchanged = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM users WHERE id = ? AND password_hash = ? \
             AND suspended_at IS NULL",
        )
        .bind(user.id.to_string())
        .bind(observed_password_hash)
        .fetch_one(&mut *transaction)
        .await?;
        if credential_unchanged != 1 {
            return Err(IdentityError::InvalidCredential);
        }
        if let Some(replacement_hash) = replacement_hash {
            sqlx::query(
                "UPDATE users SET password_hash = ?, updated_at = ?, version = version + 1 WHERE id = ?",
            )
            .bind(replacement_hash)
            .bind(now.as_millis())
            .bind(user.id.to_string())
            .execute(&mut *transaction)
            .await?;
            audit::record_global(
                &mut transaction,
                Some(user.id),
                "password.rehashed",
                AuditOutcome::Success,
                "user",
                Some(user.id),
                request_id,
                serde_json::json!({}),
                now,
            )
            .await?;
        }
        let session = insert_session(&mut transaction, id, &token, user.id, now).await?;
        audit::record_global(
            &mut transaction,
            Some(user.id),
            "authentication.login",
            AuditOutcome::Success,
            "session",
            Some(id),
            request_id,
            serde_json::json!({}),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(session)
    }

    pub async fn authenticate_session(
        &self,
        token: &str,
        now: TimestampMillis,
    ) -> Result<AuthenticatedSession, IdentityError> {
        const ACTIVITY_WRITE_INTERVAL: i64 = 5 * 60 * 1_000;
        const IDLE_LIFETIME: i64 = 30 * 24 * 60 * 60 * 1_000;
        let supplied = token_hash(token);
        let row = sqlx::query(
            "SELECT sessions.id AS session_id, sessions.token_hash, sessions.created_at, \
             sessions.last_activity_at, sessions.idle_expires_at, sessions.absolute_expires_at, \
             users.id AS user_id, users.email, users.display_name, users.suspended_at \
             FROM sessions JOIN users ON users.id = sessions.user_id \
             WHERE sessions.token_hash = ? AND sessions.revoked_at IS NULL",
        )
        .bind(supplied.to_vec())
        .fetch_optional(self.database.pool())
        .await?
        .ok_or(IdentityError::InvalidCredential)?;
        if !constant_time_eq(&row.get::<Vec<u8>, _>("token_hash"), &supplied) {
            return Err(IdentityError::InvalidCredential);
        }
        let session_id = row
            .get::<String, _>("session_id")
            .parse::<Id>()
            .map_err(|_| IdentityError::InvalidIdentifier)?;
        let user_id = row
            .get::<String, _>("user_id")
            .parse::<Id>()
            .map_err(|_| IdentityError::InvalidIdentifier)?;
        let idle_expires_at = row.get::<i64, _>("idle_expires_at");
        let absolute_expires_at = row.get::<i64, _>("absolute_expires_at");
        let email = row.get::<String, _>("email");
        let display_name = row.get::<String, _>("display_name");
        if now.as_millis() >= idle_expires_at
            || now.as_millis() >= absolute_expires_at
            || row.get::<Option<i64>, _>("suspended_at").is_some()
        {
            sqlx::query("DELETE FROM sessions WHERE id = ?")
                .bind(session_id.to_string())
                .execute(self.database.pool())
                .await?;
            return Err(IdentityError::InvalidCredential);
        }
        let last_activity_at = row.get::<i64, _>("last_activity_at");
        drop(row);
        if now.as_millis() - last_activity_at >= ACTIVITY_WRITE_INTERVAL {
            let idle_expires_at = (now.as_millis() + IDLE_LIFETIME).min(absolute_expires_at);
            sqlx::query(
                "UPDATE sessions SET last_activity_at = ?, idle_expires_at = ? \
                 WHERE id = ? AND last_activity_at = ? AND revoked_at IS NULL",
            )
            .bind(now.as_millis())
            .bind(idle_expires_at)
            .bind(session_id.to_string())
            .bind(last_activity_at)
            .execute(self.database.pool())
            .await?;
        }
        Ok(AuthenticatedSession {
            id: session_id,
            user: AuthenticatedUser {
                id: user_id,
                email,
                display_name,
            },
            absolute_expires_at: TimestampMillis::from_millis(absolute_expires_at),
        })
    }

    pub async fn list_sessions(
        &self,
        user_id: Id,
        now: TimestampMillis,
    ) -> Result<Vec<SessionRecord>, IdentityError> {
        let rows = sqlx::query(
            "SELECT sessions.id, sessions.created_at, sessions.last_activity_at, \
             sessions.idle_expires_at, sessions.absolute_expires_at, users.email, users.display_name \
             FROM sessions JOIN users ON users.id = sessions.user_id \
             WHERE sessions.user_id = ? AND sessions.revoked_at IS NULL \
             AND sessions.idle_expires_at > ? AND sessions.absolute_expires_at > ? \
             ORDER BY sessions.created_at DESC",
        )
        .bind(user_id.to_string())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .fetch_all(self.database.pool())
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(SessionRecord {
                    id: row
                        .get::<String, _>("id")
                        .parse()
                        .map_err(|_| IdentityError::InvalidIdentifier)?,
                    user: AuthenticatedUser {
                        id: user_id,
                        email: row.get("email"),
                        display_name: row.get("display_name"),
                    },
                    created_at: TimestampMillis::from_millis(row.get("created_at")),
                    last_activity_at: TimestampMillis::from_millis(row.get("last_activity_at")),
                    idle_expires_at: TimestampMillis::from_millis(row.get("idle_expires_at")),
                    absolute_expires_at: TimestampMillis::from_millis(
                        row.get("absolute_expires_at"),
                    ),
                    current: false,
                })
            })
            .collect()
    }

    pub async fn revoke_session(
        &self,
        session_id: Id,
        user_id: Id,
        now: TimestampMillis,
    ) -> Result<bool, IdentityError> {
        let affected = sqlx::query(
            "UPDATE sessions SET revoked_at = ? WHERE id = ? AND user_id = ? AND revoked_at IS NULL",
        )
        .bind(now.as_millis())
        .bind(session_id.to_string())
        .bind(user_id.to_string())
        .execute(self.database.pool())
        .await?
        .rows_affected();
        Ok(affected == 1)
    }

    pub async fn revoke_session_audited(
        &self,
        session_id: Id,
        user_id: Id,
        action: &str,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<bool, IdentityError> {
        let mut transaction = self.database.immediate_transaction().await?;
        let affected = sqlx::query(
            "UPDATE sessions SET revoked_at = ? WHERE id = ? AND user_id = ? AND revoked_at IS NULL",
        )
        .bind(now.as_millis())
        .bind(session_id.to_string())
        .bind(user_id.to_string())
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        audit::record_global(
            &mut transaction,
            Some(user_id),
            action,
            if affected == 1 {
                AuditOutcome::Success
            } else {
                AuditOutcome::Failure
            },
            "session",
            Some(session_id),
            request_id,
            if affected == 1 {
                serde_json::json!({})
            } else {
                serde_json::json!({"reason":"not_found"})
            },
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(affected == 1)
    }
}

fn issue_setup_token(now: TimestampMillis) -> IssuedToken {
    IssuedToken {
        token: generate_opaque_token(),
        expires_at: TimestampMillis::from_millis(
            now.as_millis() + IdentityRepository::SETUP_TOKEN_LIFETIME_MILLIS,
        ),
    }
}

async fn insert_setup_token(
    transaction: &mut Transaction<'_, Sqlite>,
    issued: &IssuedToken,
    now: TimestampMillis,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO setup_tokens (token_hash, expires_at, created_at) VALUES (?, ?, ?)")
        .bind(token_hash(&issued.token).to_vec())
        .bind(issued.expires_at.as_millis())
        .bind(now.as_millis())
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

async fn insert_session(
    transaction: &mut Transaction<'_, Sqlite>,
    id: Id,
    token: &str,
    user_id: Id,
    now: TimestampMillis,
) -> Result<IssuedSession, sqlx::Error> {
    const DAY: i64 = 24 * 60 * 60 * 1_000;
    let idle_expires_at = TimestampMillis::from_millis(now.as_millis() + 30 * DAY);
    let absolute_expires_at = TimestampMillis::from_millis(now.as_millis() + 90 * DAY);
    sqlx::query(
        "INSERT INTO sessions (id, token_hash, user_id, created_at, last_activity_at, \
         idle_expires_at, absolute_expires_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id.to_string())
    .bind(token_hash(token).to_vec())
    .bind(user_id.to_string())
    .bind(now.as_millis())
    .bind(now.as_millis())
    .bind(idle_expires_at.as_millis())
    .bind(absolute_expires_at.as_millis())
    .execute(&mut **transaction)
    .await?;
    Ok(IssuedSession {
        id,
        token: token.to_owned(),
        idle_expires_at,
        absolute_expires_at,
    })
}

fn decode_identity(row: sqlx::sqlite::SqliteRow) -> Result<StoredIdentity, IdentityError> {
    let id = row
        .get::<String, _>("id")
        .parse()
        .map_err(|_| IdentityError::InvalidIdentifier)?;
    Ok(StoredIdentity {
        id,
        email: row.get("email"),
        normalized_email: row.get("normalized_email"),
        display_name: row.get("display_name"),
        password_hash: row.get("password_hash"),
        suspended: row.get::<Option<i64>, _>("suspended_at").is_some(),
    })
}

fn token_hash(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

fn constant_time_eq(stored: &[u8], supplied: &[u8; 32]) -> bool {
    if stored.len() != supplied.len() {
        return false;
    }
    let mut difference = 0_u8;
    for (left, right) in stored.iter().zip(supplied) {
        difference |= left ^ right;
    }
    difference == 0
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use orbit_platform::{PasswordService, TestDatabase, TimestampMillis};

    use super::{IdentityError, IdentityRepository, SetupError, SetupRequest};

    #[tokio::test]
    async fn login_rejects_a_credential_observation_changed_by_recovery() {
        let database = TestDatabase::new().await.unwrap();
        let repository = IdentityRepository::new((*database).clone());
        repository
            .store_setup_token("operator-secret", TimestampMillis::from_millis(60_000))
            .await
            .unwrap();
        let setup = repository
            .complete_setup(
                setup_request("operator-secret"),
                TimestampMillis::from_millis(1_000),
            )
            .await
            .unwrap();
        let observed = repository
            .find_by_email("owner@example.com")
            .await
            .unwrap()
            .unwrap();
        repository
            .store_recovery_token(
                &setup.user_id.to_string(),
                "recovery-secret",
                TimestampMillis::from_millis(60_000),
            )
            .await
            .unwrap();
        let recovered_hash = PasswordService::default()
            .hash("a different secure password")
            .unwrap();
        repository
            .complete_recovery(
                "recovery-secret",
                &recovered_hash,
                TimestampMillis::from_millis(2_000),
            )
            .await
            .unwrap();

        let result = repository
            .create_session_audited(
                &orbit_platform::AuthenticatedUser {
                    id: observed.id,
                    email: observed.email,
                    display_name: observed.display_name,
                },
                &observed.password_hash,
                Some("rehash-of-old-password"),
                "login-request",
                TimestampMillis::from_millis(3_000),
            )
            .await;

        assert!(matches!(result, Err(IdentityError::InvalidCredential)));
        assert_eq!(
            database
                .scalar::<String>("SELECT password_hash FROM users")
                .await
                .unwrap(),
            recovered_hash
        );
        assert_eq!(
            database
                .scalar::<i64>("SELECT COUNT(*) FROM sessions WHERE revoked_at IS NULL")
                .await
                .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn login_rechecks_suspension_when_creating_the_session() {
        let database = TestDatabase::new().await.unwrap();
        let repository = IdentityRepository::new((*database).clone());
        repository
            .store_setup_token("operator-secret", TimestampMillis::from_millis(60_000))
            .await
            .unwrap();
        let setup = repository
            .complete_setup(
                setup_request("operator-secret"),
                TimestampMillis::from_millis(1_000),
            )
            .await
            .unwrap();
        let observed = repository
            .find_by_email("owner@example.com")
            .await
            .unwrap()
            .unwrap();
        repository
            .set_suspended(
                setup.user_id,
                setup.user_id,
                true,
                "suspend-request",
                TimestampMillis::from_millis(2_000),
            )
            .await
            .unwrap();

        let result = repository
            .create_session_audited(
                &orbit_platform::AuthenticatedUser {
                    id: observed.id,
                    email: observed.email,
                    display_name: observed.display_name,
                },
                &observed.password_hash,
                None,
                "login-request",
                TimestampMillis::from_millis(3_000),
            )
            .await;

        assert!(matches!(result, Err(IdentityError::InvalidCredential)));
        assert_eq!(
            database
                .scalar::<i64>("SELECT COUNT(*) FROM sessions WHERE revoked_at IS NULL")
                .await
                .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn auth_setup_and_initial_session_roll_back_together() {
        let database = TestDatabase::new().await.unwrap();
        let repository = IdentityRepository::new((*database).clone());
        repository
            .store_setup_token("operator-secret", TimestampMillis::from_millis(60_000))
            .await
            .unwrap();
        database
            .execute(
                "CREATE TRIGGER reject_initial_session BEFORE INSERT ON sessions \
                 BEGIN SELECT RAISE(ABORT, 'session unavailable'); END",
            )
            .await
            .unwrap();

        let result = repository
            .complete_setup(
                setup_request("operator-secret"),
                TimestampMillis::from_millis(1_000),
            )
            .await;

        assert!(result.is_err());
        assert!(!repository.setup_complete().await.unwrap());
        assert_eq!(
            database
                .scalar::<i64>("SELECT COUNT(*) FROM users")
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            database
                .scalar::<i64>("SELECT COUNT(*) FROM setup_tokens")
                .await
                .unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn audited_setup_rolls_back_when_its_success_event_cannot_commit() {
        let database = TestDatabase::new().await.unwrap();
        let repository = IdentityRepository::new((*database).clone());
        repository
            .store_setup_token("operator-secret", TimestampMillis::from_millis(60_000))
            .await
            .unwrap();
        database.execute("CREATE TRIGGER reject_setup_audit BEFORE INSERT ON audit_events WHEN NEW.action = 'setup.complete' BEGIN SELECT RAISE(ABORT, 'audit unavailable'); END").await.unwrap();

        assert!(
            repository
                .complete_setup_audited(
                    setup_request("operator-secret"),
                    "setup-request",
                    TimestampMillis::from_millis(1_000)
                )
                .await
                .is_err()
        );
        assert_eq!(
            database
                .scalar::<i64>("SELECT COUNT(*) FROM users")
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            database
                .scalar::<i64>("SELECT COUNT(*) FROM setup_tokens")
                .await
                .unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn auth_recovery_password_change_token_use_and_session_revocation_are_atomic() {
        let database = TestDatabase::new().await.unwrap();
        let repository = IdentityRepository::new((*database).clone());
        repository
            .store_setup_token("operator-secret", TimestampMillis::from_millis(60_000))
            .await
            .unwrap();
        let setup = repository
            .complete_setup(
                setup_request("operator-secret"),
                TimestampMillis::from_millis(1_000),
            )
            .await
            .unwrap();
        repository
            .store_recovery_token(
                &setup.user_id.to_string(),
                "recovery-secret",
                TimestampMillis::from_millis(60_000),
            )
            .await
            .unwrap();
        database
            .execute(
                "CREATE TRIGGER reject_revocation BEFORE UPDATE OF revoked_at ON sessions \
                 BEGIN SELECT RAISE(ABORT, 'revocation unavailable'); END",
            )
            .await
            .unwrap();
        let old_hash = database
            .scalar::<String>("SELECT password_hash FROM users")
            .await
            .unwrap();
        let new_hash = PasswordService::default()
            .hash("a different secure password")
            .unwrap();

        assert!(
            repository
                .complete_recovery(
                    "recovery-secret",
                    &new_hash,
                    TimestampMillis::from_millis(2_000)
                )
                .await
                .is_err()
        );
        assert_eq!(
            database
                .scalar::<String>("SELECT password_hash FROM users")
                .await
                .unwrap(),
            old_hash
        );
        assert_eq!(
            database
                .scalar::<i64>("SELECT COUNT(*) FROM recovery_tokens")
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            database
                .scalar::<i64>("SELECT COUNT(*) FROM sessions WHERE revoked_at IS NULL")
                .await
                .unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn auth_concurrent_session_activity_performs_one_conditional_touch() {
        let database = TestDatabase::new().await.unwrap();
        let repository = IdentityRepository::new((*database).clone());
        repository
            .store_setup_token("operator-secret", TimestampMillis::from_millis(60_000))
            .await
            .unwrap();
        let setup = repository
            .complete_setup(
                setup_request("operator-secret"),
                TimestampMillis::from_millis(1_000),
            )
            .await
            .unwrap();
        database
            .execute(
                "CREATE TABLE session_touch_log (value INTEGER NOT NULL); \
            CREATE TRIGGER count_session_touch AFTER UPDATE OF last_activity_at ON sessions \
            BEGIN INSERT INTO session_touch_log VALUES (1); END;",
            )
            .await
            .unwrap();
        let first = repository
            .authenticate_session(&setup.session.token, TimestampMillis::from_millis(301_000));
        let second = repository
            .authenticate_session(&setup.session.token, TimestampMillis::from_millis(301_000));
        let (first, second) = tokio::join!(first, second);
        first.unwrap();
        second.unwrap();
        assert_eq!(
            database
                .scalar::<i64>("SELECT COUNT(*) FROM session_touch_log")
                .await
                .unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn auth_setup_token_initialization_is_one_time_and_rotation_is_explicit() {
        let database = TestDatabase::new().await.unwrap();
        let repository = IdentityRepository::new((*database).clone());
        let now = TimestampMillis::from_millis(1_000);
        let first = repository
            .initialize_setup_token(now)
            .await
            .unwrap()
            .unwrap();
        assert!(
            repository
                .initialize_setup_token(now)
                .await
                .unwrap()
                .is_none()
        );
        let rotated = repository.rotate_setup_token(now).await.unwrap();
        assert_ne!(first.token, rotated.token);
        repository
            .complete_setup(setup_request(&rotated.token), now)
            .await
            .unwrap();
        assert!(
            repository
                .initialize_setup_token(now)
                .await
                .unwrap()
                .is_none()
        );
        assert!(repository.rotate_setup_token(now).await.is_err());
    }

    #[tokio::test]
    async fn auth_concurrent_setup_requests_have_exactly_one_winner() {
        let database = TestDatabase::new().await.unwrap();
        let repository = Arc::new(IdentityRepository::new((*database).clone()));
        let now = TimestampMillis::from_millis(1_000);
        repository
            .store_setup_token("operator-secret", TimestampMillis::from_millis(60_000))
            .await
            .unwrap();
        let request = SetupRequest {
            token: "operator-secret".to_owned(),
            email: "Owner@Example.com".to_owned(),
            display_name: "Owner".to_owned(),
            password_hash: PasswordService::default()
                .hash("correct horse battery")
                .unwrap(),
            workspace_name: "Orbit".to_owned(),
            project_name: "General".to_owned(),
        };

        let first_repository = Arc::clone(&repository);
        let first_request = request.clone();
        let second_repository = Arc::clone(&repository);
        let (first, second) = tokio::join!(
            async move { first_repository.complete_setup(first_request, now).await },
            async move { second_repository.complete_setup(request, now).await },
        );

        let results = [first, second];
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, Err(SetupError::AlreadyComplete)))
                .count(),
            1
        );
        assert!(repository.setup_complete().await.unwrap());
    }

    #[tokio::test]
    async fn auth_setup_preserves_display_email_and_stores_normalized_email_separately() {
        let database = TestDatabase::new().await.unwrap();
        let repository = IdentityRepository::new((*database).clone());
        let now = TimestampMillis::from_millis(1_000);
        repository
            .store_setup_token("operator-secret", TimestampMillis::from_millis(60_000))
            .await
            .unwrap();
        repository
            .complete_setup(
                SetupRequest {
                    token: "operator-secret".to_owned(),
                    email: " Owner@Example.COM ".to_owned(),
                    display_name: "Owner".to_owned(),
                    password_hash: PasswordService::default()
                        .hash("correct horse battery")
                        .unwrap(),
                    workspace_name: "Orbit".to_owned(),
                    project_name: "General".to_owned(),
                },
                now,
            )
            .await
            .unwrap();

        let identity = repository
            .find_by_email("owner@example.com")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(identity.email, "Owner@Example.COM");
        assert_eq!(identity.normalized_email, "owner@example.com");
        assert_eq!(
            database
                .scalar::<i64>("SELECT COUNT(*) FROM workspaces")
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            database
                .scalar::<i64>("SELECT COUNT(*) FROM projects")
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            database
                .scalar::<i64>("SELECT COUNT(*) FROM task_statuses")
                .await
                .unwrap(),
            6
        );
    }

    #[tokio::test]
    async fn auth_setup_and_recovery_tokens_are_persisted_only_as_hashes() {
        let database = TestDatabase::new().await.unwrap();
        let repository = IdentityRepository::new((*database).clone());

        repository
            .store_setup_token(
                "plaintext-setup-token",
                TimestampMillis::from_millis(10_000),
            )
            .await
            .unwrap();
        let setup = database
            .scalar::<Vec<u8>>("SELECT token_hash FROM setup_tokens")
            .await
            .unwrap();
        let setup_result = repository
            .complete_setup(
                SetupRequest {
                    token: "plaintext-setup-token".to_owned(),
                    email: "owner@example.com".to_owned(),
                    display_name: "Owner".to_owned(),
                    password_hash: PasswordService::default()
                        .hash("correct horse battery")
                        .unwrap(),
                    workspace_name: "Orbit".to_owned(),
                    project_name: "General".to_owned(),
                },
                TimestampMillis::from_millis(1_000),
            )
            .await
            .unwrap();
        repository
            .store_recovery_token(
                &setup_result.user_id.to_string(),
                "plaintext-recovery-token",
                TimestampMillis::from_millis(10_000),
            )
            .await
            .unwrap();

        let recovery = database
            .scalar::<Vec<u8>>("SELECT token_hash FROM recovery_tokens")
            .await
            .unwrap();
        assert_ne!(setup, b"plaintext-setup-token");
        assert_ne!(recovery, b"plaintext-recovery-token");
        assert_eq!(setup.len(), 32);
        assert_eq!(recovery.len(), 32);
    }

    #[tokio::test]
    async fn auth_recovery_tokens_are_bounded_to_one_live_record_per_user() {
        let database = TestDatabase::new().await.unwrap();
        let repository = IdentityRepository::new((*database).clone());
        repository
            .store_setup_token("operator-secret", TimestampMillis::from_millis(i64::MAX))
            .await
            .unwrap();
        let setup = repository
            .complete_setup(
                setup_request("operator-secret"),
                TimestampMillis::from_millis(1_000),
            )
            .await
            .unwrap();
        for token in ["first-recovery-token", "replacement-recovery-token"] {
            repository
                .store_recovery_token(
                    &setup.user_id.to_string(),
                    token,
                    TimestampMillis::from_millis(i64::MAX),
                )
                .await
                .unwrap();
        }

        assert_eq!(
            database
                .scalar::<i64>("SELECT COUNT(*) FROM recovery_tokens")
                .await
                .unwrap(),
            1
        );
        assert!(
            !repository
                .recovery_token_valid("first-recovery-token", TimestampMillis::from_millis(2_000))
                .await
                .unwrap()
        );
        assert!(
            repository
                .recovery_token_valid(
                    "replacement-recovery-token",
                    TimestampMillis::from_millis(2_000)
                )
                .await
                .unwrap()
        );
    }

    fn setup_request(token: &str) -> SetupRequest {
        SetupRequest {
            token: token.to_owned(),
            email: "owner@example.com".to_owned(),
            display_name: "Owner".to_owned(),
            password_hash: PasswordService::default()
                .hash("correct horse battery")
                .unwrap(),
            workspace_name: "Orbit".to_owned(),
            project_name: "General".to_owned(),
        }
    }
}
