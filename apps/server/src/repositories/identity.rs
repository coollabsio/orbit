use orbit_domain::{StatusCategory, WorkspaceDefaults};
use orbit_platform::{
    AuthenticatedUser, Database, Id, IssuedSession, SessionRecord, TimestampMillis,
    generate_opaque_token, normalize_email,
};
use sha2::{Digest, Sha256};
use sqlx::Row;
use thiserror::Error;

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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthenticatedSession {
    pub id: Id,
    pub user: AuthenticatedUser,
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

#[derive(Clone, Debug)]
pub struct IdentityRepository {
    database: Database,
}

impl IdentityRepository {
    #[must_use]
    pub fn new(database: Database) -> Self {
        Self { database }
    }

    pub async fn setup_complete(&self) -> Result<bool, IdentityError> {
        let initialized = sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE((SELECT initialized FROM installation_state WHERE id = 1), 0)",
        )
        .fetch_one(self.database.pool())
        .await?;
        Ok(initialized != 0)
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
            return Err(SetupError::InvalidToken);
        }

        let email = request.email.trim().to_owned();
        let normalized_email = normalize_email(&email);
        let user_id = Id::new_v7();
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
            "INSERT INTO workspaces (id, name, version, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(defaults.workspace.id.to_string())
        .bind(&defaults.workspace.name)
        .bind(defaults.workspace.version as i64)
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
            .bind(status_category(status.category))
            .bind(status.position)
            .bind(status.version as i64)
            .bind(timestamp)
            .bind(timestamp)
            .execute(&mut *transaction)
            .await
            .map_err(SetupError::Unavailable)?;
        }
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
        transaction
            .commit()
            .await
            .map_err(SetupError::Unavailable)?;

        Ok(SetupResult {
            user_id,
            workspace_id: defaults.workspace.id,
            project_id: defaults.project.id,
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

    pub async fn store_recovery_token(
        &self,
        user_id: &str,
        token: &str,
        expires_at: TimestampMillis,
    ) -> Result<(), IdentityError> {
        let now = self.database.database_now().await?;
        sqlx::query(
            "INSERT INTO recovery_tokens (token_hash, user_id, expires_at, created_at) \
             VALUES (?, ?, ?, ?)",
        )
        .bind(token_hash(token).to_vec())
        .bind(user_id)
        .bind(expires_at.as_millis())
        .bind(now.as_millis())
        .execute(self.database.pool())
        .await?;
        Ok(())
    }

    pub async fn consume_recovery_token(
        &self,
        token: &str,
        now: TimestampMillis,
    ) -> Result<Id, IdentityError> {
        let supplied = token_hash(token);
        let mut transaction = self.database.immediate_transaction().await?;
        let rows = sqlx::query("SELECT token_hash, user_id, expires_at FROM recovery_tokens")
            .fetch_all(&mut *transaction)
            .await?;
        let matched = rows.into_iter().find_map(|row| {
            let stored = row.get::<Vec<u8>, _>("token_hash");
            (constant_time_eq(&stored, &supplied)
                && row.get::<i64, _>("expires_at") > now.as_millis())
            .then(|| (stored, row.get::<String, _>("user_id")))
        });
        let (stored_hash, user_id) = matched.ok_or(IdentityError::InvalidIdentifier)?;
        sqlx::query("DELETE FROM recovery_tokens WHERE token_hash = ?")
            .bind(stored_hash)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        user_id
            .parse()
            .map_err(|_| IdentityError::InvalidIdentifier)
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

    pub async fn authenticate_session(
        &self,
        token: &str,
        now: TimestampMillis,
    ) -> Result<AuthenticatedSession, IdentityError> {
        const ACTIVITY_WRITE_INTERVAL: i64 = 5 * 60 * 1_000;
        const IDLE_LIFETIME: i64 = 30 * 24 * 60 * 60 * 1_000;
        let supplied = token_hash(token);
        let rows = sqlx::query(
            "SELECT sessions.id AS session_id, sessions.token_hash, sessions.created_at, \
             sessions.last_activity_at, sessions.idle_expires_at, sessions.absolute_expires_at, \
             users.id AS user_id, users.email, users.display_name, users.suspended_at \
             FROM sessions JOIN users ON users.id = sessions.user_id \
             WHERE sessions.revoked_at IS NULL",
        )
        .fetch_all(self.database.pool())
        .await?;
        let row = rows
            .into_iter()
            .find(|row| constant_time_eq(&row.get::<Vec<u8>, _>("token_hash"), &supplied))
            .ok_or(IdentityError::InvalidCredential)?;
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
        if now.as_millis() - last_activity_at >= ACTIVITY_WRITE_INTERVAL {
            let idle_expires_at = (now.as_millis() + IDLE_LIFETIME).min(absolute_expires_at);
            sqlx::query(
                "UPDATE sessions SET last_activity_at = ?, idle_expires_at = ? WHERE id = ?",
            )
            .bind(now.as_millis())
            .bind(idle_expires_at)
            .bind(session_id.to_string())
            .execute(self.database.pool())
            .await?;
        }
        Ok(AuthenticatedSession {
            id: session_id,
            user: AuthenticatedUser {
                id: user_id,
                email: row.get("email"),
                display_name: row.get("display_name"),
            },
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

    pub async fn revoke_user_sessions(
        &self,
        user_id: Id,
        now: TimestampMillis,
    ) -> Result<(), IdentityError> {
        sqlx::query("UPDATE sessions SET revoked_at = ? WHERE user_id = ? AND revoked_at IS NULL")
            .bind(now.as_millis())
            .bind(user_id.to_string())
            .execute(self.database.pool())
            .await?;
        Ok(())
    }
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

fn status_category(category: StatusCategory) -> &'static str {
    match category {
        StatusCategory::Unstarted => "unstarted",
        StatusCategory::Started => "started",
        StatusCategory::Completed => "completed",
        StatusCategory::Cancelled => "cancelled",
    }
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

    use super::{IdentityRepository, SetupError, SetupRequest};

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
            5
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
}
