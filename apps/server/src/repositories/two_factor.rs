//! Two-factor sign-in (an authenticator app with recovery codes) and passkeys of an account.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use orbit_platform::{
    AuthenticatedUser, Id, IssuedSession, TimestampMillis, generate_opaque_token,
};
use serde::Serialize;
use sqlx::Row;
use utoipa::ToSchema;
use webauthn_rs::prelude::Passkey;

use super::identity::{
    AdminAccountError, IdentityError, IdentityRepository, insert_session, token_hash,
};
use crate::audit::{self, AuditOutcome};

/// How many recovery codes a user gets at a time.
pub const RECOVERY_CODE_COUNT: usize = 10;

/// What the Security page shows about the account's sign-in methods.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct TwoFactorStatus {
    /// Sign-in asks for an authenticator app code after the password.
    pub totp_enabled: bool,
    /// Unused recovery codes.
    pub recovery_codes_left: i64,
}

/// A passkey as the account's Security page lists it.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct PasskeyRecord {
    pub id: String,
    pub name: String,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
    /// The last sign-in with it, `null` before the first.
    #[schema(value_type = Option<String>, format = DateTime)]
    pub last_used_at: Option<TimestampMillis>,
}

/// The authenticator app secret of an account, encrypted, with the newest step a code was accepted for.
pub struct StoredTotp {
    pub secret: Vec<u8>,
    pub enabled: bool,
    pub last_used_step: i64,
}

/// A stored passkey with the user it belongs to.
pub struct StoredPasskey {
    pub id: Id,
    pub user_id: Id,
    pub passkey: Passkey,
}

/// Why a passkey could not be saved.
#[derive(Debug, thiserror::Error)]
pub enum PasskeySaveError {
    #[error("the credential is already registered")]
    Duplicate,
    #[error(transparent)]
    Unavailable(#[from] IdentityError),
}

impl From<sqlx::Error> for PasskeySaveError {
    fn from(error: sqlx::Error) -> Self {
        Self::Unavailable(error.into())
    }
}

/// The base64url form a credential ID is stored and looked up in.
pub fn credential_key(credential_id: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(credential_id)
}

/// A recovery code without the dash, spaces and case the user may type.
fn normalized_recovery_code(code: &str) -> String {
    code.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|character| character.to_ascii_lowercase())
        .collect()
}

/// Ten new codes like `k3x9p-7mq2d`: 50 random bits each.
fn new_recovery_codes() -> Vec<String> {
    // Crockford base32: 32 characters, so each byte modulo 32 is unbiased, and no i, l, o or u to misread.
    const ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";
    (0..RECOVERY_CODE_COUNT)
        .map(|_| {
            // 32 random bytes from the platform token generator; each picks a character.
            let bytes = URL_SAFE_NO_PAD
                .decode(generate_opaque_token())
                .expect("opaque tokens are base64url");
            let characters = bytes
                .iter()
                .take(10)
                .map(|byte| ALPHABET[usize::from(*byte) % ALPHABET.len()] as char)
                .collect::<String>();
            format!("{}-{}", &characters[..5], &characters[5..])
        })
        .collect()
}

impl IdentityRepository {
    pub async fn two_factor_status(&self, user_id: Id) -> Result<TwoFactorStatus, IdentityError> {
        let row = sqlx::query(
            "SELECT \
               EXISTS (SELECT 1 FROM user_totp WHERE user_id = ?1 AND enabled_at IS NOT NULL) AS totp_enabled, \
               (SELECT COUNT(*) FROM user_recovery_codes WHERE user_id = ?1) AS recovery_codes_left",
        )
        .bind(user_id.to_string())
        .fetch_one(self.database().pool())
        .await?;
        Ok(TwoFactorStatus {
            totp_enabled: row.get::<i64, _>("totp_enabled") == 1,
            recovery_codes_left: row.get("recovery_codes_left"),
        })
    }

    pub async fn totp(&self, user_id: Id) -> Result<Option<StoredTotp>, IdentityError> {
        let row = sqlx::query(
            "SELECT secret, enabled_at, last_used_step FROM user_totp WHERE user_id = ?",
        )
        .bind(user_id.to_string())
        .fetch_optional(self.database().pool())
        .await?;
        Ok(row.map(|row| StoredTotp {
            secret: row.get("secret"),
            enabled: row.get::<Option<i64>, _>("enabled_at").is_some(),
            last_used_step: row.get("last_used_step"),
        }))
    }

    /// Starts the authenticator app setup with a new (encrypted) secret. Refused while the app is on.
    pub async fn begin_totp_setup(
        &self,
        user_id: Id,
        encrypted_secret: &[u8],
        now: TimestampMillis,
    ) -> Result<bool, IdentityError> {
        let changed = sqlx::query(
            "INSERT INTO user_totp (user_id, secret, created_at) VALUES (?1, ?2, ?3) \
             ON CONFLICT (user_id) DO UPDATE SET secret = ?2, created_at = ?3, last_used_step = 0 \
             WHERE user_totp.enabled_at IS NULL",
        )
        .bind(user_id.to_string())
        .bind(encrypted_secret)
        .bind(now.as_millis())
        .execute(self.database().pool())
        .await?
        .rows_affected();
        Ok(changed == 1)
    }

    /// Turns the authenticator app on after the first good code (`step`) and replaces the recovery codes.
    /// Returns the new codes, or `None` when the setup was not started or the app is already on.
    pub async fn enable_totp(
        &self,
        user_id: Id,
        step: i64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<Option<Vec<String>>, IdentityError> {
        let mut transaction = self.database().immediate_transaction().await?;
        let changed = sqlx::query(
            "UPDATE user_totp SET enabled_at = ?, last_used_step = ? \
             WHERE user_id = ? AND enabled_at IS NULL",
        )
        .bind(now.as_millis())
        .bind(step)
        .bind(user_id.to_string())
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if changed != 1 {
            return Ok(None);
        }
        let codes = replace_recovery_codes(&mut transaction, user_id, now).await?;
        audit::record_global(
            &mut transaction,
            Some(user_id),
            "two_factor.enabled",
            AuditOutcome::Success,
            "user",
            Some(user_id),
            request_id,
            serde_json::json!({}),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(Some(codes))
    }

    /// Records that a code for `step` was used, so the same code cannot sign in again. False when a code for this
    /// step or a newer one was already accepted.
    pub async fn use_totp_step(&self, user_id: Id, step: i64) -> Result<bool, IdentityError> {
        let changed = sqlx::query(
            "UPDATE user_totp SET last_used_step = ? \
             WHERE user_id = ? AND enabled_at IS NOT NULL AND last_used_step < ?",
        )
        .bind(step)
        .bind(user_id.to_string())
        .bind(step)
        .execute(self.database().pool())
        .await?
        .rows_affected();
        Ok(changed == 1)
    }

    /// Deletes a recovery code if the account has it. True when it was there.
    pub async fn use_recovery_code(
        &self,
        user_id: Id,
        code: &str,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<bool, IdentityError> {
        let mut transaction = self.database().immediate_transaction().await?;
        let used =
            sqlx::query("DELETE FROM user_recovery_codes WHERE user_id = ? AND code_hash = ?")
                .bind(user_id.to_string())
                .bind(token_hash(&normalized_recovery_code(code)).to_vec())
                .execute(&mut *transaction)
                .await?
                .rows_affected()
                == 1;
        if used {
            audit::record_global(
                &mut transaction,
                Some(user_id),
                "two_factor.recovery_code_used",
                AuditOutcome::Success,
                "user",
                Some(user_id),
                request_id,
                serde_json::json!({}),
                now,
            )
            .await?;
        }
        transaction.commit().await?;
        Ok(used)
    }

    /// Replaces the recovery codes of an account that has the authenticator app on. `None` when it is off.
    pub async fn regenerate_recovery_codes(
        &self,
        user_id: Id,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<Option<Vec<String>>, IdentityError> {
        let mut transaction = self.database().immediate_transaction().await?;
        let enabled = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM user_totp WHERE user_id = ? AND enabled_at IS NOT NULL",
        )
        .bind(user_id.to_string())
        .fetch_one(&mut *transaction)
        .await?;
        if enabled == 0 {
            return Ok(None);
        }
        let codes = replace_recovery_codes(&mut transaction, user_id, now).await?;
        audit::record_global(
            &mut transaction,
            Some(user_id),
            "two_factor.recovery_codes_replaced",
            AuditOutcome::Success,
            "user",
            Some(user_id),
            request_id,
            serde_json::json!({}),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(Some(codes))
    }

    /// Turns the authenticator app off and deletes the recovery codes.
    pub async fn disable_totp(
        &self,
        user_id: Id,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), IdentityError> {
        let mut transaction = self.database().immediate_transaction().await?;
        delete_two_factor(&mut transaction, user_id).await?;
        audit::record_global(
            &mut transaction,
            Some(user_id),
            "two_factor.disabled",
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

    /// Turns another account's authenticator app off from the Admin area, for a user who lost the device and the
    /// recovery codes. The same accounts an admin may send a recovery link to.
    pub async fn reset_two_factor_as_admin(
        &self,
        actor_id: Id,
        user_id: Id,
        request_id: &str,
    ) -> Result<(), AdminAccountError> {
        let now = self.database().database_now().await?;
        let mut transaction = self.database().immediate_transaction().await?;
        if !Self::is_active_admin(&mut transaction, actor_id).await? {
            return Err(AdminAccountError::Forbidden);
        }
        if actor_id == user_id {
            return Err(AdminAccountError::OwnAccount);
        }
        Self::may_manage(&mut transaction, actor_id, user_id).await?;
        let exists = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users WHERE id = ?")
            .bind(user_id.to_string())
            .fetch_one(&mut *transaction)
            .await?;
        if exists == 0 {
            return Err(AdminAccountError::NotFound);
        }
        delete_two_factor(&mut transaction, user_id).await?;
        audit::record_global(
            &mut transaction,
            Some(actor_id),
            "two_factor.disabled",
            AuditOutcome::Success,
            "user",
            Some(user_id),
            request_id,
            serde_json::json!({"issued_by": "admin"}),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn list_passkeys(&self, user_id: Id) -> Result<Vec<PasskeyRecord>, IdentityError> {
        let rows = sqlx::query(
            "SELECT id, name, created_at, last_used_at FROM user_passkeys \
             WHERE user_id = ? ORDER BY created_at, id",
        )
        .bind(user_id.to_string())
        .fetch_all(self.database().pool())
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| PasskeyRecord {
                id: row.get("id"),
                name: row.get("name"),
                created_at: TimestampMillis::from_millis(row.get("created_at")),
                last_used_at: row
                    .get::<Option<i64>, _>("last_used_at")
                    .map(TimestampMillis::from_millis),
            })
            .collect())
    }

    /// The credentials of an account, so a new passkey on the same device is refused by the browser.
    pub async fn passkeys_of(&self, user_id: Id) -> Result<Vec<Passkey>, IdentityError> {
        let rows =
            sqlx::query_scalar::<_, String>("SELECT passkey FROM user_passkeys WHERE user_id = ?")
                .bind(user_id.to_string())
                .fetch_all(self.database().pool())
                .await?;
        rows.iter()
            .map(|json| serde_json::from_str(json).map_err(|_| IdentityError::InvalidIdentifier))
            .collect()
    }

    pub async fn save_passkey(
        &self,
        user_id: Id,
        name: &str,
        passkey: &Passkey,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<PasskeyRecord, PasskeySaveError> {
        let id = Id::new_v7();
        let json = serde_json::to_string(passkey).map_err(|_| IdentityError::InvalidIdentifier)?;
        let mut transaction = self.database().immediate_transaction().await?;
        let inserted = sqlx::query(
            "INSERT INTO user_passkeys (id, user_id, credential_id, name, passkey, created_at) \
             VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT (credential_id) DO NOTHING",
        )
        .bind(id.to_string())
        .bind(user_id.to_string())
        .bind(credential_key(passkey.cred_id()))
        .bind(name)
        .bind(json)
        .bind(now.as_millis())
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if inserted != 1 {
            return Err(PasskeySaveError::Duplicate);
        }
        audit::record_global(
            &mut transaction,
            Some(user_id),
            "passkey.added",
            AuditOutcome::Success,
            "passkey",
            Some(id),
            request_id,
            serde_json::json!({}),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(PasskeyRecord {
            id: id.to_string(),
            name: name.to_owned(),
            created_at: now,
            last_used_at: None,
        })
    }

    /// Deletes one of the user's passkeys. False when the user has no passkey with this ID.
    pub async fn delete_passkey(
        &self,
        user_id: Id,
        passkey_id: Id,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<bool, IdentityError> {
        let mut transaction = self.database().immediate_transaction().await?;
        let deleted = sqlx::query("DELETE FROM user_passkeys WHERE id = ? AND user_id = ?")
            .bind(passkey_id.to_string())
            .bind(user_id.to_string())
            .execute(&mut *transaction)
            .await?
            .rows_affected()
            == 1;
        if deleted {
            audit::record_global(
                &mut transaction,
                Some(user_id),
                "passkey.removed",
                AuditOutcome::Success,
                "passkey",
                Some(passkey_id),
                request_id,
                serde_json::json!({}),
                now,
            )
            .await?;
        }
        transaction.commit().await?;
        Ok(deleted)
    }

    pub async fn passkey_by_credential(
        &self,
        credential_id: &[u8],
    ) -> Result<Option<StoredPasskey>, IdentityError> {
        let row =
            sqlx::query("SELECT id, user_id, passkey FROM user_passkeys WHERE credential_id = ?")
                .bind(credential_key(credential_id))
                .fetch_optional(self.database().pool())
                .await?;
        row.map(|row| {
            Ok(StoredPasskey {
                id: row
                    .get::<String, _>("id")
                    .parse()
                    .map_err(|_| IdentityError::InvalidIdentifier)?,
                user_id: row
                    .get::<String, _>("user_id")
                    .parse()
                    .map_err(|_| IdentityError::InvalidIdentifier)?,
                passkey: serde_json::from_str(&row.get::<String, _>("passkey"))
                    .map_err(|_| IdentityError::InvalidIdentifier)?,
            })
        })
        .transpose()
    }

    /// Signs in with a verified passkey: stores its new counter and creates the session. Fails for a suspended
    /// account or a passkey deleted meanwhile.
    pub async fn create_passkey_session_audited(
        &self,
        passkey: &StoredPasskey,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(AuthenticatedUser, IssuedSession), IdentityError> {
        let json = serde_json::to_string(&passkey.passkey)
            .map_err(|_| IdentityError::InvalidIdentifier)?;
        let mut transaction = self.database().immediate_transaction().await?;
        let updated = sqlx::query(
            "UPDATE user_passkeys SET passkey = ?, last_used_at = ? WHERE id = ? AND user_id = ?",
        )
        .bind(json)
        .bind(now.as_millis())
        .bind(passkey.id.to_string())
        .bind(passkey.user_id.to_string())
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        let user = sqlx::query(
            "SELECT email, display_name FROM users WHERE id = ? AND suspended_at IS NULL",
        )
        .bind(passkey.user_id.to_string())
        .fetch_optional(&mut *transaction)
        .await?;
        let (Some(user), 1) = (user, updated) else {
            return Err(IdentityError::InvalidCredential);
        };
        let user = AuthenticatedUser {
            id: passkey.user_id,
            email: user.get("email"),
            display_name: user.get("display_name"),
        };
        let id = Id::new_v7();
        let session =
            insert_session(&mut transaction, id, &generate_opaque_token(), user.id, now).await?;
        audit::record_global(
            &mut transaction,
            Some(user.id),
            "authentication.login",
            AuditOutcome::Success,
            "session",
            Some(id),
            request_id,
            serde_json::json!({"method": "passkey"}),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok((user, session))
    }
}

async fn replace_recovery_codes(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    user_id: Id,
    now: TimestampMillis,
) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query("DELETE FROM user_recovery_codes WHERE user_id = ?")
        .bind(user_id.to_string())
        .execute(&mut **transaction)
        .await?;
    let codes = new_recovery_codes();
    for code in &codes {
        sqlx::query(
            "INSERT INTO user_recovery_codes (user_id, code_hash, created_at) VALUES (?, ?, ?)",
        )
        .bind(user_id.to_string())
        .bind(token_hash(&normalized_recovery_code(code)).to_vec())
        .bind(now.as_millis())
        .execute(&mut **transaction)
        .await?;
    }
    Ok(codes)
}

async fn delete_two_factor(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    user_id: Id,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM user_totp WHERE user_id = ?")
        .bind(user_id.to_string())
        .execute(&mut **transaction)
        .await?;
    sqlx::query("DELETE FROM user_recovery_codes WHERE user_id = ?")
        .bind(user_id.to_string())
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{RECOVERY_CODE_COUNT, new_recovery_codes, normalized_recovery_code};

    #[test]
    fn recovery_codes_are_distinct_and_typed_loosely() {
        let codes = new_recovery_codes();
        assert_eq!(codes.len(), RECOVERY_CODE_COUNT);
        let unique = codes.iter().collect::<std::collections::BTreeSet<_>>();
        assert_eq!(unique.len(), RECOVERY_CODE_COUNT);
        assert!(
            codes
                .iter()
                .all(|code| code.len() == 11 && code.as_bytes()[5] == b'-')
        );
        assert_eq!(normalized_recovery_code(" ABCDE-fghjk "), "abcdefghjk");
    }
}
