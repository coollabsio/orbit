use std::collections::BTreeMap;
use std::fmt;
use std::fs::File;
use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use axum::http::Uri;
use ipnet::IpNet;
use thiserror::Error;

const MAX_SECRET_FILE_BYTES: u64 = 64 * 1024;
const HTTP_PORT_ENV: &str = "ORBIT_PORT";
const HTTP_BIND_ENV: &str = "ORBIT_BIND";
const HTTP_PUBLIC_ORIGIN_ENV: &str = "ORBIT_PUBLIC_ORIGIN";
const HTTP_TRUSTED_PROXIES_ENV: &str = "ORBIT_TRUSTED_PROXIES";
const HTTP_SHUTDOWN_DELAY_ENV: &str = "ORBIT_SHUTDOWN_DELAY_SECONDS";
const HTTP_DEV_NATIVE_REDIRECTS_ENV: &str = "ORBIT_DEV_NATIVE_REDIRECTS";
const ENVIRONMENT_ENV: &str = "ORBIT_ENV";
const DATA_DATABASE_ENV: &str = "ORBIT_DATABASE_PATH";
const DATA_DATABASE_URL_ENV: &str = "ORBIT_DATABASE_URL";
const DATA_ATTACHMENTS_ENV: &str = "ORBIT_ATTACHMENTS_PATH";
const DATA_BACKUPS_ENV: &str = "ORBIT_BACKUPS_PATH";
const JOBS_CONCURRENCY_ENV: &str = "ORBIT_JOBS_CONCURRENCY";
const UPLOADS_MAX_FILE_BYTES_ENV: &str = "ORBIT_UPLOAD_MAX_FILE_BYTES";
const UPLOADS_MAX_REQUEST_BYTES_ENV: &str = "ORBIT_UPLOAD_MAX_REQUEST_BYTES";
const METRICS_LISTEN_ENV: &str = "ORBIT_METRICS_LISTEN";
const RATE_LIMITS_ENV_PREFIX: &str = "ORBIT_RATE_LIMIT_";
const APP_KEY_ENV: &str = "ORBIT_APP_KEY";
const APP_KEY_FILE_ENV: &str = "ORBIT_APP_KEY_FILE";
const CLOUD_ENV: &str = "ORBIT_CLOUD";
const STRIPE_SECRET_KEY_ENV: &str = "ORBIT_STRIPE_SECRET_KEY";
const STRIPE_SECRET_KEY_FILE_ENV: &str = "ORBIT_STRIPE_SECRET_KEY_FILE";
const STRIPE_WEBHOOK_SECRET_ENV: &str = "ORBIT_STRIPE_WEBHOOK_SECRET";
const STRIPE_WEBHOOK_SECRET_FILE_ENV: &str = "ORBIT_STRIPE_WEBHOOK_SECRET_FILE";
const STRIPE_PRICES_ENV: &str = "ORBIT_STRIPE_PRICES";
const TRIAL_DAYS_ENV: &str = "ORBIT_TRIAL_DAYS";
const TRIAL_TASKS_ENV: &str = "ORBIT_TRIAL_TASKS";
const TRIAL_PAGES_ENV: &str = "ORBIT_TRIAL_PAGES";
const TRIAL_MEMBERS_ENV: &str = "ORBIT_TRIAL_MEMBERS";
const STRIPE_API_BASE: &str = "https://api.stripe.com";
/// Variables that scripts and tests use, not the server.
const IGNORED_ENV_PREFIXES: [&str; 2] = ["ORBIT_DEV_", "ORBIT_TEST_"];

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Config {
    pub environment: EnvironmentMode,
    pub http: HttpConfig,
    pub data: DataConfig,
    pub jobs: JobsConfig,
    pub uploads: UploadConfig,
    pub metrics: MetricsConfig,
    pub rate_limits: RateLimitConfig,
    /// The key for secrets saved in the database (`ORBIT_APP_KEY`). Required: [`Config::validate`]
    /// refuses a configuration without it.
    pub app_key: Option<AppKey>,
    /// Stripe billing for Orbit Cloud (`ORBIT_CLOUD=true`); `None` on a self-hosted server.
    pub billing: Option<BillingConfig>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum EnvironmentMode {
    #[default]
    Unspecified,
    Development,
    Production,
}

impl FromStr for EnvironmentMode {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "development" => Ok(Self::Development),
            "production" => Ok(Self::Production),
            _ => Err(()),
        }
    }
}

impl EnvironmentMode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unspecified => "unspecified",
            Self::Development => "development",
            Self::Production => "production",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpConfig {
    pub bind: IpAddr,
    pub port: u16,
    pub public_origin: String,
    pub trusted_proxies: Vec<IpNet>,
    /// How long the server still serves after a stop signal while `/health/ready` reports 503,
    /// so a load balancer takes it out first.
    pub shutdown_delay_seconds: u64,
    /// Extra redirect URIs a native sign-in may return to, besides `orbit://auth/callback`
    /// (`ORBIT_DEV_NATIVE_REDIRECTS`, comma-separated), for Expo Go. Refused in production.
    pub dev_native_redirects: Vec<String>,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 8080,
            public_origin: "http://127.0.0.1:8080".to_owned(),
            trusted_proxies: Vec::new(),
            shutdown_delay_seconds: 0,
            dev_native_redirects: Vec::new(),
        }
    }
}

impl HttpConfig {
    #[must_use]
    pub const fn listen_addr(&self) -> SocketAddr {
        SocketAddr::new(self.bind, self.port)
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct DataConfig {
    /// The SQLite database; with `database_url` only the directory that holds `app.key`.
    pub database: PathBuf,
    /// A Postgres URL (`postgres://user:password@host/database`) used instead of SQLite.
    pub database_url: Option<String>,
    pub attachments: PathBuf,
    pub backups: PathBuf,
}

impl Default for DataConfig {
    fn default() -> Self {
        Self {
            database: PathBuf::from("data/orbit.sqlite"),
            database_url: None,
            attachments: PathBuf::from("data/attachments"),
            backups: PathBuf::from("backups"),
        }
    }
}

impl fmt::Debug for DataConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DataConfig")
            .field("database", &self.database)
            .field(
                "database_url",
                &self.database_url.as_deref().map(redact_database_url),
            )
            .field("attachments", &self.attachments)
            .field("backups", &self.backups)
            .finish()
    }
}

/// `url` with its password replaced, for logs and error messages.
#[must_use]
pub fn redact_database_url(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(mut parsed) => {
            if parsed.password().is_some() {
                let _ = parsed.set_password(Some("REDACTED"));
            }
            parsed.into()
        }
        Err(_) => "[invalid database URL]".to_owned(),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JobsConfig {
    pub concurrency: usize,
}

impl Default for JobsConfig {
    fn default() -> Self {
        Self { concurrency: 4 }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UploadConfig {
    pub max_file_bytes: u64,
    pub max_request_bytes: u64,
}

impl Default for UploadConfig {
    fn default() -> Self {
        Self {
            max_file_bytes: 25 * 1024 * 1024,
            max_request_bytes: 100 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MetricsConfig {
    pub listen: Option<SocketAddr>,
}

/// `ORBIT_RATE_LIMIT_<FIELD>`. The `*_per_minute` limits apply to each client IP address, each credential
/// (session or token) and, multiplied by `workspace_multiplier`, each workspace. The email limits
/// hold for every server of the database.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RateLimitConfig {
    pub authentication_per_minute: u32,
    pub recovery_per_minute: u32,
    pub invitation_per_minute: u32,
    pub upload_per_minute: u32,
    pub general_per_minute: u32,
    pub workspace_multiplier: u32,
    /// The time between two registration or password reset emails to one address.
    pub account_email_cooldown_seconds: u32,
    /// Registration and password reset emails to one address in 24 hours.
    pub account_emails_per_day: u32,
    pub invitation_emails_per_workspace_per_day: u32,
    pub invitation_emails_per_inviter_per_day: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            authentication_per_minute: 30,
            recovery_per_minute: 10,
            invitation_per_minute: 60,
            upload_per_minute: 120,
            general_per_minute: 600,
            workspace_multiplier: 10,
            account_email_cooldown_seconds: 60,
            account_emails_per_day: 5,
            invitation_emails_per_workspace_per_day: 100,
            invitation_emails_per_inviter_per_day: 50,
        }
    }
}

impl RateLimitConfig {
    /// The setting named `name` (the lowercase rest of its `ORBIT_RATE_LIMIT_` variable).
    fn setting_mut(&mut self, name: &str) -> Option<&mut u32> {
        Some(match name {
            "authentication_per_minute" => &mut self.authentication_per_minute,
            "recovery_per_minute" => &mut self.recovery_per_minute,
            "invitation_per_minute" => &mut self.invitation_per_minute,
            "upload_per_minute" => &mut self.upload_per_minute,
            "general_per_minute" => &mut self.general_per_minute,
            "workspace_multiplier" => &mut self.workspace_multiplier,
            "account_email_cooldown_seconds" => &mut self.account_email_cooldown_seconds,
            "account_emails_per_day" => &mut self.account_emails_per_day,
            "invitation_emails_per_workspace_per_day" => {
                &mut self.invitation_emails_per_workspace_per_day
            }
            "invitation_emails_per_inviter_per_day" => {
                &mut self.invitation_emails_per_inviter_per_day
            }
            _ => return None,
        })
    }

    fn validate(&self) -> Result<(), ConfigError> {
        let invalid = |key, detail: &str| {
            Err(ConfigError::InvalidSetting {
                key,
                detail: detail.to_owned(),
            })
        };
        let per_minute = [
            self.authentication_per_minute,
            self.recovery_per_minute,
            self.invitation_per_minute,
            self.upload_per_minute,
            self.general_per_minute,
        ];
        if per_minute.contains(&0) {
            return invalid(
                "ORBIT_RATE_LIMIT_*",
                "all endpoint-class limits must be positive",
            );
        }
        if per_minute.into_iter().any(|limit| limit > 1_000_000) {
            return invalid(
                "ORBIT_RATE_LIMIT_*",
                "endpoint-class limits cannot exceed 1000000 per minute",
            );
        }
        if !(1..=1_000).contains(&self.workspace_multiplier) {
            return invalid(
                "ORBIT_RATE_LIMIT_WORKSPACE_MULTIPLIER",
                "must be between 1 and 1000",
            );
        }
        if self.account_email_cooldown_seconds > 86_400 {
            return invalid(
                "ORBIT_RATE_LIMIT_ACCOUNT_EMAIL_COOLDOWN_SECONDS",
                "cannot exceed 86400 seconds",
            );
        }
        let per_day = [
            self.account_emails_per_day,
            self.invitation_emails_per_workspace_per_day,
            self.invitation_emails_per_inviter_per_day,
        ];
        if per_day
            .into_iter()
            .any(|limit| !(1..=100_000).contains(&limit))
        {
            return invalid(
                "ORBIT_RATE_LIMIT_*",
                "daily email limits must be between 1 and 100000",
            );
        }
        Ok(())
    }
}

/// The 32-byte key that encrypts secrets saved in the database. Set as base64 in `ORBIT_APP_KEY`.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct AppKey([u8; 32]);

impl AppKey {
    #[must_use]
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl FromStr for AppKey {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        use base64::Engine as _;

        base64::engine::general_purpose::STANDARD
            .decode(value.trim())
            .ok()
            .and_then(|bytes| <[u8; 32]>::try_from(bytes).ok())
            .map(Self)
            .ok_or(())
    }
}

impl fmt::Debug for AppKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

/// Orbit Cloud billing: every workspace needs a Stripe subscription to write.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BillingConfig {
    pub secret_key: Secret,
    pub webhook_secret: Secret,
    /// The plans offered, in `ORBIT_STRIPE_PRICES` order. Never empty, keys are unique.
    pub plans: Vec<Plan>,
    /// The Stripe API. Only tests change it.
    pub api_base: url::Url,
    pub trial: TrialConfig,
}

/// The trial of a workspace without a subscription: its length from the workspace's creation and
/// what it may hold (`ORBIT_TRIAL_*`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TrialConfig {
    /// 0 to 365; 0 means no trial: a new workspace is read-only until it subscribes.
    pub days: u32,
    /// Tasks not in the trash.
    pub tasks: i64,
    /// Pages not in the trash.
    pub pages: i64,
    /// Members and pending invitations, the owner included, so at least 1.
    pub members: i64,
}

impl Default for TrialConfig {
    fn default() -> Self {
        Self {
            days: 7,
            tasks: 50,
            pages: 20,
            members: 3,
        }
    }
}

impl TrialConfig {
    /// The trial length in milliseconds.
    #[must_use]
    pub const fn millis(&self) -> i64 {
        self.days as i64 * 24 * 60 * 60 * 1000
    }

    fn validate(&self) -> Result<(), ConfigError> {
        let invalid = |key, detail: &str| {
            Err(ConfigError::InvalidSetting {
                key,
                detail: detail.to_owned(),
            })
        };
        if self.days > 365 {
            return invalid(TRIAL_DAYS_ENV, "must be 0 to 365 days");
        }
        if self.tasks < 0 {
            return invalid(TRIAL_TASKS_ENV, "must be 0 or more");
        }
        if self.pages < 0 {
            return invalid(TRIAL_PAGES_ENV, "must be 0 or more");
        }
        if self.members < 1 {
            return invalid(TRIAL_MEMBERS_ENV, "must be 1 or more (the owner counts)");
        }
        Ok(())
    }
}

impl BillingConfig {
    #[must_use]
    pub fn new(secret_key: Secret, webhook_secret: Secret, plans: Vec<Plan>) -> Self {
        Self {
            secret_key,
            webhook_secret,
            plans,
            api_base: url::Url::parse(STRIPE_API_BASE).expect("the Stripe API base is a URL"),
            trial: TrialConfig::default(),
        }
    }

    #[must_use]
    pub fn plan(&self, key: &str) -> Option<&Plan> {
        self.plans.iter().find(|plan| plan.key == key)
    }

    #[must_use]
    pub fn plan_for_price(&self, price_id: &str) -> Option<&Plan> {
        self.plans.iter().find(|plan| plan.price_id == price_id)
    }

    fn validate(&self) -> Result<(), ConfigError> {
        let invalid = |detail: String| {
            Err(ConfigError::InvalidSetting {
                key: STRIPE_PRICES_ENV,
                detail,
            })
        };
        if self.plans.is_empty() {
            return invalid("must name at least one plan".to_owned());
        }
        for (index, plan) in self.plans.iter().enumerate() {
            if !valid_plan_key(&plan.key) {
                return invalid(format!(
                    "plan key {:?} must be 1 to 32 lowercase letters, digits or hyphens",
                    plan.key
                ));
            }
            if plan.price_id.is_empty() || plan.price_id.contains(char::is_whitespace) {
                return invalid(format!("plan {:?} needs a Stripe price id", plan.key));
            }
            if self.plans[..index]
                .iter()
                .any(|other| other.key == plan.key)
            {
                return invalid(format!("plan {:?} is listed twice", plan.key));
            }
            if self.plans[..index]
                .iter()
                .any(|other| other.price_id == plan.price_id)
            {
                return invalid(format!("price {:?} is listed twice", plan.price_id));
            }
        }
        if self.secret_key.expose().is_empty() {
            return Err(ConfigError::InvalidSetting {
                key: STRIPE_SECRET_KEY_ENV,
                detail: "must not be empty".to_owned(),
            });
        }
        if self.webhook_secret.expose().is_empty() {
            return Err(ConfigError::InvalidSetting {
                key: STRIPE_WEBHOOK_SECRET_ENV,
                detail: "must not be empty".to_owned(),
            });
        }
        self.trial.validate()
    }
}

/// A plan a workspace can subscribe to: its key in Orbit and its Stripe price.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Plan {
    pub key: String,
    pub price_id: String,
}

fn valid_plan_key(key: &str) -> bool {
    (1..=32).contains(&key.len())
        && key
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// A secret string (an API key) that `Debug` never prints.
#[derive(Clone, Eq, PartialEq)]
pub struct Secret(String);

impl Secret {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

/// The billing variables, collected before they are checked together.
#[derive(Default)]
struct BillingEnvironment {
    cloud: bool,
    secret_key: Option<String>,
    webhook_secret: Option<String>,
    prices: Option<String>,
    trial_days: Option<String>,
    trial_tasks: Option<String>,
    trial_pages: Option<String>,
    trial_members: Option<String>,
}

impl BillingEnvironment {
    fn into_config(self) -> Result<Option<BillingConfig>, ConfigError> {
        let missing = |key: &'static str| ConfigError::InvalidSetting {
            key,
            detail: "is required when ORBIT_CLOUD is true".to_owned(),
        };
        if !self.cloud {
            let set = [
                (STRIPE_SECRET_KEY_ENV, self.secret_key.is_some()),
                (STRIPE_WEBHOOK_SECRET_ENV, self.webhook_secret.is_some()),
                (STRIPE_PRICES_ENV, self.prices.is_some()),
                (TRIAL_DAYS_ENV, self.trial_days.is_some()),
                (TRIAL_TASKS_ENV, self.trial_tasks.is_some()),
                (TRIAL_PAGES_ENV, self.trial_pages.is_some()),
                (TRIAL_MEMBERS_ENV, self.trial_members.is_some()),
            ];
            return match set.into_iter().find(|(_, set)| *set) {
                Some((key, _)) => Err(ConfigError::InvalidSetting {
                    key,
                    detail: "is only used with ORBIT_CLOUD=true".to_owned(),
                }),
                None => Ok(None),
            };
        }
        let secret_key = self
            .secret_key
            .ok_or_else(|| missing(STRIPE_SECRET_KEY_ENV))?;
        let webhook_secret = self
            .webhook_secret
            .ok_or_else(|| missing(STRIPE_WEBHOOK_SECRET_ENV))?;
        let prices = self.prices.ok_or_else(|| missing(STRIPE_PRICES_ENV))?;
        let plans = prices
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .map(|entry| {
                let (key, price_id) =
                    entry
                        .split_once(':')
                        .ok_or_else(|| ConfigError::InvalidSetting {
                            key: STRIPE_PRICES_ENV,
                            detail: format!("{entry:?} must be plan:price_id"),
                        })?;
                Ok(Plan {
                    key: key.trim().to_owned(),
                    price_id: price_id.trim().to_owned(),
                })
            })
            .collect::<Result<Vec<_>, ConfigError>>()?;
        let defaults = TrialConfig::default();
        let trial = TrialConfig {
            days: parse_trial(TRIAL_DAYS_ENV, self.trial_days, defaults.days)?,
            tasks: parse_trial(TRIAL_TASKS_ENV, self.trial_tasks, defaults.tasks)?,
            pages: parse_trial(TRIAL_PAGES_ENV, self.trial_pages, defaults.pages)?,
            members: parse_trial(TRIAL_MEMBERS_ENV, self.trial_members, defaults.members)?,
        };
        Ok(Some(BillingConfig {
            trial,
            ..BillingConfig::new(
                Secret::new(secret_key.trim()),
                Secret::new(webhook_secret.trim()),
                plans,
            )
        }))
    }
}

fn parse_trial<T: std::str::FromStr>(
    key: &'static str,
    value: Option<String>,
    default: T,
) -> Result<T, ConfigError> {
    value.map_or(Ok(default), |value| {
        value
            .trim()
            .parse()
            .map_err(|_| ConfigError::InvalidSetting {
                key,
                detail: format!("{value:?} must be a whole number"),
            })
    })
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("invalid value for {key}: {value}")]
    InvalidEnvironment { key: String, value: String },
    #[error("invalid configuration setting {key}: {detail}")]
    InvalidSetting { key: &'static str, detail: String },
    #[error("unknown Orbit environment setting {0}")]
    UnknownEnvironment(String),
    #[error("both {key} and {key}_FILE are set")]
    AmbiguousSecret { key: String },
    #[error("failed to read secret for {key} from {path}: {source}")]
    ReadSecret {
        key: String,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("secret file for {key} is not a regular file: {path}")]
    SecretNotFile { key: String, path: PathBuf },
    #[error("secret file for {key} exceeds {MAX_SECRET_FILE_BYTES} bytes: {path}")]
    SecretTooLarge { key: String, path: PathBuf },
    #[error("secret file for {key} is not valid UTF-8: {path}")]
    SecretNotUtf8 { key: String, path: PathBuf },
}

impl Config {
    pub fn from_environment(env: BTreeMap<String, String>) -> Result<Self, ConfigError> {
        let mut config = Self::default();
        apply_environment(&mut config, &env)?;
        if !env.contains_key(HTTP_PUBLIC_ORIGIN_ENV) {
            config.http.public_origin = default_public_origin(config.http.bind, config.http.port);
        }
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        let valid_origin = self
            .http
            .public_origin
            .parse::<Uri>()
            .ok()
            .is_some_and(|origin| {
                matches!(origin.scheme_str(), Some("http" | "https"))
                    && origin.authority().is_some()
                    && origin.path() == "/"
                    && origin.query().is_none()
            });
        if !valid_origin {
            return Err(ConfigError::InvalidSetting {
                key: HTTP_PUBLIC_ORIGIN_ENV,
                detail: "must be an absolute http or https origin without a path or query"
                    .to_owned(),
            });
        }
        match self.environment {
            EnvironmentMode::Unspecified => {
                return Err(ConfigError::InvalidSetting {
                    key: ENVIRONMENT_ENV,
                    detail: "must be explicitly set to development or production".to_owned(),
                });
            }
            EnvironmentMode::Development if !self.http.bind.is_loopback() => {
                return Err(ConfigError::InvalidSetting {
                    key: HTTP_BIND_ENV,
                    detail: "development mode must bind to a loopback address".to_owned(),
                });
            }
            EnvironmentMode::Development if !self.http.trusted_proxies.is_empty() => {
                return Err(ConfigError::InvalidSetting {
                    key: HTTP_TRUSTED_PROXIES_ENV,
                    detail: "development mode does not accept trusted proxies".to_owned(),
                });
            }
            EnvironmentMode::Production if !self.http.public_origin.starts_with("https://") => {
                return Err(ConfigError::InvalidSetting {
                    key: HTTP_PUBLIC_ORIGIN_ENV,
                    detail: format!(
                        "production mode requires an https public origin, got {:?}",
                        self.http.public_origin
                    ),
                });
            }
            EnvironmentMode::Production if !self.http.dev_native_redirects.is_empty() => {
                return Err(ConfigError::InvalidSetting {
                    key: HTTP_DEV_NATIVE_REDIRECTS_ENV,
                    detail: "is for development only and cannot be set in production mode"
                        .to_owned(),
                });
            }
            EnvironmentMode::Production if self.http.trusted_proxies.is_empty() => {
                return Err(ConfigError::InvalidSetting {
                    key: HTTP_TRUSTED_PROXIES_ENV,
                    detail:
                        "production mode requires at least one trusted proxy for the HTTPS boundary"
                            .to_owned(),
                });
            }
            _ => {}
        }
        if let Some(url) = &self.data.database_url
            && !(url.starts_with("postgres://") || url.starts_with("postgresql://"))
        {
            return Err(ConfigError::InvalidSetting {
                key: DATA_DATABASE_URL_ENV,
                detail: "must be a postgres:// or postgresql:// URL".to_owned(),
            });
        }
        if self.app_key.is_none() {
            return Err(ConfigError::InvalidSetting {
                key: APP_KEY_ENV,
                detail: "is required: create it with `openssl rand -base64 32` and keep a copy \
                         outside the server, because saved secrets cannot be read without it"
                    .to_owned(),
            });
        }
        if self.jobs.concurrency == 0 {
            return Err(ConfigError::InvalidSetting {
                key: JOBS_CONCURRENCY_ENV,
                detail: "must be positive".to_owned(),
            });
        }
        if self.uploads.max_file_bytes == 0 {
            return Err(ConfigError::InvalidSetting {
                key: UPLOADS_MAX_FILE_BYTES_ENV,
                detail: "must be positive".to_owned(),
            });
        }
        if self.uploads.max_request_bytes < self.uploads.max_file_bytes {
            return Err(ConfigError::InvalidSetting {
                key: UPLOADS_MAX_REQUEST_BYTES_ENV,
                detail: "must be at least ORBIT_UPLOAD_MAX_FILE_BYTES".to_owned(),
            });
        }
        if self.http.shutdown_delay_seconds > 300 {
            return Err(ConfigError::InvalidSetting {
                key: HTTP_SHUTDOWN_DELAY_ENV,
                detail: "the delay cannot exceed 300 seconds".to_owned(),
            });
        }
        self.rate_limits.validate()?;
        match &self.billing {
            Some(billing) => billing.validate(),
            None => Ok(()),
        }
    }
}

fn apply_environment(
    config: &mut Config,
    environment: &BTreeMap<String, String>,
) -> Result<(), ConfigError> {
    for key in [
        APP_KEY_ENV,
        STRIPE_SECRET_KEY_ENV,
        STRIPE_WEBHOOK_SECRET_ENV,
    ] {
        if environment.contains_key(key) && environment.contains_key(&format!("{key}_FILE")) {
            return Err(ConfigError::AmbiguousSecret {
                key: key.to_owned(),
            });
        }
    }
    let mut billing = BillingEnvironment::default();

    for (key, value) in environment {
        if key == ENVIRONMENT_ENV {
            config.environment = parse_environment(key, value)?;
        } else if key == HTTP_PORT_ENV {
            config.http.port = value.parse().map_err(|_| ConfigError::InvalidEnvironment {
                key: key.clone(),
                value: value.clone(),
            })?;
        } else if key == HTTP_BIND_ENV {
            config.http.bind = parse_environment(key, value)?;
        } else if key == HTTP_PUBLIC_ORIGIN_ENV {
            config.http.public_origin = value.clone();
        } else if key == HTTP_TRUSTED_PROXIES_ENV {
            config.http.trusted_proxies = value
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| parse_environment(key, value))
                .collect::<Result<Vec<_>, _>>()?;
        } else if key == HTTP_DEV_NATIVE_REDIRECTS_ENV {
            config.http.dev_native_redirects = value
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .collect();
        } else if key == HTTP_SHUTDOWN_DELAY_ENV {
            config.http.shutdown_delay_seconds = parse_environment(key, value)?;
        } else if key == DATA_DATABASE_ENV {
            config.data.database = PathBuf::from(value);
        } else if key == DATA_DATABASE_URL_ENV {
            config.data.database_url = Some(value.clone()).filter(|value| !value.is_empty());
        } else if key == DATA_ATTACHMENTS_ENV {
            config.data.attachments = PathBuf::from(value);
        } else if key == DATA_BACKUPS_ENV {
            config.data.backups = PathBuf::from(value);
        } else if key == JOBS_CONCURRENCY_ENV {
            config.jobs.concurrency = parse_environment(key, value)?;
        } else if key == UPLOADS_MAX_FILE_BYTES_ENV {
            config.uploads.max_file_bytes = parse_environment(key, value)?;
        } else if key == UPLOADS_MAX_REQUEST_BYTES_ENV {
            config.uploads.max_request_bytes = parse_environment(key, value)?;
        } else if key == METRICS_LISTEN_ENV {
            config.metrics.listen = Some(parse_environment(key, value)?);
        } else if let Some(setting) = key
            .strip_prefix(RATE_LIMITS_ENV_PREFIX)
            .and_then(|name| config.rate_limits.setting_mut(&name.to_ascii_lowercase()))
        {
            *setting = parse_environment(key, value)?;
        } else if key == APP_KEY_ENV {
            config.app_key = Some(parse_app_key(value)?);
        } else if key == APP_KEY_FILE_ENV {
            config.app_key = Some(parse_app_key(&read_secret_file(key, Path::new(value))?)?);
        } else if key == CLOUD_ENV {
            billing.cloud = parse_environment(key, value)?;
        } else if key == STRIPE_SECRET_KEY_ENV {
            billing.secret_key = Some(value.clone());
        } else if key == STRIPE_SECRET_KEY_FILE_ENV {
            billing.secret_key = Some(read_secret_file(key, Path::new(value))?);
        } else if key == STRIPE_WEBHOOK_SECRET_ENV {
            billing.webhook_secret = Some(value.clone());
        } else if key == STRIPE_WEBHOOK_SECRET_FILE_ENV {
            billing.webhook_secret = Some(read_secret_file(key, Path::new(value))?);
        } else if key == STRIPE_PRICES_ENV {
            billing.prices = Some(value.clone());
        } else if key == TRIAL_DAYS_ENV {
            billing.trial_days = Some(value.clone());
        } else if key == TRIAL_TASKS_ENV {
            billing.trial_tasks = Some(value.clone());
        } else if key == TRIAL_PAGES_ENV {
            billing.trial_pages = Some(value.clone());
        } else if key == TRIAL_MEMBERS_ENV {
            billing.trial_members = Some(value.clone());
        } else if key.starts_with("ORBIT_")
            && !IGNORED_ENV_PREFIXES
                .iter()
                .any(|prefix| key.starts_with(prefix))
        {
            return Err(ConfigError::UnknownEnvironment(key.clone()));
        }
    }
    config.billing = billing.into_config()?;
    Ok(())
}

fn parse_environment<T>(key: &str, value: &str) -> Result<T, ConfigError>
where
    T: std::str::FromStr,
{
    value.parse().map_err(|_| ConfigError::InvalidEnvironment {
        key: key.to_owned(),
        value: value.to_owned(),
    })
}

fn display_ip(address: IpAddr) -> String {
    match address {
        IpAddr::V4(address) => address.to_string(),
        IpAddr::V6(address) => format!("[{address}]"),
    }
}

fn default_public_origin(bind: IpAddr, port: u16) -> String {
    format!("http://{}:{port}", display_ip(bind))
}

fn read_secret_file(key: &str, path: &Path) -> Result<String, ConfigError> {
    let file = File::open(path).map_err(|source| ConfigError::ReadSecret {
        key: key.to_owned(),
        path: path.to_owned(),
        source,
    })?;
    let metadata = file.metadata().map_err(|source| ConfigError::ReadSecret {
        key: key.to_owned(),
        path: path.to_owned(),
        source,
    })?;
    if !metadata.is_file() {
        return Err(ConfigError::SecretNotFile {
            key: key.to_owned(),
            path: path.to_owned(),
        });
    }
    if metadata.len() > MAX_SECRET_FILE_BYTES {
        return Err(ConfigError::SecretTooLarge {
            key: key.to_owned(),
            path: path.to_owned(),
        });
    }
    let mut bytes = Vec::new();
    file.take(MAX_SECRET_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| ConfigError::ReadSecret {
            key: key.to_owned(),
            path: path.to_owned(),
            source,
        })?;
    if bytes.len() as u64 > MAX_SECRET_FILE_BYTES {
        return Err(ConfigError::SecretTooLarge {
            key: key.to_owned(),
            path: path.to_owned(),
        });
    }
    String::from_utf8(bytes).map_err(|_| ConfigError::SecretNotUtf8 {
        key: key.to_owned(),
        path: path.to_owned(),
    })
}

/// Unlike [`parse_environment`], the error does not repeat the value: it is a secret.
fn parse_app_key(value: &str) -> Result<AppKey, ConfigError> {
    value.parse().map_err(|()| ConfigError::InvalidSetting {
        key: APP_KEY_ENV,
        detail: "must be base64 of 32 bytes (`openssl rand -base64 32`)".to_owned(),
    })
}
