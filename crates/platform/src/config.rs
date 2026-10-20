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
const ENVIRONMENT_ENV: &str = "ORBIT_ENV";
const DATA_DATABASE_ENV: &str = "ORBIT_DATABASE_PATH";
const DATA_DATABASE_URL_ENV: &str = "ORBIT_DATABASE_URL";
const DATA_ATTACHMENTS_ENV: &str = "ORBIT_ATTACHMENTS_PATH";
const DATA_BACKUPS_ENV: &str = "ORBIT_BACKUPS_PATH";
const JOBS_CONCURRENCY_ENV: &str = "ORBIT_JOBS_CONCURRENCY";
const UPLOADS_MAX_FILE_BYTES_ENV: &str = "ORBIT_UPLOAD_MAX_FILE_BYTES";
const UPLOADS_MAX_REQUEST_BYTES_ENV: &str = "ORBIT_UPLOAD_MAX_REQUEST_BYTES";
const METRICS_LISTEN_ENV: &str = "ORBIT_METRICS_LISTEN";
const RATE_AUTH_ENV: &str = "ORBIT_RATE_LIMIT_AUTHENTICATION_PER_MINUTE";
const RATE_RECOVERY_ENV: &str = "ORBIT_RATE_LIMIT_RECOVERY_PER_MINUTE";
const RATE_INVITATION_ENV: &str = "ORBIT_RATE_LIMIT_INVITATION_PER_MINUTE";
const RATE_UPLOAD_ENV: &str = "ORBIT_RATE_LIMIT_UPLOAD_PER_MINUTE";
const RATE_GENERAL_ENV: &str = "ORBIT_RATE_LIMIT_GENERAL_PER_MINUTE";
const APP_KEY_ENV: &str = "ORBIT_APP_KEY";
const APP_KEY_FILE_ENV: &str = "ORBIT_APP_KEY_FILE";
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
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 8080,
            public_origin: "http://127.0.0.1:8080".to_owned(),
            trusted_proxies: Vec::new(),
            shutdown_delay_seconds: 0,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RateLimitConfig {
    pub authentication_per_minute: u32,
    pub recovery_per_minute: u32,
    pub invitation_per_minute: u32,
    pub upload_per_minute: u32,
    pub general_per_minute: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            authentication_per_minute: 30,
            recovery_per_minute: 10,
            invitation_per_minute: 60,
            upload_per_minute: 120,
            general_per_minute: 600,
        }
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
        let rate_limits = [
            self.rate_limits.authentication_per_minute,
            self.rate_limits.recovery_per_minute,
            self.rate_limits.invitation_per_minute,
            self.rate_limits.upload_per_minute,
            self.rate_limits.general_per_minute,
        ];
        if self.http.shutdown_delay_seconds > 300 {
            return Err(ConfigError::InvalidSetting {
                key: HTTP_SHUTDOWN_DELAY_ENV,
                detail: "the delay cannot exceed 300 seconds".to_owned(),
            });
        }
        if rate_limits.contains(&0) {
            return Err(ConfigError::InvalidSetting {
                key: "ORBIT_RATE_LIMIT_*",
                detail: "all endpoint-class limits must be positive".to_owned(),
            });
        }
        if rate_limits.into_iter().any(|limit| limit > 1_000_000) {
            return Err(ConfigError::InvalidSetting {
                key: "ORBIT_RATE_LIMIT_*",
                detail: "endpoint-class limits cannot exceed 1000000 per minute".to_owned(),
            });
        }
        Ok(())
    }
}

fn apply_environment(
    config: &mut Config,
    environment: &BTreeMap<String, String>,
) -> Result<(), ConfigError> {
    if environment.contains_key(APP_KEY_ENV) && environment.contains_key(APP_KEY_FILE_ENV) {
        return Err(ConfigError::AmbiguousSecret {
            key: APP_KEY_ENV.to_owned(),
        });
    }

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
        } else if key == RATE_AUTH_ENV {
            config.rate_limits.authentication_per_minute = parse_environment(key, value)?;
        } else if key == RATE_RECOVERY_ENV {
            config.rate_limits.recovery_per_minute = parse_environment(key, value)?;
        } else if key == RATE_INVITATION_ENV {
            config.rate_limits.invitation_per_minute = parse_environment(key, value)?;
        } else if key == RATE_UPLOAD_ENV {
            config.rate_limits.upload_per_minute = parse_environment(key, value)?;
        } else if key == RATE_GENERAL_ENV {
            config.rate_limits.general_per_minute = parse_environment(key, value)?;
        } else if key == APP_KEY_ENV {
            config.app_key = Some(parse_app_key(value)?);
        } else if key == APP_KEY_FILE_ENV {
            config.app_key = Some(parse_app_key(&read_secret_file(key, Path::new(value))?)?);
        } else if key.starts_with("ORBIT_")
            && !IGNORED_ENV_PREFIXES
                .iter()
                .any(|prefix| key.starts_with(prefix))
        {
            return Err(ConfigError::UnknownEnvironment(key.clone()));
        }
    }
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
