//! DNS checks of mail domains (spec §6.1): the records Orbit asks an admin to publish, the
//! lookups, and the `mail.dns_check` job.
//!
//! Lookups go through [`DnsLookup`]: [`HickoryDns`] in the server (the hickory resolver that
//! `mail-auth` uses, so there is one version of it) and [`FakeDns`] in tests. A missing name and
//! a name without records of the type are both [`DnsError::NotFound`]; a timeout or another
//! failure makes the result `unknown`, which changes nothing. [`ZoneLookup`] asks the zone's own
//! name servers as well as the resolver, so a negative answer that the resolver keeps in its cache
//! does not hide a record that was just published.
//!
//! [`check_ptr`] is the health card's reverse DNS check of `ORBIT_MAIL_HOSTNAME`.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{Arc, Mutex, Once};
use std::time::Duration;

use futures_util::future::{BoxFuture, join_all};
use orbit_mail::hickory_resolver::TokioResolver;
use orbit_mail::hickory_resolver::config::{NameServerConfig, ResolverConfig};
use orbit_mail::hickory_resolver::net::NetError;
use orbit_mail::hickory_resolver::net::runtime::TokioRuntimeProvider;
use orbit_mail::hickory_resolver::proto::rr::{Name, RData};
use orbit_platform::{Id, JobError, MailMode, TimestampMillis};
use serde::Serialize;
use serde_json::Value;
use thiserror::Error;
use utoipa::ToSchema;

use crate::repositories::mail::{
    DnsReport, Domain, DomainStatus, MailError, MailRepository, RecordStatus, SendRouteKind,
};

/// Longest wait for one lookup, on top of the resolver's own timeouts.
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(15);
/// Wait for one try of a lookup at a zone's own name servers: a firewall that blocks port 53 to
/// the internet makes these lookups time out, and the resolver's answer is used.
const AUTHORITATIVE_TIMEOUT: Duration = Duration::from_secs(5);
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
    /// The A records of `name`.
    fn a<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Vec<Ipv4Addr>, DnsError>>;
    /// The AAAA records of `name`.
    fn aaaa<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Vec<Ipv6Addr>, DnsError>>;
    /// The PTR names of `ip` (lower case, no trailing dot).
    fn ptr(&self, ip: IpAddr) -> BoxFuture<'_, Result<Vec<String>, DnsError>>;
    /// The NS hosts of `name` (lower case, no trailing dot).
    fn ns<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Vec<String>, DnsError>>;
    /// The same lookups sent to `servers` (a zone's own name servers, port 53) instead of the
    /// resolver. `None` when such a lookup cannot be set up.
    fn at_servers(&self, servers: &[IpAddr]) -> Option<Arc<dyn DnsLookup>>;
}

/// Lookups through hickory, without a cache, so **Check now** sees a record that was just added.
/// The resolver (the system's, or `ORBIT_MAIL_DNS_RESOLVER`) can still keep an old answer in its
/// own cache, so the checks of a mail domain go through [`ZoneLookup`], which also asks the
/// domain's own name servers ([`DnsLookup::at_servers`]).
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
    fn at_servers(&self, servers: &[IpAddr]) -> Option<Arc<dyn DnsLookup>> {
        let config = ResolverConfig::from_name_servers(
            servers
                .iter()
                .map(|ip| NameServerConfig::udp_and_tcp(*ip))
                .collect(),
        );
        let mut builder =
            TokioResolver::builder_with_config(config, TokioRuntimeProvider::default());
        let options = builder.options_mut();
        options.cache_size = 0;
        options.timeout = AUTHORITATIVE_TIMEOUT;
        options.attempts = 1;
        match builder.build() {
            Ok(resolver) => Some(Arc::new(Self {
                resolver: Ok(resolver),
            })),
            Err(error) => {
                static WARNED: Once = Once::new();
                WARNED.call_once(|| {
                    tracing::warn!(%error, "the DNS lookup at a zone's name servers could not be set up; mail domain checks use the resolver only");
                });
                None
            }
        }
    }

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

    fn a<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Vec<Ipv4Addr>, DnsError>> {
        Box::pin(async move {
            let resolver = self.resolver()?;
            let lookup =
                tokio::time::timeout(LOOKUP_TIMEOUT, resolver.ipv4_lookup(absolute(name)?))
                    .await
                    .map_err(|_| DnsError::Timeout)?
                    .map_err(|error| net_error(&error))?;
            Ok(lookup
                .answers()
                .iter()
                .filter_map(|record| match &record.data {
                    RData::A(a) => Some(a.0),
                    _ => None,
                })
                .collect())
        })
    }

    fn aaaa<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Vec<Ipv6Addr>, DnsError>> {
        Box::pin(async move {
            let resolver = self.resolver()?;
            let lookup =
                tokio::time::timeout(LOOKUP_TIMEOUT, resolver.ipv6_lookup(absolute(name)?))
                    .await
                    .map_err(|_| DnsError::Timeout)?
                    .map_err(|error| net_error(&error))?;
            Ok(lookup
                .answers()
                .iter()
                .filter_map(|record| match &record.data {
                    RData::AAAA(aaaa) => Some(aaaa.0),
                    _ => None,
                })
                .collect())
        })
    }

    fn ptr(&self, ip: IpAddr) -> BoxFuture<'_, Result<Vec<String>, DnsError>> {
        Box::pin(async move {
            let resolver = self.resolver()?;
            // `in-addr.arpa` / `ip6.arpa` name, absolute.
            let lookup = tokio::time::timeout(LOOKUP_TIMEOUT, resolver.reverse_lookup(ip))
                .await
                .map_err(|_| DnsError::Timeout)?
                .map_err(|error| net_error(&error))?;
            Ok(lookup
                .answers()
                .iter()
                .filter_map(|record| match &record.data {
                    RData::PTR(ptr) => Some(host(&ptr.0.to_lowercase().to_ascii())),
                    _ => None,
                })
                .collect())
        })
    }

    fn ns<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Vec<String>, DnsError>> {
        Box::pin(async move {
            let resolver = self.resolver()?;
            let lookup = tokio::time::timeout(LOOKUP_TIMEOUT, resolver.ns_lookup(absolute(name)?))
                .await
                .map_err(|_| DnsError::Timeout)?
                .map_err(|error| net_error(&error))?;
            Ok(lookup
                .answers()
                .iter()
                .filter_map(|record| match &record.data {
                    RData::NS(ns) => Some(host(&ns.0.to_lowercase().to_ascii())),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum RecordType {
    Txt,
    Mx,
    A,
    Aaaa,
    Ptr,
    Ns,
}

const RECORD_TYPES: [RecordType; 6] = [
    RecordType::Txt,
    RecordType::Mx,
    RecordType::A,
    RecordType::Aaaa,
    RecordType::Ptr,
    RecordType::Ns,
];

#[derive(Clone)]
enum Answer {
    Txt(Vec<String>),
    Mx(Vec<(u16, String)>),
    A(Vec<Ipv4Addr>),
    Aaaa(Vec<Ipv6Addr>),
    Ptr(Vec<String>),
    Ns(Vec<String>),
    Fail(DnsError),
}

/// Answers from a table, for tests. A name that is not in the table is [`DnsError::NotFound`].
/// Clones share the table, so a test can change the records between checks. PTR records are
/// kept under the address's text (`192.0.2.1`). [`set_authoritative`](Self::set_authoritative)
/// gives the answers of the zones' own name servers; without it there are none.
#[derive(Clone, Default)]
pub struct FakeDns {
    answers: Arc<Mutex<HashMap<(RecordType, String), Answer>>>,
    authoritative: Arc<Mutex<Option<FakeDns>>>,
}

impl FakeDns {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn table(&self) -> std::sync::MutexGuard<'_, HashMap<(RecordType, String), Answer>> {
        self.answers
            .lock()
            .expect("the fake DNS table is not poisoned")
    }

    fn set(&self, kind: RecordType, name: &str, answer: Answer) {
        self.table().insert((kind, host(name)), answer);
    }

    /// Replaces the TXT records of `name`.
    pub fn set_txt(&self, name: &str, records: &[&str]) {
        let records = records.iter().map(|record| (*record).to_owned()).collect();
        self.set(RecordType::Txt, name, Answer::Txt(records));
    }

    /// Replaces the MX records of `name`.
    pub fn set_mx(&self, name: &str, records: &[(u16, &str)]) {
        let records = records
            .iter()
            .map(|(preference, exchange)| (*preference, host(exchange)))
            .collect();
        self.set(RecordType::Mx, name, Answer::Mx(records));
    }

    /// Replaces the A records of `name`.
    pub fn set_a(&self, name: &str, records: &[Ipv4Addr]) {
        self.set(RecordType::A, name, Answer::A(records.to_vec()));
    }

    /// Replaces the AAAA records of `name`.
    pub fn set_aaaa(&self, name: &str, records: &[Ipv6Addr]) {
        self.set(RecordType::Aaaa, name, Answer::Aaaa(records.to_vec()));
    }

    /// Replaces the PTR records of `ip`.
    pub fn set_ptr(&self, ip: IpAddr, names: &[&str]) {
        let names = names.iter().map(|name| host(name)).collect();
        self.set(RecordType::Ptr, &ip.to_string(), Answer::Ptr(names));
    }

    /// Replaces the NS records of `name`.
    pub fn set_ns(&self, name: &str, hosts: &[&str]) {
        let hosts = hosts.iter().map(|name| host(name)).collect();
        self.set(RecordType::Ns, name, Answer::Ns(hosts));
    }

    /// Makes every lookup of `name` (or of the address `name`) fail with `error`.
    pub fn fail(&self, name: &str, error: DnsError) {
        for kind in RECORD_TYPES {
            self.set(kind, name, Answer::Fail(error.clone()));
        }
    }

    /// Removes every record of `name`.
    pub fn remove(&self, name: &str) {
        let mut answers = self.table();
        for kind in RECORD_TYPES {
            answers.remove(&(kind, host(name)));
        }
    }

    fn get(&self, kind: RecordType, name: &str) -> Option<Answer> {
        self.table().get(&(kind, host(name))).cloned()
    }

    /// Makes [`DnsLookup::at_servers`] answer from `dns`, whatever the servers.
    pub fn set_authoritative(&self, dns: FakeDns) {
        *self
            .authoritative
            .lock()
            .expect("the fake DNS table is not poisoned") = Some(dns);
    }
}

impl DnsLookup for FakeDns {
    fn at_servers(&self, _servers: &[IpAddr]) -> Option<Arc<dyn DnsLookup>> {
        let authoritative = self
            .authoritative
            .lock()
            .expect("the fake DNS table is not poisoned")
            .clone();
        authoritative.map(|dns| Arc::new(dns) as Arc<dyn DnsLookup>)
    }

    fn txt<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Vec<String>, DnsError>> {
        Box::pin(async move {
            match self.get(RecordType::Txt, name) {
                Some(Answer::Txt(records)) => Ok(records),
                Some(Answer::Fail(error)) => Err(error),
                _ => Err(DnsError::NotFound),
            }
        })
    }

    fn mx<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Vec<(u16, String)>, DnsError>> {
        Box::pin(async move {
            match self.get(RecordType::Mx, name) {
                Some(Answer::Mx(records)) => Ok(records),
                Some(Answer::Fail(error)) => Err(error),
                _ => Err(DnsError::NotFound),
            }
        })
    }

    fn a<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Vec<Ipv4Addr>, DnsError>> {
        Box::pin(async move {
            match self.get(RecordType::A, name) {
                Some(Answer::A(records)) => Ok(records),
                Some(Answer::Fail(error)) => Err(error),
                _ => Err(DnsError::NotFound),
            }
        })
    }

    fn aaaa<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Vec<Ipv6Addr>, DnsError>> {
        Box::pin(async move {
            match self.get(RecordType::Aaaa, name) {
                Some(Answer::Aaaa(records)) => Ok(records),
                Some(Answer::Fail(error)) => Err(error),
                _ => Err(DnsError::NotFound),
            }
        })
    }

    fn ptr(&self, ip: IpAddr) -> BoxFuture<'_, Result<Vec<String>, DnsError>> {
        Box::pin(async move {
            match self.get(RecordType::Ptr, &ip.to_string()) {
                Some(Answer::Ptr(names)) => Ok(names),
                Some(Answer::Fail(error)) => Err(error),
                _ => Err(DnsError::NotFound),
            }
        })
    }

    fn ns<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Vec<String>, DnsError>> {
        Box::pin(async move {
            match self.get(RecordType::Ns, name) {
                Some(Answer::Ns(hosts)) => Ok(hosts),
                Some(Answer::Fail(error)) => Err(error),
                _ => Err(DnsError::NotFound),
            }
        })
    }
}

/// The result of the reverse DNS check.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PtrState {
    /// Every address of the hostname has a PTR record naming the hostname.
    Ok,
    /// An address has no PTR record, or one that names another host.
    Mismatch,
    /// The hostname has no A or AAAA record.
    NoAddress,
    /// A lookup failed or timed out; nothing is known.
    Unknown,
    /// `ORBIT_MAIL_HOSTNAME` is not set.
    NoHostname,
}

/// Reverse DNS of the mail hostname: receivers of Orbit's mail (a later slice) and some
/// senders' checks expect the PTR of each address to name the hostname.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct PtrStatus {
    pub status: PtrState,
    /// The A and AAAA addresses of the hostname with their PTR names.
    pub addresses: Vec<PtrAddress>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct PtrAddress {
    pub ip: String,
    /// The PTR names (lower case, no trailing dot); empty when there is none.
    pub names: Vec<String>,
    /// One of `names` is the hostname.
    pub matches: bool,
}

/// Longest wait for the whole PTR check, so the Admin mail page stays quick.
const PTR_CHECK_TIMEOUT: Duration = Duration::from_secs(5);

/// Resolves the A and AAAA records of `hostname`, then the PTR of each address; each must
/// name `hostname`. Gives `unknown` after [`PTR_CHECK_TIMEOUT`].
pub async fn check_ptr(dns: &dyn DnsLookup, hostname: Option<&str>) -> PtrStatus {
    let Some(hostname) = hostname.map(host) else {
        return PtrStatus {
            status: PtrState::NoHostname,
            addresses: Vec::new(),
        };
    };
    tokio::time::timeout(PTR_CHECK_TIMEOUT, ptr_report(dns, hostname))
        .await
        .unwrap_or(PtrStatus {
            status: PtrState::Unknown,
            addresses: Vec::new(),
        })
}

async fn ptr_report(dns: &dyn DnsLookup, hostname: String) -> PtrStatus {
    let mut unknown = false;
    let mut ips = Vec::new();
    let (v4, v6) = futures_util::join!(dns.a(&hostname), dns.aaaa(&hostname));
    match v4 {
        Ok(found) => ips.extend(found.into_iter().map(IpAddr::V4)),
        Err(DnsError::NotFound) => {}
        Err(_) => unknown = true,
    }
    match v6 {
        Ok(found) => ips.extend(found.into_iter().map(IpAddr::V6)),
        Err(DnsError::NotFound) => {}
        Err(_) => unknown = true,
    }
    let answers = futures_util::future::join_all(ips.iter().map(|ip| dns.ptr(*ip))).await;
    let mut addresses = Vec::with_capacity(ips.len());
    for (ip, answer) in ips.into_iter().zip(answers) {
        let names = match answer {
            Ok(names) => names,
            Err(DnsError::NotFound) => Vec::new(),
            Err(_) => {
                unknown = true;
                Vec::new()
            }
        };
        addresses.push(PtrAddress {
            ip: ip.to_string(),
            matches: names.contains(&hostname),
            names,
        });
    }
    let status = if unknown {
        PtrState::Unknown
    } else if addresses.is_empty() {
        PtrState::NoAddress
    } else if addresses.iter().all(|address| address.matches) {
        PtrState::Ok
    } else {
        PtrState::Mismatch
    };
    PtrStatus { status, addresses }
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

/// The records of the setup page. A domain routed to a provider gets the provider's MX hosts;
/// otherwise the MX record is listed only when `hostname` is known (`ORBIT_MAIL_HOSTNAME`); in
/// relay mode it names the relay instead.
#[must_use]
pub fn expected_records(domain: &Domain, hostname: Option<&str>) -> Vec<ExpectedRecord> {
    let mut records = vec![ExpectedRecord {
        kind: RecordKind::Verify,
        record_type: "TXT",
        name: format!("_orbit-verify.{}", domain.name),
        value: verify_value(&domain.verify_token),
        priority: None,
    }];
    if let Some(provider) = &domain.provider {
        for (index, mx) in provider.mx_hosts.iter().enumerate() {
            records.push(ExpectedRecord {
                kind: RecordKind::Mx,
                record_type: "MX",
                name: domain.name.clone(),
                value: host(mx),
                priority: Some(MX_PREFERENCE * (u16::try_from(index).unwrap_or(9) + 1)),
            });
        }
    } else if let Some(hostname) = hostname {
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
        value: expected_spf(
            domain.send_route.as_ref().map(|route| &route.kind),
            hostname,
            domain
                .send_route
                .as_ref()
                .and_then(|route| route.spf.as_deref()),
        ),
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

/// The SPF record the domain publishes for its send route. The domain's own MX hosts always
/// stay allowed. Direct sending adds Orbit's host; a provider adds its include. `route_spf` is the
/// route's `spf` setting for a relay Orbit knows no include of: a mechanism list such as
/// `include:relay.example` (or a whole `v=spf1 …` record, used as it is).
#[must_use]
pub fn expected_spf(
    route: Option<&SendRouteKind>,
    hostname: Option<&str>,
    route_spf: Option<&str>,
) -> String {
    let known = match route {
        Some(SendRouteKind::Postmark) => Some("include:spf.mtasv.net"),
        Some(SendRouteKind::Resend | SendRouteKind::Ses) => Some("include:amazonses.com"),
        Some(SendRouteKind::Mailgun) => Some("include:mailgun.org"),
        _ => None,
    };
    let route_spf = route_spf.map(str::trim).filter(|spf| !spf.is_empty());
    if let (None, Some(spf)) = (known, route_spf)
        && has_version(spf, "v=spf1")
    {
        return spf.to_owned();
    }
    let mut parts = vec!["v=spf1".to_owned()];
    if let (Some(SendRouteKind::Direct), Some(hostname)) = (route, hostname) {
        parts.push(format!("a:{}", host(hostname)));
    }
    parts.push("mx".to_owned());
    match (known, route_spf) {
        (Some(include), _) => parts.push(include.to_owned()),
        (None, Some(spf)) => parts.push(spf.to_owned()),
        (None, None) => {}
    }
    parts.push("~all".to_owned());
    parts.join(" ")
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
    provider_hosts: Option<&[String]>,
    result: Result<Vec<(u16, String)>, DnsError>,
) -> RecordStatus {
    let records = match lookup_status(result) {
        Ok(records) => records,
        Err(status) => return status,
    };
    if records.is_empty() {
        return RecordStatus::Missing;
    }
    let wrong = || {
        let found: Vec<String> = records
            .iter()
            .map(|(preference, exchange)| format!("{preference} {exchange}"))
            .collect();
        RecordStatus::wrong(&found.join(", "))
    };
    // A provider route: mail must reach the provider (any MX when it names no host).
    if let Some(hosts) = provider_hosts {
        let hosts: Vec<String> = hosts.iter().map(|mx| host(mx)).collect();
        return if hosts.is_empty() || records.iter().any(|(_, exchange)| hosts.contains(exchange)) {
            RecordStatus::Ok
        } else {
            wrong()
        };
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

/// The DNS zone that holds `domain` and its NS hosts: the first of `domain` and its parents that
/// has NS records (`mail.example.com` usually has none; `example.com` has). Stops before the last
/// label, so a top-level domain is never taken. `None` when nothing was found or a lookup failed.
pub async fn dns_zone(dns: &dyn DnsLookup, domain: &str) -> Option<(String, Vec<String>)> {
    let mut name = domain.trim_end_matches('.');
    while name.contains('.') {
        match dns.ns(name).await {
            Ok(mut hosts) if !hosts.is_empty() => {
                hosts.sort();
                hosts.dedup();
                return Some((name.to_ascii_lowercase(), hosts));
            }
            Ok(_) | Err(DnsError::NotFound) => {}
            Err(_) => return None,
        }
        name = name.split_once('.').map_or("", |(_, parent)| parent);
    }
    None
}

/// Lookups of names in one DNS zone. Each lookup goes to the resolver and, at the same time, to
/// the zone's own name servers; the zone's answer wins when it has records. Else the resolver's
/// answer is used: a zone's server does not follow a CNAME out of its zone, and a firewall can
/// block port 53 to the internet.
pub struct ZoneLookup<'a> {
    resolver: &'a dyn DnsLookup,
    authoritative: Option<Arc<dyn DnsLookup>>,
    /// The zone and its NS hosts, as [`dns_zone`] found them.
    pub zone: Option<(String, Vec<String>)>,
}

impl<'a> ZoneLookup<'a> {
    /// Finds the zone of `name` and the addresses of its name servers through `dns`.
    pub async fn new(dns: &'a dyn DnsLookup, name: &str) -> Self {
        let zone = dns_zone(dns, name).await;
        let servers = match &zone {
            Some((_, hosts)) => name_server_addresses(dns, hosts).await,
            None => Vec::new(),
        };
        let authoritative = if servers.is_empty() {
            None
        } else {
            dns.at_servers(&servers)
        };
        Self {
            resolver: dns,
            authoritative,
            zone,
        }
    }

    pub async fn txt(&self, name: &str) -> Result<Vec<String>, DnsError> {
        combine(
            self.resolver.txt(name),
            self.authoritative.as_ref().map(|dns| dns.txt(name)),
        )
        .await
    }

    pub async fn mx(&self, name: &str) -> Result<Vec<(u16, String)>, DnsError> {
        combine(
            self.resolver.mx(name),
            self.authoritative.as_ref().map(|dns| dns.mx(name)),
        )
        .await
    }
}

/// The zone's answer when it has records, else the resolver's. Both lookups run at once.
async fn combine<T>(
    resolver: BoxFuture<'_, Result<Vec<T>, DnsError>>,
    authoritative: Option<BoxFuture<'_, Result<Vec<T>, DnsError>>>,
) -> Result<Vec<T>, DnsError> {
    let Some(authoritative) = authoritative else {
        return resolver.await;
    };
    match tokio::join!(resolver, authoritative) {
        (_, Ok(records)) if !records.is_empty() => Ok(records),
        (resolver, _) => resolver,
    }
}

/// The IPv4 and IPv6 addresses of `hosts`; a host whose lookup fails adds none.
async fn name_server_addresses(dns: &dyn DnsLookup, hosts: &[String]) -> Vec<IpAddr> {
    let lookups = hosts.iter().map(|host| async move {
        let (v4, v6) = tokio::join!(dns.a(host), dns.aaaa(host));
        let v4 = v4.unwrap_or_default().into_iter().map(IpAddr::V4);
        let v6 = v6.unwrap_or_default().into_iter().map(IpAddr::V6);
        v4.chain(v6).collect::<Vec<_>>()
    });
    let mut addresses: Vec<IpAddr> = join_all(lookups).await.into_iter().flatten().collect();
    addresses.sort();
    addresses.dedup();
    addresses
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
    let lookup = ZoneLookup::new(dns, &domain.name).await;
    let (verify, mx, spf, dmarc, dkim) = tokio::join!(
        lookup.txt(&verify_name),
        lookup.mx(&domain.name),
        lookup.txt(&domain.name),
        lookup.txt(&dmarc_name),
        async {
            match &dkim_name {
                Some(name) => Some(lookup.txt(name).await),
                None => None,
            }
        },
    );
    let (zone, nameservers) = lookup
        .zone
        .map_or((None, Vec::new()), |(zone, hosts)| (Some(zone), hosts));
    let (dmarc, dmarc_policy) = dmarc_status(dmarc);
    DnsReport {
        verify: verify_status(&domain.verify_token, verify),
        mx: mx_status(
            hostname,
            mode,
            domain
                .provider
                .as_ref()
                .map(|provider| provider.mx_hosts.as_slice()),
            mx,
        ),
        dkim: match (&domain.dkim, dkim) {
            (Some(key), Some(result)) => dkim_status(&key.public_key, result),
            _ => RecordStatus::Missing,
        },
        spf: spf_status(spf),
        dmarc,
        dmarc_policy,
        zone,
        nameservers,
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
        let dkim_ok = report.dkim == RecordStatus::Ok;
        let status = self
            .repository
            .record_dns_report(domain_id, report, now)
            .await?;
        // The record checked is the newest pending or active key's: when it is a pending key
        // and its record is published, it takes over.
        if dkim_ok && let Some(key) = domain.dkim.as_ref().filter(|key| key.status == "pending") {
            self.repository
                .activate_dkim(domain_id, &key.selector, now)
                .await?;
        }
        self.repository.retire_dkim_keys(domain_id, now).await?;
        Ok(status)
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

    #[tokio::test]
    async fn dns_zone_walks_up_to_the_zone() {
        let dns = FakeDns::new();
        dns.set_ns("example.com", &["ns2.example.net.", "NS1.example.net"]);
        dns.set_ns("com", &["a.gtld-servers.net"]);
        let zone = |name: &str| {
            (
                name.to_owned(),
                vec!["ns1.example.net".to_owned(), "ns2.example.net".to_owned()],
            )
        };
        // the zone's own hosts, sorted and lower case
        assert_eq!(
            dns_zone(&dns, "example.com").await,
            Some(zone("example.com"))
        );
        // a subdomain without its own NS records is in its parent's zone
        assert_eq!(
            dns_zone(&dns, "mail.example.com").await,
            Some(zone("example.com"))
        );
        // never the top-level domain
        assert_eq!(dns_zone(&dns, "other.com").await, None);
        // a failed lookup gives nothing, not a guess from a parent
        dns.fail("eu.example.com", DnsError::Timeout);
        assert_eq!(dns_zone(&dns, "eu.example.com").await, None);
    }

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
                None,
                Ok(vec![(10, "mx.orbit.test".to_owned())])
            ),
            RecordStatus::Ok
        );
        assert_eq!(
            mx_status(
                Some("mx.orbit.test"),
                MailMode::Direct,
                None,
                Ok(vec![(5, "aspmx.l.google.com".to_owned())])
            ),
            RecordStatus::wrong("5 aspmx.l.google.com")
        );
        assert_eq!(
            mx_status(
                Some("mx.orbit.test"),
                MailMode::Relay,
                None,
                Ok(vec![(5, "relay.example".to_owned())])
            ),
            RecordStatus::Ok
        );
    }
}
