//! DNS checks of mail domains (spec §6.1): the records Orbit asks an admin to publish, the
//! lookups, and the `mail.dns_check` job.
//!
//! Lookups go through [`DnsLookup`]: [`HickoryDns`] in the server (the hickory resolver that
//! `mail-auth` uses, so there is one version of it) and [`FakeDns`] in tests. A missing name and
//! a name without records of the type are both [`DnsError::NotFound`]; a timeout or another
//! failure makes the result `unknown`, which changes nothing.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::future::BoxFuture;
use orbit_mail::hickory_resolver::TokioResolver;
use orbit_mail::hickory_resolver::config::{NameServerConfig, ResolverConfig};
use orbit_mail::hickory_resolver::net::NetError;
use orbit_mail::hickory_resolver::net::runtime::TokioRuntimeProvider;
use orbit_mail::hickory_resolver::proto::rr::{Name, RData};
use orbit_platform::{Id, JobError, MailMode, TimestampMillis};
use serde_json::Value;
use thiserror::Error;

use crate::repositories::mail::{
    DnsReport, Domain, DomainStatus, MailError, MailRepository, RecordStatus,
};

/// Longest wait for one lookup, on top of the resolver's own timeouts.
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(15);
/// Largest string in a TXT record (RFC 1035 §3.3.14).
pub const TXT_STRING_MAX: usize = 255;
/// MX preference Orbit suggests.
pub const MX_PREFERENCE: u16 = 10;

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum DnsError {
    /// NXDOMAIN, or the name has no records of this type.
    #[error("no such record")]
    NotFound,
    #[error("the DNS lookup timed out")]
    Timeout,
    #[error("the DNS lookup failed: {0}")]
    Other(String),
}

pub trait DnsLookup: Send + Sync {
    /// The TXT records of `name`, each with its strings joined.
    fn txt<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Vec<String>, DnsError>>;
    /// The MX records of `name`: preference and exchange (lower case, no trailing dot).
    fn mx<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Vec<(u16, String)>, DnsError>>;
}

/// Lookups through hickory, without a cache, so **Check now** sees a record that was just added.
pub struct HickoryDns {
    resolver: Result<TokioResolver, String>,
}

impl HickoryDns {
    /// `resolver` is `ORBIT_MAIL_DNS_RESOLVER` (UDP with TCP fallback); `None` reads the system
    /// configuration. A resolver that cannot be set up fails each lookup instead of the server.
    #[must_use]
    pub fn new(resolver: Option<SocketAddr>) -> Self {
        let built = match resolver {
            Some(address) => {
                let mut server = NameServerConfig::udp_and_tcp(address.ip());
                for connection in &mut server.connections {
                    connection.port = address.port();
                }
                Ok(TokioResolver::builder_with_config(
                    ResolverConfig::from_name_servers(vec![server]),
                    TokioRuntimeProvider::default(),
                ))
            }
            None => TokioResolver::builder_tokio().map_err(|error| error.to_string()),
        }
        .and_then(|mut builder| {
            builder.options_mut().cache_size = 0;
            builder.build().map_err(|error| error.to_string())
        });
        if let Err(error) = &built {
            tracing::warn!(%error, "the DNS resolver for mail domain checks could not be set up");
        }
        Self { resolver: built }
    }

    fn resolver(&self) -> Result<&TokioResolver, DnsError> {
        self.resolver
            .as_ref()
            .map_err(|error| DnsError::Other(error.clone()))
    }
}

fn net_error(error: &NetError) -> DnsError {
    if error.is_no_records_found() {
        DnsError::NotFound
    } else if matches!(error, NetError::Timeout) {
        DnsError::Timeout
    } else {
        DnsError::Other(error.to_string())
    }
}

/// `name` as an absolute name, so the resolver's search domains are never appended. Relaxed
/// parsing keeps the `_` of `_dmarc` and `_orbit-verify`.
fn absolute(name: &str) -> Result<Name, DnsError> {
    let name = name.trim_end_matches('.');
    Name::from_str_relaxed(format!("{name}.")).map_err(|error| DnsError::Other(error.to_string()))
}

impl DnsLookup for HickoryDns {
    fn txt<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Vec<String>, DnsError>> {
        Box::pin(async move {
            let resolver = self.resolver()?;
            let lookup = tokio::time::timeout(LOOKUP_TIMEOUT, resolver.txt_lookup(absolute(name)?))
                .await
                .map_err(|_| DnsError::Timeout)?
                .map_err(|error| net_error(&error))?;
            Ok(lookup
                .answers()
                .iter()
                .filter_map(|record| match &record.data {
                    RData::TXT(txt) => Some(
                        txt.txt_data
                            .iter()
                            .map(|part| String::from_utf8_lossy(part).into_owned())
                            .collect::<String>(),
                    ),
                    _ => None,
                })
                .collect())
        })
    }

    fn mx<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Vec<(u16, String)>, DnsError>> {
        Box::pin(async move {
            let resolver = self.resolver()?;
            let lookup = tokio::time::timeout(LOOKUP_TIMEOUT, resolver.mx_lookup(absolute(name)?))
                .await
                .map_err(|_| DnsError::Timeout)?
                .map_err(|error| net_error(&error))?;
            Ok(lookup
                .answers()
                .iter()
                .filter_map(|record| match &record.data {
                    RData::MX(mx) => {
                        Some((mx.preference, host(&mx.exchange.to_lowercase().to_ascii())))
                    }
                    _ => None,
                })
                .collect())
        })
    }
}

/// Lower case, without a trailing dot.
fn host(name: &str) -> String {
    name.trim_end_matches('.').to_ascii_lowercase()
}

#[derive(Clone)]
enum Answer {
    Txt(Vec<String>),
    Mx(Vec<(u16, String)>),
    Fail(DnsError),
}

/// Answers from a table, for tests. A name that is not in the table is [`DnsError::NotFound`].
/// Clones share the table, so a test can change the records between checks.
#[derive(Clone, Default)]
pub struct FakeDns {
    answers: Arc<Mutex<HashMap<(bool, String), Answer>>>,
}

impl FakeDns {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn set(&self, mx: bool, name: &str, answer: Answer) {
        self.answers
            .lock()
            .expect("the fake DNS table is not poisoned")
            .insert((mx, host(name)), answer);
    }

    /// Replaces the TXT records of `name`.
    pub fn set_txt(&self, name: &str, records: &[&str]) {
        let records = records.iter().map(|record| (*record).to_owned()).collect();
        self.set(false, name, Answer::Txt(records));
    }

    /// Replaces the MX records of `name`.
    pub fn set_mx(&self, name: &str, records: &[(u16, &str)]) {
        let records = records
            .iter()
            .map(|(preference, exchange)| (*preference, host(exchange)))
            .collect();
        self.set(true, name, Answer::Mx(records));
    }

    /// Makes both lookups of `name` fail with `error`.
    pub fn fail(&self, name: &str, error: DnsError) {
        self.set(false, name, Answer::Fail(error.clone()));
        self.set(true, name, Answer::Fail(error));
    }

    /// Removes every record of `name`.
    pub fn remove(&self, name: &str) {
        let mut answers = self
            .answers
            .lock()
            .expect("the fake DNS table is not poisoned");
        answers.remove(&(false, host(name)));
        answers.remove(&(true, host(name)));
    }

    fn get(&self, mx: bool, name: &str) -> Option<Answer> {
        self.answers
            .lock()
            .expect("the fake DNS table is not poisoned")
            .get(&(mx, host(name)))
            .cloned()
    }
}

impl DnsLookup for FakeDns {
    fn txt<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Vec<String>, DnsError>> {
        Box::pin(async move {
            match self.get(false, name) {
                Some(Answer::Txt(records)) => Ok(records),
                Some(Answer::Fail(error)) => Err(error),
                Some(Answer::Mx(_)) | None => Err(DnsError::NotFound),
            }
        })
    }

    fn mx<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Vec<(u16, String)>, DnsError>> {
        Box::pin(async move {
            match self.get(true, name) {
                Some(Answer::Mx(records)) => Ok(records),
                Some(Answer::Fail(error)) => Err(error),
                Some(Answer::Txt(_)) | None => Err(DnsError::NotFound),
            }
        })
    }
}

/// Which record of the setup list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordKind {
    Verify,
    Mx,
    Dkim,
    Spf,
    Dmarc,
}

/// One record an admin publishes for a domain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpectedRecord {
    pub kind: RecordKind,
    /// `TXT` or `MX`.
    pub record_type: &'static str,
    /// The full name, without a trailing dot.
    pub name: String,
    /// The TXT text, or the MX exchange.
    pub value: String,
    /// MX only.
    pub priority: Option<u16>,
}

/// The TXT value at `_orbit-verify.<domain>`.
#[must_use]
pub fn verify_value(token: &str) -> String {
    format!("orbit-verify={token}")
}

/// The records of the setup page. The MX record is listed only when `hostname` is known
/// (`ORBIT_MAIL_HOSTNAME`); in relay mode it names the relay instead.
#[must_use]
pub fn expected_records(domain: &Domain, hostname: Option<&str>) -> Vec<ExpectedRecord> {
    let mut records = vec![ExpectedRecord {
        kind: RecordKind::Verify,
        record_type: "TXT",
        name: format!("_orbit-verify.{}", domain.name),
        value: verify_value(&domain.verify_token),
        priority: None,
    }];
    if let Some(hostname) = hostname {
        records.push(ExpectedRecord {
            kind: RecordKind::Mx,
            record_type: "MX",
            name: domain.name.clone(),
            value: host(hostname),
            priority: Some(MX_PREFERENCE),
        });
    }
    if let Some(dkim) = &domain.dkim {
        records.push(ExpectedRecord {
            kind: RecordKind::Dkim,
            record_type: "TXT",
            name: format!("{}._domainkey.{}", dkim.selector, domain.name),
            value: format!("v=DKIM1; k=rsa; p={}", dkim.public_key),
            priority: None,
        });
    }
    records.push(ExpectedRecord {
        kind: RecordKind::Spf,
        record_type: "TXT",
        name: domain.name.clone(),
        value: "v=spf1 mx ~all".to_owned(),
        priority: None,
    });
    records.push(ExpectedRecord {
        kind: RecordKind::Dmarc,
        record_type: "TXT",
        name: format!("_dmarc.{}", domain.name),
        value: "v=DMARC1; p=none".to_owned(),
        priority: None,
    });
    records
}

/// `value` cut into the strings of one TXT record: at most 255 bytes each, on character
/// boundaries. DNS panels that take one long value split it themselves; zone files need them.
#[must_use]
pub fn txt_strings(value: &str) -> Vec<String> {
    let mut strings = Vec::new();
    let mut current = String::new();
    for character in value.chars() {
        if current.len() + character.len_utf8() > TXT_STRING_MAX {
            strings.push(std::mem::take(&mut current));
        }
        current.push(character);
    }
    if !current.is_empty() || strings.is_empty() {
        strings.push(current);
    }
    strings
}

/// The value of tag `tag` in a `k=v; k=v` record (DKIM, DMARC), without spaces.
fn tag_value(record: &str, tag: &str) -> Option<String> {
    record.split(';').find_map(|part| {
        let (key, value) = part.split_once('=')?;
        key.trim()
            .eq_ignore_ascii_case(tag)
            .then(|| value.chars().filter(|c| !c.is_whitespace()).collect())
    })
}

/// Whether `record` starts with version tag `version` (`v=spf1`, `v=DMARC1`).
fn has_version(record: &str, version: &str) -> bool {
    let record = record.trim_start();
    record.len() >= version.len()
        && record[..version.len()].eq_ignore_ascii_case(version)
        && record[version.len()..]
            .chars()
            .next()
            .is_none_or(|next| next == ' ' || next == ';')
}

fn lookup_status<T>(result: Result<T, DnsError>) -> Result<T, RecordStatus> {
    match result {
        Ok(value) => Ok(value),
        Err(DnsError::NotFound) => Err(RecordStatus::Missing),
        Err(DnsError::Timeout | DnsError::Other(_)) => Err(RecordStatus::Unknown),
    }
}

fn verify_status(token: &str, result: Result<Vec<String>, DnsError>) -> RecordStatus {
    let records = match lookup_status(result) {
        Ok(records) => records,
        Err(status) => return status,
    };
    let expected = verify_value(token);
    if records
        .iter()
        .any(|record| record.trim() == expected || record.trim() == token)
    {
        RecordStatus::Ok
    } else if let Some(found) = records.first() {
        RecordStatus::wrong(found)
    } else {
        RecordStatus::Missing
    }
}

fn mx_status(
    hostname: Option<&str>,
    mode: MailMode,
    result: Result<Vec<(u16, String)>, DnsError>,
) -> RecordStatus {
    let records = match lookup_status(result) {
        Ok(records) => records,
        Err(status) => return status,
    };
    if records.is_empty() {
        return RecordStatus::Missing;
    }
    match (mode, hostname) {
        // Direct mode: senders must reach Orbit itself.
        (MailMode::Direct, Some(hostname)) => {
            let hostname = host(hostname);
            if records.iter().any(|(_, exchange)| *exchange == hostname) {
                RecordStatus::Ok
            } else {
                let found: Vec<String> = records
                    .iter()
                    .map(|(preference, exchange)| format!("{preference} {exchange}"))
                    .collect();
                RecordStatus::wrong(&found.join(", "))
            }
        }
        // Relay mode: the MX names the relay, which Orbit does not know.
        _ => RecordStatus::Ok,
    }
}

fn dkim_status(public_key: &str, result: Result<Vec<String>, DnsError>) -> RecordStatus {
    let records = match lookup_status(result) {
        Ok(records) => records,
        Err(status) => return status,
    };
    let keys: Vec<String> = records
        .iter()
        .filter_map(|record| tag_value(record, "p"))
        .collect();
    if keys.iter().any(|key| key == public_key) {
        RecordStatus::Ok
    } else if let Some(found) = records.first() {
        RecordStatus::wrong(found)
    } else {
        RecordStatus::Missing
    }
}

fn spf_status(result: Result<Vec<String>, DnsError>) -> RecordStatus {
    let records = match lookup_status(result) {
        Ok(records) => records,
        Err(status) => return status,
    };
    let spf: Vec<&String> = records
        .iter()
        .filter(|record| has_version(record, "v=spf1"))
        .collect();
    match spf.as_slice() {
        [] => RecordStatus::Missing,
        [_] => RecordStatus::Ok,
        // Two SPF records are a permanent error for every receiver (RFC 7208 §4.5).
        several => RecordStatus::wrong(
            &several
                .iter()
                .map(|record| record.as_str())
                .collect::<Vec<_>>()
                .join(" | "),
        ),
    }
}

fn dmarc_status(result: Result<Vec<String>, DnsError>) -> (RecordStatus, Option<String>) {
    let records = match lookup_status(result) {
        Ok(records) => records,
        Err(status) => return (status, None),
    };
    let dmarc: Vec<&String> = records
        .iter()
        .filter(|record| has_version(record, "v=DMARC1"))
        .collect();
    match dmarc.as_slice() {
        [] => (RecordStatus::Missing, None),
        [record] => match tag_value(record, "p").map(|policy| policy.to_ascii_lowercase()) {
            Some(policy) if matches!(policy.as_str(), "none" | "quarantine" | "reject") => {
                (RecordStatus::Ok, Some(policy))
            }
            _ => (RecordStatus::wrong(record), None),
        },
        several => (
            RecordStatus::wrong(
                &several
                    .iter()
                    .map(|record| record.as_str())
                    .collect::<Vec<_>>()
                    .join(" | "),
            ),
            None,
        ),
    }
}

/// Looks up every record of `domain` and compares it with what Orbit expects.
pub async fn inspect(
    dns: &dyn DnsLookup,
    domain: &Domain,
    hostname: Option<&str>,
    mode: MailMode,
) -> DnsReport {
    let verify_name = format!("_orbit-verify.{}", domain.name);
    let dkim_name = domain
        .dkim
        .as_ref()
        .map(|dkim| format!("{}._domainkey.{}", dkim.selector, domain.name));
    let dmarc_name = format!("_dmarc.{}", domain.name);
    let (verify, mx, spf, dmarc, dkim) = tokio::join!(
        dns.txt(&verify_name),
        dns.mx(&domain.name),
        dns.txt(&domain.name),
        dns.txt(&dmarc_name),
        async {
            match &dkim_name {
                Some(name) => Some(dns.txt(name).await),
                None => None,
            }
        },
    );
    let (dmarc, dmarc_policy) = dmarc_status(dmarc);
    DnsReport {
        verify: verify_status(&domain.verify_token, verify),
        mx: mx_status(hostname, mode, mx),
        dkim: match (&domain.dkim, dkim) {
            (Some(key), Some(result)) => dkim_status(&key.public_key, result),
            _ => RecordStatus::Missing,
        },
        spf: spf_status(spf),
        dmarc,
        dmarc_policy,
    }
}

/// Runs the DNS checks: the `mail.dns_check` job and its scheduled sweep.
#[derive(Clone)]
pub struct DnsChecker {
    repository: Arc<MailRepository>,
    dns: Arc<dyn DnsLookup>,
    hostname: Option<String>,
    mode: MailMode,
}

impl DnsChecker {
    #[must_use]
    pub fn new(
        repository: Arc<MailRepository>,
        dns: Arc<dyn DnsLookup>,
        hostname: Option<String>,
        mode: MailMode,
    ) -> Self {
        Self {
            repository,
            dns,
            hostname,
            mode,
        }
    }

    /// Checks one domain at `now` and stores the result. `None` when the domain is gone.
    pub async fn check_domain(
        &self,
        domain_id: Id,
        now: TimestampMillis,
    ) -> Result<Option<DomainStatus>, MailError> {
        let Some(domain) = self.repository.domain_for_check(domain_id).await? else {
            return Ok(None);
        };
        let report = inspect(
            self.dns.as_ref(),
            &domain,
            self.hostname.as_deref(),
            self.mode,
        )
        .await;
        self.repository
            .record_dns_report(domain_id, report, now)
            .await
    }

    /// Checks every domain that is due at `now`; returns how many were checked.
    pub async fn check_due(&self, now: TimestampMillis) -> Result<usize, MailError> {
        let due = self.repository.domains_due(now).await?;
        for domain_id in &due {
            self.check_domain(*domain_id, now).await?;
        }
        Ok(due.len())
    }

    /// The job handler. Payload `{"domain_id": …}` checks that domain (**Check now**); `{}` is
    /// the scheduled sweep.
    pub async fn run(&self, payload: &Value) -> Result<(), JobError> {
        let now = self
            .repository
            .database()
            .database_now()
            .await
            .map_err(|_| JobError::Retryable("mail DNS check: database time".to_owned()))?;
        let result = match payload.get("domain_id") {
            Some(Value::String(id)) => {
                let Ok(domain_id) = id.parse() else {
                    return Err(JobError::Permanent(
                        "invalid mail.dns_check payload".to_owned(),
                    ));
                };
                self.check_domain(domain_id, now).await.map(|_| ())
            }
            None | Some(Value::Null) => self.check_due(now).await.map(|_| ()),
            Some(_) => {
                return Err(JobError::Permanent(
                    "invalid mail.dns_check payload".to_owned(),
                ));
            }
        };
        result.map_err(|error| JobError::Retryable(format!("mail DNS check: {error}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn txt_strings_are_at_most_255_bytes() {
        let value = format!("v=DKIM1; k=rsa; p={}", "A".repeat(392));
        let strings = txt_strings(&value);
        assert_eq!(strings.len(), 2);
        assert!(strings.iter().all(|part| part.len() <= TXT_STRING_MAX));
        assert_eq!(strings.concat(), value);
        assert_eq!(txt_strings("short"), ["short"]);
        assert_eq!(txt_strings(""), [""]);
    }

    #[test]
    fn version_tags_and_values() {
        assert!(has_version("v=spf1 mx -all", "v=spf1"));
        assert!(has_version("V=SPF1", "v=spf1"));
        assert!(!has_version("v=spf10 mx", "v=spf1"));
        assert!(has_version("v=DMARC1;p=reject", "v=DMARC1"));
        assert_eq!(
            tag_value("v=DMARC1; p = Reject ; rua=mailto:x@y", "p").as_deref(),
            Some("Reject")
        );
        assert_eq!(
            tag_value("v=DKIM1; k=rsa; p=AB CD", "p").as_deref(),
            Some("ABCD")
        );
    }

    #[test]
    fn statuses_of_lookups() {
        assert_eq!(
            verify_status("t", Ok(vec!["orbit-verify=t".to_owned()])),
            RecordStatus::Ok
        );
        assert_eq!(
            verify_status("t", Err(DnsError::NotFound)),
            RecordStatus::Missing
        );
        assert_eq!(
            verify_status("t", Err(DnsError::Timeout)),
            RecordStatus::Unknown
        );
        assert_eq!(
            verify_status("t", Ok(vec!["orbit-verify=other".to_owned()])),
            RecordStatus::wrong("orbit-verify=other")
        );
        assert_eq!(
            spf_status(Ok(vec![
                "v=spf1 a -all".to_owned(),
                "v=spf1 mx -all".to_owned()
            ])),
            RecordStatus::wrong("v=spf1 a -all | v=spf1 mx -all")
        );
        assert_eq!(
            mx_status(
                Some("mx.orbit.test."),
                MailMode::Direct,
                Ok(vec![(10, "mx.orbit.test".to_owned())])
            ),
            RecordStatus::Ok
        );
        assert_eq!(
            mx_status(
                Some("mx.orbit.test"),
                MailMode::Direct,
                Ok(vec![(5, "aspmx.l.google.com".to_owned())])
            ),
            RecordStatus::wrong("5 aspmx.l.google.com")
        );
        assert_eq!(
            mx_status(
                Some("mx.orbit.test"),
                MailMode::Relay,
                Ok(vec![(5, "relay.example".to_owned())])
            ),
            RecordStatus::Ok
        );
    }
}
