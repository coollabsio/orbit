//! Mail that a provider received and delivered over HTTPS (provider intake design §3 P4–P9): the
//! shared path after a provider's request was authenticated. Each envelope recipient must be on a
//! domain routed to this provider (P6); the others are recorded as rejected (P9), never stored.
//! Orbit checks DKIM and DMARC itself with the provider's SPF result, puts its trace headers on top
//! and stores the message through [`MailIntake`], so everything after that is the SMTP path.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use orbit_mail::inbound::postshiba::PostshibaEvent;
use orbit_mail::inbound::resend::ResendEvent;
use orbit_mail::inbound::{Delivery, InboundError, SpfHeaderOwned, SpfSource, ses};
use orbit_mail::{Authenticator, authentication_results, provider_spf, strip_auth_results};
use orbit_platform::{S3Bucket, S3Config, TimestampMillis};
use serde::Deserialize;

use crate::mail_image_proxy::{FetchError, Fetching, SafeFetch};

use crate::mail_intake::{InboundMessage, IntakeError, MailIntake, Recipient, Source};
use crate::repositories::mail::{
    MailProvider, MailRepository, NewRejection, RecipientLookup, RejectReason,
};

/// The DKIM and DMARC lookups of one message, as for the listener.
const AUTH_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a Mailgun token is remembered (its timestamp is valid for 5 minutes).
const TOKEN_MEMORY_SECS: i64 = orbit_mail::inbound::mailgun::RETRY_WINDOW_SECS + 60 * 60;
/// Request bodies read at once: a provider's message can be tens of MiB.
const BODIES_AT_ONCE: usize = 4;
/// PostShiba's Inbound messages API.
const POSTSHIBA_API: &str = "https://app.postshiba.com/api/v1/inboxes/";
/// Resend's Receiving API.
const RESEND_API: &str = "https://api.resend.com/emails/receiving/";
/// Small answers: Resend's message details, SNS certificates and confirmations.
const SMALL_BYTES: usize = 256 * 1024;
const API_TIMEOUT: Duration = Duration::from_secs(20);
/// Downloading a raw message (up to the size limit).
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(120);
/// How long an SNS signing certificate is kept.
const CERT_CACHE: Duration = Duration::from_secs(60 * 60);

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
    /// An SNS subscription was confirmed (SES).
    Confirmed,
    /// Handled before: SNS sent a notification again after Orbit stored the message and deleted
    /// S3's copy.
    AlreadyHandled,
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
    /// Request bodies being read; see [`BODIES_AT_ONCE`].
    bodies: tokio::sync::Semaphore,
    /// Fetches of Resend downloads and SNS certificates and confirmations.
    fetch: SafeFetch,
    /// Tests only: an S3 endpoint instead of `https://s3.<region>.amazonaws.com`.
    s3_endpoint: Option<String>,
    /// SNS signing certificates by URL, with when they were fetched.
    certs: Mutex<HashMap<String, (Vec<u8>, Instant)>>,
}

impl ProviderIntake {
    #[must_use]
    pub fn new(
        mail: Arc<MailRepository>,
        max_message_bytes: u64,
        auth: Arc<Authenticator>,
        hostname: String,
    ) -> Self {
        Self {
            // the limit is for the provider's message; Orbit's trace headers come on top
            intake: MailIntake::new(
                Arc::clone(&mail),
                max_message_bytes + crate::mail_listener::TRACE_HEADROOM,
            ),
            mail,
            auth,
            hostname,
            max_message_bytes,
            tokens: Mutex::new(HashMap::new()),
            bodies: tokio::sync::Semaphore::new(BODIES_AT_ONCE),
            fetch: SafeFetch::new(),
            s3_endpoint: None,
            certs: Mutex::new(HashMap::new()),
        }
    }

    /// Tests only: fetches through local servers.
    #[doc(hidden)]
    #[must_use]
    pub fn with_fetch(mut self, fetch: SafeFetch) -> Self {
        self.fetch = fetch;
        self
    }

    /// Tests only: SES reads S3 at `endpoint` (path style).
    #[doc(hidden)]
    #[must_use]
    pub fn with_s3_endpoint(mut self, endpoint: String) -> Self {
        self.s3_endpoint = Some(endpoint);
        self
    }

    /// Resend: fetches the message that the webhook names, then delivers it.
    pub async fn receive_resend(
        &self,
        provider: &MailProvider,
        api_key: &str,
        event: ResendEvent,
        now: TimestampMillis,
    ) -> DeliveryResult {
        #[derive(Deserialize)]
        struct Received {
            raw: Raw,
        }
        #[derive(Deserialize)]
        struct Raw {
            download_url: String,
        }
        let url = format!("{RESEND_API}{}", event.email_id);
        let details = match self
            .fetch
            .get(
                &url,
                SMALL_BYTES,
                "application/json",
                Some(api_key),
                API_TIMEOUT,
                Fetching::Provider,
            )
            .await
        {
            Ok(details) => details,
            Err(error) => {
                return self
                    .failed(provider, now, format!("Resend API: {error}"))
                    .await;
            }
        };
        let Ok(received) = serde_json::from_slice::<Received>(&details) else {
            return self
                .failed(provider, now, "Resend API: no raw message".to_owned())
                .await;
        };
        let max = usize::try_from(self.max_message_bytes).unwrap_or(usize::MAX);
        let raw = match self
            .fetch
            .get(
                &received.raw.download_url,
                max,
                "message/rfc822, */*",
                None,
                DOWNLOAD_TIMEOUT,
                Fetching::Provider,
            )
            .await
        {
            Ok(raw) => raw,
            Err(error) => {
                return self
                    .failed(provider, now, format!("Resend download: {error}"))
                    .await;
            }
        };
        let delivery = Delivery {
            raw,
            mail_from: event.mail_from,
            recipients: event.recipients,
            peer_ip: None,
            helo: None,
            spf: SpfSource::Header(SpfHeaderOwned::ReceivedSpf),
        };
        self.deliver(provider, delivery, now).await
    }

    /// PostShiba: fetches the raw message that the webhook names (PostShiba keeps it until the
    /// webhook succeeds), then delivers it.
    pub async fn receive_postshiba(
        &self,
        provider: &MailProvider,
        api_key: &str,
        event: PostshibaEvent,
        now: TimestampMillis,
    ) -> DeliveryResult {
        #[derive(Deserialize)]
        struct Message {
            raw: Option<String>,
        }
        let url = format!(
            "{POSTSHIBA_API}{}/inbound_messages/{}",
            event.inbox_id, event.message_id
        );
        let max = usize::try_from(self.max_message_bytes).unwrap_or(usize::MAX);
        // the answer holds the raw message, the decoded bodies and the attachments in base64
        let answer_max = max.saturating_mul(3).saturating_add(1024 * 1024);
        let answer = match self
            .fetch
            .get(
                &url,
                answer_max,
                "application/json",
                Some(api_key),
                DOWNLOAD_TIMEOUT,
                Fetching::Provider,
            )
            .await
        {
            Ok(answer) => answer,
            Err(error) => {
                return self
                    .failed(provider, now, format!("PostShiba API: {error}"))
                    .await;
            }
        };
        // `raw` is null for an API key of a user who is not a team admin, and after delivery
        let Some(raw) = serde_json::from_slice::<Message>(&answer)
            .ok()
            .and_then(|message| message.raw)
        else {
            return self
                .failed(
                    provider,
                    now,
                    "PostShiba API: no raw message (the API key must belong to a team admin)"
                        .to_owned(),
                )
                .await;
        };
        if raw.len() > max {
            return DeliveryResult::TooLarge;
        }
        let delivery = Delivery {
            raw: raw.into_bytes(),
            mail_from: event.mail_from,
            recipients: event.recipients,
            peer_ip: None,
            helo: None,
            spf: event.spf.map_or(SpfSource::None, SpfSource::Given),
        };
        self.deliver(provider, delivery, now).await
    }

    /// Amazon SES through SNS: checks the topic and the SNS signature, then confirms the
    /// subscription, or reads the message from S3, delivers it and deletes S3's copy.
    pub async fn receive_ses(
        &self,
        provider: &MailProvider,
        access_key_id: &str,
        secret_access_key: &str,
        body: &[u8],
        now: TimestampMillis,
    ) -> Result<DeliveryResult, InboundError> {
        let message = ses::parse_sns(body)?;
        let settings = &provider.settings;
        if settings.topic_arn.as_deref() != Some(message.topic_arn()) {
            return Err(InboundError::Unauthorized);
        }
        ses::fresh(&message, now.as_millis() / 1000)?;
        let cert_url = message.signed().cert_url.clone();
        if !ses::cert_url_ok(&cert_url) {
            return Err(InboundError::Unauthorized);
        }
        // not noted on the provider: the request is not verified yet
        let cert = match self.cert(&cert_url).await {
            Ok(cert) => cert,
            Err(error) => {
                return Ok(DeliveryResult::Temporary(format!(
                    "SNS certificate: {error}"
                )));
            }
        };
        ses::verify_sns(&message, &cert)?;
        // from here the request is AWS's: its errors show on the provider
        let result = self.receive_sns(provider, access_key_id, secret_access_key, message, now);
        match result.await {
            Err(error) => {
                self.note(provider, now, &error.to_string()).await;
                Err(error)
            }
            ok => ok,
        }
    }

    /// A verified SNS message: confirms the subscription, or stores the message from S3.
    async fn receive_sns(
        &self,
        provider: &MailProvider,
        access_key_id: &str,
        secret_access_key: &str,
        message: ses::SnsMessage,
        now: TimestampMillis,
    ) -> Result<DeliveryResult, InboundError> {
        let settings = &provider.settings;
        let receipt = match message {
            ses::SnsMessage::SubscriptionConfirmation { subscribe_url, .. } => {
                if !ses::sns_url_ok(&subscribe_url) {
                    return Err(InboundError::Unauthorized);
                }
                let confirmed = self
                    .fetch
                    .get(
                        &subscribe_url,
                        SMALL_BYTES,
                        "*/*",
                        None,
                        API_TIMEOUT,
                        Fetching::Provider,
                    )
                    .await;
                return Ok(match confirmed {
                    Ok(_) => DeliveryResult::Confirmed,
                    Err(error) => {
                        self.failed(provider, now, format!("SNS confirmation: {error}"))
                            .await
                    }
                });
            }
            ses::SnsMessage::Notification { ses, .. } => ses,
        };
        let (Some(bucket), Some(region)) = (&settings.bucket, &settings.region) else {
            return Err(InboundError::Malformed("bucket"));
        };
        let prefix = settings.prefix.as_deref().unwrap_or_default();
        if receipt.bucket != *bucket || !receipt.key.starts_with(prefix) {
            return Err(InboundError::Unauthorized);
        }
        let s3 = match S3Bucket::new(S3Config {
            endpoint: self
                .s3_endpoint
                .clone()
                .unwrap_or_else(|| format!("https://s3.{region}.amazonaws.com")),
            region: region.clone(),
            bucket: bucket.clone(),
            prefix: String::new(),
            access_key_id: access_key_id.to_owned(),
            secret_access_key: secret_access_key.to_owned(),
            path_style: self.s3_endpoint.is_some(),
        }) {
            Ok(s3) => s3,
            Err(error) => return Ok(self.failed(provider, now, format!("S3: {error}")).await),
        };
        let raw = match s3.get_bytes(&receipt.key).await {
            Ok(Some(raw)) => raw,
            // SNS sends a notification again when Orbit's answer is late; the first one already
            // stored the message and deleted the object
            Ok(None) => return Ok(DeliveryResult::AlreadyHandled),
            Err(error) => return Ok(self.failed(provider, now, format!("S3: {error}")).await),
        };
        let delivery = Delivery {
            raw,
            mail_from: receipt.mail_from,
            recipients: receipt.recipients,
            peer_ip: None,
            helo: None,
            spf: SpfSource::Given(receipt.spf),
        };
        let result = self.deliver(provider, delivery, now).await;
        // Orbit is the inbox of record: no plaintext copy stays in the bucket (design P8)
        if matches!(
            result,
            DeliveryResult::Stored { .. } | DeliveryResult::NothingDeliverable { .. }
        ) && let Err(error) = s3.delete(&receipt.key).await
        {
            tracing::warn!(%error, key = receipt.key, "SES message copy not deleted from S3");
        }
        Ok(result)
    }

    /// The PEM certificate at `url`, cached for an hour.
    async fn cert(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        let cached = self
            .certs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(url)
            .filter(|(_, at)| at.elapsed() < CERT_CACHE)
            .map(|(cert, _)| cert.clone());
        if let Some(cert) = cached {
            return Ok(cert);
        }
        let cert = self
            .fetch
            .get(
                url,
                SMALL_BYTES,
                "*/*",
                None,
                API_TIMEOUT,
                Fetching::Provider,
            )
            .await?;
        self.certs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(url.to_owned(), (cert.clone(), Instant::now()));
        Ok(cert)
    }

    /// Notes `error` on the provider and asks for the message again.
    async fn failed(
        &self,
        provider: &MailProvider,
        now: TimestampMillis,
        error: String,
    ) -> DeliveryResult {
        self.note(provider, now, &error).await;
        DeliveryResult::Temporary(error)
    }

    /// Shows `error` on the provider in settings (only for a verified request).
    pub async fn note(&self, provider: &MailProvider, now: TimestampMillis, error: &str) {
        if let Err(problem) = self
            .mail
            .record_provider_receipt(provider.id, now, Some(error))
            .await
        {
            tracing::warn!(error = %problem, "mail provider status not saved");
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

    /// Whether a delivered request had `token` (a replay).
    pub fn token_seen(&self, token: &str, now_secs: i64) -> bool {
        let mut tokens = self
            .tokens
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        tokens.retain(|_, seen| now_secs - *seen <= TOKEN_MEMORY_SECS);
        tokens.contains_key(token)
    }

    /// Remembers the token of a delivered request. A request that failed is not remembered: Mailgun
    /// sends it again with the same token.
    pub fn remember_token(&self, token: String, now_secs: i64) {
        self.tokens
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(token, now_secs);
    }

    /// Waits until a request body may be read (at most [`BODIES_AT_ONCE`] at a time).
    pub async fn body_permit(&self) -> Option<tokio::sync::SemaphorePermit<'_>> {
        self.bodies.acquire().await.ok()
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
                    // a workspace provider lists only in its own workspace: its owner signs
                    // what it sends, and must not write into another workspace's list
                    workspace_id: provider.workspace_id.or(workspace_id),
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
