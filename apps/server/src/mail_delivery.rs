//! Mail that a provider received and delivered over HTTPS (provider intake design §3 P4–P9): the
//! shared path after a provider's request was authenticated. Each envelope recipient must be on a
//! domain routed to this provider (P6); the others are recorded as rejected (P9), never stored.
//! Orbit checks DKIM and DMARC itself with the provider's SPF result, puts its trace headers on top
//! and stores the message through [`MailIntake`], so everything after that is the SMTP path.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use orbit_mail::inbound::{Delivery, SpfSource};
use orbit_mail::{Authenticator, authentication_results, provider_spf, strip_auth_results};
use orbit_platform::{JobStore, TimestampMillis};

use crate::mail_intake::{InboundMessage, IntakeError, MailIntake, Recipient, Source};
use crate::repositories::mail::{
    MailProvider, MailRepository, NewRejection, RecipientLookup, RejectReason,
};

/// The DKIM and DMARC lookups of one message, as for the listener.
const AUTH_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a Mailgun token is remembered (its timestamp is valid for 5 minutes).
const TOKEN_MEMORY_SECS: i64 = 10 * 60;

/// What became of a delivery.
#[derive(Debug, PartialEq, Eq)]
pub enum DeliveryResult {
    /// Stored for at least one recipient (the others, if any, are rejected).
    Stored {
        receipts: usize,
        rejected: usize,
    },
    /// No recipient could take it: the first reason and its address.
    NothingDeliverable {
        reason: RejectReason,
        recipient: String,
    },
    /// Storage, the database or a DNS lookup failed: the provider should send it again.
    Temporary(String),
    TooLarge,
}

/// The provider intake of one server: the authenticator, the store and the Mailgun token memory.
pub struct ProviderIntake {
    mail: Arc<MailRepository>,
    intake: MailIntake,
    auth: Arc<Authenticator>,
    /// The `authserv-id` and the `by` name of the trace headers.
    hostname: String,
    max_message_bytes: u64,
    /// Mailgun tokens seen in the last [`TOKEN_MEMORY_SECS`] (seconds), to refuse a replay. Per
    /// server: in a cluster a replay within the 5-minute timestamp window can reach another server.
    tokens: Mutex<HashMap<String, i64>>,
}

impl ProviderIntake {
    #[must_use]
    pub fn new(
        mail: Arc<MailRepository>,
        jobs: JobStore,
        max_message_bytes: u64,
        auth: Arc<Authenticator>,
        hostname: String,
    ) -> Self {
        Self {
            // the limit is for the provider's message; Orbit's trace headers come on top
            intake: MailIntake::new(
                Arc::clone(&mail),
                jobs,
                max_message_bytes + crate::mail_listener::TRACE_HEADROOM,
            ),
            mail,
            auth,
            hostname,
            max_message_bytes,
            tokens: Mutex::new(HashMap::new()),
        }
    }

    #[must_use]
    pub fn mail(&self) -> &Arc<MailRepository> {
        &self.mail
    }

    #[must_use]
    pub const fn max_message_bytes(&self) -> u64 {
        self.max_message_bytes
    }

    /// `false` when `token` was seen in the last ten minutes; remembers it otherwise.
    pub fn first_use(&self, token: &str, now_secs: i64) -> bool {
        let mut tokens = self
            .tokens
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        tokens.retain(|_, seen| now_secs - *seen <= TOKEN_MEMORY_SECS);
        tokens.insert(token.to_owned(), now_secs).is_none()
    }

    /// Delivers one authenticated message of `provider` to its routed recipients.
    pub async fn deliver(
        &self,
        provider: &MailProvider,
        delivery: Delivery,
        now: TimestampMillis,
    ) -> DeliveryResult {
        let result = self.deliver_inner(provider, delivery, now).await;
        let error = match &result {
            DeliveryResult::Temporary(error) => Some(error.as_str()),
            DeliveryResult::TooLarge => Some("message over the size limit"),
            _ => None,
        };
        if let Err(problem) = self
            .mail
            .record_provider_receipt(provider.id, now, error)
            .await
        {
            tracing::warn!(error = %problem, "mail provider status not saved");
        }
        result
    }

    async fn deliver_inner(
        &self,
        provider: &MailProvider,
        delivery: Delivery,
        now: TimestampMillis,
    ) -> DeliveryResult {
        if delivery.raw.len() as u64 > self.max_message_bytes {
            return DeliveryResult::TooLarge;
        }
        let mut seen = Vec::new();
        let mut recipients = Vec::new();
        let mut rejected = Vec::new();
        for rcpt in &delivery.recipients {
            if seen.contains(&rcpt.to_ascii_lowercase()) {
                continue;
            }
            seen.push(rcpt.to_ascii_lowercase());
            match self.route(provider, rcpt, now).await {
                Ok(Ok(recipient)) => recipients.push(recipient),
                Ok(Err((reason, workspace_id))) => rejected.push(NewRejection {
                    workspace_id: workspace_id.or(provider.workspace_id),
                    provider_id: provider.id,
                    mail_from: delivery.mail_from.clone(),
                    recipient: rcpt.clone(),
                    reason,
                }),
                Err(error) => return DeliveryResult::Temporary(error),
            }
        }
        let rejected_count = rejected.len();
        if recipients.is_empty() {
            let (reason, recipient) = rejected
                .first()
                .map_or((RejectReason::UnknownAddress, String::new()), |rejection| {
                    (rejection.reason, rejection.recipient.clone())
                });
            if let Err(error) = self.record_rejected(rejected, now).await {
                return DeliveryResult::Temporary(error);
            }
            return DeliveryResult::NothingDeliverable { reason, recipient };
        }

        let raw = strip_auth_results(&delivery.raw, &self.hostname);
        let spf = match &delivery.spf {
            SpfSource::Header(header) => provider_spf(&raw, header.as_header()),
            SpfSource::Given(spf) => Some(spf.clone()),
            SpfSource::None => None,
        };
        let helo = delivery.helo.clone().unwrap_or_default();
        let check = self
            .auth
            .check_relayed(&raw, delivery.mail_from.as_deref(), &helo, spf);
        let auth = match tokio::time::timeout(AUTH_TIMEOUT, check).await {
            Ok(outcome) if !outcome.temp_error => outcome,
            _ => return DeliveryResult::Temporary("DNS lookup failed".to_owned()),
        };
        let mut message = trace_headers(&self.hostname, provider, &auth).into_bytes();
        let trace_len = message.len();
        message.extend_from_slice(&raw);
        let inbound = InboundMessage {
            source: Source::Provider(provider.kind),
            raw: message,
            trace_len,
            mail_from: delivery.mail_from,
            recipients,
            // the provider's server connected to Orbit, not the sender's
            peer_ip: delivery
                .peer_ip
                .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED)),
            helo,
            tls: None,
            auth,
        };
        match self.intake.accept(inbound).await {
            // recorded only once stored: a provider retry after a failure must not list them twice
            Ok(receipts) => match self.record_rejected(rejected, now).await {
                Ok(()) => DeliveryResult::Stored {
                    receipts: receipts.len(),
                    rejected: rejected_count,
                },
                Err(error) => {
                    tracing::warn!(%error, "rejected deliveries not recorded");
                    DeliveryResult::Stored {
                        receipts: receipts.len(),
                        rejected: rejected_count,
                    }
                }
            },
            Err(IntakeError::TooLarge) => DeliveryResult::TooLarge,
            Err(IntakeError::QuotaExceeded) => {
                DeliveryResult::Temporary("the workspace is over its storage quota".to_owned())
            }
            Err(IntakeError::Temporary(error)) => DeliveryResult::Temporary(error),
        }
    }

    async fn record_rejected(
        &self,
        rejected: Vec<NewRejection>,
        now: TimestampMillis,
    ) -> Result<(), String> {
        for rejection in rejected {
            self.mail
                .record_rejected(rejection, now)
                .await
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    /// The recipient if `rcpt` is on a domain routed to `provider` and has a mailbox (or a test
    /// address); otherwise why not, with the domain's workspace when it has one.
    async fn route(
        &self,
        provider: &MailProvider,
        rcpt: &str,
        now: TimestampMillis,
    ) -> Result<Result<Recipient, (RejectReason, Option<orbit_platform::Id>)>, String> {
        let Ok(address) = orbit_mail::normalize_address(rcpt) else {
            return Ok(Err((RejectReason::UnknownAddress, None)));
        };
        let route = self
            .mail
            .domain_route(&address.domain)
            .await
            .map_err(|error| error.to_string())?;
        let workspace_id = match route {
            None => return Ok(Err((RejectReason::UnknownAddress, None))),
            Some((workspace_id, provider_id)) if provider_id != Some(provider.id) => {
                return Ok(Err((RejectReason::WrongRoute, Some(workspace_id))));
            }
            Some((workspace_id, _)) => workspace_id,
        };
        let lookup = self
            .mail
            .lookup_recipient(rcpt, now)
            .await
            .map_err(|error| error.to_string())?;
        let reason = match lookup {
            RecipientLookup::Accept {
                address_id,
                workspace_id,
                ..
            } => {
                return Ok(Ok(Recipient::Mailbox {
                    workspace_id,
                    address_id,
                    rcpt: rcpt.to_owned(),
                }));
            }
            RecipientLookup::HealthTest { test_id } => {
                return Ok(Ok(Recipient::HealthTest {
                    test_id,
                    rcpt: rcpt.to_owned(),
                }));
            }
            RecipientLookup::UnknownUser | RecipientLookup::Invalid => RejectReason::UnknownAddress,
            RecipientLookup::NotLocal | RecipientLookup::DomainUnavailable => {
                RejectReason::DomainUnavailable
            }
            RecipientLookup::MailboxSuspended => RejectReason::MailboxSuspended,
        };
        Ok(Err((reason, Some(workspace_id))))
    }
}

/// The text a sender's bounce shows for a rejected address (Cloudflare's `setReject`).
#[must_use]
pub fn reject_text(reason: RejectReason, recipient: &str) -> String {
    let why = match reason {
        RejectReason::UnknownAddress => "no such mailbox",
        RejectReason::WrongRoute => "this domain does not receive mail here",
        RejectReason::DomainUnavailable => "the domain does not receive mail",
        RejectReason::MailboxSuspended => "the mailbox does not receive mail",
    };
    format!("<{recipient}>: {why}")
}

/// The `Received` (RFC 5321 §4.4) and `Authentication-Results` (RFC 8601) fields Orbit puts on
/// top of a provider's message, CRLF-terminated and folded.
fn trace_headers(
    hostname: &str,
    provider: &MailProvider,
    auth: &orbit_mail::AuthOutcome,
) -> String {
    let date = chrono::Utc::now().to_rfc2822();
    let results = authentication_results(hostname, auth).replace("; ", ";\r\n\t");
    format!(
        "Received: from {} by {hostname} (Orbit) with HTTPS;\r\n\t{date}\r\n\
         Authentication-Results: {results}\r\n",
        provider.kind.as_str()
    )
}
