//! Delivery of sent mail (spec §5.5): one adapter for each route kind. An adapter takes a signed
//! message with its envelope and gives an outcome for each recipient; it never retries (the send
//! job does, Task 13).
//!
//! Every adapter that connects to a host named by an admin or by remote DNS resolves the name
//! itself, checks every address ([`address_allowed`]) and connects to that checked address. The
//! host name is used only for TLS (SNI and the certificate check), so a name that changes between
//! the check and the connection (DNS rebinding) cannot reach another address.

mod deliver;
mod direct;
mod http;
mod release;
mod smtp;

use std::net::IpAddr;
use std::sync::Arc;

use futures_util::future::BoxFuture;
use serde_json::json;

use crate::mail_dns::{DnsError, DnsLookup};
use crate::repositories::mail::{
    HostClass, RouteSecrets, RouteSettings, SendRoute, SendRouteKind, classify_ip,
};

pub use crate::repositories::mail::{
    AttemptResult, BounceKind, DELAYED_AFTER_MS, DeferCause, GAVE_UP, LOCAL_DOMAIN_UNAVAILABLE,
    LOCAL_MAILBOX_SUSPENDED, LOCAL_TOO_LARGE, RETRY_STEPS_MS, ROUTE_TOO_LARGE, SEND_TIMEOUT_MS,
    SENDING_LEASE_MS, UNREADABLE,
};
pub use deliver::{NextRun, deliver_due};
pub use http::{api_base, delivery_http_client};
pub use release::{SendJobState, release_failure, run_send_job, send_job_outbound};

/// Most characters of a provider's or server's response kept in an outcome.
const RESPONSE_MAX_CHARS: usize = 500;

/// One message to deliver: the signed bytes and the envelope.
#[derive(Clone, Copy, Debug)]
pub struct Outgoing<'a> {
    /// The signed message, with the provider headers already prepended.
    pub raw: &'a [u8],
    /// The envelope sender (`MAIL FROM`), which may be the provider's; empty for `<>`.
    pub mail_from: &'a str,
    /// The domain of the `From` header (Mailgun sends for it); Unicode or A-labels.
    pub from_domain: &'a str,
    pub recipients: &'a [String],
    /// What the provider's events carry back (the provider headers hold it too).
    pub correlation: &'a str,
}

/// What became of one recipient. The kind of a failure is data set by the code that decides it,
/// never read from a response's text: a remote server's text cannot make itself a route error or
/// one of Orbit's own refusals.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecipientOutcome {
    Accepted {
        provider_message_id: Option<String>,
    },
    /// A temporary failure: try again later.
    Deferred {
        response: String,
        /// The route is at fault (its credentials, settings, host or account), not the
        /// recipient: a give-up after it is `bounce_kind = 'route'`.
        route_error: bool,
    },
    /// A permanent failure.
    Bounced {
        response: String,
        /// Orbit refused the message itself (too large for the route, a capability the next hop
        /// lacks, a local domain or mailbox that is unavailable): `bounce_kind = 'rejected'`,
        /// not a bad address. `false`: a refusal of the address (`hard`).
        rejected: bool,
    },
}

impl RecipientOutcome {
    /// A temporary failure of the next hop (a 4xx, a network error, a timeout).
    pub fn deferred(response: impl Into<String>) -> Self {
        Self::Deferred {
            response: clip(response.into()),
            route_error: false,
        }
    }

    /// A temporary failure that is the route's fault.
    pub fn route_error(response: impl Into<String>) -> Self {
        Self::Deferred {
            response: clip(response.into()),
            route_error: true,
        }
    }

    /// A permanent refusal of the address.
    pub fn bounced(response: impl Into<String>) -> Self {
        Self::Bounced {
            response: clip(response.into()),
            rejected: false,
        }
    }

    /// A permanent refusal of the message by Orbit itself.
    pub fn rejected(response: impl Into<String>) -> Self {
        Self::Bounced {
            response: clip(response.into()),
            rejected: true,
        }
    }
}

/// A route's way of sending.
pub trait Delivery: Send + Sync {
    /// Delivers `outgoing`: one outcome for each recipient, in the order given.
    fn deliver<'a>(
        &'a self,
        outgoing: &'a Outgoing<'a>,
    ) -> BoxFuture<'a, Vec<(String, RecipientOutcome)>>;
}

/// The MX records of a domain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MxAnswer {
    /// Preference and exchange.
    Hosts(Vec<(u16, String)>),
    /// RFC 7505: the domain accepts no mail.
    NullMx,
    /// No MX record: the domain's own A/AAAA records are its mail server (RFC 5321 §5.1).
    NoMx,
}

/// The DNS lookups of delivery.
pub trait MxLookup: Send + Sync {
    fn mx<'a>(&'a self, domain: &'a str) -> BoxFuture<'a, Result<MxAnswer, String>>;
    /// The A and AAAA addresses of `host`; empty when it has none.
    fn ips<'a>(&'a self, host: &'a str) -> BoxFuture<'a, Result<Vec<IpAddr>, String>>;
}

/// Delivery uses the mail DNS lookups ([`crate::mail_dns::HickoryDns`] in the server,
/// [`crate::mail_dns::FakeDns`] in tests).
impl<T: DnsLookup + ?Sized> MxLookup for T {
    fn mx<'a>(&'a self, domain: &'a str) -> BoxFuture<'a, Result<MxAnswer, String>> {
        Box::pin(async move {
            match DnsLookup::mx(self, domain).await {
                Ok(records) if records.is_empty() => Ok(MxAnswer::NoMx),
                // RFC 7505: a single record with the root as exchange
                Ok(records) if records.len() == 1 && records[0].1.is_empty() => {
                    Ok(MxAnswer::NullMx)
                }
                Ok(records) => Ok(MxAnswer::Hosts(
                    records
                        .into_iter()
                        .filter(|(_, host)| !host.is_empty())
                        .collect(),
                )),
                Err(DnsError::NotFound) => Ok(MxAnswer::NoMx),
                Err(error) => Err(error.to_string()),
            }
        })
    }

    fn ips<'a>(&'a self, host: &'a str) -> BoxFuture<'a, Result<Vec<IpAddr>, String>> {
        Box::pin(async move {
            let (v4, v6) = futures_util::join!(self.a(host), self.aaaa(host));
            let mut addresses = Vec::new();
            let mut failure = None;
            match v4 {
                Ok(found) => addresses.extend(found.into_iter().map(IpAddr::V4)),
                Err(DnsError::NotFound) => {}
                Err(error) => failure = Some(error.to_string()),
            }
            match v6 {
                Ok(found) => addresses.extend(found.into_iter().map(IpAddr::V6)),
                Err(DnsError::NotFound) => {}
                Err(error) => failure = Some(error.to_string()),
            }
            match failure {
                // Half an answer is still an answer; nothing and a failure is a failure.
                Some(error) if addresses.is_empty() => Err(error),
                _ => Ok(addresses),
            }
        })
    }
}

/// What every adapter needs besides the route.
#[derive(Clone)]
pub struct DeliveryContext {
    /// `ORBIT_MAIL_HOSTNAME`: the EHLO name, and the host whose addresses are this server's own.
    pub hostname: Option<String>,
    pub resolver: Arc<dyn MxLookup>,
    /// Built with [`delivery_http_client`] for `allow_private`, so the two always agree.
    http: reqwest::Client,
    allow_private: bool,
    /// Tests: the adapter of every route.
    adapter: Option<Arc<dyn Delivery>>,
}

impl DeliveryContext {
    /// `allow_private`: whether the route may reach private addresses — `true` only for a route
    /// of the root user (an instance route): its relay host, and its own API base URL, may then be
    /// private. Direct sending never reaches a private address (the MX hosts come from remote
    /// DNS), whatever this says.
    #[must_use]
    pub fn new(hostname: Option<String>, resolver: Arc<dyn MxLookup>, allow_private: bool) -> Self {
        Self {
            hostname,
            resolver,
            http: delivery_http_client(allow_private),
            allow_private,
            adapter: None,
        }
    }

    /// Sends every route through `adapter` (tests); the checks of [`adapter_for`] still apply.
    #[doc(hidden)]
    #[must_use]
    pub fn with_adapter(mut self, adapter: Arc<dyn Delivery>) -> Self {
        self.adapter = Some(adapter);
        self
    }

    #[must_use]
    pub const fn allow_private(&self) -> bool {
        self.allow_private
    }
}

/// The adapter of `route`.
#[must_use]
pub fn adapter_for(
    route: &SendRoute,
    secrets: &RouteSecrets,
    ctx: &DeliveryContext,
) -> Box<dyn Delivery> {
    let max_bytes = usize::try_from(route.settings.max_message_bytes)
        .ok()
        .filter(|bytes| *bytes > 0)
        .unwrap_or_else(|| usize::try_from(route.kind.default_max_bytes()).unwrap_or(usize::MAX));
    let inner: Box<dyn Delivery> = if let Some(adapter) = &ctx.adapter {
        Box::new(Shared(Arc::clone(adapter)))
    } else if secrets.fits(route.kind) {
        match route.kind {
            SendRouteKind::Direct => Box::new(direct::Direct::new(ctx)),
            SendRouteKind::Smtp
            | SendRouteKind::Postshiba
            | SendRouteKind::Postmark
            | SendRouteKind::Resend => {
                match smtp::Relay::new(route.kind, &route.settings, secrets, ctx) {
                    Ok(relay) => Box::new(relay),
                    Err(problem) => Box::new(Refuse(problem)),
                }
            }
            SendRouteKind::Mailgun | SendRouteKind::Ses | SendRouteKind::Cloudflare => {
                match http::Api::new(route.kind, &route.settings, secrets, ctx) {
                    Ok(api) => Box::new(api),
                    Err(problem) => Box::new(Refuse(problem)),
                }
            }
        }
    } else {
        Box::new(Refuse(
            "The route's secrets do not match its kind.".to_owned(),
        ))
    };
    Box::new(Checked { max_bytes, inner })
}

/// Checks that hold for every route before the adapter runs.
struct Checked {
    max_bytes: usize,
    inner: Box<dyn Delivery>,
}

impl Delivery for Checked {
    fn deliver<'a>(
        &'a self,
        outgoing: &'a Outgoing<'a>,
    ) -> BoxFuture<'a, Vec<(String, RecipientOutcome)>> {
        Box::pin(async move {
            if outgoing.raw.len() > self.max_bytes {
                return all(
                    outgoing.recipients,
                    &RecipientOutcome::rejected(format!(
                        "{ROUTE_TOO_LARGE} ({} bytes)",
                        self.max_bytes
                    )),
                );
            }
            // A recipient goes into a command line, a form field or JSON: refuse anything that
            // is not a plain address before it reaches an adapter.
            let (valid, invalid): (Vec<String>, Vec<String>) = outgoing
                .recipients
                .iter()
                .cloned()
                .partition(|address| plain_address(address));
            let mut outcomes = Vec::with_capacity(outgoing.recipients.len());
            if !valid.is_empty() {
                let checked = Outgoing {
                    recipients: &valid,
                    ..*outgoing
                };
                outcomes.extend(self.inner.deliver(&checked).await);
            }
            for address in invalid {
                outcomes.push((
                    address,
                    RecipientOutcome::bounced("5.1.3 The address is not valid"),
                ));
            }
            // In the order given.
            let mut ordered = Vec::with_capacity(outcomes.len());
            for address in outgoing.recipients {
                if let Some(index) = outcomes.iter().position(|(given, _)| given == address) {
                    ordered.push(outcomes.swap_remove(index));
                }
            }
            ordered.extend(outcomes);
            ordered
        })
    }
}

/// An adapter given to [`DeliveryContext::with_adapter`].
struct Shared(Arc<dyn Delivery>);

impl Delivery for Shared {
    fn deliver<'a>(
        &'a self,
        outgoing: &'a Outgoing<'a>,
    ) -> BoxFuture<'a, Vec<(String, RecipientOutcome)>> {
        self.0.deliver(outgoing)
    }
}

/// An adapter that cannot send (a setting or secret is missing): every recipient is deferred, so
/// the message goes out once the admin fixes the route.
pub(super) struct Refuse(pub(super) String);

impl Delivery for Refuse {
    fn deliver<'a>(
        &'a self,
        outgoing: &'a Outgoing<'a>,
    ) -> BoxFuture<'a, Vec<(String, RecipientOutcome)>> {
        Box::pin(async move {
            all(
                outgoing.recipients,
                &RecipientOutcome::route_error(&*self.0),
            )
        })
    }
}

/// The same outcome for each recipient.
fn all(recipients: &[String], outcome: &RecipientOutcome) -> Vec<(String, RecipientOutcome)> {
    recipients
        .iter()
        .map(|address| (address.clone(), outcome.clone()))
        .collect()
}

fn plain_address(address: &str) -> bool {
    address.len() <= 320
        && address
            .rsplit_once('@')
            .is_some_and(|(local, domain)| !local.is_empty() && !domain.is_empty())
        && !address
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || matches!(c, '<' | '>' | '"' | ','))
}

/// The domain of an address, lower case.
fn domain_of(address: &str) -> String {
    address
        .rsplit_once('@')
        .map(|(_, domain)| domain.trim_end_matches('.').to_ascii_lowercase())
        .unwrap_or_default()
}

/// Whether delivery may connect to `ip`. Never to an unusable address; to a private, loopback or
/// link-local one only when `allow_private`.
fn address_allowed(ip: IpAddr, allow_private: bool) -> bool {
    match classify_ip(ip) {
        HostClass::Public => orbit_platform::is_public_ip(ip) || allow_private,
        HostClass::Private | HostClass::Loopback | HostClass::LinkLocal => allow_private,
        HostClass::Unusable => false,
    }
}

/// Keeps the start of a response, on one line.
fn clip(text: String) -> String {
    let line: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(RESPONSE_MAX_CHARS)
        .collect();
    line.trim().to_owned()
}

/// A header value without line breaks or other control characters.
fn header_value(value: &str) -> String {
    value.chars().filter(|c| !c.is_control()).collect()
}

/// The headers a provider needs, prepended to the signed message (they are not in the DKIM
/// `h=` list): correlation for its events, and its tracking turned off.
#[must_use]
pub fn provider_headers(
    kind: SendRouteKind,
    settings: &RouteSettings,
    correlation: &str,
) -> Vec<(String, String)> {
    let correlation = header_value(correlation);
    let pair = |name: &str, value: String| (name.to_owned(), value);
    match kind {
        SendRouteKind::Postmark => vec![
            pair("X-PM-KeepID", "true".to_owned()),
            pair("X-PM-TrackLinks", "None".to_owned()),
            pair(
                "X-PM-Message-Stream",
                header_value(settings.stream.as_deref().unwrap_or("outbound")),
            ),
            pair("X-PM-Metadata-orbit", correlation),
        ],
        SendRouteKind::Mailgun => vec![
            pair("X-Mailgun-Track", "no".to_owned()),
            pair(
                "X-Mailgun-Variables",
                json!({ "orbit": correlation }).to_string(),
            ),
        ],
        SendRouteKind::Resend => vec![pair("Resend-Idempotency-Key", correlation)],
        SendRouteKind::Postshiba => vec![pair(
            "X-SMTPAPI",
            json!({ "unique_args": { "orbit": correlation } }).to_string(),
        )],
        SendRouteKind::Direct
        | SendRouteKind::Smtp
        | SendRouteKind::Ses
        | SendRouteKind::Cloudflare => Vec::new(),
    }
}

/// `raw` with `headers` in front.
#[must_use]
pub fn with_provider_headers(headers: &[(String, String)], raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len() + headers.len() * 64);
    for (name, value) in headers {
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(b": ");
        out.extend_from_slice(header_value(value).as_bytes());
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(raw);
    out
}
