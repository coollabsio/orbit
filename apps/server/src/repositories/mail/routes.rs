//! Send routes (spec §5.5, §6.3): the ways Orbit sends mail out. An instance route (no workspace)
//! belongs to the root user and every workspace may pick it; a workspace route belongs to one
//! workspace. Secrets are sealed with the app key (associated data `mail-route:<id>`) and never
//! leave the repository except to the sender (Task 8 and later).

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::str::FromStr;
use std::time::Duration;

use orbit_platform::{Flag, Id, Row, TimestampMillis};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::any::AnyRow;
use sqlx::{Any, Transaction};
use utoipa::ToSchema;

use super::domains::{manager, parse_id};
use super::providers::ProviderScope;
use super::{MailError, MailRepository, ProviderKind, ProviderSecrets, record_mail_audit};
use crate::audit::{self, AuditOutcome};
use crate::secret_box;

const NAME_MAX_CHARS: usize = 80;
/// The size limit of an SMTP relay, PostShiba or Direct sending (spec §5.9), and the most a route
/// of those kinds may be set to.
const DEFAULT_MAX_BYTES: i64 = 25_000_000;
const MAX_BYTES_CAP: i64 = 100_000_000;
const RESOLVE_TIMEOUT: Duration = Duration::from_secs(5);

/// How a route sends: from Orbit itself, through an SMTP relay or through a provider's API.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SendRouteKind {
    Direct,
    Smtp,
    Postshiba,
    Postmark,
    Resend,
    Mailgun,
    Ses,
    Cloudflare,
}

impl SendRouteKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Smtp => "smtp",
            Self::Postshiba => "postshiba",
            Self::Postmark => "postmark",
            Self::Resend => "resend",
            Self::Mailgun => "mailgun",
            Self::Ses => "ses",
            Self::Cloudflare => "cloudflare",
        }
    }
}

impl FromStr for SendRouteKind {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(match value {
            "direct" => Self::Direct,
            "smtp" => Self::Smtp,
            "postshiba" => Self::Postshiba,
            "postmark" => Self::Postmark,
            "resend" => Self::Resend,
            "mailgun" => Self::Mailgun,
            "ses" => Self::Ses,
            "cloudflare" => Self::Cloudflare,
            _ => return Err(()),
        })
    }
}

impl SendRouteKind {
    /// The route kind of a provider that offers **Also send through this provider**.
    #[must_use]
    pub const fn for_provider(kind: ProviderKind) -> Self {
        match kind {
            ProviderKind::Postshiba => Self::Postshiba,
            ProviderKind::Cloudflare => Self::Cloudflare,
            ProviderKind::Postmark => Self::Postmark,
            ProviderKind::Mailgun => Self::Mailgun,
            ProviderKind::Resend => Self::Resend,
            ProviderKind::Ses => Self::Ses,
        }
    }

    /// The size limit of a sent message in bytes (encoded), by kind (spec §5.9).
    #[must_use]
    pub const fn default_max_bytes(self) -> i64 {
        match self {
            Self::Postmark => 10_000_000,
            Self::Mailgun => 25_000_000,
            Self::Cloudflare => 26_214_400,
            Self::Ses => 40_000_000,
            Self::Direct | Self::Smtp | Self::Postshiba | Self::Resend => DEFAULT_MAX_BYTES,
        }
    }

    /// Kinds that connect to a host the route names.
    const fn has_host(self) -> bool {
        matches!(self, Self::Smtp | Self::Postshiba)
    }

    /// The names of the secrets of the kind.
    #[must_use]
    pub const fn secret_names(self) -> &'static [&'static str] {
        match self {
            Self::Direct => &[],
            Self::Smtp => &["username", "password"],
            Self::Mailgun => &["api_key", "webhook_signing_key"],
            Self::Ses => &["access_key_id", "secret_access_key"],
            Self::Cloudflare => &["api_token", "event_secret"],
            Self::Postmark => &["server_token", "webhook_user", "webhook_password"],
            Self::Resend => &["api_key", "webhook_secret"],
            Self::Postshiba => &["smtp_username", "smtp_password", "webhook_secret"],
        }
    }
}

/// The secrets of one route. `None`: a route without secrets (direct, or a relay without login).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RouteSecrets {
    None,
    Smtp {
        username: String,
        password: String,
    },
    Mailgun {
        api_key: String,
        webhook_signing_key: String,
    },
    Ses {
        access_key_id: String,
        secret_access_key: String,
    },
    Cloudflare {
        api_token: String,
        event_secret: String,
    },
    Postmark {
        server_token: String,
        webhook_user: String,
        webhook_password: String,
    },
    /// `webhook_secret`: the events webhook's signing secret. Optional: without it, the route
    /// sends but Orbit refuses its events (no bounce or complaint tracking).
    Resend {
        api_key: String,
        #[serde(default)]
        webhook_secret: Option<String>,
    },
    Postshiba {
        smtp_username: String,
        smtp_password: String,
        #[serde(default)]
        webhook_secret: Option<String>,
    },
}

impl RouteSecrets {
    /// Whether the secrets belong to a route of `kind`.
    #[must_use]
    pub const fn fits(&self, kind: SendRouteKind) -> bool {
        matches!(
            (self, kind),
            (Self::None, SendRouteKind::Direct | SendRouteKind::Smtp)
                | (Self::Smtp { .. }, SendRouteKind::Smtp)
                | (Self::Mailgun { .. }, SendRouteKind::Mailgun)
                | (Self::Ses { .. }, SendRouteKind::Ses)
                | (Self::Cloudflare { .. }, SendRouteKind::Cloudflare)
                | (Self::Postmark { .. }, SendRouteKind::Postmark)
                | (Self::Resend { .. }, SendRouteKind::Resend)
                | (Self::Postshiba { .. }, SendRouteKind::Postshiba)
        )
    }

    /// Every secret as (name, value); an optional secret without a value is left out.
    #[must_use]
    pub fn fields(&self) -> Vec<(&'static str, &str)> {
        match self {
            Self::None => Vec::new(),
            Self::Smtp { username, password } => {
                vec![("username", username), ("password", password)]
            }
            Self::Mailgun {
                api_key,
                webhook_signing_key,
            } => vec![
                ("api_key", api_key),
                ("webhook_signing_key", webhook_signing_key),
            ],
            Self::Ses {
                access_key_id,
                secret_access_key,
            } => vec![
                ("access_key_id", access_key_id),
                ("secret_access_key", secret_access_key),
            ],
            Self::Cloudflare {
                api_token,
                event_secret,
            } => vec![("api_token", api_token), ("event_secret", event_secret)],
            Self::Postmark {
                server_token,
                webhook_user,
                webhook_password,
            } => vec![
                ("server_token", server_token),
                ("webhook_user", webhook_user),
                ("webhook_password", webhook_password),
            ],
            Self::Resend {
                api_key,
                webhook_secret,
            } => {
                let mut fields = vec![("api_key", api_key.as_str())];
                fields.extend(
                    webhook_secret
                        .as_deref()
                        .map(|value| ("webhook_secret", value)),
                );
                fields
            }
            Self::Postshiba {
                smtp_username,
                smtp_password,
                webhook_secret,
            } => {
                let mut fields = vec![
                    ("smtp_username", smtp_username.as_str()),
                    ("smtp_password", smtp_password.as_str()),
                ];
                fields.extend(
                    webhook_secret
                        .as_deref()
                        .map(|value| ("webhook_secret", value)),
                );
                fields
            }
        }
    }

    /// The names of the secrets that have a value (what the API shows instead of the values).
    #[must_use]
    pub fn names_set(&self) -> Vec<&'static str> {
        self.fields()
            .into_iter()
            .filter(|(_, value)| !value.trim().is_empty())
            .map(|(name, _)| name)
            .collect()
    }

    /// The secrets of `kind` from named values. `None` when a required value is missing, or a name is not
    /// one of the kind's. An SMTP relay without login (both empty) gives [`RouteSecrets::None`].
    #[must_use]
    pub fn from_fields(kind: SendRouteKind, given: &BTreeMap<String, String>) -> Option<Self> {
        let names = kind.secret_names();
        if given.keys().any(|name| !names.contains(&name.as_str())) {
            return None;
        }
        let get = |name: &str| {
            given
                .get(name)
                .filter(|value| !value.trim().is_empty())
                .cloned()
        };
        Some(match kind {
            SendRouteKind::Direct => Self::None,
            SendRouteKind::Smtp => match (get("username"), get("password")) {
                (Some(username), Some(password)) => Self::Smtp { username, password },
                (None, None) => Self::None,
                _ => return None,
            },
            SendRouteKind::Mailgun => Self::Mailgun {
                api_key: get("api_key")?,
                webhook_signing_key: get("webhook_signing_key")?,
            },
            SendRouteKind::Ses => Self::Ses {
                access_key_id: get("access_key_id")?,
                secret_access_key: get("secret_access_key")?,
            },
            SendRouteKind::Cloudflare => Self::Cloudflare {
                api_token: get("api_token")?,
                event_secret: get("event_secret")?,
            },
            SendRouteKind::Postmark => Self::Postmark {
                server_token: get("server_token")?,
                webhook_user: get("webhook_user")?,
                webhook_password: get("webhook_password")?,
            },
            SendRouteKind::Resend => Self::Resend {
                api_key: get("api_key")?,
                webhook_secret: get("webhook_secret"),
            },
            SendRouteKind::Postshiba => Self::Postshiba {
                smtp_username: get("smtp_username")?,
                smtp_password: get("smtp_password")?,
                webhook_secret: get("webhook_secret"),
            },
        })
    }

    /// Whether every secret of the kind has a value.
    fn complete(&self) -> bool {
        match self {
            Self::None => true,
            other => other.names_set().len() == other.fields().len(),
        }
    }
}

/// What a provider already knows of a route's secrets (**Also send through this provider**).
/// Not the webhook secret of PostShiba or Resend: each of their webhook endpoints has its own
/// secret (the inbound one is not the events one), so the admin enters it for the route.
fn provider_prefill(secrets: &ProviderSecrets) -> Vec<(&'static str, String)> {
    match secrets {
        ProviderSecrets::Postshiba { .. } => Vec::new(),
        ProviderSecrets::Mailgun { signing_key } => {
            vec![("webhook_signing_key", signing_key.clone())]
        }
        ProviderSecrets::Ses {
            access_key_id,
            secret_access_key,
        } => vec![
            ("access_key_id", access_key_id.clone()),
            ("secret_access_key", secret_access_key.clone()),
        ],
        ProviderSecrets::Cloudflare { signing_secret } => {
            vec![("event_secret", signing_secret.clone())]
        }
        ProviderSecrets::Postmark {
            basic_user,
            basic_password,
        } => vec![
            ("webhook_user", basic_user.clone()),
            ("webhook_password", basic_password.clone()),
        ],
        ProviderSecrets::Resend { api_key, .. } => vec![("api_key", api_key.clone())],
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SmtpSecurity {
    /// TLS from the first byte (port 465).
    Tls,
    /// `STARTTLS`, required (port 587).
    #[default]
    Starttls,
    /// No TLS. Only a root route to a private address.
    None,
}

/// How to reach the route. Every field is optional on input; the repository fills the defaults.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct RouteSettings {
    /// SMTP relay and PostShiba: the server to connect to.
    pub host: Option<String>,
    pub port: Option<u16>,
    /// SMTP relay: how the connection is secured (default `starttls`).
    pub security: SmtpSecurity,
    /// Mailgun: `us` (default), `eu` or `au`. SES: the AWS region (required).
    pub region: Option<String>,
    /// Mailgun, instance routes only: another API base URL.
    pub base_url: Option<String>,
    /// Cloudflare: the account ID (required).
    pub account_id: Option<String>,
    /// Postmark: the message stream (default `outbound`).
    pub stream: Option<String>,
    /// The SPF include (`include:…`, or a whole `v=spf1 …` record) the domain's SPF record shows.
    pub spf: Option<String>,
    /// SES: the SNS topic (`arn:aws:sns:…`, or the `aws-cn`/`aws-us-gov` partition) the configuration set publishes bounces, complaints
    /// and deliveries to. Orbit takes events of this topic only (SNS signs every topic's
    /// messages); without it, SES events are refused.
    pub topic_arn: Option<String>,
    /// The size limit of a sent message in bytes; `0` or absent: the default of the kind.
    pub max_message_bytes: i64,
}

/// A route without its secret values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SendRoute {
    pub id: Id,
    pub workspace_id: Option<Id>,
    pub kind: SendRouteKind,
    pub name: String,
    pub provider_id: Option<Id>,
    pub settings: RouteSettings,
    pub secrets_set: Vec<&'static str>,
    pub last_sent_at: Option<TimestampMillis>,
    pub last_error: Option<String>,
    /// Domains (of every workspace) that use it as their own route.
    pub domain_count: i64,
    pub created_at: TimestampMillis,
}

#[derive(Clone, Debug)]
pub struct NewRoute {
    /// The id the client chose (so it could show the events URL before the route existed);
    /// `None` makes one.
    pub id: Option<Id>,
    pub kind: SendRouteKind,
    pub name: String,
    pub provider_id: Option<Id>,
    pub secrets: RouteSecrets,
    pub settings: RouteSettings,
}

#[derive(Clone, Debug, Default)]
pub struct RouteChange {
    pub name: Option<String>,
    /// Secrets to change by name: a non-empty value replaces the stored one, the others stay.
    pub secrets: Option<BTreeMap<String, String>>,
    pub settings: Option<RouteSettings>,
    /// Removes the username and password of an `smtp` route (a relay without login).
    pub clear_login: bool,
}

/// What the admin enters for a route made from a provider: only what the provider lacks.
#[derive(Clone, Debug, Default)]
pub struct ProviderRouteInput {
    pub name: Option<String>,
    pub secrets: BTreeMap<String, String>,
    pub settings: Option<RouteSettings>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RouteStepStatus {
    Ok,
    Failed,
    Skipped,
}

/// One step of **Send test**.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct RouteStep {
    pub name: &'static str,
    pub status: RouteStepStatus,
    pub detail: Option<String>,
}

/// What kind of address a host resolves to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostClass {
    Public,
    /// RFC 1918, the shared range 100.64.0.0/10 and unique local IPv6 (fc00::/7).
    Private,
    Loopback,
    /// Includes the cloud metadata address 169.254.169.254.
    LinkLocal,
    /// Unspecified, broadcast, multicast and reserved addresses: never a mail server.
    Unusable,
}

/// Classifies one address (IPv4-mapped IPv6 addresses by their IPv4 address).
#[must_use]
pub fn classify_ip(ip: IpAddr) -> HostClass {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, ..] = ip.octets();
            if ip.is_loopback() {
                HostClass::Loopback
            } else if ip.is_link_local() {
                HostClass::LinkLocal
            } else if ip.is_unspecified()
                || ip.is_broadcast()
                || ip.is_multicast()
                || a == 0
                || a >= 240
                || ip.octets()[..3] == [192, 0, 0]
                || (a == 198 && (b == 18 || b == 19))
            {
                HostClass::Unusable
            } else if ip.is_private() || (a == 100 && (64..128).contains(&b)) {
                HostClass::Private
            } else {
                HostClass::Public
            }
        }
        IpAddr::V6(ip) => {
            if let Some(mapped) = ip.to_ipv4_mapped() {
                return classify_ip(IpAddr::V4(mapped));
            }
            let segments = ip.segments();
            let first = segments[0];
            let compatible = segments[..6].iter().all(|part| *part == 0);
            let nat64 =
                first == 0x64 && segments[1] == 0xff9b && segments[2..6].iter().all(|p| *p == 0);
            if ip.is_loopback() {
                HostClass::Loopback
            } else if ip.is_unspecified()
                || ip.is_multicast()
                || compatible
                || nat64
                || first == 0x2002
            {
                // IPv4-compatible, NAT64 and 6to4 addresses wrap an IPv4 address of any kind
                HostClass::Unusable
            } else if first & 0xffc0 == 0xfe80 {
                HostClass::LinkLocal
            } else if first & 0xffc0 == 0xfec0 || first & 0xfe00 == 0xfc00 {
                HostClass::Private
            } else {
                HostClass::Public
            }
        }
    }
}

/// The addresses of `host` (an IP literal, or looked up). `None` when it does not resolve.
pub async fn resolve_host(host: &str, port: u16) -> Option<Vec<IpAddr>> {
    let literal = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = literal.parse::<IpAddr>() {
        return Some(vec![ip]);
    }
    let found = tokio::time::timeout(RESOLVE_TIMEOUT, tokio::net::lookup_host((host, port)))
        .await
        .ok()?
        .ok()?;
    let addresses: Vec<IpAddr> = found.map(|address| address.ip()).collect();
    (!addresses.is_empty()).then_some(addresses)
}

/// Whether the host is only reachable inside a private network: every address private or loopback.
#[must_use]
pub fn all_private(addresses: &[IpAddr]) -> bool {
    addresses
        .iter()
        .all(|ip| matches!(classify_ip(*ip), HostClass::Private | HostClass::Loopback))
}

/// Whether every address is a public one.
#[must_use]
pub fn all_public(addresses: &[IpAddr]) -> bool {
    addresses
        .iter()
        .all(|ip| classify_ip(*ip) == HostClass::Public)
}

const ROUTE_COLUMNS: &str = "r.id, r.workspace_id, r.kind, r.name, r.provider_id, r.secrets, \
    r.settings, r.last_sent_at, r.last_error, r.created_at, \
    (SELECT COUNT(*) FROM mail_domains d WHERE d.send_route_id = r.id) AS domain_count";

#[must_use]
pub fn route_aad(route_id: Id) -> String {
    format!("mail-route:{route_id}")
}

fn clean_name(name: &str) -> Result<String, MailError> {
    let name = name.trim();
    let count = name.chars().count();
    if count == 0 || count > NAME_MAX_CHARS || name.chars().any(char::is_control) {
        return Err(MailError::Invalid { field: "name" });
    }
    Ok(name.to_owned())
}

fn clean_text(value: Option<String>, field: &'static str) -> Result<Option<String>, MailError> {
    let value = value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    match value {
        Some(value) if value.chars().count() > 255 || value.chars().any(char::is_control) => {
            Err(MailError::Invalid { field })
        }
        other => Ok(other),
    }
}

/// The host checks of a route that connects by SMTP: what the scope may reach.
async fn check_host(
    scope: ProviderScope,
    host: &str,
    port: u16,
    security: SmtpSecurity,
) -> Result<(), MailError> {
    let root = matches!(scope, ProviderScope::Instance);
    let plain = security == SmtpSecurity::None;
    if plain && !root {
        return Err(MailError::Unprocessable("plain_smtp_refused"));
    }
    if root && !plain {
        return Ok(());
    }
    let addresses = resolve_host(host, port)
        .await
        .ok_or(MailError::Invalid { field: "host" })?;
    if plain {
        if all_private(&addresses) {
            Ok(())
        } else {
            Err(MailError::Unprocessable("plain_smtp_refused"))
        }
    } else if all_public(&addresses) {
        Ok(())
    } else {
        Err(MailError::Unprocessable("private_host_refused"))
    }
}

/// Settings with the defaults of the kind filled in, the checks of the scope made, and the fields
/// the kind does not use cleared.
async fn clean_settings(
    kind: SendRouteKind,
    scope: ProviderScope,
    settings: RouteSettings,
    hostname: Option<&str>,
) -> Result<RouteSettings, MailError> {
    let mut clean = RouteSettings {
        spf: clean_text(settings.spf, "spf")?,
        ..RouteSettings::default()
    };
    if kind == SendRouteKind::Direct && hostname.is_none_or(|host| host.trim().is_empty()) {
        return Err(MailError::Unprocessable("hostname_required"));
    }
    let cap = match kind {
        SendRouteKind::Direct | SendRouteKind::Smtp | SendRouteKind::Postshiba => MAX_BYTES_CAP,
        other => other.default_max_bytes(),
    };
    clean.max_message_bytes = match settings.max_message_bytes {
        0 => kind.default_max_bytes(),
        value if (1..=cap).contains(&value) => value,
        _ => {
            return Err(MailError::Invalid {
                field: "max_message_bytes",
            });
        }
    };
    if kind.has_host() {
        let host = clean_text(settings.host, "host")?;
        let host = match (host, kind) {
            (Some(host), _) => {
                let literal = host.trim_start_matches('[').trim_end_matches(']');
                if literal.parse::<IpAddr>().is_ok() {
                    Some(literal.to_owned())
                } else {
                    Some(
                        orbit_mail::normalize_domain(host.trim_end_matches('.'))
                            .map_err(|_| MailError::Invalid { field: "host" })?,
                    )
                }
            }
            (None, SendRouteKind::Smtp) => return Err(MailError::Invalid { field: "host" }),
            (None, _) => None,
        };
        // only an SMTP relay may be plain
        if kind != SendRouteKind::Smtp && settings.security == SmtpSecurity::None {
            return Err(MailError::Unprocessable("plain_smtp_refused"));
        }
        clean.security = settings.security;
        let port = settings.port.unwrap_or(match clean.security {
            SmtpSecurity::Tls => 465,
            SmtpSecurity::Starttls => 587,
            SmtpSecurity::None => 25,
        });
        if port == 0 {
            return Err(MailError::Invalid { field: "port" });
        }
        if let Some(host) = &host {
            check_host(scope, host, port, clean.security).await?;
        }
        clean.host = host;
        clean.port = Some(port);
    }
    match kind {
        SendRouteKind::Mailgun => {
            let region = clean_text(settings.region, "region")?
                .map(|region| region.to_ascii_lowercase())
                .unwrap_or_else(|| "us".to_owned());
            if !["us", "eu", "au"].contains(&region.as_str()) {
                return Err(MailError::Invalid { field: "region" });
            }
            clean.region = Some(region);
            clean.base_url = clean_base_url(scope, settings.base_url)?;
        }
        SendRouteKind::Ses => {
            let region = clean_text(settings.region, "region")?
                .map(|region| region.to_ascii_lowercase())
                .filter(|region| valid_aws_region(region))
                .ok_or(MailError::Invalid { field: "region" })?;
            clean.region = Some(region);
            clean.topic_arn = clean_text(settings.topic_arn, "topic_arn")?;
            if clean
                .topic_arn
                .as_deref()
                .is_some_and(|arn| !sns_topic_arn(arn))
            {
                return Err(MailError::Invalid { field: "topic_arn" });
            }
        }
        SendRouteKind::Cloudflare => {
            let account_id = clean_text(settings.account_id, "account_id")?
                .map(|id| id.to_ascii_lowercase())
                .filter(|id| id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()))
                .ok_or(MailError::Invalid {
                    field: "account_id",
                })?;
            clean.account_id = Some(account_id);
        }
        SendRouteKind::Postmark => {
            clean.stream = Some(
                clean_text(settings.stream, "stream")?.unwrap_or_else(|| "outbound".to_owned()),
            );
        }
        _ => {}
    }
    Ok(clean)
}

/// An SNS topic ARN of any AWS partition: `arn:aws:sns:…`, `arn:aws-cn:sns:…` or
/// `arn:aws-us-gov:sns:…`.
fn sns_topic_arn(arn: &str) -> bool {
    ["arn:aws:sns:", "arn:aws-cn:sns:", "arn:aws-us-gov:sns:"]
        .iter()
        .any(|prefix| arn.starts_with(prefix) && arn.len() > prefix.len())
}

/// An AWS region such as `us-east-1`: it goes into a host name, so nothing else passes.
fn valid_aws_region(region: &str) -> bool {
    let parts: Vec<&str> = region.split('-').collect();
    let [first, middle @ .., last] = parts.as_slice() else {
        return false;
    };
    !middle.is_empty()
        && first.len() == 2
        && first.bytes().all(|b| b.is_ascii_lowercase())
        && middle
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_lowercase()))
        && !last.is_empty()
        && last.bytes().all(|b| b.is_ascii_digit())
}

/// An API base URL: HTTPS, and only on a route of the root user (it makes the server call it).
fn clean_base_url(scope: ProviderScope, url: Option<String>) -> Result<Option<String>, MailError> {
    let Some(url) = clean_text(url, "base_url")? else {
        return Ok(None);
    };
    if !matches!(scope, ProviderScope::Instance) || !url.starts_with("https://") {
        return Err(MailError::Invalid { field: "base_url" });
    }
    Ok(Some(url.trim_end_matches('/').to_owned()))
}

fn to_json(settings: &RouteSettings) -> Result<String, MailError> {
    serde_json::to_string(settings).map_err(|_| MailError::Invalid { field: "settings" })
}

impl MailRepository {
    fn seal_route_secrets(
        &self,
        route_id: Id,
        secrets: &RouteSecrets,
    ) -> Result<Vec<u8>, MailError> {
        let json = serde_json::to_vec(secrets).map_err(|_| MailError::Crypto)?;
        secret_box::seal(&self.app_key, route_aad(route_id).as_bytes(), &json)
            .map_err(|()| MailError::Crypto)
    }

    fn open_route_secrets(&self, route_id: Id, sealed: &[u8]) -> Result<RouteSecrets, MailError> {
        let json = secret_box::open(&self.app_key, route_aad(route_id).as_bytes(), sealed)
            .map_err(|()| MailError::Crypto)?;
        serde_json::from_slice(&json).map_err(|_| MailError::Crypto)
    }

    fn route(&self, row: &AnyRow) -> Result<(SendRoute, RouteSecrets), MailError> {
        let id = parse_id(&row.try_get::<String, _>("id")?)?;
        let secrets = self.open_route_secrets(id, &row.try_get::<Vec<u8>, _>("secrets")?)?;
        let settings: String = row.try_get("settings")?;
        let route = SendRoute {
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
            provider_id: row
                .try_get::<Option<String>, _>("provider_id")?
                .map(|id| parse_id(&id))
                .transpose()?,
            settings: serde_json::from_str(&settings)
                .map_err(|_| MailError::Invalid { field: "settings" })?,
            secrets_set: secrets.names_set(),
            last_sent_at: row
                .try_get::<Option<i64>, _>("last_sent_at")?
                .map(TimestampMillis::from_millis),
            last_error: row.try_get("last_error")?,
            domain_count: row.try_get("domain_count")?,
            created_at: TimestampMillis::from_millis(row.try_get("created_at")?),
        };
        Ok((route, secrets))
    }

    /// Checks the caller for `scope` (owners and admins of the workspace; the instance scope is
    /// checked by the handler) and, for `creating`, the instance switch.
    async fn route_scope(
        transaction: &mut Transaction<'_, Any>,
        scope: ProviderScope,
        actor_id: Id,
        creating: bool,
    ) -> Result<(), MailError> {
        if let ProviderScope::Workspace(workspace_id) = scope {
            manager(transaction, workspace_id, actor_id).await?;
            if creating {
                let enabled: i64 = sqlx::query_scalar(
                    "SELECT mail_workspace_providers_enabled FROM instance_settings WHERE id = 1",
                )
                .fetch_one(&mut **transaction)
                .await?;
                if enabled != 1 {
                    return Err(MailError::Forbidden("workspace_providers_disabled"));
                }
            }
        }
        Ok(())
    }

    /// The same check before any DNS lookup, so a caller who may not create a route cannot make
    /// the server resolve names.
    async fn authorize_route(
        &self,
        scope: ProviderScope,
        actor_id: Id,
        creating: bool,
    ) -> Result<(), MailError> {
        let mut transaction = self.database.pool().begin().await?;
        Self::route_scope(&mut transaction, scope, actor_id, creating).await?;
        transaction.commit().await?;
        Ok(())
    }

    async fn load_route_in_scope(
        &self,
        transaction: &mut Transaction<'_, Any>,
        scope: ProviderScope,
        route_id: Id,
    ) -> Result<(SendRoute, RouteSecrets), MailError> {
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {ROUTE_COLUMNS} FROM mail_send_routes r \
             WHERE r.id = $1 AND r.workspace_id IS NOT DISTINCT FROM $2"
        )))
        .bind(route_id.to_string())
        .bind(workspace_of(scope).map(|id| id.to_string()))
        .fetch_optional(&mut **transaction)
        .await?
        .ok_or(MailError::NotFound)?;
        self.route(&row)
    }

    #[allow(clippy::too_many_arguments)]
    async fn route_audit(
        transaction: &mut Transaction<'_, Any>,
        scope: ProviderScope,
        actor_id: Id,
        action: &str,
        route_id: Id,
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
                    "mail_send_route",
                    route_id,
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
                    "mail_send_route",
                    Some(route_id),
                    request_id,
                    metadata,
                    now,
                )
                .await?;
                Ok(())
            }
        }
    }

    /// Makes a route. `hostname` is `ORBIT_MAIL_HOSTNAME`, which `direct` needs.
    pub async fn create_route(
        &self,
        scope: ProviderScope,
        actor_id: Id,
        input: NewRoute,
        hostname: Option<&str>,
        request_id: &str,
    ) -> Result<SendRoute, MailError> {
        let name = clean_name(&input.name)?;
        if !input.secrets.fits(input.kind) || !input.secrets.complete() {
            return Err(MailError::Invalid { field: "secrets" });
        }
        self.authorize_route(scope, actor_id, true).await?;
        let settings = clean_settings(input.kind, scope, input.settings, hostname).await?;
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        Self::route_scope(&mut transaction, scope, actor_id, true).await?;
        if let Some(provider_id) = input.provider_id {
            let kind: Option<String> = sqlx::query_scalar(
                "SELECT kind FROM mail_providers WHERE id = $1 \
                 AND workspace_id IS NOT DISTINCT FROM $2",
            )
            .bind(provider_id.to_string())
            .bind(workspace_of(scope).map(|id| id.to_string()))
            .fetch_optional(&mut *transaction)
            .await?;
            let provider_kind = kind.and_then(|kind| kind.parse::<ProviderKind>().ok());
            if provider_kind.map(SendRouteKind::for_provider) != Some(input.kind) {
                return Err(MailError::Invalid {
                    field: "provider_id",
                });
            }
        }
        let id = match input.id {
            Some(id) => {
                let taken: Flag = sqlx::query_scalar(
                    "SELECT EXISTS (SELECT 1 FROM mail_send_routes WHERE id = $1)",
                )
                .bind(id.to_string())
                .fetch_one(&mut *transaction)
                .await?;
                if taken.0 {
                    return Err(MailError::Invalid { field: "id" });
                }
                id
            }
            None => Id::new_v7(),
        };
        sqlx::query(
            "INSERT INTO mail_send_routes (id, workspace_id, kind, name, provider_id, secrets, \
             settings, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $8)",
        )
        .bind(id.to_string())
        .bind(workspace_of(scope).map(|id| id.to_string()))
        .bind(input.kind.as_str())
        .bind(&name)
        .bind(input.provider_id.map(|id| id.to_string()))
        .bind(self.seal_route_secrets(id, &input.secrets)?)
        .bind(to_json(&settings)?)
        .bind(now.as_millis())
        .execute(&mut *transaction)
        .await?;
        Self::route_audit(
            &mut transaction,
            scope,
            actor_id,
            "mail.send_route_created",
            id,
            request_id,
            json!({ "kind": input.kind.as_str(), "name": name }),
            now,
        )
        .await?;
        let (route, _) = self
            .load_route_in_scope(&mut transaction, scope, id)
            .await?;
        transaction.commit().await?;
        Ok(route)
    }

    /// **Also send through this provider**: a route with the provider's account. The provider's
    /// own secrets fill what they can; `input.secrets` supplies the rest (and may override).
    pub async fn create_route_for_provider(
        &self,
        scope: ProviderScope,
        actor_id: Id,
        provider_id: Id,
        input: ProviderRouteInput,
        hostname: Option<&str>,
        request_id: &str,
    ) -> Result<SendRoute, MailError> {
        self.authorize_route(scope, actor_id, true).await?;
        let (provider, provider_secrets) = self
            .load_provider(provider_id)
            .await?
            .filter(|(provider, _)| provider.workspace_id == workspace_of(scope))
            .ok_or(MailError::NotFound)?;
        let kind = SendRouteKind::for_provider(provider.kind);
        // A provider-made Cloudflare route signs with the provider's secret.
        if kind == SendRouteKind::Cloudflare
            && input
                .secrets
                .get("event_secret")
                .is_some_and(|value| !value.trim().is_empty())
        {
            return Err(MailError::Invalid { field: "secrets" });
        }
        let mut fields: BTreeMap<String, String> = provider_prefill(&provider_secrets)
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect();
        for (name, value) in input.secrets {
            if !value.trim().is_empty() {
                fields.insert(name, value);
            }
        }
        let secrets = RouteSecrets::from_fields(kind, &fields)
            .filter(RouteSecrets::complete)
            .ok_or(MailError::Invalid { field: "secrets" })?;
        let mut settings = input.settings.unwrap_or_default();
        if settings.region.is_none() {
            settings.region.clone_from(&provider.settings.region);
        }
        self.create_route(
            scope,
            actor_id,
            NewRoute {
                id: None,
                kind,
                name: input.name.unwrap_or(provider.name),
                provider_id: Some(provider_id),
                secrets,
                settings,
            },
            hostname,
            request_id,
        )
        .await
    }

    pub async fn update_route(
        &self,
        scope: ProviderScope,
        actor_id: Id,
        route_id: Id,
        change: RouteChange,
        hostname: Option<&str>,
        request_id: &str,
    ) -> Result<SendRoute, MailError> {
        self.authorize_route(scope, actor_id, false).await?;
        let current = {
            let mut transaction = self.database.pool().begin().await?;
            let loaded = self
                .load_route_in_scope(&mut transaction, scope, route_id)
                .await?;
            transaction.commit().await?;
            loaded
        };
        let (route, route_secrets) = current;
        let name = change
            .name
            .as_deref()
            .map(clean_name)
            .transpose()?
            .unwrap_or(route.name);
        if change.clear_login && (route.kind != SendRouteKind::Smtp || change.secrets.is_some()) {
            return Err(MailError::Invalid {
                field: "clear_login",
            });
        }
        let secrets = match change.secrets {
            _ if change.clear_login => RouteSecrets::None,
            Some(given) => {
                let mut merged: BTreeMap<String, String> = route_secrets
                    .fields()
                    .into_iter()
                    .map(|(name, value)| (name.to_owned(), value.to_owned()))
                    .collect();
                if given
                    .keys()
                    .any(|name| !route.kind.secret_names().contains(&name.as_str()))
                    || (route.kind == SendRouteKind::Cloudflare
                        && route.provider_id.is_some()
                        && given
                            .get("event_secret")
                            .is_some_and(|value| !value.trim().is_empty()))
                {
                    return Err(MailError::Invalid { field: "secrets" });
                }
                for (name, value) in given {
                    if !value.trim().is_empty() {
                        merged.insert(name, value);
                    }
                }
                RouteSecrets::from_fields(route.kind, &merged)
                    .filter(RouteSecrets::complete)
                    .ok_or(MailError::Invalid { field: "secrets" })?
            }
            None => route_secrets,
        };
        let settings = match change.settings {
            Some(settings) => clean_settings(route.kind, scope, settings, hostname).await?,
            None => route.settings,
        };
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        Self::route_scope(&mut transaction, scope, actor_id, false).await?;
        // gone, or moved to another scope, since the first read
        self.load_route_in_scope(&mut transaction, scope, route_id)
            .await?;
        sqlx::query(
            "UPDATE mail_send_routes SET name = $1, settings = $2, secrets = $3, updated_at = $4 \
             WHERE id = $5",
        )
        .bind(&name)
        .bind(to_json(&settings)?)
        .bind(self.seal_route_secrets(route_id, &secrets)?)
        .bind(now.as_millis())
        .bind(route_id.to_string())
        .execute(&mut *transaction)
        .await?;
        Self::route_audit(
            &mut transaction,
            scope,
            actor_id,
            "mail.send_route_updated",
            route_id,
            request_id,
            json!({ "name": name, "login_cleared": change.clear_login }),
            now,
        )
        .await?;
        let (route, _) = self
            .load_route_in_scope(&mut transaction, scope, route_id)
            .await?;
        transaction.commit().await?;
        Ok(route)
    }

    /// Refuses a route that a domain still uses (`Conflict("route_in_use")`) and the instance
    /// default (`Conflict("route_is_default")`): the root user clears or changes it first.
    pub async fn delete_route(
        &self,
        scope: ProviderScope,
        actor_id: Id,
        route_id: Id,
        request_id: &str,
    ) -> Result<(), MailError> {
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        Self::route_scope(&mut transaction, scope, actor_id, false).await?;
        let (route, _) = self
            .load_route_in_scope(&mut transaction, scope, route_id)
            .await?;
        if route.domain_count > 0 {
            return Err(MailError::Conflict("route_in_use"));
        }
        let default: Option<String> =
            sqlx::query_scalar("SELECT mail_send_route_id FROM instance_settings WHERE id = 1")
                .fetch_one(&mut *transaction)
                .await?;
        if default.as_deref() == Some(route_id.to_string().as_str()) {
            return Err(MailError::Conflict("route_is_default"));
        }
        sqlx::query("DELETE FROM mail_send_routes WHERE id = $1")
            .bind(route_id.to_string())
            .execute(&mut *transaction)
            .await?;
        Self::route_audit(
            &mut transaction,
            scope,
            actor_id,
            "mail.send_route_deleted",
            route_id,
            request_id,
            json!({ "kind": route.kind.as_str(), "name": route.name }),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// The routes of `scope` by name. A workspace sees the instance routes too (`workspace_id`
    /// tells them apart).
    pub async fn list_routes(
        &self,
        scope: ProviderScope,
        actor_id: Id,
    ) -> Result<Vec<SendRoute>, MailError> {
        let mut transaction = self.database.pool().begin().await?;
        Self::route_scope(&mut transaction, scope, actor_id, false).await?;
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {ROUTE_COLUMNS} FROM mail_send_routes r \
             WHERE r.workspace_id IS NOT DISTINCT FROM $1 OR ($1 IS NOT NULL AND r.workspace_id IS NULL) \
             ORDER BY r.workspace_id IS NULL, lower(r.name), r.id"
        )))
        .bind(workspace_of(scope).map(|id| id.to_string()))
        .fetch_all(&mut *transaction)
        .await?;
        transaction.commit().await?;
        rows.iter()
            .map(|row| self.route(row).map(|(route, _)| route))
            .collect()
    }

    /// One route of `scope`.
    pub async fn get_route(
        &self,
        scope: ProviderScope,
        actor_id: Id,
        route_id: Id,
    ) -> Result<SendRoute, MailError> {
        let mut transaction = self.database.pool().begin().await?;
        Self::route_scope(&mut transaction, scope, actor_id, false).await?;
        let (route, _) = self
            .load_route_in_scope(&mut transaction, scope, route_id)
            .await?;
        transaction.commit().await?;
        Ok(route)
    }

    /// A route with its secrets, for the sender (no caller to check).
    pub async fn load_route(
        &self,
        route_id: Id,
    ) -> Result<Option<(SendRoute, RouteSecrets)>, MailError> {
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {ROUTE_COLUMNS} FROM mail_send_routes r WHERE r.id = $1"
        )))
        .bind(route_id.to_string())
        .fetch_optional(self.database.pool())
        .await?;
        row.as_ref().map(|row| self.route(row)).transpose()
    }

    /// The route a domain sends through: its own, else the instance default.
    pub async fn route_for_domain(
        &self,
        domain_id: Id,
    ) -> Result<Option<(SendRoute, RouteSecrets)>, MailError> {
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {ROUTE_COLUMNS} FROM mail_domains d \
             JOIN mail_send_routes r ON r.id = COALESCE(d.send_route_id, \
             (SELECT s.mail_send_route_id FROM instance_settings s WHERE s.id = 1)) \
             WHERE d.id = $1"
        )))
        .bind(domain_id.to_string())
        .fetch_optional(self.database.pool())
        .await?;
        row.as_ref().map(|row| self.route(row)).transpose()
    }

    /// The last send: its time on success, its error otherwise.
    pub async fn record_route_result(
        &self,
        route_id: Id,
        now: TimestampMillis,
        error: Option<&str>,
    ) -> Result<(), MailError> {
        let query = match error {
            None => sqlx::query(
                "UPDATE mail_send_routes SET last_sent_at = $1, last_error = NULL WHERE id = $2",
            )
            .bind(now.as_millis()),
            Some(error) => sqlx::query("UPDATE mail_send_routes SET last_error = $1 WHERE id = $2")
                .bind(error.chars().take(500).collect::<String>()),
        };
        query
            .bind(route_id.to_string())
            .execute(self.database.pool())
            .await?;
        Ok(())
    }

    /// Sets the route of a domain of the workspace, or the instance default with `None`. An
    /// instance route or one of the workspace.
    pub async fn set_domain_send_route(
        &self,
        workspace_id: Id,
        actor_id: Id,
        domain_id: Id,
        route_id: Option<Id>,
        request_id: &str,
    ) -> Result<(), MailError> {
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        manager(&mut transaction, workspace_id, actor_id).await?;
        if let Some(route_id) = route_id {
            let allowed: Option<String> = sqlx::query_scalar(
                "SELECT id FROM mail_send_routes WHERE id = $1 \
                 AND (workspace_id IS NULL OR workspace_id = $2)",
            )
            .bind(route_id.to_string())
            .bind(workspace_id.to_string())
            .fetch_optional(&mut *transaction)
            .await?;
            if allowed.is_none() {
                return Err(MailError::Invalid { field: "route_id" });
            }
        }
        let updated = sqlx::query(
            "UPDATE mail_domains SET send_route_id = $1, updated_at = $2 \
             WHERE id = $3 AND workspace_id = $4",
        )
        .bind(route_id.map(|id| id.to_string()))
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
            json!({ "send_route_id": route_id.map(|id| id.to_string()) }),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// The instance default route (root user's choice), when there is one.
    pub async fn default_send_route(&self) -> Result<Option<Id>, MailError> {
        let id: Option<String> =
            sqlx::query_scalar("SELECT mail_send_route_id FROM instance_settings WHERE id = 1")
                .fetch_one(self.database.pool())
                .await?;
        id.map(|id| parse_id(&id)).transpose()
    }

    /// Sets the instance default route (an instance route), or clears it. The handler checks that
    /// the caller is the root user.
    pub async fn set_default_send_route(
        &self,
        actor_id: Id,
        route_id: Option<Id>,
        request_id: &str,
    ) -> Result<(), MailError> {
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        if let Some(route_id) = route_id {
            let found: Option<String> = sqlx::query_scalar(
                "SELECT id FROM mail_send_routes WHERE id = $1 AND workspace_id IS NULL",
            )
            .bind(route_id.to_string())
            .fetch_optional(&mut *transaction)
            .await?;
            if found.is_none() {
                return Err(MailError::Invalid { field: "route_id" });
            }
        }
        sqlx::query(
            "UPDATE instance_settings SET mail_send_route_id = $1, updated_at = $2 WHERE id = 1",
        )
        .bind(route_id.map(|id| id.to_string()))
        .bind(now.as_millis())
        .execute(&mut *transaction)
        .await?;
        audit::record_global(
            &mut transaction,
            Some(actor_id),
            "mail.send_default_updated",
            AuditOutcome::Success,
            "mail_send_route",
            route_id,
            request_id,
            json!({ "route_id": route_id.map(|id| id.to_string()) }),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// **Send test**: the checks that need no queue. Records a failure as the route's last error.
    /// `to` is the address the admin typed.
    pub async fn check_route(
        &self,
        scope: ProviderScope,
        actor_id: Id,
        route_id: Id,
        to: &str,
        hostname: Option<&str>,
    ) -> Result<Vec<RouteStep>, MailError> {
        let route = self.get_route(scope, actor_id, route_id).await?;
        let mut steps = Vec::new();
        let secrets_ok = match self.load_route(route_id).await? {
            Some((_, secrets)) => secrets.complete(),
            None => return Err(MailError::NotFound),
        };
        steps.push(RouteStep {
            name: "secrets",
            status: if secrets_ok {
                RouteStepStatus::Ok
            } else {
                RouteStepStatus::Failed
            },
            detail: (!secrets_ok).then(|| "A secret of the route is empty.".to_owned()),
        });
        let host_step = if route.kind == SendRouteKind::Direct {
            let named = hostname.is_some_and(|host| !host.trim().is_empty());
            RouteStep {
                name: "hostname",
                status: if named {
                    RouteStepStatus::Ok
                } else {
                    RouteStepStatus::Failed
                },
                detail: (!named).then(|| "ORBIT_MAIL_HOSTNAME is not set.".to_owned()),
            }
        } else if let (Some(host), Some(port)) = (&route.settings.host, route.settings.port) {
            match check_host(scope, host, port, route.settings.security).await {
                Ok(()) => RouteStep {
                    name: "host",
                    status: RouteStepStatus::Ok,
                    detail: None,
                },
                Err(MailError::Unprocessable(code)) => RouteStep {
                    name: "host",
                    status: RouteStepStatus::Failed,
                    detail: Some(code.to_owned()),
                },
                Err(_) => RouteStep {
                    name: "host",
                    status: RouteStepStatus::Failed,
                    detail: Some("The host does not resolve.".to_owned()),
                },
            }
        } else {
            RouteStep {
                name: "host",
                status: RouteStepStatus::Skipped,
                detail: Some("The provider's own servers are used.".to_owned()),
            }
        };
        steps.push(host_step);
        steps.push(RouteStep {
            name: "delivery",
            status: RouteStepStatus::Skipped,
            detail: Some(format!(
                "No message is sent to {to} yet: delivery arrives with the send queue."
            )),
        });
        let failed = steps
            .iter()
            .find(|step| step.status == RouteStepStatus::Failed);
        let error = failed.map(|step| {
            format!(
                "{}: {}",
                step.name,
                step.detail.as_deref().unwrap_or("failed")
            )
        });
        if let Some(error) = error {
            self.record_route_result(route_id, TimestampMillis::now(), Some(&error))
                .await?;
        }
        Ok(steps)
    }
}

const fn workspace_of(scope: ProviderScope) -> Option<Id> {
    match scope {
        ProviderScope::Instance => None,
        ProviderScope::Workspace(id) => Some(id),
    }
}
