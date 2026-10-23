//! SPF, DKIM and DMARC checks for inbound mail, and the `Authentication-Results` header (RFC 8601).
//!
//! The checks run through `mail-auth` with its hickory resolver. Orbit keeps only verdicts: DMARC decides the folder
//! (a failure goes to Spam, spec D4) and a DNS temporary error makes the listener answer `451 4.4.3`. ARC is not
//! evaluated (spec D9).
//!
//! In relay mode the relay has already run the checks; [`trusted_relay_results`] reads its pinned header instead.
//! In both modes [`strip_auth_results`] removes incoming headers that claim Orbit's own `authserv-id`.

use std::borrow::Borrow;
use std::collections::HashMap;
use std::hash::Hash;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Instant;

use mail_auth::common::parse::TxtRecordParser;
use mail_auth::common::verify::DomainKey;
use mail_auth::dkim::DkimError;
use mail_auth::dmarc::verify::DmarcParameters;
use mail_auth::dmarc::{Dmarc, Policy};
use mail_auth::hickory_resolver::config::{
    NameServerConfig, ResolveHosts, ResolverConfig, ResolverOpts,
};
use mail_auth::hickory_resolver::proto::op::ResponseCode;
use mail_auth::spf::Spf;
use mail_auth::spf::verify::SpfParameters;
use mail_auth::{
    AuthenticatedMessage, DkimOutput, DkimResult, DmarcResult, DnsError, DnssecStatus,
    Error as MailAuthError, MX, MessageAuthenticator, Parameters, RecordSet, ResolverCache,
    SpfResult, Txt,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// SPF `%{r}` macro value: RFC 7208 §7.3 allows `unknown` when the checking host has no name to give.
const HOST_DOMAIN: &str = "unknown";

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AuthError {
    #[error("the DNS resolver could not be set up: {0}")]
    Resolver(String),
}

/// The result of one SPF or DKIM check (RFC 8601 result names; serialized as `"pass"`, `"softfail"`, …).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Pass,
    Fail,
    SoftFail,
    Neutral,
    None,
    TempError,
    PermError,
}

/// The policy of a DMARC record that failed. `t=y` (test mode) is already applied: it lowers the policy one step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DmarcPolicy {
    None,
    Quarantine,
    Reject,
}

/// The DMARC result. A broken record (permerror) counts as no record. Serialized as
/// `{"result": "pass"}` or `{"result": "fail", "policy": "reject"}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "lowercase")]
pub enum DmarcVerdict {
    Pass,
    Fail { policy: DmarcPolicy },
    None,
    TempError,
}

/// What the checks found for one message; the server stores it as the receipt's `auth` JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthOutcome {
    pub spf: Verdict,
    pub dkim: Verdict,
    pub dmarc: DmarcVerdict,
    /// The `From` domain when DMARC passed (the only case in which the author domain is proven).
    pub aligned_domain: Option<String>,
    /// A DNS temporary error that could change the decision: the listener answers `451 4.4.3`.
    pub temp_error: bool,
    /// The domain SPF checked (MAIL FROM, or HELO for the null sender).
    pub spf_domain: Option<String>,
    /// The `d=` domain of the DKIM signature behind `dkim`.
    pub dkim_domain: Option<String>,
    /// The RFC 5322 `From` domain DMARC looked at.
    pub from_domain: Option<String>,
}

impl AuthOutcome {
    fn new(
        spf: Verdict,
        dkim: Verdict,
        dmarc: DmarcVerdict,
        spf_domain: Option<String>,
        dkim_domain: Option<String>,
        from_domain: Option<String>,
    ) -> Self {
        // A DNS error only matters while DMARC is undecided; once DMARC passes, a retry cannot change anything.
        let temp_error = dmarc == DmarcVerdict::TempError
            || (dmarc != DmarcVerdict::Pass
                && (spf == Verdict::TempError || dkim == Verdict::TempError));
        let aligned_domain = match dmarc {
            DmarcVerdict::Pass => from_domain.clone(),
            _ => None,
        };
        AuthOutcome {
            spf,
            dkim,
            dmarc,
            aligned_domain,
            temp_error,
            spf_domain,
            dkim_domain,
            from_domain,
        }
    }
}

// ---------------------------------------------------------------- fixed DNS answers

/// DNS answers fixed in advance, for tests: no lookup leaves the process.
///
/// TXT names that are not listed answer NXDOMAIN. A name with an A, AAAA or MX record answers "no records" for the
/// other two types. A, AAAA and MX lookups of any other name fail as a temporary error (the resolver behind them has
/// no name server), so preload every `a:` / `mx:` target an SPF record names.
#[derive(Clone, Default)]
pub struct StaticDns {
    txt: HashMap<Box<str>, Txt>,
    mx: HashMap<Box<str>, RecordSet<MX>>,
    ipv4: HashMap<Box<str>, RecordSet<Ipv4Addr>>,
    ipv6: HashMap<Box<str>, RecordSet<Ipv6Addr>>,
}

impl StaticDns {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a TXT record. `_dmarc.` names hold DMARC records, `._domainkey.` names DKIM keys, others SPF.
    pub fn txt(mut self, name: &str, record: &str) -> Self {
        let name = fqdn(name);
        let bytes = record.as_bytes();
        let value: Txt = if name.starts_with("_dmarc.") {
            Dmarc::parse(bytes).into()
        } else if name.contains("._domainkey.") {
            DomainKey::parse(bytes).into()
        } else {
            Spf::parse(bytes).into()
        };
        self.txt.insert(name, value);
        self
    }

    /// Makes the TXT lookup of `name` fail as a resolver timeout does.
    pub fn temp_error(mut self, name: &str) -> Self {
        let error = MailAuthError::Dns(DnsError::Resolver("request timed out".into()));
        self.txt.insert(fqdn(name), Txt::Error(error));
        self
    }

    pub fn a(mut self, name: &str, ip: Ipv4Addr) -> Self {
        push_record(&mut self.ipv4, name, ip);
        self
    }

    pub fn aaaa(mut self, name: &str, ip: Ipv6Addr) -> Self {
        push_record(&mut self.ipv6, name, ip);
        self
    }

    pub fn mx(mut self, name: &str, preference: u16, exchange: &str) -> Self {
        let exchange = MX {
            exchanges: vec![fqdn(exchange)].into_boxed_slice(),
            preference,
        };
        push_record(&mut self.mx, name, exchange);
        self
    }
}

fn fqdn(name: &str) -> Box<str> {
    let name = name.to_ascii_lowercase();
    if name.ends_with('.') {
        name.into_boxed_str()
    } else {
        format!("{name}.").into_boxed_str()
    }
}

fn empty_set<T>() -> RecordSet<T> {
    RecordSet {
        rrset: Arc::from(Vec::new()),
        dnssec_status: DnssecStatus::Indeterminate,
    }
}

fn push_record<T: Clone>(map: &mut HashMap<Box<str>, RecordSet<T>>, name: &str, value: T) {
    let entry = map.entry(fqdn(name)).or_insert_with(empty_set);
    let mut values = entry.rrset.to_vec();
    values.push(value);
    entry.rrset = Arc::from(values);
}

/// A read-only answer table in front of the resolver. Orbit does not store resolver answers itself (hickory caches
/// them), so `insert` does nothing.
struct Fixed<K, V> {
    answers: HashMap<K, V>,
    miss: Option<V>,
}

impl<K: Hash + Eq, V: Clone> ResolverCache<K, V> for Fixed<K, V> {
    fn get<Q>(&self, name: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.answers
            .get(name)
            .cloned()
            .or_else(|| self.miss.clone())
    }

    fn remove<Q>(&self, _: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        None
    }

    fn insert(&self, _: K, _: V, _: Instant) {}
}

type FixedParameters<'x, P> = Parameters<
    'x,
    P,
    Fixed<Box<str>, Txt>,
    Fixed<Box<str>, RecordSet<MX>>,
    Fixed<Box<str>, RecordSet<Ipv4Addr>>,
    Fixed<Box<str>, RecordSet<Ipv6Addr>>,
    Fixed<IpAddr, RecordSet<Box<str>>>,
>;

struct Answers {
    txt: Fixed<Box<str>, Txt>,
    mx: Fixed<Box<str>, RecordSet<MX>>,
    ipv4: Fixed<Box<str>, RecordSet<Ipv4Addr>>,
    ipv6: Fixed<Box<str>, RecordSet<Ipv6Addr>>,
    ptr: Fixed<IpAddr, RecordSet<Box<str>>>,
}

impl Answers {
    fn live() -> Self {
        Self::from_static(StaticDns::default(), None)
    }

    fn from_static(mut dns: StaticDns, txt_miss: Option<Txt>) -> Self {
        let hosts: Vec<Box<str>> = dns
            .mx
            .keys()
            .chain(dns.ipv4.keys())
            .chain(dns.ipv6.keys())
            .cloned()
            .collect();
        for host in hosts {
            dns.mx.entry(host.clone()).or_insert_with(empty_set);
            dns.ipv4.entry(host.clone()).or_insert_with(empty_set);
            dns.ipv6.entry(host).or_insert_with(empty_set);
        }
        Answers {
            txt: Fixed {
                answers: dns.txt,
                miss: txt_miss,
            },
            mx: Fixed {
                answers: dns.mx,
                miss: None,
            },
            ipv4: Fixed {
                answers: dns.ipv4,
                miss: None,
            },
            ipv6: Fixed {
                answers: dns.ipv6,
                miss: None,
            },
            ptr: Fixed {
                answers: HashMap::new(),
                miss: None,
            },
        }
    }
}

// ---------------------------------------------------------------- checks

/// Runs SPF, DKIM and DMARC for inbound messages.
pub struct Authenticator {
    inner: MessageAuthenticator,
    answers: Answers,
}

impl Authenticator {
    /// Uses the system resolver configuration (`/etc/resolv.conf`).
    pub fn system() -> Result<Self, AuthError> {
        let inner = MessageAuthenticator::new_system_conf()
            .map_err(|error| AuthError::Resolver(error.to_string()))?;
        Ok(Authenticator {
            inner,
            answers: Answers::live(),
        })
    }

    /// Uses one recursive resolver, over UDP with TCP fallback (for example a local Unbound).
    pub fn with_resolver(addr: SocketAddr) -> Result<Self, AuthError> {
        let mut server = NameServerConfig::udp_and_tcp(addr.ip());
        for connection in &mut server.connections {
            connection.port = addr.port();
        }
        let config = ResolverConfig::from_name_servers(vec![server]);
        Self::with_config(config, ResolverOpts::default(), Answers::live())
    }

    /// Answers every lookup from `dns`, for tests. No query leaves the process.
    pub fn with_static_dns(dns: StaticDns) -> Result<Self, AuthError> {
        let nxdomain = MailAuthError::Dns(DnsError::RecordNotFound(ResponseCode::NXDomain));
        let mut options = ResolverOpts::default();
        options.use_hosts_file = ResolveHosts::Never;
        let config = ResolverConfig::from_name_servers(Vec::new());
        Self::with_config(
            config,
            options,
            Answers::from_static(dns, Some(Txt::Error(nxdomain))),
        )
    }

    fn with_config(
        config: ResolverConfig,
        options: ResolverOpts,
        answers: Answers,
    ) -> Result<Self, AuthError> {
        let inner = MessageAuthenticator::new(config, options)
            .map_err(|error| AuthError::Resolver(error.to_string()))?;
        Ok(Authenticator { inner, answers })
    }

    fn parameters<P>(&self, params: P) -> FixedParameters<'_, P> {
        Parameters::new(params)
            .with_txt_cache(&self.answers.txt)
            .with_mx_cache(&self.answers.mx)
            .with_ipv4_cache(&self.answers.ipv4)
            .with_ipv6_cache(&self.answers.ipv6)
            .with_ptr_cache(&self.answers.ptr)
    }

    /// Checks one message. `mail_from` is `None` for the null sender `<>`; SPF then checks the HELO name
    /// (RFC 7208 §2.4) and DMARC uses it as the SPF domain (RFC 9989).
    pub async fn check(
        &self,
        raw: &[u8],
        peer_ip: IpAddr,
        helo: &str,
        mail_from: Option<&str>,
    ) -> AuthOutcome {
        let helo = helo.trim_end_matches('.');
        let mail_from = mail_from.filter(|sender| !sender.is_empty());
        let spf_domain = mail_from
            .and_then(|sender| sender.rsplit_once('@'))
            .map_or(helo, |(_, domain)| domain)
            .to_ascii_lowercase();

        let spf_params = match mail_from {
            Some(sender) => SpfParameters::verify_mail_from(peer_ip, helo, HOST_DOMAIN, sender),
            None => SpfParameters::verify_ehlo(peer_ip, helo, HOST_DOMAIN),
        };
        let spf_output = self.inner.verify_spf(self.parameters(spf_params)).await;
        let spf = spf_verdict(spf_output.result());

        let Some(message) = AuthenticatedMessage::parse(raw) else {
            // No header section to check: nothing is signed and there is no author domain.
            return AuthOutcome::new(
                spf,
                Verdict::None,
                DmarcVerdict::None,
                Some(spf_domain),
                None,
                None,
            );
        };

        let dkim_outputs = self.inner.verify_dkim(self.parameters(&message)).await;
        let (dkim, dkim_domain) = dkim_verdict(&dkim_outputs);

        let dmarc_output = self
            .inner
            .verify_dmarc(self.parameters(DmarcParameters::new(
                &message,
                &dkim_outputs,
                &spf_domain,
                &spf_output,
            )))
            .await;
        let dmarc = match dmarc_output.result() {
            DmarcResult::Pass => DmarcVerdict::Pass,
            DmarcResult::Fail(_) => DmarcVerdict::Fail {
                policy: dmarc_policy(dmarc_output.policy()),
            },
            DmarcResult::TempError(_) => DmarcVerdict::TempError,
            DmarcResult::PermError(_) | DmarcResult::None => DmarcVerdict::None,
        };
        let from_domain = Some(dmarc_output.domain())
            .filter(|domain| !domain.is_empty())
            .map(str::to_owned)
            .or_else(|| {
                let from = message.from();
                from.rsplit_once('@')
                    .map(|(_, domain)| domain.to_ascii_lowercase())
            });

        AuthOutcome::new(spf, dkim, dmarc, Some(spf_domain), dkim_domain, from_domain)
    }
}

fn spf_verdict(result: SpfResult) -> Verdict {
    match result {
        SpfResult::Pass => Verdict::Pass,
        SpfResult::Fail => Verdict::Fail,
        SpfResult::SoftFail => Verdict::SoftFail,
        SpfResult::Neutral => Verdict::Neutral,
        SpfResult::TempError => Verdict::TempError,
        SpfResult::PermError => Verdict::PermError,
        SpfResult::None => Verdict::None,
    }
}

/// One verdict for all signatures (see [`Verdict::rank`]) and the `d=` domain of the chosen one.
fn dkim_verdict(outputs: &[DkimOutput<'_>]) -> (Verdict, Option<String>) {
    outputs
        .iter()
        .map(|output| {
            let verdict = match output.result() {
                DkimResult::Pass => Verdict::Pass,
                // mail-auth reports a changed body as neutral; RFC 6376 §6.1.3 makes it a PERMFAIL, which is
                // `fail` in RFC 8601 §2.7.1.
                DkimResult::Neutral(MailAuthError::Dkim(DkimError::FailedBodyHashMatch)) => {
                    Verdict::Fail
                }
                DkimResult::Neutral(_) => Verdict::Neutral,
                DkimResult::Fail(_) => Verdict::Fail,
                DkimResult::PermError(_) => Verdict::PermError,
                DkimResult::TempError(_) => Verdict::TempError,
                DkimResult::None => Verdict::None,
            };
            let domain = output
                .signature()
                .map(|signature| signature.d.to_ascii_lowercase());
            (verdict, domain)
        })
        .min_by_key(|(verdict, _)| verdict.rank())
        .unwrap_or((Verdict::None, None))
}

fn dmarc_policy(policy: Policy) -> DmarcPolicy {
    match policy {
        Policy::Reject => DmarcPolicy::Reject,
        Policy::Quarantine => DmarcPolicy::Quarantine,
        Policy::None | Policy::Unspecified => DmarcPolicy::None,
    }
}

// ---------------------------------------------------------------- Authentication-Results

impl Verdict {
    /// Order for combining several DKIM signatures: any pass wins, then a temporary error (a retry may pass).
    fn rank(self) -> u8 {
        match self {
            Verdict::Pass => 0,
            Verdict::TempError => 1,
            Verdict::Fail | Verdict::SoftFail => 2,
            Verdict::PermError => 3,
            Verdict::Neutral => 4,
            Verdict::None => 5,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Verdict::Pass => "pass",
            Verdict::Fail => "fail",
            Verdict::SoftFail => "softfail",
            Verdict::Neutral => "neutral",
            Verdict::None => "none",
            Verdict::TempError => "temperror",
            Verdict::PermError => "permerror",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "pass" => Verdict::Pass,
            "fail" | "hardfail" | "policy" => Verdict::Fail,
            "softfail" => Verdict::SoftFail,
            "neutral" => Verdict::Neutral,
            "temperror" => Verdict::TempError,
            "permerror" => Verdict::PermError,
            _ => Verdict::None,
        }
    }
}

impl DmarcPolicy {
    fn as_str(self) -> &'static str {
        match self {
            DmarcPolicy::None => "none",
            DmarcPolicy::Quarantine => "quarantine",
            DmarcPolicy::Reject => "reject",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "none" => Some(DmarcPolicy::None),
            "quarantine" => Some(DmarcPolicy::Quarantine),
            "reject" => Some(DmarcPolicy::Reject),
            _ => None,
        }
    }
}

/// The value of Orbit's `Authentication-Results` header (RFC 8601), without the field name.
///
/// Domains come from the sender (HELO, MAIL FROM, `d=`, `From`), so a domain that is not plain
/// letters, digits, `-`, `_` and `.` is left out rather than escaped.
pub fn authentication_results(authserv_id: &str, outcome: &AuthOutcome) -> String {
    let mut value = String::with_capacity(160);
    value.push_str(authserv_id);

    value.push_str("; spf=");
    value.push_str(outcome.spf.as_str());
    push_property(&mut value, "smtp.mailfrom", outcome.spf_domain.as_deref());

    value.push_str("; dkim=");
    // RFC 8601 has no `softfail` for DKIM.
    let dkim = match outcome.dkim {
        Verdict::SoftFail => Verdict::Fail,
        other => other,
    };
    value.push_str(dkim.as_str());
    push_property(&mut value, "header.d", outcome.dkim_domain.as_deref());

    value.push_str("; dmarc=");
    match outcome.dmarc {
        DmarcVerdict::Pass => value.push_str("pass"),
        DmarcVerdict::Fail { policy } => {
            value.push_str("fail policy.dmarc=");
            value.push_str(policy.as_str());
        }
        DmarcVerdict::None => value.push_str("none"),
        DmarcVerdict::TempError => value.push_str("temperror"),
    }
    push_property(&mut value, "header.from", outcome.from_domain.as_deref());
    value
}

fn push_property(value: &mut String, name: &str, domain: Option<&str>) {
    if let Some(domain) = domain.filter(|domain| is_safe_domain(domain)) {
        value.push(' ');
        value.push_str(name);
        value.push('=');
        value.push_str(domain);
    }
}

fn is_safe_domain(domain: &str) -> bool {
    !domain.is_empty()
        && domain.len() <= 253
        && domain
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

// ---------------------------------------------------------------- header scanning

/// One header field: its byte range in the message (with the line ending), its name and its unfolded value.
struct Field<'a> {
    range: std::ops::Range<usize>,
    name: &'a [u8],
    value: &'a [u8],
}

/// The fields of the header section, which ends at the first empty line (CRLF or bare LF).
fn header_fields(raw: &[u8]) -> Vec<Field<'_>> {
    let mut fields: Vec<Field<'_>> = Vec::new();
    let mut position = 0;
    while position < raw.len() {
        let line_end = raw[position..]
            .iter()
            .position(|&byte| byte == b'\n')
            .map_or(raw.len(), |offset| position + offset + 1);
        let line = &raw[position..line_end];
        if line == b"\r\n" || line == b"\n" {
            break;
        }
        let continues = matches!(line.first(), Some(b' ' | b'\t'));
        match fields.last_mut() {
            Some(field) if continues => {
                field.range.end = line_end;
            }
            _ => {
                let colon = line.iter().position(|&byte| byte == b':');
                fields.push(Field {
                    range: position..line_end,
                    name: colon.map_or(&[][..], |colon| &line[..colon]),
                    value: &[],
                });
            }
        }
        position = line_end;
    }
    for field in &mut fields {
        let text = &raw[field.range.clone()];
        field.value = match text.iter().position(|&byte| byte == b':') {
            Some(colon) => &text[colon + 1..],
            None => &[],
        };
    }
    fields
}

fn is_auth_results(field: &Field<'_>) -> bool {
    field
        .name
        .trim_ascii_end()
        .eq_ignore_ascii_case(b"Authentication-Results")
}

/// Skips whitespace, line breaks and (nested) comments.
fn skip_cfws(value: &[u8], mut position: usize) -> usize {
    let mut depth = 0usize;
    while position < value.len() {
        match value[position] {
            b'\\' if depth > 0 => position += 1,
            b'(' => depth += 1,
            b')' if depth > 0 => depth -= 1,
            b' ' | b'\t' | b'\r' | b'\n' => {}
            _ if depth > 0 => {}
            _ => break,
        }
        position += 1;
    }
    position
}

/// The `authserv-id` (a token or a quoted string) and the offset just after it.
fn authserv_id(value: &[u8]) -> (String, usize) {
    let start = skip_cfws(value, 0);
    let mut id = Vec::new();
    let mut position = start;
    if value.get(start) == Some(&b'"') {
        position += 1;
        while position < value.len() && value[position] != b'"' {
            if value[position] == b'\\' {
                position += 1;
            }
            if let Some(&byte) = value.get(position) {
                id.push(byte);
            }
            position += 1;
        }
        position += 1;
    } else {
        while position < value.len()
            && !matches!(value[position], b';' | b'(' | b' ' | b'\t' | b'\r' | b'\n')
        {
            id.push(value[position]);
            position += 1;
        }
    }
    (
        String::from_utf8_lossy(&id).to_ascii_lowercase(),
        position.min(value.len()),
    )
}

/// Removes every `Authentication-Results` field whose `authserv-id` is `authserv_id` (case-insensitive), so a sender
/// cannot forge Orbit's own results (RFC 8601 §5). Other fields and the body are kept byte for byte.
pub fn strip_auth_results(raw: &[u8], authserv_id_value: &str) -> Vec<u8> {
    let wanted = authserv_id_value.to_ascii_lowercase();
    let mut output = Vec::with_capacity(raw.len());
    let mut copied = 0;
    for field in header_fields(raw) {
        if is_auth_results(&field) && authserv_id(field.value).0 == wanted {
            output.extend_from_slice(&raw[copied..field.range.start]);
            copied = field.range.end;
        }
    }
    output.extend_from_slice(&raw[copied..]);
    output
}

/// Reads the topmost `Authentication-Results` field with the relay's `authserv-id` (the one the relay added last).
/// Lower fields with the same id are older or forged and are ignored. `None` when the relay added no field.
pub fn trusted_relay_results(raw: &[u8], relay_id: &str) -> Option<AuthOutcome> {
    let wanted = relay_id.to_ascii_lowercase();
    let field = header_fields(raw)
        .into_iter()
        .find(|field| is_auth_results(field) && authserv_id(field.value).0 == wanted)?;
    let value = String::from_utf8_lossy(field.value);
    let (_, after_id) = authserv_id(value.as_bytes());
    Some(parse_results(&value[after_id..]))
}

/// One `method=result` entry with its properties and the text of its comments.
struct ResInfo {
    method: String,
    result: String,
    properties: Vec<(String, String)>,
    comments: String,
}

impl ResInfo {
    fn property(&self, name: &str) -> Option<&str> {
        self.properties
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

/// Splits the part after the `authserv-id` at `;` outside comments and quoted strings.
fn parse_results(text: &str) -> AuthOutcome {
    let mut entries = Vec::new();
    let mut current = String::new();
    let mut comments = String::new();
    let mut depth = 0usize;
    let mut quoted = false;
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' if depth > 0 || quoted => {
                let next = chars.next().unwrap_or(' ');
                if depth > 0 {
                    comments.push(next);
                } else {
                    current.push(next);
                }
            }
            '(' if !quoted => {
                depth += 1;
                comments.push(' ');
            }
            ')' if depth > 0 => depth -= 1,
            _ if depth > 0 => comments.push(ch),
            '"' => quoted = !quoted,
            ';' if !quoted => {
                entries.push((std::mem::take(&mut current), std::mem::take(&mut comments)));
            }
            '\r' | '\n' | '\t' => current.push(' '),
            _ => current.push(ch),
        }
    }
    entries.push((current, comments));

    let infos: Vec<ResInfo> = entries
        .into_iter()
        .filter_map(|(entry, comments)| {
            let mut words = entry.split_ascii_whitespace();
            let (method, result) = words.next()?.split_once('=')?;
            let method = method.split('/').next().unwrap_or(method);
            let properties = words
                .filter_map(|word| word.split_once('='))
                .map(|(key, value)| (key.to_ascii_lowercase(), value.to_owned()))
                .collect();
            Some(ResInfo {
                method: method.to_ascii_lowercase(),
                result: result.to_ascii_lowercase(),
                properties,
                comments,
            })
        })
        .collect();

    let spf_info = by_method(&infos, "spf")
        .find(|info| info.property("smtp.mailfrom").is_some())
        .or_else(|| by_method(&infos, "spf").next());
    let spf = spf_info.map_or(Verdict::None, |info| Verdict::parse(&info.result));
    let spf_domain = spf_info.and_then(|info| {
        info.property("smtp.mailfrom")
            .or_else(|| info.property("smtp.helo"))
            .and_then(domain_of)
    });

    let (dkim, dkim_domain) = by_method(&infos, "dkim")
        .map(|info| {
            let domain = info
                .property("header.d")
                .or_else(|| info.property("header.i"))
                .and_then(domain_of);
            (Verdict::parse(&info.result), domain)
        })
        .min_by_key(|(verdict, _)| verdict.rank())
        .unwrap_or((Verdict::None, None));

    let dmarc_info = by_method(&infos, "dmarc").next();
    let dmarc = match dmarc_info {
        Some(info) => match info.result.as_str() {
            "pass" => DmarcVerdict::Pass,
            "fail" => DmarcVerdict::Fail {
                policy: info
                    .property("policy.dmarc")
                    .and_then(DmarcPolicy::parse)
                    .or_else(|| comment_policy(&info.comments))
                    .unwrap_or(DmarcPolicy::None),
            },
            "temperror" => DmarcVerdict::TempError,
            _ => DmarcVerdict::None,
        },
        None => DmarcVerdict::None,
    };
    let from_domain = dmarc_info
        .and_then(|info| info.property("header.from"))
        .and_then(domain_of);

    AuthOutcome::new(spf, dkim, dmarc, spf_domain, dkim_domain, from_domain)
}

fn by_method<'a>(infos: &'a [ResInfo], method: &'a str) -> impl Iterator<Item = &'a ResInfo> {
    infos.iter().filter(move |info| info.method == method)
}

/// The domain of an address or a bare domain, if it is safe to keep.
fn domain_of(value: &str) -> Option<String> {
    let domain = value
        .rsplit_once('@')
        .map_or(value, |(_, domain)| domain)
        .trim_end_matches('.')
        .to_ascii_lowercase();
    is_safe_domain(&domain).then_some(domain)
}

/// Gmail writes the policy in a comment: `dmarc=fail (p=REJECT sp=REJECT dis=NONE)`.
fn comment_policy(comments: &str) -> Option<DmarcPolicy> {
    comments
        .split_ascii_whitespace()
        .find_map(|word| word.strip_prefix("p=").or_else(|| word.strip_prefix("P=")))
        .and_then(DmarcPolicy::parse)
}
