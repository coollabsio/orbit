//! Mail providers (provider intake design §3 P2): a provider receives mail for a domain and delivers
//! it to Orbit over HTTPS. An instance provider (no workspace) belongs to the root user and every
//! workspace may pick it; a workspace provider belongs to one workspace. Secrets are sealed with the
//! app key (associated data `mail-provider:<id>`) and never leave the repository except to the
//! public intake route and, once, to whoever created them.

use std::str::FromStr;

use base64::Engine as _;
use orbit_platform::{Id, Row, TimestampMillis};
use rand_core::RngCore as _;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::any::AnyRow;
use sqlx::{Any, Transaction};

use super::domains::{manager, parse_id};
use super::{MailError, MailRepository, record_mail_audit};
use crate::audit::{self, AuditOutcome};
use crate::secret_box;

const NAME_MAX_CHARS: usize = 80;
/// How long a rejected delivery stays in the Failed messages list.
pub const REJECTED_RETENTION_MS: i64 = 30 * 24 * 60 * 60 * 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Postshiba,
    Cloudflare,
    Postmark,
    Mailgun,
    Resend,
    Ses,
}

impl ProviderKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Postshiba => "postshiba",
            Self::Cloudflare => "cloudflare",
            Self::Postmark => "postmark",
            Self::Mailgun => "mailgun",
            Self::Resend => "resend",
            Self::Ses => "ses",
        }
    }

    /// The MX hosts the provider asks for. Resend has none to suggest: its value is per account.
    #[must_use]
    pub fn default_mx_hosts(self, region: Option<&str>) -> Vec<String> {
        let hosts: Vec<String> = match self {
            Self::Postshiba => vec!["inbound.postshiba.com".to_owned()],
            Self::Cloudflare => ["route1", "route2", "route3"]
                .iter()
                .map(|host| format!("{host}.mx.cloudflare.net"))
                .collect(),
            Self::Postmark => vec!["inbound.postmarkapp.com".to_owned()],
            Self::Mailgun if region == Some("eu") => {
                vec![
                    "mxa.eu.mailgun.org".to_owned(),
                    "mxb.eu.mailgun.org".to_owned(),
                ]
            }
            Self::Mailgun => vec!["mxa.mailgun.org".to_owned(), "mxb.mailgun.org".to_owned()],
            Self::Ses => vec![format!(
                "inbound-smtp.{}.amazonaws.com",
                region.unwrap_or("us-east-1")
            )],
            Self::Resend => Vec::new(),
        };
        hosts
    }
}

impl FromStr for ProviderKind {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(match value {
            "postshiba" => Self::Postshiba,
            "cloudflare" => Self::Cloudflare,
            "postmark" => Self::Postmark,
            "mailgun" => Self::Mailgun,
            "resend" => Self::Resend,
            "ses" => Self::Ses,
            _ => return Err(()),
        })
    }
}

/// The secrets of one provider. Orbit makes the Cloudflare and Postmark ones itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProviderSecrets {
    Postshiba {
        api_key: String,
        webhook_secret: String,
    },
    Cloudflare {
        signing_secret: String,
    },
    Postmark {
        basic_user: String,
        basic_password: String,
    },
    Mailgun {
        signing_key: String,
    },
    Resend {
        api_key: String,
        webhook_secret: String,
    },
    Ses {
        access_key_id: String,
        secret_access_key: String,
    },
}

impl ProviderSecrets {
    #[must_use]
    pub const fn kind(&self) -> ProviderKind {
        match self {
            Self::Postshiba { .. } => ProviderKind::Postshiba,
            Self::Cloudflare { .. } => ProviderKind::Cloudflare,
            Self::Postmark { .. } => ProviderKind::Postmark,
            Self::Mailgun { .. } => ProviderKind::Mailgun,
            Self::Resend { .. } => ProviderKind::Resend,
            Self::Ses { .. } => ProviderKind::Ses,
        }
    }

    /// The names of the secrets that have a value (what the API shows instead of the values).
    #[must_use]
    pub fn names_set(&self) -> Vec<&'static str> {
        let fields: Vec<(&'static str, &str)> = match self {
            Self::Cloudflare { signing_secret } => vec![("signing_secret", signing_secret)],
            Self::Postmark {
                basic_user,
                basic_password,
            } => vec![
                ("basic_user", basic_user),
                ("basic_password", basic_password),
            ],
            Self::Mailgun { signing_key } => vec![("signing_key", signing_key)],
            Self::Postshiba {
                api_key,
                webhook_secret,
            }
            | Self::Resend {
                api_key,
                webhook_secret,
            } => vec![("api_key", api_key), ("webhook_secret", webhook_secret)],
            Self::Ses {
                access_key_id,
                secret_access_key,
            } => vec![
                ("access_key_id", access_key_id),
                ("secret_access_key", secret_access_key),
            ],
        };
        fields
            .into_iter()
            .filter(|(_, value)| !value.trim().is_empty())
            .map(|(name, _)| name)
            .collect()
    }

    /// The secrets Orbit makes for `kind` (32 random bytes, base64url), or `None` when the provider
    /// gives them.
    #[must_use]
    pub fn generate(kind: ProviderKind) -> Option<Self> {
        match kind {
            ProviderKind::Cloudflare => Some(Self::Cloudflare {
                signing_secret: random_secret(),
            }),
            ProviderKind::Postmark => Some(Self::Postmark {
                basic_user: format!("orbit-{}", &random_secret()[..12]),
                basic_password: random_secret(),
            }),
            _ => None,
        }
    }
}

fn random_secret() -> String {
    let mut bytes = [0_u8; 32];
    rand_core::OsRng.fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderSettings {
    /// The domain's MX must point to one of these for the route to work.
    #[serde(default)]
    pub mx_hosts: Vec<String>,
    /// SES: the AWS region; Mailgun: `eu` for the EU region.
    #[serde(default)]
    pub region: Option<String>,
    /// SES: the S3 bucket of the receipt rule, and its key prefix.
    #[serde(default)]
    pub bucket: Option<String>,
    #[serde(default)]
    pub prefix: Option<String>,
    /// SES: the SNS topic of the receipt rule. Orbit refuses notifications of any other topic: SNS
    /// signs every topic's messages, and Orbit confirms subscriptions by itself.
    #[serde(default)]
    pub topic_arn: Option<String>,
}

impl ProviderSettings {
    /// Trimmed, lower-case MX hosts without trailing dots; the provider's defaults when empty.
    fn cleaned(mut self, kind: ProviderKind) -> Result<Self, MailError> {
        let clean = |value: Option<String>| {
            value
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        };
        self.region = clean(self.region).map(|region| region.to_ascii_lowercase());
        self.bucket = clean(self.bucket);
        self.prefix = clean(self.prefix).map(|prefix| prefix.trim_matches('/').to_owned());
        self.topic_arn = clean(self.topic_arn);
        let mut hosts = Vec::new();
        for host in &self.mx_hosts {
            let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
            if host.is_empty() {
                continue;
            }
            let host = orbit_mail::normalize_domain(&host)
                .map_err(|_| MailError::Invalid { field: "mx_hosts" })?;
            if !hosts.contains(&host) {
                hosts.push(host);
            }
        }
        self.mx_hosts = if hosts.is_empty() {
            kind.default_mx_hosts(self.region.as_deref())
        } else {
            hosts
        };
        if kind == ProviderKind::Ses {
            if self.bucket.is_none() || self.region.is_none() {
                return Err(MailError::Invalid { field: "bucket" });
            }
            if !self
                .topic_arn
                .as_deref()
                .is_some_and(|arn| arn.starts_with("arn:aws:sns:"))
            {
                return Err(MailError::Invalid { field: "topic_arn" });
            }
        }
        Ok(self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderScope {
    /// The instance providers. The route checks that the caller is the root user.
    Instance,
    /// One workspace's providers: its owners and admins, and only while the instance switch is on
    /// (for creating).
    Workspace(Id),
}

impl ProviderScope {
    const fn workspace(self) -> Option<Id> {
        match self {
            Self::Instance => None,
            Self::Workspace(id) => Some(id),
        }
    }
}

#[derive(Clone, Debug)]
pub struct NewProvider {
    pub kind: ProviderKind,
    pub name: String,
    /// Required for PostShiba, Mailgun, Resend and SES; ignored for Cloudflare and Postmark (Orbit makes them).
    pub secrets: Option<ProviderSecrets>,
    pub settings: ProviderSettings,
}

#[derive(Clone, Debug, Default)]
pub struct ProviderChange {
    pub name: Option<String>,
    /// Replaces the secrets (of the same kind).
    pub secrets: Option<ProviderSecrets>,
    pub settings: Option<ProviderSettings>,
    /// Makes new secrets for a provider whose secrets Orbit makes (Cloudflare, Postmark).
    pub regenerate: bool,
}

/// A provider without its secret values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MailProvider {
    pub id: Id,
    pub workspace_id: Option<Id>,
    pub kind: ProviderKind,
    pub name: String,
    pub settings: ProviderSettings,
    pub secrets_set: Vec<&'static str>,
    pub last_received_at: Option<TimestampMillis>,
    pub last_error: Option<String>,
    /// Domains whose mail arrives through it (of every workspace).
    pub domain_count: i64,
    pub created_at: TimestampMillis,
    pub updated_at: TimestampMillis,
}

/// A created or changed provider, with the secrets Orbit made for it (shown once).
#[derive(Clone, Debug)]
pub struct CreatedProvider {
    pub provider: MailProvider,
    pub generated: Option<ProviderSecrets>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectReason {
    UnknownAddress,
    WrongRoute,
    DomainUnavailable,
    MailboxSuspended,
}

impl RejectReason {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnknownAddress => "unknown_address",
            Self::WrongRoute => "wrong_route",
            Self::DomainUnavailable => "domain_unavailable",
            Self::MailboxSuspended => "mailbox_suspended",
        }
    }
}

impl FromStr for RejectReason {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(match value {
            "unknown_address" => Self::UnknownAddress,
            "wrong_route" => Self::WrongRoute,
            "domain_unavailable" => Self::DomainUnavailable,
            "mailbox_suspended" => Self::MailboxSuspended,
            _ => return Err(()),
        })
    }
}

/// Mail a provider accepted that Orbit could not deliver (no body is kept).
#[derive(Clone, Debug)]
pub struct NewRejection {
    /// The workspace of the recipient's domain; `None` when the domain is not on this instance.
    pub workspace_id: Option<Id>,
    pub provider_id: Id,
    pub mail_from: Option<String>,
    pub recipient: String,
    pub reason: RejectReason,
}

const PROVIDER_COLUMNS: &str = "p.id, p.workspace_id, p.kind, p.name, p.secrets, p.settings, \
    p.last_received_at, p.last_error, p.created_at, p.updated_at, \
    (SELECT COUNT(*) FROM mail_domains d WHERE d.provider_id = p.id) AS domain_count";

/// [`PROVIDER_COLUMNS`] with the domain count of the workspace `$1`.
const WORKSPACE_PROVIDER_COLUMNS: &str = "p.id, p.workspace_id, p.kind, p.name, p.secrets, \
    p.settings, p.last_received_at, p.last_error, p.created_at, p.updated_at, \
    (SELECT COUNT(*) FROM mail_domains d WHERE d.provider_id = p.id AND d.workspace_id = $1) \
    AS domain_count";

#[must_use]
pub fn provider_aad(provider_id: Id) -> String {
    format!("mail-provider:{provider_id}")
}

impl MailRepository {
    fn seal_secrets(
        &self,
        provider_id: Id,
        secrets: &ProviderSecrets,
    ) -> Result<Vec<u8>, MailError> {
        let json = serde_json::to_vec(secrets).map_err(|_| MailError::Crypto)?;
        secret_box::seal(&self.app_key, provider_aad(provider_id).as_bytes(), &json)
            .map_err(|()| MailError::Crypto)
    }

    fn open_secrets(&self, provider_id: Id, sealed: &[u8]) -> Result<ProviderSecrets, MailError> {
        let json = secret_box::open(&self.app_key, provider_aad(provider_id).as_bytes(), sealed)
            .map_err(|()| MailError::Crypto)?;
        serde_json::from_slice(&json).map_err(|_| MailError::Crypto)
    }

    /// The provider and its secrets.
    fn provider(&self, row: &AnyRow) -> Result<(MailProvider, ProviderSecrets), MailError> {
        let id = parse_id(&row.try_get::<String, _>("id")?)?;
        let secrets = self.open_secrets(id, &row.try_get::<Vec<u8>, _>("secrets")?)?;
        let settings: String = row.try_get("settings")?;
        let provider = MailProvider {
            id,
            workspace_id: row
                .try_get::<Option<String>, _>("workspace_id")?
                .map(|id| parse_id(&id))
                .transpose()?,
            kind: row
                .try_get::<String, _>("kind")?
                .parse()
                .map_err(|()| MailError::Invalid { field: "kind" })?,
            name: row.try_get("name")?,
            settings: serde_json::from_str(&settings)
                .map_err(|_| MailError::Invalid { field: "settings" })?,
            secrets_set: secrets.names_set(),
            last_received_at: row
                .try_get::<Option<i64>, _>("last_received_at")?
                .map(TimestampMillis::from_millis),
            last_error: row.try_get("last_error")?,
            domain_count: row.try_get("domain_count")?,
            created_at: TimestampMillis::from_millis(row.try_get("created_at")?),
            updated_at: TimestampMillis::from_millis(row.try_get("updated_at")?),
        };
        Ok((provider, secrets))
    }

    /// Checks the caller for `scope` (owners and admins of the workspace; the instance scope is
    /// checked by the route).
    async fn provider_scope(
        transaction: &mut Transaction<'_, Any>,
        scope: ProviderScope,
        actor_id: Id,
    ) -> Result<(), MailError> {
        if let ProviderScope::Workspace(workspace_id) = scope {
            manager(transaction, workspace_id, actor_id).await?;
        }
        Ok(())
    }

    async fn load_in_scope(
        &self,
        transaction: &mut Transaction<'_, Any>,
        scope: ProviderScope,
        provider_id: Id,
    ) -> Result<(MailProvider, ProviderSecrets), MailError> {
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {PROVIDER_COLUMNS} FROM mail_providers p \
             WHERE p.id = $1 AND p.workspace_id IS NOT DISTINCT FROM $2"
        )))
        .bind(provider_id.to_string())
        .bind(scope.workspace().map(|id| id.to_string()))
        .fetch_optional(&mut **transaction)
        .await?
        .ok_or(MailError::NotFound)?;
        self.provider(&row)
    }

    #[allow(clippy::too_many_arguments)]
    async fn provider_audit(
        transaction: &mut Transaction<'_, Any>,
        scope: ProviderScope,
        actor_id: Id,
        action: &str,
        provider_id: Id,
        request_id: &str,
        metadata: serde_json::Value,
        now: TimestampMillis,
    ) -> Result<(), MailError> {
        match scope {
            ProviderScope::Workspace(workspace_id) => {
                record_mail_audit(
                    transaction,
                    workspace_id,
                    actor_id,
                    action,
                    "mail_provider",
                    provider_id,
                    request_id,
                    metadata,
                    now,
                )
                .await
            }
            ProviderScope::Instance => {
                audit::record_global(
                    transaction,
                    Some(actor_id),
                    action,
                    AuditOutcome::Success,
                    "mail_provider",
                    Some(provider_id),
                    request_id,
                    metadata,
                    now,
                )
                .await?;
                Ok(())
            }
        }
    }

    pub async fn create_provider(
        &self,
        scope: ProviderScope,
        actor_id: Id,
        input: NewProvider,
        request_id: &str,
    ) -> Result<CreatedProvider, MailError> {
        let name = clean_name(&input.name)?;
        let settings = input.settings.cleaned(input.kind)?;
        let generated = ProviderSecrets::generate(input.kind);
        let secrets = match (&generated, input.secrets) {
            (Some(generated), _) => generated.clone(),
            (None, Some(given)) if given.kind() == input.kind && given_complete(&given) => given,
            (None, _) => return Err(MailError::Invalid { field: "secrets" }),
        };
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        Self::provider_scope(&mut transaction, scope, actor_id).await?;
        if matches!(scope, ProviderScope::Workspace(_)) {
            let enabled: i64 = sqlx::query_scalar(
                "SELECT mail_workspace_providers_enabled FROM instance_settings WHERE id = 1",
            )
            .fetch_one(&mut *transaction)
            .await?;
            if enabled != 1 {
                return Err(MailError::Forbidden("workspace_providers_disabled"));
            }
        }
        let id = Id::new_v7();
        sqlx::query(
            "INSERT INTO mail_providers (id, workspace_id, kind, name, secrets, settings, created_at, \
             updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $7)",
        )
        .bind(id.to_string())
        .bind(scope.workspace().map(|id| id.to_string()))
        .bind(input.kind.as_str())
        .bind(&name)
        .bind(self.seal_secrets(id, &secrets)?)
        .bind(serde_json::to_string(&settings).map_err(|_| MailError::Invalid { field: "settings" })?)
        .bind(now.as_millis())
        .execute(&mut *transaction)
        .await?;
        Self::provider_audit(
            &mut transaction,
            scope,
            actor_id,
            "mail.provider_created",
            id,
            request_id,
            json!({ "kind": input.kind.as_str(), "name": name }),
            now,
        )
        .await?;
        let (provider, _) = self.load_in_scope(&mut transaction, scope, id).await?;
        transaction.commit().await?;
        Ok(CreatedProvider {
            provider,
            generated,
        })
    }

    pub async fn update_provider(
        &self,
        scope: ProviderScope,
        actor_id: Id,
        provider_id: Id,
        change: ProviderChange,
        request_id: &str,
    ) -> Result<CreatedProvider, MailError> {
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        Self::provider_scope(&mut transaction, scope, actor_id).await?;
        let (current, current_secrets) = self
            .load_in_scope(&mut transaction, scope, provider_id)
            .await?;
        let name = change
            .name
            .as_deref()
            .map(clean_name)
            .transpose()?
            .unwrap_or(current.name);
        let settings = change
            .settings
            .map(|settings| settings.cleaned(current.kind))
            .transpose()?
            .unwrap_or(current.settings);
        let generated = if change.regenerate {
            let new = ProviderSecrets::generate(current.kind).filter(|_| change.secrets.is_none());
            Some(new.ok_or(MailError::Invalid {
                field: "regenerate_secrets",
            })?)
        } else {
            None
        };
        let secrets = match (change.secrets, &generated) {
            (_, Some(new)) => new.clone(),
            (Some(given), None) if given.kind() == current.kind && given_complete(&given) => given,
            (Some(_), None) => return Err(MailError::Invalid { field: "secrets" }),
            (None, None) => current_secrets,
        };
        sqlx::query(
            "UPDATE mail_providers SET name = $1, settings = $2, secrets = $3, updated_at = $4 \
             WHERE id = $5",
        )
        .bind(&name)
        .bind(
            serde_json::to_string(&settings)
                .map_err(|_| MailError::Invalid { field: "settings" })?,
        )
        .bind(self.seal_secrets(provider_id, &secrets)?)
        .bind(now.as_millis())
        .bind(provider_id.to_string())
        .execute(&mut *transaction)
        .await?;
        Self::provider_audit(
            &mut transaction,
            scope,
            actor_id,
            "mail.provider_updated",
            provider_id,
            request_id,
            json!({ "name": name, "secrets_regenerated": generated.is_some() }),
            now,
        )
        .await?;
        let (provider, _) = self
            .load_in_scope(&mut transaction, scope, provider_id)
            .await?;
        transaction.commit().await?;
        Ok(CreatedProvider {
            provider,
            generated,
        })
    }

    /// Refuses a provider that a domain still uses (`Conflict("provider_in_use")`).
    pub async fn delete_provider(
        &self,
        scope: ProviderScope,
        actor_id: Id,
        provider_id: Id,
        request_id: &str,
    ) -> Result<(), MailError> {
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        Self::provider_scope(&mut transaction, scope, actor_id).await?;
        let (provider, _) = self
            .load_in_scope(&mut transaction, scope, provider_id)
            .await?;
        if provider.domain_count > 0 {
            return Err(MailError::Conflict("provider_in_use"));
        }
        sqlx::query("DELETE FROM mail_providers WHERE id = $1")
            .bind(provider_id.to_string())
            .execute(&mut *transaction)
            .await?;
        Self::provider_audit(
            &mut transaction,
            scope,
            actor_id,
            "mail.provider_deleted",
            provider_id,
            request_id,
            json!({ "kind": provider.kind.as_str(), "name": provider.name }),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// The providers of `scope` by name.
    pub async fn list_providers(
        &self,
        scope: ProviderScope,
        actor_id: Id,
    ) -> Result<Vec<MailProvider>, MailError> {
        let mut transaction = self.database.pool().begin().await?;
        Self::provider_scope(&mut transaction, scope, actor_id).await?;
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {PROVIDER_COLUMNS} FROM mail_providers p \
             WHERE p.workspace_id IS NOT DISTINCT FROM $1 ORDER BY lower(p.name), p.id"
        )))
        .bind(scope.workspace().map(|id| id.to_string()))
        .fetch_all(&mut *transaction)
        .await?;
        transaction.commit().await?;
        rows.iter()
            .map(|row| self.provider(row).map(|(provider, _)| provider))
            .collect()
    }

    /// The providers a workspace may route a domain to: the instance ones and its own. The domain
    /// count is of this workspace's domains (an instance provider serves other workspaces too).
    pub async fn providers_for_workspace(
        &self,
        workspace_id: Id,
        actor_id: Id,
    ) -> Result<Vec<MailProvider>, MailError> {
        let mut transaction = self.database.pool().begin().await?;
        manager(&mut transaction, workspace_id, actor_id).await?;
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {WORKSPACE_PROVIDER_COLUMNS} FROM mail_providers p \
             WHERE p.workspace_id IS NULL OR p.workspace_id = $1 \
             ORDER BY p.workspace_id IS NULL, lower(p.name), p.id"
        )))
        .bind(workspace_id.to_string())
        .fetch_all(&mut *transaction)
        .await?;
        transaction.commit().await?;
        rows.iter()
            .map(|row| self.provider(row).map(|(provider, _)| provider))
            .collect()
    }

    /// A provider with its secrets, for the public intake route (no caller to check).
    pub async fn load_provider(
        &self,
        provider_id: Id,
    ) -> Result<Option<(MailProvider, ProviderSecrets)>, MailError> {
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {PROVIDER_COLUMNS} FROM mail_providers p WHERE p.id = $1"
        )))
        .bind(provider_id.to_string())
        .fetch_optional(self.database.pool())
        .await?;
        row.as_ref().map(|row| self.provider(row)).transpose()
    }

    /// The last delivery: its time on success, its error otherwise.
    pub async fn record_provider_receipt(
        &self,
        provider_id: Id,
        now: TimestampMillis,
        error: Option<&str>,
    ) -> Result<(), MailError> {
        let query = match error {
            None => sqlx::query(
                "UPDATE mail_providers SET last_received_at = $1, last_error = NULL WHERE id = $2",
            )
            .bind(now.as_millis()),
            Some(error) => sqlx::query("UPDATE mail_providers SET last_error = $1 WHERE id = $2")
                .bind(error.chars().take(500).collect::<String>()),
        };
        query
            .bind(provider_id.to_string())
            .execute(self.database.pool())
            .await?;
        Ok(())
    }

    pub async fn record_rejected(
        &self,
        rejection: NewRejection,
        now: TimestampMillis,
    ) -> Result<(), MailError> {
        sqlx::query(
            "INSERT INTO mail_rejected_deliveries (id, workspace_id, provider_id, mail_from, \
             recipient, reason, received_at) VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(Id::new_v7().to_string())
        .bind(rejection.workspace_id.map(|id| id.to_string()))
        .bind(rejection.provider_id.to_string())
        .bind(rejection.mail_from)
        .bind(rejection.recipient.chars().take(320).collect::<String>())
        .bind(rejection.reason.as_str())
        .bind(now.as_millis())
        .execute(self.database.pool())
        .await?;
        Ok(())
    }

    /// Removes rejected deliveries older than [`REJECTED_RETENTION_MS`]; returns how many.
    pub async fn purge_rejected(&self, now: TimestampMillis) -> Result<u64, MailError> {
        let removed = sqlx::query("DELETE FROM mail_rejected_deliveries WHERE received_at < $1")
            .bind(now.as_millis() - REJECTED_RETENTION_MS)
            .execute(self.database.pool())
            .await?
            .rows_affected();
        Ok(removed)
    }

    /// The workspace of the domain `name` and its provider (`None`: the SMTP route), or `None`
    /// when no workspace has it.
    pub async fn domain_route(&self, name: &str) -> Result<Option<(Id, Option<Id>)>, MailError> {
        let row = sqlx::query(
            "SELECT workspace_id, provider_id FROM mail_domains WHERE lower(name) = lower($1)",
        )
        .bind(name.trim_end_matches('.'))
        .fetch_optional(self.database.pool())
        .await?;
        row.map(|row| {
            Ok((
                parse_id(&row.try_get::<String, _>("workspace_id")?)?,
                row.try_get::<Option<String>, _>("provider_id")?
                    .map(|id| parse_id(&id))
                    .transpose()?,
            ))
        })
        .transpose()
    }

    /// Routes the domain's mail through `provider_id` (an instance provider or one of the
    /// workspace), or through Orbit's own listener with `None`.
    pub async fn set_domain_route(
        &self,
        workspace_id: Id,
        actor_id: Id,
        domain_id: Id,
        provider_id: Option<Id>,
        request_id: &str,
    ) -> Result<(), MailError> {
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        manager(&mut transaction, workspace_id, actor_id).await?;
        if let Some(provider_id) = provider_id {
            let allowed: Option<String> = sqlx::query_scalar(
                "SELECT id FROM mail_providers WHERE id = $1 \
                 AND (workspace_id IS NULL OR workspace_id = $2)",
            )
            .bind(provider_id.to_string())
            .bind(workspace_id.to_string())
            .fetch_optional(&mut *transaction)
            .await?;
            if allowed.is_none() {
                return Err(MailError::Invalid {
                    field: "provider_id",
                });
            }
        }
        let updated = sqlx::query(
            "UPDATE mail_domains SET provider_id = $1, updated_at = $2 \
             WHERE id = $3 AND workspace_id = $4",
        )
        .bind(provider_id.map(|id| id.to_string()))
        .bind(now.as_millis())
        .bind(domain_id.to_string())
        .bind(workspace_id.to_string())
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if updated == 0 {
            return Err(MailError::NotFound);
        }
        record_mail_audit(
            &mut transaction,
            workspace_id,
            actor_id,
            "mail.domain_updated",
            "mail_domain",
            domain_id,
            request_id,
            json!({ "provider_id": provider_id.map(|id| id.to_string()) }),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }
}

fn clean_name(name: &str) -> Result<String, MailError> {
    let name = name.trim();
    let count = name.chars().count();
    if count == 0 || count > NAME_MAX_CHARS || name.chars().any(char::is_control) {
        return Err(MailError::Invalid { field: "name" });
    }
    Ok(name.to_owned())
}

fn given_complete(secrets: &ProviderSecrets) -> bool {
    let all = match secrets {
        ProviderSecrets::Cloudflare { .. } | ProviderSecrets::Mailgun { .. } => 1,
        _ => 2,
    };
    secrets.names_set().len() == all
}
