//! The SMTP client side (lettre's connection, one transaction for each delivery) and the relay
//! adapter: an SMTP relay, PostShiba, Postmark and Resend.

use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use futures_util::future::BoxFuture;
use lettre::Address;
use lettre::transport::smtp::Error as SmtpError;
use lettre::transport::smtp::authentication::{Credentials, Mechanism};
use lettre::transport::smtp::client::{AsyncSmtpConnection, TlsParameters};
use lettre::transport::smtp::commands::{Data, Mail, Rcpt};
use lettre::transport::smtp::extension::{ClientId, Extension, MailBodyParameter, MailParameter};

use super::{
    Delivery, DeliveryContext, MxLookup, Outgoing, RecipientOutcome, address_allowed, all,
};
use crate::repositories::mail::{
    RouteSecrets, RouteSettings, SendRouteKind, SmtpSecurity, all_private,
};

/// TCP connect, greeting and EHLO.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(60);
/// One command (RFC 5321 §4.5.3.2 gives 5 minutes for MAIL and RCPT).
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// The message text and the final reply (RFC 5321 §4.5.3.2: 10 minutes for the reply).
const DATA_TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// Addresses of one relay host tried before giving up.
const RELAY_ADDRESSES: usize = 3;

/// How a connection is secured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TlsMode {
    /// TLS from the first byte, certificate checked.
    Wrapper,
    /// `STARTTLS`, certificate checked; no `STARTTLS` offered: no delivery.
    Required,
    /// `STARTTLS` when offered, certificate not checked (RFC 7435 opportunistic security, as an
    /// MTA does to MX hosts).
    Opportunistic,
    Plain,
}

/// One server to deliver to: the checked address, and the name for TLS.
pub(super) struct Target<'a> {
    pub ip: IpAddr,
    pub port: u16,
    /// The host name: TLS SNI and certificate check only, never resolved.
    pub tls_name: &'a str,
    pub mode: TlsMode,
    pub hello: &'a ClientId,
    pub credentials: Option<&'a Credentials>,
    pub mechanisms: &'a [Mechanism],
}

/// Why a connection gave no transaction (nothing was sent).
#[derive(Debug)]
pub(super) struct ConnectFailure {
    pub response: String,
    /// The `STARTTLS` handshake failed: the connection may be tried again without TLS.
    pub tls_failed: bool,
}

impl ConnectFailure {
    fn new(response: impl Into<String>) -> Self {
        Self {
            response: response.into(),
            tls_failed: false,
        }
    }
}

enum StepError {
    Smtp(SmtpError),
    Timeout,
}

impl StepError {
    /// A reply of the server (4xx or 5xx), as opposed to a broken or timed-out connection.
    fn is_reply(&self) -> bool {
        matches!(self, Self::Smtp(error) if error.is_transient() || error.is_permanent())
    }

    fn outcome(&self) -> RecipientOutcome {
        match self {
            Self::Smtp(error) if error.is_permanent() => {
                RecipientOutcome::bounced(error.to_string())
            }
            Self::Smtp(error) => RecipientOutcome::deferred(error.to_string()),
            Self::Timeout => RecipientOutcome::deferred("The server did not answer in time"),
        }
    }

    fn text(&self) -> String {
        match self {
            Self::Smtp(error) => error.to_string(),
            Self::Timeout => "The server did not answer in time".to_owned(),
        }
    }
}

async fn step<T>(
    limit: Duration,
    future: impl Future<Output = Result<T, SmtpError>>,
) -> Result<T, StepError> {
    match tokio::time::timeout(limit, future).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(StepError::Smtp(error)),
        Err(_) => Err(StepError::Timeout),
    }
}

fn tls_parameters(name: &str, verify: bool) -> Result<TlsParameters, ConnectFailure> {
    TlsParameters::builder(name.to_owned())
        .dangerous_accept_invalid_certs(!verify)
        .build()
        .map_err(|error| ConnectFailure::new(format!("TLS setup failed: {error}")))
}

/// Connects to `target` and runs one transaction for `recipients`. `Err`: nothing was sent;
/// `Ok`: an outcome for each recipient.
pub(super) async fn transaction(
    target: &Target<'_>,
    outgoing: &Outgoing<'_>,
    recipients: &[String],
) -> Result<Vec<(String, RecipientOutcome)>, ConnectFailure> {
    let address = SocketAddr::new(target.ip, target.port);
    let wrapper = match target.mode {
        TlsMode::Wrapper => Some(tls_parameters(target.tls_name, true)?),
        _ => None,
    };
    let mut conn = step(
        CONNECT_TIMEOUT,
        AsyncSmtpConnection::connect_tokio1(
            address,
            Some(CONNECT_TIMEOUT),
            target.hello,
            wrapper,
            None,
        ),
    )
    .await
    .map_err(|error| ConnectFailure::new(format!("Connecting to {address}: {}", error.text())))?;

    let starttls = match target.mode {
        TlsMode::Required => {
            if !conn.can_starttls() {
                conn.abort().await;
                return Err(ConnectFailure::new(
                    "TLS required: the server does not offer STARTTLS",
                ));
            }
            Some(true)
        }
        TlsMode::Opportunistic => conn.can_starttls().then_some(false),
        TlsMode::Wrapper | TlsMode::Plain => None,
    };
    if let Some(verify) = starttls {
        let parameters = tls_parameters(target.tls_name, verify)?;
        if let Err(error) = step(COMMAND_TIMEOUT, conn.starttls(parameters, target.hello)).await {
            conn.abort().await;
            return Err(ConnectFailure {
                response: format!("TLS with {address} failed: {}", error.text()),
                tls_failed: true,
            });
        }
    }

    if let Some(credentials) = target.credentials {
        if !conn.is_encrypted() && target.mode != TlsMode::Plain {
            conn.abort().await;
            return Err(ConnectFailure::new(
                "TLS required: the login is never sent without it",
            ));
        }
        if let Err(error) = step(COMMAND_TIMEOUT, conn.auth(target.mechanisms, credentials)).await {
            conn.abort().await;
            return Err(ConnectFailure::new(format!(
                "Login refused: {}",
                error.text()
            )));
        }
    }

    let outcomes = send(&mut conn, outgoing, recipients).await;
    let _ = tokio::time::timeout(Duration::from_secs(10), conn.quit()).await;
    Ok(outcomes)
}

/// `MAIL`, a `RCPT` for each recipient, then `DATA` to the accepted ones. A refused `RCPT` gives
/// that recipient its outcome by the reply's class (4xx deferred, 5xx bounced); the others still
/// get the message in the same transaction.
async fn send(
    conn: &mut AsyncSmtpConnection,
    outgoing: &Outgoing<'_>,
    recipients: &[String],
) -> Vec<(String, RecipientOutcome)> {
    let info = conn.server_info().clone();
    let mut parameters = Vec::new();
    let international =
        !outgoing.mail_from.is_ascii() || recipients.iter().any(|address| !address.is_ascii());
    if international {
        if !info.supports_feature(Extension::SmtpUtfEight) {
            return all(
                recipients,
                &RecipientOutcome::bounced(
                    "5.6.7 The server does not take international addresses (no SMTPUTF8)",
                ),
            );
        }
        parameters.push(MailParameter::SmtpUtfEight);
    }
    if !outgoing.raw.is_ascii() {
        if !info.supports_feature(Extension::EightBitMime) {
            return all(
                recipients,
                &RecipientOutcome::bounced("5.6.3 The server does not take 8-bit messages"),
            );
        }
        parameters.push(MailParameter::Body(MailBodyParameter::EightBitMime));
    }
    let sender = if outgoing.mail_from.is_empty() {
        None
    } else {
        match Address::from_str(outgoing.mail_from) {
            Ok(address) => Some(address),
            Err(_) => {
                return all(
                    recipients,
                    &RecipientOutcome::bounced("5.1.7 The sender address is not valid"),
                );
            }
        }
    };
    if let Err(error) = step(COMMAND_TIMEOUT, conn.command(Mail::new(sender, parameters))).await {
        return all(recipients, &error.outcome());
    }

    let mut outcomes = Vec::with_capacity(recipients.len());
    let mut accepted = Vec::new();
    for (index, recipient) in recipients.iter().enumerate() {
        let Ok(address) = Address::from_str(recipient) else {
            outcomes.push((
                recipient.clone(),
                RecipientOutcome::bounced("5.1.3 The address is not valid"),
            ));
            continue;
        };
        match step(
            COMMAND_TIMEOUT,
            conn.command(Rcpt::new(address, Vec::new())),
        )
        .await
        {
            Ok(_) => accepted.push(recipient.clone()),
            Err(error) if error.is_reply() => outcomes.push((recipient.clone(), error.outcome())),
            Err(error) => {
                // The connection broke: nothing was sent to anyone.
                let outcome = RecipientOutcome::deferred(error.text());
                outcomes.extend(all(&accepted, &outcome));
                outcomes.extend(all(&recipients[index..], &outcome));
                return outcomes;
            }
        }
    }
    if accepted.is_empty() {
        return outcomes;
    }
    let result = match step(COMMAND_TIMEOUT, conn.command(Data)).await {
        // lettre ends the text with CRLF . CRLF itself; a final CRLF of the message would add an
        // empty line.
        Ok(_) => {
            let text = outgoing.raw.strip_suffix(b"\r\n").unwrap_or(outgoing.raw);
            step(DATA_TIMEOUT, conn.message(text)).await
        }
        Err(error) => Err(error),
    };
    let outcome = match result {
        Ok(_) => RecipientOutcome::Accepted {
            provider_message_id: None,
        },
        Err(error) => error.outcome(),
    };
    outcomes.extend(all(&accepted, &outcome));
    outcomes
}

/// The `EHLO` name: `ORBIT_MAIL_HOSTNAME`, or the machine's name.
pub(super) fn hello_name(hostname: Option<&str>) -> ClientId {
    hostname
        .filter(|name| !name.trim().is_empty())
        .map_or_else(ClientId::default, |name| ClientId::Domain(name.to_owned()))
}

/// The relay adapter: one host the route names (or the provider's), TLS as the route says.
pub(super) struct Relay {
    host: String,
    port: u16,
    mode: TlsMode,
    credentials: Option<Credentials>,
    mechanisms: Vec<Mechanism>,
    hello: ClientId,
    resolver: Arc<dyn MxLookup>,
    allow_private: bool,
}

impl Relay {
    /// `Err`: the route lacks a setting it needs (every recipient is deferred).
    pub(super) fn new(
        kind: SendRouteKind,
        settings: &RouteSettings,
        secrets: &RouteSecrets,
        ctx: &DeliveryContext,
    ) -> Result<Self, String> {
        let credentials = |user: &str, password: &str| {
            Some(Credentials::new(user.to_owned(), password.to_owned()))
        };
        let both = vec![Mechanism::Plain, Mechanism::Login];
        let route_host = || {
            settings
                .host
                .clone()
                .filter(|host| !host.trim().is_empty())
                .ok_or_else(|| "The route has no host.".to_owned())
        };
        let mode_of = |security: SmtpSecurity| match security {
            SmtpSecurity::Tls => TlsMode::Wrapper,
            SmtpSecurity::Starttls => TlsMode::Required,
            SmtpSecurity::None => TlsMode::Plain,
        };
        let (host, mode, credentials, mechanisms) = match (kind, secrets) {
            (SendRouteKind::Smtp, RouteSecrets::Smtp { username, password }) => (
                route_host()?,
                mode_of(settings.security),
                credentials(username, password),
                both,
            ),
            (SendRouteKind::Smtp, _) => (route_host()?, mode_of(settings.security), None, both),
            (
                SendRouteKind::Postshiba,
                RouteSecrets::Postshiba {
                    smtp_username,
                    smtp_password,
                    ..
                },
            ) => (
                route_host()?,
                // PostShiba is never plain.
                if settings.security == SmtpSecurity::Tls {
                    TlsMode::Wrapper
                } else {
                    TlsMode::Required
                },
                credentials(smtp_username, smtp_password),
                vec![Mechanism::Plain],
            ),
            (SendRouteKind::Postmark, RouteSecrets::Postmark { server_token, .. }) => (
                "smtp.postmarkapp.com".to_owned(),
                TlsMode::Required,
                credentials(server_token, server_token),
                both,
            ),
            (SendRouteKind::Resend, RouteSecrets::Resend { api_key, .. }) => (
                "smtp.resend.com".to_owned(),
                TlsMode::Required,
                credentials("resend", api_key),
                both,
            ),
            _ => return Err("The route's secrets do not match its kind.".to_owned()),
        };
        let port = match (kind, settings.port) {
            (SendRouteKind::Smtp | SendRouteKind::Postshiba, Some(port)) if port != 0 => port,
            _ => match mode {
                TlsMode::Wrapper => 465,
                TlsMode::Plain => 25,
                TlsMode::Required | TlsMode::Opportunistic => 587,
            },
        };
        Ok(Self {
            host: host
                .trim()
                .trim_start_matches('[')
                .trim_end_matches(']')
                .to_owned(),
            port,
            mode,
            credentials,
            mechanisms,
            hello: hello_name(ctx.hostname.as_deref()),
            resolver: ctx.resolver.clone(),
            allow_private: ctx.allow_private,
        })
    }

    /// The addresses to connect to, checked. `Err`: why none may be used.
    async fn addresses(&self) -> Result<Vec<IpAddr>, String> {
        let addresses = match self.host.parse::<IpAddr>() {
            Ok(ip) => vec![ip],
            Err(_) => self
                .resolver
                .ips(&self.host)
                .await
                .map_err(|error| format!("The DNS lookup of {} failed: {error}", self.host))?,
        };
        if addresses.is_empty() {
            return Err(format!("{} has no address", self.host));
        }
        if self.mode == TlsMode::Plain {
            // Plain SMTP only inside a private network, and only for an instance route.
            if !(self.allow_private && all_private(&addresses)) {
                return Err(format!(
                    "Plain SMTP to {} refused: it is only allowed to a private address",
                    self.host
                ));
            }
            return Ok(addresses);
        }
        if self.allow_private {
            let usable: Vec<IpAddr> = addresses
                .into_iter()
                .filter(|ip| address_allowed(*ip, true))
                .collect();
            if usable.is_empty() {
                return Err(format!("{} refused: it has no usable address", self.host));
            }
            Ok(usable)
        } else if addresses.iter().all(|ip| address_allowed(*ip, false)) {
            Ok(addresses)
        } else {
            Err(format!(
                "{} refused: it resolves to a private or reserved address",
                self.host
            ))
        }
    }
}

impl Delivery for Relay {
    fn deliver<'a>(
        &'a self,
        outgoing: &'a Outgoing<'a>,
    ) -> BoxFuture<'a, Vec<(String, RecipientOutcome)>> {
        Box::pin(async move {
            let addresses = match self.addresses().await {
                Ok(addresses) => addresses,
                Err(problem) => {
                    return all(outgoing.recipients, &RecipientOutcome::deferred(problem));
                }
            };
            let mut last = String::new();
            for ip in addresses.into_iter().take(RELAY_ADDRESSES) {
                let target = Target {
                    ip,
                    port: self.port,
                    tls_name: &self.host,
                    mode: self.mode,
                    hello: &self.hello,
                    credentials: self.credentials.as_ref(),
                    mechanisms: &self.mechanisms,
                };
                match transaction(&target, outgoing, outgoing.recipients).await {
                    Ok(outcomes) => return outcomes,
                    Err(failure) => last = failure.response,
                }
            }
            all(outgoing.recipients, &RecipientOutcome::deferred(last))
        })
    }
}
