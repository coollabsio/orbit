//! Direct sending: Orbit delivers to each recipient domain's MX hosts on port 25, with
//! `STARTTLS` when offered.

use std::net::IpAddr;
use std::sync::Arc;

use futures_util::future::{BoxFuture, join_all};

use super::smtp::{Target, TlsMode, hello_name, transaction};
use super::{
    Delivery, DeliveryContext, MxAnswer, MxLookup, Outgoing, RecipientOutcome, address_allowed,
    all, domain_of,
};

const SMTP_PORT: u16 = 25;
/// MX hosts of one domain tried, best preference first.
const MAX_MX_HOSTS: usize = 5;
/// Connections to one domain tried (over all its MX hosts).
const MAX_CONNECTIONS: usize = 6;

pub(super) struct Direct {
    hostname: Option<String>,
    resolver: Arc<dyn MxLookup>,
}

impl Direct {
    pub(super) fn new(ctx: &DeliveryContext) -> Self {
        Self {
            hostname: ctx
                .hostname
                .as_deref()
                .map(|name| name.trim().trim_end_matches('.').to_ascii_lowercase())
                .filter(|name| !name.is_empty()),
            resolver: ctx.resolver.clone(),
        }
    }

    /// Delivers to the recipients of one domain.
    async fn domain(
        &self,
        hostname: &str,
        own: &[IpAddr],
        domain: &str,
        outgoing: &Outgoing<'_>,
        recipients: &[String],
    ) -> Vec<(String, RecipientOutcome)> {
        let (hosts, implicit) = match self.resolver.mx(domain).await {
            Err(error) => {
                return all(
                    recipients,
                    &RecipientOutcome::deferred(format!(
                        "4.4.3 The MX lookup of {domain} failed: {error}"
                    )),
                );
            }
            Ok(MxAnswer::NullMx) => {
                return all(
                    recipients,
                    &RecipientOutcome::bounced(format!(
                        "5.1.10 {domain} accepts no mail (null MX)"
                    )),
                );
            }
            Ok(MxAnswer::NoMx) => (vec![domain.to_owned()], true),
            Ok(MxAnswer::Hosts(mut hosts)) => {
                hosts.sort_by_key(|(preference, _)| *preference);
                let hosts: Vec<String> = hosts.into_iter().map(|(_, host)| host).collect();
                if hosts.is_empty() {
                    return all(
                        recipients,
                        &RecipientOutcome::bounced(format!(
                            "5.1.10 {domain} accepts no mail (null MX)"
                        )),
                    );
                }
                (hosts, false)
            }
        };
        let hello = hello_name(Some(hostname));
        let mut temporary: Option<String> = None;
        let mut refused = false;
        let mut connections = 0;
        for host in hosts.iter().take(MAX_MX_HOSTS) {
            if connections == MAX_CONNECTIONS {
                break;
            }
            let host = host.trim_end_matches('.').to_ascii_lowercase();
            if host == hostname {
                refused = true;
                continue;
            }
            let addresses = match self.resolver.ips(&host).await {
                Ok(addresses) => addresses,
                Err(error) => {
                    temporary = Some(format!(
                        "4.4.3 The address lookup of {host} failed: {error}"
                    ));
                    continue;
                }
            };
            for ip in addresses {
                // Remote DNS picks these addresses: only public ones, even for an instance
                // route (spec §11), and never this server's own.
                if own.contains(&ip) || !address_allowed(ip, false) {
                    refused = true;
                    continue;
                }
                if connections == MAX_CONNECTIONS {
                    break;
                }
                connections += 1;
                let mut target = Target {
                    ip,
                    port: SMTP_PORT,
                    tls_name: &host,
                    mode: TlsMode::Opportunistic,
                    hello: &hello,
                    credentials: None,
                    mechanisms: &[],
                };
                let failure = match transaction(&target, outgoing, recipients).await {
                    Ok(outcomes) => return outcomes,
                    // A broken TLS handshake: plain text, as an MTA does.
                    Err(failure) if failure.tls_failed => {
                        target.mode = TlsMode::Plain;
                        match transaction(&target, outgoing, recipients).await {
                            Ok(outcomes) => return outcomes,
                            Err(failure) => failure,
                        }
                    }
                    Err(failure) => failure,
                };
                temporary = Some(failure.response);
            }
        }
        let outcome = match temporary {
            Some(response) => RecipientOutcome::deferred(response),
            None if refused => RecipientOutcome::bounced(format!(
                "5.4.4 The mail servers of {domain} cannot be reached: their addresses are \
                 private, reserved or this server's own"
            )),
            None if implicit => {
                RecipientOutcome::bounced(format!("5.1.2 {domain} has no mail server"))
            }
            None => RecipientOutcome::bounced(format!(
                "5.4.4 The mail servers of {domain} have no address"
            )),
        };
        all(recipients, &outcome)
    }
}

impl Delivery for Direct {
    fn deliver<'a>(
        &'a self,
        outgoing: &'a Outgoing<'a>,
    ) -> BoxFuture<'a, Vec<(String, RecipientOutcome)>> {
        Box::pin(async move {
            let Some(hostname) = self.hostname.as_deref() else {
                return all(
                    outgoing.recipients,
                    &RecipientOutcome::route_error(
                        "ORBIT_MAIL_HOSTNAME is not set: direct sending needs it",
                    ),
                );
            };
            // This server's own addresses: a domain whose MX is this server would loop.
            let own = self.resolver.ips(hostname).await.unwrap_or_default();
            let mut domains: Vec<(String, Vec<String>)> = Vec::new();
            for recipient in outgoing.recipients {
                let domain = domain_of(recipient);
                match domains.iter_mut().find(|(name, _)| *name == domain) {
                    Some((_, list)) => list.push(recipient.clone()),
                    None => domains.push((domain, vec![recipient.clone()])),
                }
            }
            let own = &own;
            let results = join_all(domains.iter().map(|(domain, recipients)| {
                self.domain(hostname, own, domain, outgoing, recipients)
            }))
            .await;
            results.into_iter().flatten().collect()
        })
    }
}
