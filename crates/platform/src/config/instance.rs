//! Instance settings from the environment: the mail server, the S3 bucket, the sign-in providers, open registration
//! and automatic backups. Without these variables, the root user saves them in Admin. A set variable replaces the
//! saved value, and Admin shows it read-only.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::{ConfigError, Secret, read_secret_file};
use crate::S3Config;

const OPEN_REGISTRATION: &str = "ORBIT_OPEN_REGISTRATION";
const AUTOMATIC_BACKUPS: &str = "ORBIT_AUTOMATIC_BACKUPS";
const SMTP_HOST: &str = "ORBIT_SMTP_HOST";
const SMTP_PORT: &str = "ORBIT_SMTP_PORT";
const SMTP_SECURITY: &str = "ORBIT_SMTP_SECURITY";
const SMTP_USERNAME: &str = "ORBIT_SMTP_USERNAME";
const SMTP_PASSWORD: &str = "ORBIT_SMTP_PASSWORD";
const SMTP_FROM: &str = "ORBIT_SMTP_FROM";
const SMTP_FROM_NAME: &str = "ORBIT_SMTP_FROM_NAME";
const S3_ENDPOINT: &str = "ORBIT_S3_ENDPOINT";
const S3_REGION: &str = "ORBIT_S3_REGION";
const S3_BUCKET: &str = "ORBIT_S3_BUCKET";
const S3_PREFIX: &str = "ORBIT_S3_PREFIX";
const S3_ACCESS_KEY_ID: &str = "ORBIT_S3_ACCESS_KEY_ID";
const S3_SECRET_ACCESS_KEY: &str = "ORBIT_S3_SECRET_ACCESS_KEY";
const S3_PATH_STYLE: &str = "ORBIT_S3_PATH_STYLE";
const S3_ATTACHMENTS: &str = "ORBIT_S3_ATTACHMENTS";
const S3_BACKUPS: &str = "ORBIT_S3_BACKUPS";
const GOOGLE_CLIENT_ID: &str = "ORBIT_GOOGLE_CLIENT_ID";
const GOOGLE_CLIENT_SECRET: &str = "ORBIT_GOOGLE_CLIENT_SECRET";
const GITHUB_CLIENT_ID: &str = "ORBIT_GITHUB_CLIENT_ID";
const GITHUB_CLIENT_SECRET: &str = "ORBIT_GITHUB_CLIENT_SECRET";

const SECRETS: [&str; 4] = [
    SMTP_PASSWORD,
    S3_SECRET_ACCESS_KEY,
    GOOGLE_CLIENT_SECRET,
    GITHUB_CLIENT_SECRET,
];
const SMTP_KEYS: [&str; 7] = [
    SMTP_HOST,
    SMTP_PORT,
    SMTP_SECURITY,
    SMTP_USERNAME,
    SMTP_PASSWORD,
    SMTP_FROM,
    SMTP_FROM_NAME,
];
const S3_KEYS: [&str; 7] = [
    S3_ENDPOINT,
    S3_REGION,
    S3_BUCKET,
    S3_PREFIX,
    S3_ACCESS_KEY_ID,
    S3_SECRET_ACCESS_KEY,
    S3_PATH_STYLE,
];
const OTHER_KEYS: [&str; 8] = [
    OPEN_REGISTRATION,
    AUTOMATIC_BACKUPS,
    S3_ATTACHMENTS,
    S3_BACKUPS,
    GOOGLE_CLIENT_ID,
    GOOGLE_CLIENT_SECRET,
    GITHUB_CLIENT_ID,
    GITHUB_CLIENT_SECRET,
];

/// The variable is read by [`InstanceConfig::from_environment`].
pub(super) fn is_instance_key(key: &str) -> bool {
    let key = key
        .strip_suffix("_FILE")
        .filter(|base| SECRETS.contains(base))
        .unwrap_or(key);
    SMTP_KEYS.contains(&key) || S3_KEYS.contains(&key) || OTHER_KEYS.contains(&key)
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum SmtpSecurity {
    /// TLS from the first byte (usually port 465).
    Tls,
    /// Plain connection upgraded with STARTTLS (usually port 587). Refuses servers without STARTTLS.
    Starttls,
    /// No encryption. Only for a mail relay on the same host or private network.
    None,
}

impl SmtpSecurity {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tls => "tls",
            Self::Starttls => "starttls",
            Self::None => "none",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "tls" => Some(Self::Tls),
            "starttls" => Some(Self::Starttls),
            "none" => Some(Self::None),
            _ => None,
        }
    }

    const fn default_port(self) -> u16 {
        match self {
            Self::Tls => 465,
            Self::Starttls => 587,
            Self::None => 25,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum BackupSchedule {
    #[default]
    Off,
    Hourly,
    Daily,
}

impl BackupSchedule {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Hourly => "hourly",
            Self::Daily => "daily",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "off" => Some(Self::Off),
            "hourly" => Some(Self::Hourly),
            "daily" => Some(Self::Daily),
            _ => None,
        }
    }
}

/// The mail server from `ORBIT_SMTP_*`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub security: SmtpSecurity,
    pub username: Option<String>,
    pub password: Option<Secret>,
    pub from_address: String,
    pub from_name: Option<String>,
}

/// A sign-in provider's OAuth client from `ORBIT_GOOGLE_*` or `ORBIT_GITHUB_*`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OAuthClientConfig {
    pub client_id: String,
    pub client_secret: Secret,
}

/// The instance settings that the environment sets. `None` keeps the value saved in Admin.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InstanceConfig {
    pub registration_open: Option<bool>,
    pub backup_schedule: Option<BackupSchedule>,
    pub smtp: Option<SmtpConfig>,
    pub s3: Option<S3Config>,
    pub attachments_in_s3: Option<bool>,
    pub backups_in_s3: Option<bool>,
    pub google: Option<OAuthClientConfig>,
    pub github: Option<OAuthClientConfig>,
}

impl InstanceConfig {
    /// The variables of one group (`ORBIT_SMTP_*`, `ORBIT_S3_*`, one provider) are set together.
    pub(super) fn from_environment(
        environment: &BTreeMap<String, String>,
    ) -> Result<Self, ConfigError> {
        let values = Values(environment);
        for key in SECRETS {
            if environment.contains_key(key) && environment.contains_key(&format!("{key}_FILE")) {
                return Err(ConfigError::AmbiguousSecret {
                    key: key.to_owned(),
                });
            }
        }
        let s3 = s3(&values)?;
        let attachments_in_s3 = values.flag(S3_ATTACHMENTS)?;
        let backups_in_s3 = values.flag(S3_BACKUPS)?;
        for (key, on) in [
            (S3_ATTACHMENTS, attachments_in_s3),
            (S3_BACKUPS, backups_in_s3),
        ] {
            if on == Some(true) && s3.is_none() {
                return Err(invalid(
                    key,
                    "needs the bucket in ORBIT_S3_ENDPOINT, ORBIT_S3_BUCKET, ORBIT_S3_ACCESS_KEY_ID and ORBIT_S3_SECRET_ACCESS_KEY",
                ));
            }
        }
        let config = Self {
            registration_open: values.flag(OPEN_REGISTRATION)?,
            backup_schedule: values
                .policy(AUTOMATIC_BACKUPS)
                .map(|value| {
                    BackupSchedule::parse(&value)
                        .ok_or_else(|| invalid(AUTOMATIC_BACKUPS, "must be off, hourly or daily"))
                })
                .transpose()?,
            smtp: smtp(&values)?,
            s3,
            attachments_in_s3,
            backups_in_s3,
            google: oauth_client(&values, GOOGLE_CLIENT_ID, GOOGLE_CLIENT_SECRET)?,
            github: oauth_client(&values, GITHUB_CLIENT_ID, GITHUB_CLIENT_SECRET)?,
        };
        if config.registration_open == Some(true)
            && config.smtp.is_none()
            && config.google.is_none()
            && config.github.is_none()
        {
            return Err(invalid(
                OPEN_REGISTRATION,
                "needs a way to confirm the email of a new account: set ORBIT_SMTP_* or a sign-in provider",
            ));
        }
        Ok(config)
    }

    /// The variables that are set, as `orbit config show` prints them. Secrets are redacted.
    #[must_use]
    pub fn redacted_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        let mut push = |key: &str, value: &str| lines.push(format!("{key}={value}"));
        if let Some(open) = self.registration_open {
            push(OPEN_REGISTRATION, &open.to_string());
        }
        if let Some(schedule) = self.backup_schedule {
            push(AUTOMATIC_BACKUPS, schedule.as_str());
        }
        if let Some(smtp) = &self.smtp {
            push(SMTP_HOST, &smtp.host);
            push(SMTP_PORT, &smtp.port.to_string());
            push(SMTP_SECURITY, smtp.security.as_str());
            if let Some(username) = &smtp.username {
                push(SMTP_USERNAME, username);
            }
            if smtp.password.is_some() {
                push(SMTP_PASSWORD, "[REDACTED]");
            }
            push(SMTP_FROM, &smtp.from_address);
            if let Some(name) = &smtp.from_name {
                push(SMTP_FROM_NAME, name);
            }
        }
        if let Some(s3) = &self.s3 {
            push(S3_ENDPOINT, &s3.endpoint);
            push(S3_REGION, &s3.region);
            push(S3_BUCKET, &s3.bucket);
            push(S3_PREFIX, &s3.prefix);
            push(S3_ACCESS_KEY_ID, &s3.access_key_id);
            push(S3_SECRET_ACCESS_KEY, "[REDACTED]");
            push(S3_PATH_STYLE, &s3.path_style.to_string());
        }
        if let Some(on) = self.attachments_in_s3 {
            push(S3_ATTACHMENTS, &on.to_string());
        }
        if let Some(on) = self.backups_in_s3 {
            push(S3_BACKUPS, &on.to_string());
        }
        for (client, id_key, secret_key) in [
            (&self.google, GOOGLE_CLIENT_ID, GOOGLE_CLIENT_SECRET),
            (&self.github, GITHUB_CLIENT_ID, GITHUB_CLIENT_SECRET),
        ] {
            if let Some(client) = client {
                push(id_key, &client.client_id);
                push(secret_key, "[REDACTED]");
            }
        }
        lines
    }
}

struct Values<'a>(&'a BTreeMap<String, String>);

impl Values<'_> {
    /// Trimmed; an empty value counts as not set.
    fn text(&self, key: &str) -> Option<String> {
        self.0
            .get(key)
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    }

    /// Trimmed; an empty value stays set, so its parser rejects it instead of keeping the saved value.
    fn policy(&self, key: &str) -> Option<String> {
        self.0.get(key).map(|value| value.trim().to_owned())
    }

    /// `KEY`, or the contents of the file in `KEY_FILE`.
    fn secret(&self, key: &str) -> Result<Option<String>, ConfigError> {
        let file_key = format!("{key}_FILE");
        match self.text(&file_key) {
            Some(path) => Ok(Some(
                read_secret_file(&file_key, Path::new(&path))?
                    .trim()
                    .to_owned(),
            )
            .filter(|value| !value.is_empty())),
            None => Ok(self.text(key)),
        }
    }

    fn flag(&self, key: &'static str) -> Result<Option<bool>, ConfigError> {
        self.policy(key)
            .map(|value| match value.to_ascii_lowercase().as_str() {
                "true" | "1" => Ok(true),
                "false" | "0" => Ok(false),
                _ => Err(invalid(key, "must be true or false")),
            })
            .transpose()
    }

    fn any(&self, keys: &[&str]) -> bool {
        keys.iter()
            .any(|key| self.0.contains_key(*key) || self.0.contains_key(&format!("{key}_FILE")))
    }
}

fn invalid(key: &'static str, detail: &str) -> ConfigError {
    ConfigError::InvalidSetting {
        key,
        detail: detail.to_owned(),
    }
}

fn required(values: &Values<'_>, key: &'static str, group: &str) -> Result<String, ConfigError> {
    values.text(key).ok_or_else(|| {
        invalid(
            key,
            &format!("is required with the other {group} variables"),
        )
    })
}

fn smtp(values: &Values<'_>) -> Result<Option<SmtpConfig>, ConfigError> {
    if !values.any(&SMTP_KEYS) {
        return Ok(None);
    }
    let host = required(values, SMTP_HOST, "ORBIT_SMTP_*")?;
    if host.len() > 253 || host.contains(char::is_whitespace) {
        return Err(invalid(SMTP_HOST, "must be the mail server's host name"));
    }
    let security = values
        .text(SMTP_SECURITY)
        .map(|value| {
            SmtpSecurity::parse(&value)
                .ok_or_else(|| invalid(SMTP_SECURITY, "must be tls, starttls or none"))
        })
        .transpose()?
        .unwrap_or(SmtpSecurity::Starttls);
    let port = values
        .text(SMTP_PORT)
        .map(|value| {
            value
                .parse::<u16>()
                .ok()
                .filter(|port| *port > 0)
                .ok_or_else(|| invalid(SMTP_PORT, "must be a port from 1 to 65535"))
        })
        .transpose()?
        .unwrap_or_else(|| security.default_port());
    let from_address = required(values, SMTP_FROM, "ORBIT_SMTP_*")?;
    if !valid_address(&from_address) {
        return Err(invalid(SMTP_FROM, "must be one email address"));
    }
    let from_name = values.text(SMTP_FROM_NAME);
    if from_name
        .as_ref()
        .is_some_and(|name| name.chars().count() > 120)
    {
        return Err(invalid(SMTP_FROM_NAME, "must be at most 120 characters"));
    }
    Ok(Some(SmtpConfig {
        host,
        port,
        security,
        username: values.text(SMTP_USERNAME),
        password: values.secret(SMTP_PASSWORD)?.map(Secret::new),
        from_address,
        from_name,
    }))
}

/// A rough check for `orbit config check`; the server checks the address fully when it starts.
fn valid_address(address: &str) -> bool {
    address.len() <= 320
        && !address.contains(char::is_whitespace)
        && address
            .rsplit_once('@')
            .is_some_and(|(local, domain)| !local.is_empty() && !domain.is_empty())
}

fn s3(values: &Values<'_>) -> Result<Option<S3Config>, ConfigError> {
    if !values.any(&S3_KEYS) {
        return Ok(None);
    }
    let group = "ORBIT_S3_*";
    let endpoint = required(values, S3_ENDPOINT, group)?
        .trim_end_matches('/')
        .to_owned();
    if !(endpoint.starts_with("https://") || endpoint.starts_with("http://"))
        || endpoint.len() > 2048
    {
        return Err(invalid(
            S3_ENDPOINT,
            "must be the http(s) URL of the S3 server",
        ));
    }
    let bucket = required(values, S3_BUCKET, group)?;
    if !(3..=63).contains(&bucket.len()) {
        return Err(invalid(
            S3_BUCKET,
            "must be a bucket name of 3 to 63 characters",
        ));
    }
    let region = values
        .text(S3_REGION)
        .unwrap_or_else(|| "us-east-1".to_owned());
    if region.len() > 64 {
        return Err(invalid(S3_REGION, "must be at most 64 characters"));
    }
    let prefix = values
        .text(S3_PREFIX)
        .unwrap_or_default()
        .trim_matches('/')
        .to_owned();
    if prefix.len() > 512
        || !prefix
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'))
        || prefix.split('/').any(|part| part == "." || part == "..")
    {
        return Err(invalid(
            S3_PREFIX,
            "may use letters, digits, '-', '_', '.' and '/' (at most 512)",
        ));
    }
    let access_key_id = required(values, S3_ACCESS_KEY_ID, group)?;
    let secret_access_key = values.secret(S3_SECRET_ACCESS_KEY)?.ok_or_else(|| {
        invalid(
            S3_SECRET_ACCESS_KEY,
            "is required with the other ORBIT_S3_* variables",
        )
    })?;
    Ok(Some(S3Config {
        endpoint,
        region,
        bucket,
        prefix,
        access_key_id,
        secret_access_key,
        path_style: values.flag(S3_PATH_STYLE)?.unwrap_or(false),
    }))
}

fn oauth_client(
    values: &Values<'_>,
    id_key: &'static str,
    secret_key: &'static str,
) -> Result<Option<OAuthClientConfig>, ConfigError> {
    if !values.any(&[id_key, secret_key]) {
        return Ok(None);
    }
    let client_id = values
        .text(id_key)
        .ok_or_else(|| invalid(id_key, &format!("is required with {secret_key}")))?;
    if client_id.len() > 256 {
        return Err(invalid(id_key, "must be at most 256 characters"));
    }
    let client_secret = values
        .secret(secret_key)?
        .ok_or_else(|| invalid(secret_key, &format!("is required with {id_key}")))?;
    if client_secret.len() > 512 {
        return Err(invalid(secret_key, "must be at most 512 characters"));
    }
    Ok(Some(OAuthClientConfig {
        client_id,
        client_secret: Secret::new(client_secret),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(pairs: &[(&str, &str)]) -> Result<InstanceConfig, ConfigError> {
        InstanceConfig::from_environment(
            &pairs
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect(),
        )
    }

    fn invalid_key(result: Result<InstanceConfig, ConfigError>) -> &'static str {
        match result {
            Err(ConfigError::InvalidSetting { key, .. }) => key,
            other => panic!("expected an invalid setting, got {other:?}"),
        }
    }

    #[test]
    fn nothing_set_keeps_every_saved_setting() {
        assert_eq!(parse(&[]).unwrap(), InstanceConfig::default());
    }

    #[test]
    fn smtp_defaults_to_starttls_on_587_and_reads_the_password_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("password");
        std::fs::write(&path, "hunter2\n").unwrap();
        let config = parse(&[
            ("ORBIT_SMTP_HOST", "smtp.example.com"),
            ("ORBIT_SMTP_FROM", "orbit@example.com"),
            ("ORBIT_SMTP_PASSWORD_FILE", path.to_str().unwrap()),
        ])
        .unwrap();
        let smtp = config.smtp.unwrap();
        assert_eq!(
            (
                smtp.port,
                smtp.security,
                smtp.password.as_ref().map(Secret::expose)
            ),
            (587, SmtpSecurity::Starttls, Some("hunter2"))
        );
        assert_eq!(
            parse(&[
                ("ORBIT_SMTP_HOST", "smtp.example.com"),
                ("ORBIT_SMTP_FROM", "orbit@example.com"),
                ("ORBIT_SMTP_SECURITY", "tls"),
            ])
            .unwrap()
            .smtp
            .unwrap()
            .port,
            465
        );
    }

    #[test]
    fn a_partial_group_names_the_missing_variable() {
        assert_eq!(
            invalid_key(parse(&[("ORBIT_SMTP_HOST", "smtp.example.com")])),
            SMTP_FROM
        );
        assert_eq!(
            invalid_key(parse(&[
                ("ORBIT_S3_ENDPOINT", "https://s3.example.com"),
                ("ORBIT_S3_BUCKET", "orbit-files"),
                ("ORBIT_S3_ACCESS_KEY_ID", "key"),
            ])),
            S3_SECRET_ACCESS_KEY
        );
        assert_eq!(
            invalid_key(parse(&[("ORBIT_GITHUB_CLIENT_ID", "id")])),
            GITHUB_CLIENT_SECRET
        );
    }

    #[test]
    fn s3_options_need_the_bucket_and_registration_needs_a_way_to_confirm_emails() {
        assert_eq!(
            invalid_key(parse(&[("ORBIT_S3_ATTACHMENTS", "true")])),
            S3_ATTACHMENTS
        );
        assert_eq!(
            parse(&[("ORBIT_S3_BACKUPS", "false")])
                .unwrap()
                .backups_in_s3,
            Some(false)
        );
        assert_eq!(
            invalid_key(parse(&[("ORBIT_OPEN_REGISTRATION", "true")])),
            OPEN_REGISTRATION
        );
        let config = parse(&[
            ("ORBIT_OPEN_REGISTRATION", "true"),
            ("ORBIT_GOOGLE_CLIENT_ID", "id"),
            ("ORBIT_GOOGLE_CLIENT_SECRET", "secret"),
            ("ORBIT_AUTOMATIC_BACKUPS", "daily"),
        ])
        .unwrap();
        assert_eq!(
            (config.registration_open, config.backup_schedule),
            (Some(true), Some(BackupSchedule::Daily))
        );
    }

    #[test]
    fn an_empty_flag_or_schedule_is_an_error() {
        for key in [
            OPEN_REGISTRATION,
            AUTOMATIC_BACKUPS,
            S3_ATTACHMENTS,
            S3_BACKUPS,
        ] {
            assert_eq!(invalid_key(parse(&[(key, " ")])), key);
        }
    }

    #[test]
    fn a_secret_and_its_file_together_are_ambiguous() {
        assert!(matches!(
            parse(&[
                ("ORBIT_S3_SECRET_ACCESS_KEY", "secret"),
                ("ORBIT_S3_SECRET_ACCESS_KEY_FILE", "/run/secrets/s3"),
            ]),
            Err(ConfigError::AmbiguousSecret { .. })
        ));
    }

    #[test]
    fn redacted_lines_hide_every_secret() {
        let config = parse(&[
            ("ORBIT_S3_ENDPOINT", "https://s3.example.com/"),
            ("ORBIT_S3_BUCKET", "orbit-files"),
            ("ORBIT_S3_ACCESS_KEY_ID", "key"),
            ("ORBIT_S3_SECRET_ACCESS_KEY", "very-secret"),
            ("ORBIT_S3_ATTACHMENTS", "true"),
        ])
        .unwrap();
        let lines = config.redacted_lines().join("\n");
        assert!(
            lines.contains("ORBIT_S3_ENDPOINT=https://s3.example.com\n"),
            "{lines}"
        );
        assert!(
            lines.contains("ORBIT_S3_SECRET_ACCESS_KEY=[REDACTED]"),
            "{lines}"
        );
        assert!(lines.contains("ORBIT_S3_ATTACHMENTS=true"), "{lines}");
        assert!(!lines.contains("very-secret"), "{lines}");
    }
}
