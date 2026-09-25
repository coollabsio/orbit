use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, File};
use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use axum::http::Uri;
use ipnet::IpNet;
use serde::Deserialize;
use thiserror::Error;

const MAX_SECRET_FILE_BYTES: u64 = 64 * 1024;
const HTTP_PORT_ENV: &str = "ORBIT__HTTP__PORT";
const HTTP_BIND_ENV: &str = "ORBIT__HTTP__BIND";
const HTTP_PUBLIC_ORIGIN_ENV: &str = "ORBIT__HTTP__PUBLIC_ORIGIN";
const HTTP_TRUSTED_PROXIES_ENV: &str = "ORBIT__HTTP__TRUSTED_PROXIES";
const ENVIRONMENT_ENV: &str = "ORBIT__ENVIRONMENT";
const DATA_DATABASE_ENV: &str = "ORBIT__DATA__DATABASE";
const DATA_ATTACHMENTS_ENV: &str = "ORBIT__DATA__ATTACHMENTS";
const DATA_BACKUPS_ENV: &str = "ORBIT__DATA__BACKUPS";
const JOBS_CONCURRENCY_ENV: &str = "ORBIT__JOBS__CONCURRENCY";
const UPLOADS_MAX_FILE_BYTES_ENV: &str = "ORBIT__UPLOADS__MAX_FILE_BYTES";
const UPLOADS_MAX_REQUEST_BYTES_ENV: &str = "ORBIT__UPLOADS__MAX_REQUEST_BYTES";
const METRICS_LISTEN_ENV: &str = "ORBIT__METRICS__LISTEN";
const RATE_AUTH_ENV: &str = "ORBIT__RATE_LIMITS__AUTHENTICATION_PER_MINUTE";
const RATE_RECOVERY_ENV: &str = "ORBIT__RATE_LIMITS__RECOVERY_PER_MINUTE";
const RATE_INVITATION_ENV: &str = "ORBIT__RATE_LIMITS__INVITATION_PER_MINUTE";
const RATE_UPLOAD_ENV: &str = "ORBIT__RATE_LIMITS__UPLOAD_PER_MINUTE";
const RATE_GENERAL_ENV: &str = "ORBIT__RATE_LIMITS__GENERAL_PER_MINUTE";
const SECRETS_ENV_PREFIX: &str = "ORBIT__SECRETS__";

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Config {
    pub environment: EnvironmentMode,
    pub http: HttpConfig,
    pub data: DataConfig,
    pub jobs: JobsConfig,
    pub uploads: UploadConfig,
    pub metrics: MetricsConfig,
    pub rate_limits: RateLimitConfig,
    pub secrets: BTreeMap<String, Secret>,
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
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 8080,
            public_origin: "http://127.0.0.1:8080".to_owned(),
            trusted_proxies: Vec::new(),
        }
    }
}

impl HttpConfig {
    #[must_use]
    pub const fn listen_addr(&self) -> SocketAddr {
        SocketAddr::new(self.bind, self.port)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataConfig {
    pub database: PathBuf,
    pub attachments: PathBuf,
    pub backups: PathBuf,
}

impl Default for DataConfig {
    fn default() -> Self {
        Self {
            database: PathBuf::from("data/orbit.sqlite"),
            attachments: PathBuf::from("data/attachments"),
            backups: PathBuf::from("backups"),
        }
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

#[derive(Clone, Eq, PartialEq)]
pub struct Secret(String);

impl Secret {
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

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ConfigOverride {
    pub http_port: Option<u16>,
}

#[derive(Clone, Eq, PartialEq)]
pub struct ConfigSources {
    pub path: PathBuf,
    pub env: BTreeMap<String, String>,
    pub cli: ConfigOverride,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read configuration file {path}: {source}")]
    ReadConfig {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid configuration file {path}")]
    ParseConfig { path: PathBuf },
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
    pub fn load(sources: ConfigSources) -> Result<Self, ConfigError> {
        let contents =
            fs::read_to_string(&sources.path).map_err(|source| ConfigError::ReadConfig {
                path: sources.path.clone(),
                source,
            })?;
        let raw: RawConfig = toml::from_str(&contents).map_err(|_| ConfigError::ParseConfig {
            path: sources.path.clone(),
        })?;
        let uses_default_origin = raw.http.public_origin.is_none();
        let mut config = raw.into_config()?;

        apply_environment(&mut config, &sources.env)?;
        if let Some(port) = sources.cli.http_port {
            config.http.port = port;
        }
        if uses_default_origin && !sources.env.contains_key(HTTP_PUBLIC_ORIGIN_ENV) {
            config.http.public_origin = default_public_origin(config.http.bind, config.http.port);
        }
        config.validate()?;
        Ok(config)
    }

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
                key: "http.public_origin",
                detail: "must be an absolute http or https origin without a path or query"
                    .to_owned(),
            });
        }
        match self.environment {
            EnvironmentMode::Unspecified => {
                return Err(ConfigError::InvalidSetting {
                    key: "environment",
                    detail: "must be explicitly set to development or production".to_owned(),
                });
            }
            EnvironmentMode::Development if !self.http.bind.is_loopback() => {
                return Err(ConfigError::InvalidSetting {
                    key: "http.bind",
                    detail: "development mode must bind to a loopback address".to_owned(),
                });
            }
            EnvironmentMode::Development if !self.http.trusted_proxies.is_empty() => {
                return Err(ConfigError::InvalidSetting {
                    key: "http.trusted_proxies",
                    detail: "development mode does not accept trusted proxies".to_owned(),
                });
            }
            EnvironmentMode::Production if !self.http.public_origin.starts_with("https://") => {
                return Err(ConfigError::InvalidSetting {
                    key: "http.public_origin",
                    detail: "production mode requires an https public origin".to_owned(),
                });
            }
            EnvironmentMode::Production if self.http.trusted_proxies.is_empty() => {
                return Err(ConfigError::InvalidSetting {
                    key: "http.trusted_proxies",
                    detail:
                        "production mode requires at least one trusted proxy for the HTTPS boundary"
                            .to_owned(),
                });
            }
            _ => {}
        }
        if self.jobs.concurrency == 0 {
            return Err(ConfigError::InvalidSetting {
                key: "jobs.concurrency",
                detail: "must be positive".to_owned(),
            });
        }
        if self.uploads.max_file_bytes == 0 {
            return Err(ConfigError::InvalidSetting {
                key: "uploads.max_file_bytes",
                detail: "must be positive".to_owned(),
            });
        }
        if self.uploads.max_request_bytes < self.uploads.max_file_bytes {
            return Err(ConfigError::InvalidSetting {
                key: "uploads.max_request_bytes",
                detail: "must be at least uploads.max_file_bytes".to_owned(),
            });
        }
        let rate_limits = [
            self.rate_limits.authentication_per_minute,
            self.rate_limits.recovery_per_minute,
            self.rate_limits.invitation_per_minute,
            self.rate_limits.upload_per_minute,
            self.rate_limits.general_per_minute,
        ];
        if rate_limits.contains(&0) {
            return Err(ConfigError::InvalidSetting {
                key: "rate_limits",
                detail: "all endpoint-class limits must be positive".to_owned(),
            });
        }
        if rate_limits.into_iter().any(|limit| limit > 1_000_000) {
            return Err(ConfigError::InvalidSetting {
                key: "rate_limits",
                detail: "endpoint-class limits cannot exceed 1000000 per minute".to_owned(),
            });
        }
        Ok(())
    }
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct RawConfig {
    environment: String,
    http: RawHttpConfig,
    data: RawDataConfig,
    jobs: RawJobsConfig,
    uploads: RawUploadConfig,
    metrics: RawMetricsConfig,
    rate_limits: RawRateLimitConfig,
    secrets: BTreeMap<String, String>,
}

impl RawConfig {
    fn into_config(self) -> Result<Config, ConfigError> {
        let bind = parse_setting("http.bind", &self.http.bind)?;
        let trusted_proxies = self
            .http
            .trusted_proxies
            .iter()
            .map(|value| parse_setting("http.trusted_proxies", value))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Config {
            environment: parse_setting("environment", &self.environment)?,
            http: HttpConfig {
                bind,
                port: self.http.port,
                public_origin: self
                    .http
                    .public_origin
                    .unwrap_or_else(|| default_public_origin(bind, self.http.port)),
                trusted_proxies,
            },
            data: DataConfig {
                database: self.data.database,
                attachments: self.data.attachments,
                backups: self.data.backups,
            },
            jobs: JobsConfig {
                concurrency: self.jobs.concurrency,
            },
            uploads: UploadConfig {
                max_file_bytes: self.uploads.max_file_bytes,
                max_request_bytes: self.uploads.max_request_bytes,
            },
            metrics: MetricsConfig {
                listen: self
                    .metrics
                    .listen
                    .as_deref()
                    .map(|value| parse_setting("metrics.listen", value))
                    .transpose()?,
            },
            rate_limits: RateLimitConfig {
                authentication_per_minute: self.rate_limits.authentication_per_minute,
                recovery_per_minute: self.rate_limits.recovery_per_minute,
                invitation_per_minute: self.rate_limits.invitation_per_minute,
                upload_per_minute: self.rate_limits.upload_per_minute,
                general_per_minute: self.rate_limits.general_per_minute,
            },
            secrets: self
                .secrets
                .into_iter()
                .map(|(name, value)| (name, Secret(value)))
                .collect(),
        })
    }
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawHttpConfig {
    bind: String,
    port: u16,
    public_origin: Option<String>,
    trusted_proxies: Vec<String>,
}

impl Default for RawHttpConfig {
    fn default() -> Self {
        let defaults = HttpConfig::default();
        Self {
            bind: defaults.bind.to_string(),
            port: defaults.port,
            public_origin: None,
            trusted_proxies: Vec::new(),
        }
    }
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawDataConfig {
    database: PathBuf,
    attachments: PathBuf,
    backups: PathBuf,
}

impl Default for RawDataConfig {
    fn default() -> Self {
        let defaults = DataConfig::default();
        Self {
            database: defaults.database,
            attachments: defaults.attachments,
            backups: defaults.backups,
        }
    }
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawJobsConfig {
    concurrency: usize,
}

impl Default for RawJobsConfig {
    fn default() -> Self {
        Self {
            concurrency: JobsConfig::default().concurrency,
        }
    }
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawUploadConfig {
    max_file_bytes: u64,
    max_request_bytes: u64,
}

impl Default for RawUploadConfig {
    fn default() -> Self {
        let defaults = UploadConfig::default();
        Self {
            max_file_bytes: defaults.max_file_bytes,
            max_request_bytes: defaults.max_request_bytes,
        }
    }
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct RawMetricsConfig {
    listen: Option<String>,
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawRateLimitConfig {
    authentication_per_minute: u32,
    recovery_per_minute: u32,
    invitation_per_minute: u32,
    upload_per_minute: u32,
    general_per_minute: u32,
}

impl Default for RawRateLimitConfig {
    fn default() -> Self {
        let defaults = RateLimitConfig::default();
        Self {
            authentication_per_minute: defaults.authentication_per_minute,
            recovery_per_minute: defaults.recovery_per_minute,
            invitation_per_minute: defaults.invitation_per_minute,
            upload_per_minute: defaults.upload_per_minute,
            general_per_minute: defaults.general_per_minute,
        }
    }
}

fn apply_environment(
    config: &mut Config,
    environment: &BTreeMap<String, String>,
) -> Result<(), ConfigError> {
    for key in environment.keys() {
        if let Some(secret_name) = key
            .strip_prefix(SECRETS_ENV_PREFIX)
            .and_then(|name| name.strip_suffix("_FILE"))
        {
            let direct_key = format!("{SECRETS_ENV_PREFIX}{secret_name}");
            if environment.contains_key(&direct_key) {
                return Err(ConfigError::AmbiguousSecret { key: direct_key });
            }
        }
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
        } else if key == DATA_DATABASE_ENV {
            config.data.database = PathBuf::from(value);
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
        } else if let Some(secret_name) = key.strip_prefix(SECRETS_ENV_PREFIX) {
            apply_secret(config, key, secret_name, value)?;
        } else if key.starts_with("ORBIT__") {
            return Err(ConfigError::UnknownEnvironment(key.clone()));
        }
    }
    Ok(())
}

fn parse_setting<T>(key: &'static str, value: &str) -> Result<T, ConfigError>
where
    T: std::str::FromStr,
{
    value.parse().map_err(|_| ConfigError::InvalidSetting {
        key,
        detail: format!("invalid value {value}"),
    })
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

fn apply_secret(
    config: &mut Config,
    key: &str,
    secret_name: &str,
    value: &str,
) -> Result<(), ConfigError> {
    if let Some(name) = secret_name.strip_suffix("_FILE") {
        if name.is_empty() {
            return Err(ConfigError::UnknownEnvironment(key.to_owned()));
        }
        let path = Path::new(value);
        let mut secret = read_secret_file(key, path)?;
        remove_one_trailing_line_ending(&mut secret);
        config
            .secrets
            .insert(name.to_ascii_lowercase(), Secret(secret));
    } else {
        if secret_name.is_empty() {
            return Err(ConfigError::UnknownEnvironment(key.to_owned()));
        }
        config
            .secrets
            .insert(secret_name.to_ascii_lowercase(), Secret(value.to_owned()));
    }
    Ok(())
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

fn remove_one_trailing_line_ending(value: &mut String) {
    if value.ends_with("\r\n") {
        value.truncate(value.len() - 2);
    } else if value.ends_with('\n') {
        value.pop();
    }
}
