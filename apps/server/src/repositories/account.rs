//! The account owner's own lifecycle: deleting the account (GDPR erasure) and changing its email, with a link sent
//! to the new address or by an admin.

use orbit_platform::{Id, Row, TimestampMillis, normalize_email};
use serde::Serialize;
use sqlx::{Any, Transaction};
use thiserror::Error;
use utoipa::ToSchema;

use super::identity::{AdminAccountError, IdentityRepository, NO_PASSWORD, token_hash};
use crate::audit::{self, AuditOutcome};

/// The name a deleted account shows wherever it authored something.
pub const DELETED_USER_NAME: &str = "Deleted user";

/// A workspace outside the trash that the account owns, which blocks deleting the account.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
pub struct OwnedWorkspace {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Error)]
pub enum DeleteAccountError {
    #[error("the root account cannot be deleted")]
    RootAccount,
    #[error("the account owns workspaces")]
    OwnsWorkspaces(Vec<OwnedWorkspace>),
    #[error("the account does not exist or was deleted")]
    NotFound,
    #[error(transparent)]
    Unavailable(#[from] sqlx::Error),
}

#[derive(Debug, Error)]
pub enum EmailChangeError {
    #[error("the email change link is invalid, used or expired")]
    InvalidToken,
    #[error("another account has this email")]
    EmailTaken,
    #[error(transparent)]
    Unavailable(#[from] sqlx::Error),
}

/// A confirmed email change.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmailChanged {
    pub user_id: Id,
    pub old_email: String,
    pub new_email: String,
}

/// The address a deleted account gets: unique, and in the reserved `.invalid` domain so nothing can be delivered.
fn deleted_email(user_id: Id) -> String {
    format!("deleted-{user_id}@deleted.invalid")
}

impl IdentityRepository {
    /// Whether another account (deleted ones included, by their placeholder) has `email`.
    pub async fn email_taken_by_other(
        &self,
        user_id: Id,
        email: &str,
    ) -> Result<bool, sqlx::Error> {
        Ok(sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM users WHERE normalized_email = $1 AND id <> $2",
        )
        .bind(normalize_email(email))
        .bind(user_id.to_string())
        .fetch_one(self.database().pool())
        .await?
            != 0)
    }

    /// Erases the account in one serialized transaction. The row stays, so authored tasks, comments and messages
    /// keep their author, now "Deleted user". Everything that signs in or holds personal data goes: sessions, push
    /// subscriptions, second factors, passkeys, provider identities, OAuth grants, personal API tokens, pending links,
    /// the avatar (its blob is unreferenced and the attachment reconcile deletes it), and every membership. The owner
    /// membership of a trashed workspace stays until the purge removes that workspace with it.
    pub async fn delete_account(
        &self,
        user_id: Id,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), DeleteAccountError> {
        let result = self
            .delete_account_unchecked(user_id, request_id, now)
            .await;
        // Open co-editing sockets re-check their access right away.
        crate::collab::CollabHub::revalidate_database(self.database(), None);
        result
    }

    async fn delete_account_unchecked(
        &self,
        user_id: Id,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), DeleteAccountError> {
        let mut transaction = self.database().immediate_transaction().await?;
        let id = user_id.to_string();
        let live = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM users WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(&id)
        .fetch_one(&mut *transaction)
        .await?;
        if live == 0 {
            return Err(DeleteAccountError::NotFound);
        }
        if Self::is_root_account(&mut transaction, user_id).await? {
            return Err(DeleteAccountError::RootAccount);
        }
        let owned = owned_workspaces(&mut transaction, user_id).await?;
        if !owned.is_empty() {
            return Err(DeleteAccountError::OwnsWorkspaces(owned));
        }
        sqlx::query(
            "DELETE FROM memberships WHERE user_id = $1 AND id NOT IN \
             (SELECT owner_membership_id FROM workspaces WHERE owner_membership_id IS NOT NULL)",
        )
        .bind(&id)
        .execute(&mut *transaction)
        .await?;
        let email = deleted_email(user_id);
        sqlx::query(
            "UPDATE users SET display_name = $1, email = $2, normalized_email = $2, password_hash = $3, \
             title = NULL, pronouns = NULL, timezone = NULL, bio = NULL, phone = NULL, installation_admin = 0, \
             email_verified_at = NULL, suspended_at = COALESCE(suspended_at, $4), deleted_at = $4, \
             version = version + 1, updated_at = $4 WHERE id = $5",
        )
        .bind(DELETED_USER_NAME)
        .bind(&email)
        .bind(NO_PASSWORD)
        .bind(now.as_millis())
        .bind(&id)
        .execute(&mut *transaction)
        .await?;
        for statement in [
            "DELETE FROM push_subscriptions WHERE user_id = $1",
            "DELETE FROM sessions WHERE user_id = $1",
            "DELETE FROM recovery_tokens WHERE user_id = $1",
            "DELETE FROM email_change_tokens WHERE user_id = $1",
            "DELETE FROM user_totp WHERE user_id = $1",
            "DELETE FROM user_recovery_codes WHERE user_id = $1",
            "DELETE FROM user_passkeys WHERE user_id = $1",
            "DELETE FROM user_identities WHERE user_id = $1",
            "DELETE FROM oauth_requests WHERE user_id = $1",
            "DELETE FROM oauth_grants WHERE user_id = $1",
            "DELETE FROM user_avatars WHERE user_id = $1",
            "DELETE FROM user_status WHERE user_id = $1",
            "DELETE FROM user_shortcuts WHERE user_id = $1",
            "DELETE FROM notification_prefs WHERE user_id = $1",
            "DELETE FROM user_notes WHERE author_id = $1 OR subject_id = $1",
            "DELETE FROM view_preferences WHERE user_id = $1",
            "DELETE FROM page_favorites WHERE user_id = $1",
            "DELETE FROM page_visits WHERE user_id = $1",
            "DELETE FROM saved_view_favorites WHERE user_id = $1",
            "DELETE FROM notifications WHERE recipient_user_id = $1",
        ] {
            sqlx::query(statement)
                .bind(&id)
                .execute(&mut *transaction)
                .await?;
        }
        sqlx::query(
            "UPDATE api_tokens SET revoked_at = $1 WHERE created_by = $2 \
             AND service_account_id IS NULL AND revoked_at IS NULL",
        )
        .bind(now.as_millis())
        .bind(&id)
        .execute(&mut *transaction)
        .await?;
        audit::record_global(
            &mut transaction,
            Some(user_id),
            "account.deleted",
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

    /// Stores the link for changing the account's email to `email`, replacing an older request of the account.
    pub async fn store_email_change_token(
        &self,
        user_id: Id,
        email: &str,
        token: &str,
        expires_at: TimestampMillis,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), sqlx::Error> {
        let mut transaction = self.database().immediate_transaction().await?;
        sqlx::query("DELETE FROM email_change_tokens WHERE expires_at <= $1 OR user_id = $2")
            .bind(now.as_millis())
            .bind(user_id.to_string())
            .execute(&mut *transaction)
            .await?;
        sqlx::query(
            "INSERT INTO email_change_tokens (token_hash, user_id, email, normalized_email, expires_at, \
             created_at) VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(token_hash(token).to_vec())
        .bind(user_id.to_string())
        .bind(email.trim())
        .bind(normalize_email(email))
        .bind(expires_at.as_millis())
        .bind(now.as_millis())
        .execute(&mut *transaction)
        .await?;
        audit::record_global(
            &mut transaction,
            Some(user_id),
            "account.email_change_requested",
            AuditOutcome::Success,
            "user",
            Some(user_id),
            request_id,
            serde_json::json!({}),
            now,
        )
        .await?;
        transaction.commit().await
    }

    /// Uses up an email change link and applies the change (see `apply_email_change`); the confirming browser's own
    /// session of this account (`keep_session`), if any, stays signed in. Fails with `EmailTaken` (and uses the link
    /// up) when another account took the address in the meantime.
    pub async fn confirm_email_change(
        &self,
        token: &str,
        keep_session: Option<Id>,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<EmailChanged, EmailChangeError> {
        let mut transaction = self.database().immediate_transaction().await?;
        let request = sqlx::query(
            "SELECT email_change_tokens.user_id, email_change_tokens.email, users.email AS old_email \
             FROM email_change_tokens JOIN users ON users.id = email_change_tokens.user_id \
             WHERE email_change_tokens.token_hash = $1 AND email_change_tokens.expires_at > $2 \
             AND users.suspended_at IS NULL",
        )
        .bind(token_hash(token).to_vec())
        .bind(now.as_millis())
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(EmailChangeError::InvalidToken)?;
        let user_id: String = request.get("user_id");
        let change = EmailChanged {
            user_id: user_id
                .parse()
                .map_err(|error| sqlx::Error::Decode(Box::new(error)))?,
            old_email: request.get("old_email"),
            new_email: request.get("email"),
        };
        if !self
            .apply_email_change(
                &mut transaction,
                &change,
                keep_session,
                EmailChangedBy::Owner,
                request_id,
                now,
            )
            .await?
        {
            sqlx::query("DELETE FROM email_change_tokens WHERE user_id = $1")
                .bind(&user_id)
                .execute(&mut *transaction)
                .await?;
            transaction.commit().await?;
            return Err(EmailChangeError::EmailTaken);
        }
        transaction.commit().await?;
        Ok(change)
    }

    /// Changes an account's email from the Admin area, with no link: for instances without a mail server, or a
    /// person who lost the old address. Any admin may change their own email; for other accounts the rules of
    /// `may_manage` apply. Every session of the account is revoked except `keep_session` (the admin's own when they
    /// change their own email).
    pub async fn change_email_as_admin(
        &self,
        actor_id: Id,
        user_id: Id,
        email: &str,
        keep_session: Option<Id>,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<EmailChanged, AdminAccountError> {
        let mut transaction = self.database().immediate_transaction().await?;
        if !Self::is_active_admin(&mut transaction, actor_id).await? {
            return Err(AdminAccountError::Forbidden);
        }
        if actor_id != user_id {
            Self::may_manage(&mut transaction, actor_id, user_id).await?;
        }
        let old_email = sqlx::query_scalar::<_, String>("SELECT email FROM users WHERE id = $1")
            .bind(user_id.to_string())
            .fetch_optional(&mut *transaction)
            .await?
            .ok_or(AdminAccountError::NotFound)?;
        if normalize_email(email) == normalize_email(&old_email) {
            return Err(AdminAccountError::SameEmail);
        }
        let change = EmailChanged {
            user_id,
            old_email,
            new_email: email.trim().to_owned(),
        };
        if !self
            .apply_email_change(
                &mut transaction,
                &change,
                keep_session,
                EmailChangedBy::Admin(actor_id),
                request_id,
                now,
            )
            .await?
        {
            return Err(AdminAccountError::EmailTaken);
        }
        transaction.commit().await?;
        Ok(change)
    }

    /// Makes `change.new_email` the account's email, verified. Pending email change links and password recovery links
    /// (sent to the old address) are deleted, and every session of the account except `keep_session` is revoked. With
    /// billing on, queues the Stripe customer email of each workspace the account owns. Returns false, having written
    /// nothing, when another account has the address.
    async fn apply_email_change(
        &self,
        transaction: &mut Transaction<'_, Any>,
        change: &EmailChanged,
        keep_session: Option<Id>,
        by: EmailChangedBy,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<bool, sqlx::Error> {
        let user_id = change.user_id.to_string();
        let normalized = normalize_email(&change.new_email);
        let taken = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM users WHERE normalized_email = $1 AND id <> $2",
        )
        .bind(&normalized)
        .bind(&user_id)
        .fetch_one(&mut **transaction)
        .await?;
        if taken != 0 {
            return Ok(false);
        }
        sqlx::query(
            "UPDATE users SET email = $1, normalized_email = $2, email_verified_at = $3, \
             version = version + 1, updated_at = $3 WHERE id = $4",
        )
        .bind(&change.new_email)
        .bind(&normalized)
        .bind(now.as_millis())
        .bind(&user_id)
        .execute(&mut **transaction)
        .await?;
        match keep_session {
            Some(session_id) => sqlx::query(
                "UPDATE sessions SET revoked_at = $1 WHERE user_id = $2 AND id <> $3 AND revoked_at IS NULL",
            )
            .bind(now.as_millis())
            .bind(&user_id)
            .bind(session_id.to_string()),
            None => sqlx::query(
                "UPDATE sessions SET revoked_at = $1 WHERE user_id = $2 AND revoked_at IS NULL",
            )
            .bind(now.as_millis())
            .bind(&user_id),
        }
        .execute(&mut **transaction)
        .await?;
        for statement in [
            "DELETE FROM email_change_tokens WHERE user_id = $1",
            "DELETE FROM recovery_tokens WHERE user_id = $1",
        ] {
            sqlx::query(statement)
                .bind(&user_id)
                .execute(&mut **transaction)
                .await?;
        }
        if crate::repositories::billing::enabled(self.database()) {
            let customers = sqlx::query_scalar::<_, String>(
                "SELECT workspace_subscriptions.stripe_customer_id FROM workspace_subscriptions \
                 JOIN workspaces ON workspaces.id = workspace_subscriptions.workspace_id \
                 JOIN memberships ON memberships.id = workspaces.owner_membership_id \
                 WHERE memberships.user_id = $1 AND workspaces.deleted_at IS NULL \
                 ORDER BY workspace_subscriptions.stripe_customer_id",
            )
            .bind(&user_id)
            .fetch_all(&mut **transaction)
            .await?;
            for customer_id in customers {
                crate::billing::enqueue_customer_email_in(
                    transaction,
                    &customer_id,
                    change.user_id,
                    &change.new_email,
                    now,
                )
                .await
                .map_err(|error| sqlx::Error::Protocol(error.to_string()))?;
            }
        }
        let (actor_id, changed_by) = match by {
            EmailChangedBy::Owner => (change.user_id, "self"),
            EmailChangedBy::Admin(actor_id) => (actor_id, "admin"),
        };
        audit::record_global(
            transaction,
            Some(actor_id),
            "account.email_changed",
            AuditOutcome::Success,
            "user",
            Some(change.user_id),
            request_id,
            serde_json::json!({"changed_by": changed_by}),
            now,
        )
        .await?;
        Ok(true)
    }
}

/// Who changed an account's email, for the audit event.
#[derive(Clone, Copy)]
enum EmailChangedBy {
    /// The account owner, with the emailed link.
    Owner,
    /// An admin, from the Admin area.
    Admin(Id),
}

/// The workspaces outside the trash that `user_id` owns, by name.
async fn owned_workspaces(
    transaction: &mut Transaction<'_, Any>,
    user_id: Id,
) -> Result<Vec<OwnedWorkspace>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT workspaces.id, workspaces.name FROM workspaces \
         JOIN memberships ON memberships.id = workspaces.owner_membership_id \
         WHERE memberships.user_id = $1 AND workspaces.deleted_at IS NULL \
         ORDER BY workspaces.name, workspaces.id",
    )
    .bind(user_id.to_string())
    .fetch_all(&mut **transaction)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| OwnedWorkspace {
            id: row.get("id"),
            name: row.get("name"),
        })
        .collect())
}
