use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

const MAX_SECRET_FILE_BYTES: u64 = 64 * 1024;
const HTTP_PORT_ENV: &str = "ORBIT__HTTP__PORT";
const SECRETS_ENV_PREFIX: &str = "ORBIT__SECRETS__";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub http: HttpConfig,
    pub secrets: BTreeMap<String, Secret>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpConfig {
    pub port: u16,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self { port: 8080 }
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
        let mut config = raw.into_config();

        apply_environment(&mut config, &sources.env)?;
        if let Some(port) = sources.cli.http_port {
            config.http.port = port;
        }

        Ok(config)
    }
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct RawConfig {
    http: RawHttpConfig,
    secrets: BTreeMap<String, String>,
}

impl RawConfig {
    fn into_config(self) -> Config {
        Config {
            http: HttpConfig {
                port: self.http.port,
            },
            secrets: self
                .secrets
                .into_iter()
                .map(|(name, value)| (name, Secret(value)))
                .collect(),
        }
    }
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawHttpConfig {
    port: u16,
}

impl Default for RawHttpConfig {
    fn default() -> Self {
        Self {
            port: HttpConfig::default().port,
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
        if key == HTTP_PORT_ENV {
            config.http.port = value.parse().map_err(|_| ConfigError::InvalidEnvironment {
                key: key.clone(),
                value: value.clone(),
            })?;
        } else if let Some(secret_name) = key.strip_prefix(SECRETS_ENV_PREFIX) {
            apply_secret(config, key, secret_name, value)?;
        } else if key.starts_with("ORBIT__") {
            return Err(ConfigError::UnknownEnvironment(key.clone()));
        }
    }
    Ok(())
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
