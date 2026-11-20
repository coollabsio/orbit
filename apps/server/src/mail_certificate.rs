//! The SMTP listener's certificate from Admin → Mail: an ACME (Let's Encrypt) certificate for
//! `ORBIT_MAIL_HOSTNAME`. HTTP-01 goes through the reverse proxy (`/.well-known/acme-challenge/{token}`, answered
//! from the database, so any server of a cluster answers); DNS-01 is the manual fallback: the admin adds the TXT
//! record and clicks **Verify**.
//!
//! `ORBIT_MAIL_TLS_CERT` / `ORBIT_MAIL_TLS_KEY` win: with them, nothing here runs. Otherwise each server loads the
//! stored certificate into its listener and polls `mail_certificate.version` every 60 s, so a certificate that
//! another server got reaches every listener. A daily job renews it with HTTP-01.
//!
//! No ACME or DNS call runs inside a database transaction.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use axum::body::Bytes;
use http_body_util::{BodyExt, Full};
use instant_acme::{
    Account, AuthorizationStatus, BodyWrapper, BytesResponse, ChallengeType, HttpClient,
    Identifier, NewAccount, NewOrder, Order, OrderStatus, RetryPolicy,
};
use orbit_mail::{TlsAcceptorHandle, pem_cert_info};
use orbit_platform::{Id, MailConfig, TimestampMillis};
use tokio_util::sync::CancellationToken;

use crate::mail_dns::{DnsLookup, ZoneLookup};
use crate::repositories::mail::{
    CertificateState, ChallengeMethod, MailError, MailRepository, NewCert, PendingOrder,
};

/// How often each server checks for a new certificate.
const RELOAD_POLL: Duration = Duration::from_secs(60);
/// How long the CA may take to validate a challenge, then to issue.
const VALIDATION_TIMEOUT: Duration = Duration::from_secs(60);
/// Below the 60 s HTTP request timeout.
const ANSWER_WITHIN: Duration = Duration::from_secs(45);
const PENDING_ORDER_KEEP_MS: i64 = 24 * 60 * 60 * 1000;
const RENEW_BEFORE_EXPIRY_SECS: i64 = 30 * 24 * 60 * 60;
/// The daily renewal job.
pub const RENEW_JOB: &str = "mail.certificate.renew";

/// When Orbit renews a certificate (Unix seconds): after 2/3 of its lifetime, or 30 days before it expires,
/// whichever comes first, but not before half of its lifetime.
#[must_use]
pub fn renew_after(not_before: i64, not_after: i64) -> i64 {
    let lifetime = (not_after - not_before).max(0);
    // Without the floor, a certificate of 45 days or less would be renewed on each daily run.
    (not_before + lifetime * 2 / 3)
        .min(not_after - RENEW_BEFORE_EXPIRY_SECS)
        .max(not_before + lifetime / 2)
}

/// Where the listener's certificate comes from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CertificateSource {
    /// `ORBIT_MAIL_TLS_CERT` / `ORBIT_MAIL_TLS_KEY`: Admin → Mail cannot change it.
    Environment,
    /// Admin → Mail got it.
    Orbit,
    None,
}

/// Why a certificate action was refused.
#[derive(Debug, thiserror::Error)]
pub enum CertificateError {
    #[error("the certificate comes from ORBIT_MAIL_TLS_CERT and ORBIT_MAIL_TLS_KEY")]
    FromEnvironment,
    #[error("ORBIT_MAIL_HOSTNAME is not set")]
    HostnameRequired,
    #[error("no DNS-01 order is pending")]
    NoDnsOrder,
    #[error("the TXT record is not there yet")]
    DnsRecordNotFound,
    #[error("another certificate request runs on this server")]
    Busy,
    #[error(transparent)]
    Mail(#[from] MailError),
}

/// The certificate service of one server. Shared by the admin routes, the challenge route, the renewal job and
/// the reload loop.
pub struct MailCertificates {
    mail: Arc<MailRepository>,
    hostname: Option<String>,
    from_environment: bool,
    directory: String,
    http: reqwest::Client,
    dns: Arc<dyn DnsLookup>,
    /// This server's listener, when it uses the stored certificate.
    tls: Option<Arc<TlsAcceptorHandle>>,
    /// The version in `tls` (-1: none read yet).
    loaded: AtomicI64,
    /// One certificate request at a time on this server.
    busy: Arc<tokio::sync::Mutex<()>>,
}

impl MailCertificates {
    /// `tls` is the listener's handle when this server runs the listener without certificate files.
    pub fn new(
        mail: Arc<MailRepository>,
        config: &MailConfig,
        dns: Arc<dyn DnsLookup>,
        tls: Option<Arc<TlsAcceptorHandle>>,
    ) -> Result<Self, String> {
        let mut http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent(concat!("orbit/", env!("CARGO_PKG_VERSION")));
        if let Some(path) = &config.acme_root_ca {
            let pem = std::fs::read(path)
                .map_err(|error| format!("ORBIT_MAIL_ACME_ROOT_CA {}: {error}", path.display()))?;
            for cert in reqwest::Certificate::from_pem_bundle(&pem)
                .map_err(|error| format!("ORBIT_MAIL_ACME_ROOT_CA {}: {error}", path.display()))?
            {
                http = http.add_root_certificate(cert);
            }
        }
        Ok(Self {
            mail,
            hostname: config.hostname.clone().filter(|host| !host.is_empty()),
            from_environment: config.tls_cert.is_some(),
            directory: config.acme_directory.clone(),
            http: http.build().map_err(|error| error.to_string())?,
            dns,
            tls,
            loaded: AtomicI64::new(-1),
            busy: Arc::default(),
        })
    }

    #[must_use]
    pub fn hostname(&self) -> Option<&str> {
        self.hostname.as_deref()
    }

    #[must_use]
    pub fn source(&self, state: &CertificateState) -> CertificateSource {
        if self.from_environment {
            CertificateSource::Environment
        } else if state.issued.is_some() {
            CertificateSource::Orbit
        } else {
            CertificateSource::None
        }
    }

    pub async fn state(&self) -> Result<CertificateState, MailError> {
        self.mail.certificate_state().await
    }

    /// The key authorization for a pending HTTP-01 token.
    pub async fn http_challenge(&self, token: &str) -> Result<Option<String>, MailError> {
        if self.from_environment {
            return Ok(None);
        }
        self.mail.http_challenge(token).await
    }

    /// Starts an order. HTTP-01 runs to the end; DNS-01 stores the TXT record to add and waits for
    /// [`verify_dns`](Self::verify_dns). An ACME failure is kept in `last_error`, not returned.
    pub async fn start(
        self: &Arc<Self>,
        method: ChallengeMethod,
        actor_id: Option<Id>,
        request_id: &str,
    ) -> Result<(), CertificateError> {
        let hostname = self.check_allowed()?.to_owned();
        let busy = Arc::clone(&self.busy)
            .try_lock_owned()
            .map_err(|_| CertificateError::Busy)?;
        let this = Arc::clone(self);
        let request_id = request_id.to_owned();
        Self::finish(tokio::spawn(async move {
            let _busy = busy;
            let outcome = this
                .run_start(method, &hostname, actor_id, &request_id)
                .await;
            this.record(outcome).await
        }))
        .await
    }

    /// Checks the TXT record of the pending DNS-01 order, then lets the CA validate it and stores the
    /// certificate. A missing record is refused before the CA is asked, so the authorization stays usable.
    pub async fn verify_dns(
        self: &Arc<Self>,
        actor_id: Option<Id>,
        request_id: &str,
    ) -> Result<(), CertificateError> {
        self.check_allowed()?;
        let busy = Arc::clone(&self.busy)
            .try_lock_owned()
            .map_err(|_| CertificateError::Busy)?;
        let Some(PendingOrder::Dns {
            url,
            hostname,
            name,
            value,
            ..
        }) = self.mail.certificate_state().await?.pending
        else {
            return Err(CertificateError::NoDnsOrder);
        };
        let found = ZoneLookup::new(self.dns.as_ref(), &hostname)
            .await
            .txt(&name)
            .await
            .is_ok_and(|records| records.iter().any(|record| record.trim() == value));
        if !found {
            return Err(CertificateError::DnsRecordNotFound);
        }
        let this = Arc::clone(self);
        let request_id = request_id.to_owned();
        Self::finish(tokio::spawn(async move {
            let _busy = busy;
            let outcome = async {
                let account = this.account().await?;
                let mut order = account.order(url).await.map_err(acme)?;
                ready_challenge(&mut order, ChallengeType::Dns01).await?;
                this.complete(
                    order,
                    &hostname,
                    ChallengeMethod::Dns,
                    actor_id,
                    &request_id,
                )
                .await
            }
            .await;
            this.record(outcome).await
        }))
        .await
    }

    /// Waits for an ACME task, but answers before the HTTP request timeout: the task runs on (it is not
    /// cancelled with the request), and the admin page shows its pending order until it ends.
    async fn finish(
        task: tokio::task::JoinHandle<Result<(), CertificateError>>,
    ) -> Result<(), CertificateError> {
        match tokio::time::timeout(ANSWER_WITHIN, task).await {
            Ok(Ok(result)) => result,
            Ok(Err(error)) => {
                tracing::error!(%error, "mail TLS certificate task failed");
                Ok(())
            }
            Err(_) => Ok(()),
        }
    }

    /// Drops the pending order.
    pub async fn cancel(&self) -> Result<(), CertificateError> {
        self.check_allowed()?;
        self.mail
            .end_pending_order(None, TimestampMillis::now())
            .await?;
        Ok(())
    }

    /// The renewal job: renews with HTTP-01 when the stored certificate is due and no recent order is pending.
    pub async fn renew_if_due(self: &Arc<Self>) -> Result<(), MailError> {
        let Some(hostname) = self.hostname.as_deref() else {
            return Ok(());
        };
        if self.from_environment {
            return Ok(());
        }
        let state = self.mail.certificate_state().await?;
        let Some(issued) = state.issued else {
            return Ok(());
        };
        let now = TimestampMillis::now();
        // A DNS order the admin started today may still be verified; an older one is abandoned.
        let recent_order = state.pending.as_ref().is_some_and(|order| {
            now.as_millis() - order.started_at().as_millis() < PENDING_ORDER_KEEP_MS
        });
        if recent_order || now.as_millis() / 1000 < renew_after(issued.not_before, issued.not_after)
        {
            return Ok(());
        }
        tracing::info!(hostname, "renewing the mail TLS certificate");
        match self.start(ChallengeMethod::Http, None, "").await {
            Ok(()) => Ok(()),
            Err(CertificateError::Mail(error)) => Err(error),
            Err(error) => {
                tracing::warn!(%error, "mail TLS certificate not renewed");
                Ok(())
            }
        }
    }

    /// Loads the stored certificate into this server's listener when its version changed. A certificate that
    /// does not load keeps the last good one.
    pub async fn reload_if_changed(&self) -> Result<(), MailError> {
        let Some(tls) = &self.tls else {
            return Ok(());
        };
        let version = self.mail.certificate_version().await?;
        if version == self.loaded.load(Ordering::SeqCst) {
            return Ok(());
        }
        let state = self.mail.certificate_state().await?;
        if let Some(issued) = &state.issued {
            match tls.replace(issued.cert_pem.as_bytes(), issued.key_pem.as_bytes()) {
                Ok(info) => tracing::info!(
                    names = ?info.names,
                    not_after = info.not_after,
                    version = issued.version,
                    "mail TLS certificate loaded"
                ),
                Err(error) => tracing::warn!(
                    %error,
                    version = issued.version,
                    "stored mail TLS certificate not loaded, keeping the previous one"
                ),
            }
        }
        // Also after a failure: the same broken version is not retried every poll.
        self.loaded.store(
            state.issued.map_or(version, |issued| issued.version),
            Ordering::SeqCst,
        );
        Ok(())
    }

    /// Polls for a new certificate every 60 s until `cancel`. Errors are logged and retried on the next round.
    pub async fn run_reloader(self: Arc<Self>, cancel: CancellationToken) -> Result<(), String> {
        if self.tls.is_none() {
            cancel.cancelled().await;
            return Ok(());
        }
        loop {
            if let Err(error) = self.reload_if_changed().await {
                tracing::warn!(%error, "mail TLS certificate check failed; retrying on the next round");
            }
            tokio::select! {
                () = cancel.cancelled() => return Ok(()),
                () = tokio::time::sleep(RELOAD_POLL) => {}
            }
        }
    }

    fn check_allowed(&self) -> Result<&str, CertificateError> {
        if self.from_environment {
            return Err(CertificateError::FromEnvironment);
        }
        self.hostname
            .as_deref()
            .ok_or(CertificateError::HostnameRequired)
    }

    async fn run_start(
        &self,
        method: ChallengeMethod,
        hostname: &str,
        actor_id: Option<Id>,
        request_id: &str,
    ) -> Result<(), Failure> {
        let account = self.account().await?;
        let identifiers = [Identifier::Dns(hostname.to_owned())];
        let mut order = account
            .new_order(&NewOrder::new(&identifiers))
            .await
            .map_err(acme)?;
        let url = order.url().to_owned();
        let started_at = TimestampMillis::now();
        let challenge_type = match method {
            ChallengeMethod::Http => ChallengeType::Http01,
            ChallengeMethod::Dns => ChallengeType::Dns01,
        };
        let pending = {
            let mut authorizations = order.authorizations();
            let mut authorization = authorizations
                .next()
                .await
                .ok_or(Failure::Acme("the order has no authorization".to_owned()))?
                .map_err(acme)?;
            if authorization.status == AuthorizationStatus::Valid {
                // The CA still trusts an earlier validation of this host: no challenge.
                None
            } else {
                let challenge =
                    authorization
                        .challenge(challenge_type.clone())
                        .ok_or(Failure::Acme(format!(
                            "the CA offers no {challenge_type:?} challenge"
                        )))?;
                Some(match method {
                    ChallengeMethod::Http => PendingOrder::Http {
                        url,
                        hostname: hostname.to_owned(),
                        token: challenge.token.clone(),
                        key_authorization: challenge.key_authorization().as_str().to_owned(),
                        started_at,
                    },
                    ChallengeMethod::Dns => PendingOrder::Dns {
                        url,
                        hostname: hostname.to_owned(),
                        name: format!("_acme-challenge.{hostname}"),
                        value: challenge.key_authorization().dns_value(),
                        started_at,
                    },
                })
            }
        };
        match pending {
            None => {}
            Some(pending @ PendingOrder::Dns { .. }) => {
                self.mail.set_pending_order(&pending).await?;
                return Ok(());
            }
            Some(pending @ PendingOrder::Http { .. }) => {
                // Stored first: the CA's request may reach any server.
                self.mail.set_pending_order(&pending).await?;
                ready_challenge(&mut order, ChallengeType::Http01).await?;
            }
        }
        self.complete(order, hostname, method, actor_id, request_id)
            .await
    }

    /// Waits for the order to be ready, finalizes it with a new key, and stores and loads the certificate.
    async fn complete(
        &self,
        mut order: Order,
        hostname: &str,
        method: ChallengeMethod,
        actor_id: Option<Id>,
        request_id: &str,
    ) -> Result<(), Failure> {
        let retries = RetryPolicy::new()
            .initial_delay(Duration::from_millis(500))
            .backoff(1.5)
            .timeout(VALIDATION_TIMEOUT);
        let status = order.poll_ready(&retries).await.map_err(acme)?;
        if status != OrderStatus::Ready {
            return Err(Failure::Acme(validation_problem(&mut order).await));
        }
        let key_pem = order.finalize().await.map_err(acme)?;
        let cert_pem = order.poll_certificate(&retries).await.map_err(acme)?;
        let info = pem_cert_info(cert_pem.as_bytes())
            .map_err(|error| Failure::Acme(format!("the CA sent a bad certificate: {error}")))?;
        let version = self
            .mail
            .store_certificate(
                &NewCert {
                    cert_pem: &cert_pem,
                    key_pem: &key_pem,
                    hostname,
                    not_before: info.not_before,
                    not_after: info.not_after,
                    method,
                },
                actor_id,
                request_id,
                TimestampMillis::now(),
            )
            .await?;
        tracing::info!(
            hostname,
            version,
            not_after = info.not_after,
            "mail TLS certificate issued"
        );
        if let Err(error) = self.reload_if_changed().await {
            tracing::warn!(%error, "new mail TLS certificate stored but not loaded yet");
        }
        Ok(())
    }

    /// The stored ACME account for the configured directory, or a new one.
    async fn account(&self) -> Result<Account, Failure> {
        let stored = self.mail.certificate_state().await?.account;
        let builder = Account::builder_with_http(Box::new(AcmeHttp(self.http.clone())));
        if let Some(stored) = stored
            && stored.directory == self.directory
        {
            return builder
                .from_credentials(stored.credentials)
                .await
                .map_err(acme);
        }
        let (account, credentials) = builder
            .create(
                &NewAccount {
                    contact: &[],
                    terms_of_service_agreed: true,
                    only_return_existing: false,
                },
                self.directory.clone(),
                None,
            )
            .await
            .map_err(acme)?;
        self.mail
            .save_acme_account(&self.directory, &credentials)
            .await?;
        Ok(account)
    }

    /// Keeps an ACME failure in `last_error` (and drops the pending order); a database error is returned.
    async fn record(&self, outcome: Result<(), Failure>) -> Result<(), CertificateError> {
        match outcome {
            Ok(()) => Ok(()),
            Err(Failure::Mail(error)) => Err(error.into()),
            Err(Failure::Acme(message)) => {
                tracing::warn!(error = %message, "mail TLS certificate request failed");
                self.mail
                    .end_pending_order(Some(&message), TimestampMillis::now())
                    .await?;
                Ok(())
            }
        }
    }
}

enum Failure {
    Acme(String),
    Mail(MailError),
}

impl From<MailError> for Failure {
    fn from(error: MailError) -> Self {
        Self::Mail(error)
    }
}

fn acme(error: instant_acme::Error) -> Failure {
    Failure::Acme(error.to_string())
}

/// Tells the CA that the order's challenge of `kind` is in place.
async fn ready_challenge(order: &mut Order, kind: ChallengeType) -> Result<(), Failure> {
    let mut authorizations = order.authorizations();
    let mut authorization = authorizations
        .next()
        .await
        .ok_or(Failure::Acme("the order has no authorization".to_owned()))?
        .map_err(acme)?;
    if authorization.status == AuthorizationStatus::Valid {
        return Ok(());
    }
    let mut challenge = authorization
        .challenge(kind.clone())
        .ok_or(Failure::Acme(format!(
            "the CA offers no {kind:?} challenge"
        )))?;
    challenge.set_ready().await.map_err(acme)
}

/// The CA's reason for an invalid order: the error of its failed challenge.
async fn validation_problem(order: &mut Order) -> String {
    let mut authorizations = order.authorizations();
    if let Some(Ok(authorization)) = authorizations.next().await
        && let Some(error) = authorization
            .challenges
            .iter()
            .find_map(|challenge| challenge.error.as_ref())
    {
        return format!("the CA could not validate the challenge: {error}");
    }
    "the CA could not validate the challenge".to_owned()
}

/// instant-acme over the server's reqwest stack (rustls with ring), so the ACME client needs no other TLS stack.
struct AcmeHttp(reqwest::Client);

impl HttpClient for AcmeHttp {
    fn request(
        &self,
        request: axum::http::Request<BodyWrapper<Bytes>>,
    ) -> Pin<Box<dyn Future<Output = Result<BytesResponse, instant_acme::Error>> + Send>> {
        let client = self.0.clone();
        Box::pin(async move {
            let (parts, body) = request.into_parts();
            let body = body
                .collect()
                .await
                .map(http_body_util::Collected::to_bytes)
                .unwrap_or_default();
            let response = client
                .request(parts.method, parts.uri.to_string())
                .headers(parts.headers)
                .body(body)
                .send()
                .await
                .map_err(|error| instant_acme::Error::Other(Box::new(error)))?;
            let mut builder = axum::http::Response::builder().status(response.status());
            if let Some(headers) = builder.headers_mut() {
                headers.extend(
                    response
                        .headers()
                        .iter()
                        .map(|(name, value)| (name.clone(), value.clone())),
                );
            }
            let bytes = response
                .bytes()
                .await
                .map_err(|error| instant_acme::Error::Other(Box::new(error)))?;
            let response = builder.body(Full::new(bytes))?;
            Ok(BytesResponse::from(response))
        })
    }
}
