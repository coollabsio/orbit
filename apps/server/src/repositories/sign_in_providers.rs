//! Sign in with Google and GitHub: the identities that link a provider account to an Orbit account, the decision a
//! provider sign-in makes, and the OAuth clients an admin saved.

use orbit_platform::{
    AuthenticatedUser, Id, IssuedSession, Row, TimestampMillis, generate_opaque_token,
};
use serde::Serialize;
use sqlx::{Any, Transaction};
use thiserror::Error;
use utoipa::ToSchema;

use super::identity::{IdentityRepository, NO_PASSWORD, insert_session};
use super::instance_settings::InstanceSettingsRepository;
use crate::audit::{self, AuditOutcome};
use crate::sign_in_providers::{Provider, ProviderProfile};

/// Why a provider sign-in cannot continue to an account.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignInRefusal {
    /// No identity yet, and the provider does not vouch for an email to link or sign up with.
    EmailUnverified,
    /// No identity, no account with the email, and registration is closed.
    RegistrationClosed,
    /// The account with this email is already connected to another account at this provider.
    IdentityConflict,
    Suspended,
    /// The identity or account changed between the first step and the code.
    Stale,
}

#[derive(Debug, Error)]
pub enum ProviderSignInError {
    #[error("provider sign-in refused: {0:?}")]
    Refused(SignInRefusal),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

impl From<SignInRefusal> for ProviderSignInError {
    fn from(refusal: SignInRefusal) -> Self {
        Self::Refused(refusal)
    }
}

/// How a provider sign-in reaches its account.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SignInRoute {
    /// The (provider, subject) identity exists.
    Identity(Id),
    /// The provider-verified email belongs to this account: link it, after the second factor if there is one.
    Link(Id),
    Register,
}

/// The account whose email the provider verified.
#[derive(Clone, Copy, Debug)]
pub(crate) struct EmailAccount {
    pub user_id: Id,
    /// It already has an identity at this provider (another subject).
    pub has_provider: bool,
}

/// The sign-in rules: the identity wins; otherwise only a provider-verified email links or signs up.
pub(crate) fn sign_in_route(
    identity_owner: Option<Id>,
    email_verified: bool,
    email_account: Option<EmailAccount>,
    registration_open: bool,
) -> Result<SignInRoute, SignInRefusal> {
    if let Some(user_id) = identity_owner {
        return Ok(SignInRoute::Identity(user_id));
    }
    if !email_verified {
        return Err(SignInRefusal::EmailUnverified);
    }
    match email_account {
        Some(account) if account.has_provider => Err(SignInRefusal::IdentityConflict),
        Some(account) => Ok(SignInRoute::Link(account.user_id)),
        None if registration_open => Ok(SignInRoute::Register),
        None => Err(SignInRefusal::RegistrationClosed),
    }
}

/// The account a provider sign-in continues with. Nothing about a link is written yet when `link` is true.
pub struct ProviderAccount {
    pub user: AuthenticatedUser,
    pub link: bool,
}

#[derive(Debug, Error)]
pub enum IdentityChangeError {
    /// The (provider, subject) identity belongs to another account.
    #[error("identity in use")]
    InUse,
    /// The account is already connected to another account at this provider.
    #[error("provider already connected")]
    AlreadyConnected,
    #[error("identity not connected")]
    NotConnected,
    /// A confirmation came from another provider account than the connected one.
    #[error("different provider account")]
    Mismatch,
    #[error("last sign-in method")]
    LastSignInMethod,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// A connected provider account, as Security → Connected accounts lists it.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ConnectedAccount {
    pub provider: Provider,
    /// The address (or GitHub login) the provider reported when it was connected.
    pub email: Option<String>,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
}

/// A saved OAuth client. The secret is encrypted under the app key.
#[derive(Clone, Debug)]
pub struct SavedSignInProvider {
    pub provider: Provider,
    pub client_id: String,
    pub client_secret: Vec<u8>,
}

#[derive(Debug, Error)]
pub enum SaveSignInProviderError {
    /// The first save of a provider needs the secret.
    #[error("client secret required")]
    SecretRequired,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// Password, passkeys and identities of `user_id`, read inside the caller's transaction. A removal calls it after its
/// delete, so two concurrent removals cannot both leave the account without a way in.
pub(crate) async fn sign_in_methods(
    transaction: &mut Transaction<'_, Any>,
    user_id: Id,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar::<_, i64>(
        "SELECT (CASE WHEN password_hash <> $2 THEN 1 ELSE 0 END) \
         + (SELECT COUNT(*) FROM user_passkeys WHERE user_passkeys.user_id = users.id) \
         + (SELECT COUNT(*) FROM user_identities WHERE user_identities.user_id = users.id) \
         FROM users WHERE id = $1",
    )
    .bind(user_id.to_string())
    .bind(NO_PASSWORD)
    .fetch_one(&mut **transaction)
    .await
}

async fn identity_owner(
    transaction: &mut Transaction<'_, Any>,
    provider: Provider,
    subject: &str,
) -> Result<Option<Id>, sqlx::Error> {
    let owner = sqlx::query_scalar::<_, String>(
        "SELECT user_id FROM user_identities WHERE provider = $1 AND subject = $2",
    )
    .bind(provider.as_str())
    .bind(subject)
    .fetch_optional(&mut **transaction)
    .await?;
    owner
        .map(|owner| {
            owner
                .parse()
                .map_err(|error| sqlx::Error::Decode(Box::new(error)))
        })
        .transpose()
}

async fn has_provider(
    transaction: &mut Transaction<'_, Any>,
    user_id: Id,
    provider: Provider,
) -> Result<bool, sqlx::Error> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM user_identities WHERE user_id = $1 AND provider = $2",
    )
    .bind(user_id.to_string())
    .bind(provider.as_str())
    .fetch_one(&mut **transaction)
    .await?
        > 0)
}

async fn insert_identity(
    transaction: &mut Transaction<'_, Any>,
    user_id: Id,
    profile: &ProviderProfile,
    via: &str,
    request_id: &str,
    now: TimestampMillis,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO user_identities (provider, subject, user_id, email, created_at) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(profile.provider.as_str())
    .bind(&profile.subject)
    .bind(user_id.to_string())
    .bind(profile.email.as_deref())
    .bind(now.as_millis())
    .execute(&mut **transaction)
    .await?;
    audit::record_global(
        transaction,
        Some(user_id),
        "identity.connected",
        AuditOutcome::Success,
        "user",
        Some(user_id),
        request_id,
        serde_json::json!({"provider": profile.provider.as_str(), "via": via}),
        now,
    )
    .await
}

impl IdentityRepository {
    /// Finds or creates the account a provider sign-in continues with (see [`sign_in_route`]). A new account has no
    /// password and starts verified (the provider vouched for the email); it is created with its identity here. A
    /// link to an existing account is only decided: the caller writes it with the session, after a second factor.
    pub async fn provider_account(
        &self,
        profile: &ProviderProfile,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<ProviderAccount, ProviderSignInError> {
        let mut transaction = self.database().immediate_transaction().await?;
        let owner = identity_owner(&mut transaction, profile.provider, &profile.subject).await?;
        let email_account = match (&owner, &profile.verified_email) {
            (None, Some(email)) => {
                let user_id = sqlx::query_scalar::<_, String>(
                    "SELECT id FROM users WHERE normalized_email = $1",
                )
                .bind(email)
                .fetch_optional(&mut *transaction)
                .await?
                .map(|id| id.parse::<Id>())
                .transpose()
                .map_err(|error| sqlx::Error::Decode(Box::new(error)))?;
                match user_id {
                    Some(user_id) => Some(EmailAccount {
                        user_id,
                        has_provider: has_provider(&mut transaction, user_id, profile.provider)
                            .await?,
                    }),
                    None => None,
                }
            }
            _ => None,
        };
        let registration_open = owner.is_none()
            && email_account.is_none()
            && sqlx::query_scalar::<_, i64>(
                "SELECT registration_open FROM instance_settings WHERE id = 1",
            )
            .fetch_one(&mut *transaction)
            .await?
                == 1;
        let route = sign_in_route(
            owner,
            profile.verified_email.is_some(),
            email_account,
            registration_open,
        )?;
        let (user_id, link) = match route {
            SignInRoute::Identity(user_id) => (user_id, false),
            SignInRoute::Link(user_id) => (user_id, true),
            SignInRoute::Register => {
                let email = profile
                    .verified_email
                    .as_deref()
                    .expect("registration needs a verified email");
                let user_id = Id::new_v7();
                sqlx::query(
                    "INSERT INTO users (id, email, normalized_email, display_name, password_hash, \
                     email_verified_at, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
                )
                .bind(user_id.to_string())
                .bind(email)
                .bind(email)
                .bind(&profile.display_name)
                .bind(NO_PASSWORD)
                .bind(now.as_millis())
                .bind(now.as_millis())
                .bind(now.as_millis())
                .execute(&mut *transaction)
                .await?;
                audit::record_global(
                    &mut transaction,
                    Some(user_id),
                    "account.registered",
                    AuditOutcome::Success,
                    "user",
                    Some(user_id),
                    request_id,
                    serde_json::json!({"method": profile.provider.as_str()}),
                    now,
                )
                .await?;
                insert_identity(
                    &mut transaction,
                    user_id,
                    profile,
                    "sign_up",
                    request_id,
                    now,
                )
                .await?;
                (user_id, false)
            }
        };
        let user = sqlx::query("SELECT email, display_name, suspended_at FROM users WHERE id = $1")
            .bind(user_id.to_string())
            .fetch_one(&mut *transaction)
            .await?;
        if user.get::<Option<i64>, _>("suspended_at").is_some() {
            return Err(SignInRefusal::Suspended.into());
        }
        let user = AuthenticatedUser {
            id: user_id,
            email: user.get("email"),
            display_name: user.get("display_name"),
        };
        transaction.commit().await?;
        Ok(ProviderAccount { user, link })
    }

    /// Signs in after [`Self::provider_account`] (and the second factor, when the account has one). In one
    /// transaction: re-checks that the account is not suspended and that the identity is still the account's, or
    /// writes the new link (and marks the email verified); then the session and `authentication.login`.
    pub async fn create_provider_session_audited(
        &self,
        user: &AuthenticatedUser,
        profile: &ProviderProfile,
        link: bool,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<IssuedSession, ProviderSignInError> {
        let mut transaction = self.database().immediate_transaction().await?;
        let active = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM users WHERE id = $1 AND suspended_at IS NULL",
        )
        .bind(user.id.to_string())
        .fetch_one(&mut *transaction)
        .await?;
        if active != 1 {
            return Err(SignInRefusal::Suspended.into());
        }
        let owner = identity_owner(&mut transaction, profile.provider, &profile.subject).await?;
        match (owner, link) {
            (Some(owner), _) if owner == user.id => {}
            (None, true) => {
                if has_provider(&mut transaction, user.id, profile.provider).await? {
                    return Err(SignInRefusal::IdentityConflict.into());
                }
                insert_identity(
                    &mut transaction,
                    user.id,
                    profile,
                    "sign_in",
                    request_id,
                    now,
                )
                .await?;
                sqlx::query(
                    "UPDATE users SET email_verified_at = COALESCE(email_verified_at, $1) WHERE id = $2",
                )
                .bind(now.as_millis())
                .bind(user.id.to_string())
                .execute(&mut *transaction)
                .await?;
            }
            _ => return Err(SignInRefusal::Stale.into()),
        }
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
            serde_json::json!({"method": profile.provider.as_str()}),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(session)
    }

    /// Connect from Security: links the provider account to `user_id`, whatever its email (the person is signed in
    /// and proved control of both). Connecting the same provider account again changes nothing.
    pub async fn connect_identity(
        &self,
        user_id: Id,
        profile: &ProviderProfile,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), IdentityChangeError> {
        let mut transaction = self.database().immediate_transaction().await?;
        match identity_owner(&mut transaction, profile.provider, &profile.subject).await? {
            Some(owner) if owner == user_id => return Ok(()),
            Some(_) => return Err(IdentityChangeError::InUse),
            None => {}
        }
        if has_provider(&mut transaction, user_id, profile.provider).await? {
            return Err(IdentityChangeError::AlreadyConnected);
        }
        insert_identity(
            &mut transaction,
            user_id,
            profile,
            "settings",
            request_id,
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// A fresh provider sign-in proves that the person at `session_id` is the account owner: sets
    /// `sessions.reauthenticated_at`. The provider account must be the one connected to the account.
    pub async fn confirm_with_provider(
        &self,
        session_id: Id,
        user_id: Id,
        profile: &ProviderProfile,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), IdentityChangeError> {
        let mut transaction = self.database().immediate_transaction().await?;
        if identity_owner(&mut transaction, profile.provider, &profile.subject).await?
            != Some(user_id)
        {
            return Err(IdentityChangeError::Mismatch);
        }
        let updated = sqlx::query(
            "UPDATE sessions SET reauthenticated_at = $1 WHERE id = $2 AND user_id = $3 \
             AND revoked_at IS NULL",
        )
        .bind(now.as_millis())
        .bind(session_id.to_string())
        .bind(user_id.to_string())
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if updated != 1 {
            return Err(IdentityChangeError::Mismatch);
        }
        audit::record_global(
            &mut transaction,
            Some(user_id),
            "authentication.reauthenticated",
            AuditOutcome::Success,
            "session",
            Some(session_id),
            request_id,
            serde_json::json!({"method": profile.provider.as_str()}),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Whether the session signed in or confirmed with a provider at `since` or later.
    pub async fn session_fresh_since(
        &self,
        session_id: Id,
        since: TimestampMillis,
    ) -> Result<bool, sqlx::Error> {
        Ok(sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM sessions WHERE id = $1 AND revoked_at IS NULL \
             AND COALESCE(reauthenticated_at, created_at) >= $2",
        )
        .bind(session_id.to_string())
        .bind(since.as_millis())
        .fetch_one(self.database().pool())
        .await?
            == 1)
    }

    /// Removes the connection unless it is the account's last way to sign in.
    pub async fn disconnect_identity(
        &self,
        user_id: Id,
        provider: Provider,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), IdentityChangeError> {
        let mut transaction = self.database().immediate_transaction().await?;
        let deleted =
            sqlx::query("DELETE FROM user_identities WHERE user_id = $1 AND provider = $2")
                .bind(user_id.to_string())
                .bind(provider.as_str())
                .execute(&mut *transaction)
                .await?
                .rows_affected();
        if deleted == 0 {
            return Err(IdentityChangeError::NotConnected);
        }
        if sign_in_methods(&mut transaction, user_id).await? == 0 {
            return Err(IdentityChangeError::LastSignInMethod);
        }
        audit::record_global(
            &mut transaction,
            Some(user_id),
            "identity.disconnected",
            AuditOutcome::Success,
            "user",
            Some(user_id),
            request_id,
            serde_json::json!({"provider": provider.as_str()}),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn connected_accounts(
        &self,
        user_id: Id,
    ) -> Result<Vec<ConnectedAccount>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT provider, email, created_at FROM user_identities WHERE user_id = $1 \
             ORDER BY provider",
        )
        .bind(user_id.to_string())
        .fetch_all(self.database().pool())
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(ConnectedAccount {
                    provider: Provider::parse(&row.get::<String, _>("provider"))
                        .ok_or_else(|| sqlx::Error::Decode("unknown sign-in provider".into()))?,
                    email: row.get("email"),
                    created_at: TimestampMillis::from_millis(row.get("created_at")),
                })
            })
            .collect()
    }
}

impl InstanceSettingsRepository {
    pub async fn sign_in_providers(&self) -> Result<Vec<SavedSignInProvider>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT provider, client_id, client_secret FROM sign_in_providers ORDER BY provider",
        )
        .fetch_all(self.database.pool())
        .await?;
        Ok(rows
            .into_iter()
            .filter_map(|row| {
                Some(SavedSignInProvider {
                    provider: Provider::parse(&row.get::<String, _>("provider"))?,
                    client_id: row.get("client_id"),
                    client_secret: row.get("client_secret"),
                })
            })
            .collect())
    }

    pub async fn sign_in_provider(
        &self,
        provider: Provider,
    ) -> Result<Option<SavedSignInProvider>, sqlx::Error> {
        let row = sqlx::query(
            "SELECT client_id, client_secret FROM sign_in_providers WHERE provider = $1",
        )
        .bind(provider.as_str())
        .fetch_optional(self.database.pool())
        .await?;
        Ok(row.map(|row| SavedSignInProvider {
            provider,
            client_id: row.get("client_id"),
            client_secret: row.get("client_secret"),
        }))
    }

    /// Saves the OAuth client. `client_secret: None` keeps the saved secret.
    pub async fn save_sign_in_provider(
        &self,
        actor_id: Id,
        provider: Provider,
        client_id: &str,
        client_secret: Option<Vec<u8>>,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), SaveSignInProviderError> {
        let mut transaction = self.database.immediate_transaction().await?;
        match client_secret {
            Some(secret) => {
                sqlx::query(
                    "INSERT INTO sign_in_providers (provider, client_id, client_secret, created_at, updated_at) \
                     VALUES ($1, $2, $3, $4, $4) ON CONFLICT (provider) DO UPDATE SET \
                     client_id = excluded.client_id, client_secret = excluded.client_secret, \
                     updated_at = excluded.updated_at",
                )
                .bind(provider.as_str())
                .bind(client_id)
                .bind(secret)
                .bind(now.as_millis())
                .execute(&mut *transaction)
                .await?;
            }
            None => {
                let updated = sqlx::query(
                    "UPDATE sign_in_providers SET client_id = $1, updated_at = $2 WHERE provider = $3",
                )
                .bind(client_id)
                .bind(now.as_millis())
                .bind(provider.as_str())
                .execute(&mut *transaction)
                .await?
                .rows_affected();
                if updated == 0 {
                    return Err(SaveSignInProviderError::SecretRequired);
                }
            }
        }
        audit::record_global(
            &mut transaction,
            Some(actor_id),
            "instance.sign_in_provider_saved",
            AuditOutcome::Success,
            "installation",
            None,
            request_id,
            serde_json::json!({"provider": provider.as_str()}),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Stops offering the provider. Connected identities stay, so saving it again restores them.
    pub async fn remove_sign_in_provider(
        &self,
        actor_id: Id,
        provider: Provider,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), sqlx::Error> {
        let mut transaction = self.database.immediate_transaction().await?;
        let deleted = sqlx::query("DELETE FROM sign_in_providers WHERE provider = $1")
            .bind(provider.as_str())
            .execute(&mut *transaction)
            .await?
            .rows_affected();
        if deleted > 0 {
            audit::record_global(
                &mut transaction,
                Some(actor_id),
                "instance.sign_in_provider_removed",
                AuditOutcome::Success,
                "installation",
                None,
                request_id,
                serde_json::json!({"provider": provider.as_str()}),
                now,
            )
            .await?;
        }
        transaction.commit().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(user_id: Id, has_provider: bool) -> EmailAccount {
        EmailAccount {
            user_id,
            has_provider,
        }
    }

    #[test]
    fn the_identity_wins_over_the_email() {
        let owner = Id::new_v7();
        assert_eq!(
            sign_in_route(Some(owner), false, None, false),
            Ok(SignInRoute::Identity(owner))
        );
        assert_eq!(
            sign_in_route(Some(owner), true, Some(account(Id::new_v7(), false)), true),
            Ok(SignInRoute::Identity(owner))
        );
    }

    #[test]
    fn only_a_verified_email_links_or_signs_up() {
        let user_id = Id::new_v7();
        assert_eq!(
            sign_in_route(None, false, None, true),
            Err(SignInRefusal::EmailUnverified)
        );
        assert_eq!(
            sign_in_route(None, true, Some(account(user_id, false)), false),
            Ok(SignInRoute::Link(user_id))
        );
        assert_eq!(
            sign_in_route(None, true, Some(account(user_id, true)), true),
            Err(SignInRefusal::IdentityConflict)
        );
        assert_eq!(
            sign_in_route(None, true, None, true),
            Ok(SignInRoute::Register)
        );
        assert_eq!(
            sign_in_route(None, true, None, false),
            Err(SignInRefusal::RegistrationClosed)
        );
    }
}
