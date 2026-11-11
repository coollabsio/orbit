//! The SMTP listener (spec §4.5, §6.2, §9): accepts connections, applies the relay and PROXY v2
//! rules and the connection limits, and runs one `orbit_mail` session per connection with this
//! server's recipient lookup and intake.
//!
//! Each server of a cluster runs its own listener; intake commits to the shared database, so any
//! server can take any message. The listener caches nothing between sessions.

use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use orbit_mail::{
    AcceptedRecipient, AuthOutcome, Authenticator, CertInfo, ConnLimiter, DataDecision,
    DmarcVerdict, Envelope, NoTls, RcptDecision, Reply, SessionContext, SessionHandler,
    SessionLimits, TlsAcceptorHandle, Verdict, authentication_results, is_docker_bridge,
    read_proxy_v2, run_session, strip_auth_results, trusted_relay_results,
};
use orbit_platform::{Id, MailConfig, MailMode, TimestampMillis};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::mail_intake::{InboundMessage, IntakeError, MailIntake, Recipient, Source};
use crate::repositories::mail::{MailRepository, RecipientLookup};

/// How long the SPF, DKIM and DMARC checks of one message may take; then `451 4.4.3`.
const AUTH_TIMEOUT: Duration = Duration::from_secs(60);
/// How long a proxy has to send its PROXY v2 header.
const PROXY_TIMEOUT: Duration = Duration::from_secs(5);
/// How often the certificate files are checked for a change.
const CERT_POLL: Duration = Duration::from_secs(60);
/// Room for the `Received` and `Authentication-Results` headers Orbit adds to a message that is
/// at the size limit.
pub const TRACE_HEADROOM: u64 = 16 * 1024;
/// Marks a health test recipient in the session's opaque recipient ids.
const HEALTH_TEST: &str = "health-test:";

/// `554` at connect for a peer that is not a relay (relay mode).
const NOT_A_RELAY: Reply = Reply {
    code: 554,
    enhanced: "5.7.1",
    text: "This server only accepts mail from its relay",
};
const TOO_MANY_CONNECTIONS: Reply = Reply {
    code: 421,
    enhanced: "4.7.0",
    text: "Too many connections, try again later",
};

/// What the health card shows about this server's listener. Shared behind an `Arc`; the
/// listener updates it.
#[derive(Debug, Default)]
pub struct MailListenerStatus {
    running: AtomicBool,
    bridge_peers: AtomicU64,
    last_peer_ip: Mutex<Option<IpAddr>>,
    local_addr: Mutex<Option<SocketAddr>>,
    tls: Option<Arc<TlsAcceptorHandle>>,
}

impl MailListenerStatus {
    #[must_use]
    pub fn new(tls: Option<Arc<TlsAcceptorHandle>>) -> Self {
        Self {
            tls,
            ..Self::default()
        }
    }

    /// Whether the listener accepts connections now.
    #[must_use]
    pub fn running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// The loaded certificate (the last good one after a failed reload).
    #[must_use]
    pub fn cert(&self) -> Option<CertInfo> {
        self.tls.as_ref().map(|tls| tls.info())
    }

    /// Connections that came from a Docker bridge gateway (the client address was lost).
    #[must_use]
    pub fn bridge_peers(&self) -> u64 {
        self.bridge_peers.load(Ordering::SeqCst)
    }

    /// The client address of the last connection (after PROXY v2).
    #[must_use]
    pub fn last_peer_ip(&self) -> Option<IpAddr> {
        *lock(&self.last_peer_ip)
    }

    /// The bound address while running.
    #[must_use]
    pub fn local_addr(&self) -> Option<SocketAddr> {
        *lock(&self.local_addr)
    }

    fn set_running(&self, address: Option<SocketAddr>) {
        *lock(&self.local_addr) = address;
        self.running.store(address.is_some(), Ordering::SeqCst);
    }

    /// Records a client; returns whether it is the first bridge peer (worth a warning).
    fn record_peer(&self, ip: IpAddr, bridge: bool) -> bool {
        *lock(&self.last_peer_ip) = Some(ip);
        bridge && self.bridge_peers.fetch_add(1, Ordering::SeqCst) == 0
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The listener of one server. Built in `App::build` when `ORBIT_MAIL_ENABLED=true`, run in
/// `App::serve`.
pub struct MailListener {
    config: MailConfig,
    hostname: String,
    intake: MailIntake,
    repo: Arc<MailRepository>,
    /// Direct mode only: relay mode trusts the relay's results.
    auth: Option<Authenticator>,
    tls: Option<Arc<TlsAcceptorHandle>>,
    limiter: ConnLimiter,
    limits: SessionLimits,
    /// Docker gateways besides `172.17.0.1` (this host's default routes).
    gateways: Vec<IpAddr>,
    status: Arc<MailListenerStatus>,
}

impl MailListener {
    /// Loads the certificate (when configured) and, in direct mode, the system or configured DNS
    /// resolver. `config` must be valid and enabled.
    pub fn new(
        config: MailConfig,
        intake: MailIntake,
        repo: Arc<MailRepository>,
    ) -> Result<Self, String> {
        let hostname = config
            .hostname
            .clone()
            .ok_or("ORBIT_MAIL_HOSTNAME is required")?;
        let tls = match (&config.tls_cert, &config.tls_key) {
            (Some(cert), Some(key)) => Some(Arc::new(
                TlsAcceptorHandle::load(cert, key).map_err(|error| error.to_string())?,
            )),
            _ => None,
        };
        if config.mode == MailMode::Direct
            && let Some(tls) = &tls
            && !tls.info().covers(&hostname)
        {
            tracing::warn!(
                hostname,
                names = ?tls.info().names,
                "the mail TLS certificate does not cover ORBIT_MAIL_HOSTNAME"
            );
        }
        let auth = match config.mode {
            MailMode::Relay => None,
            MailMode::Direct => Some(
                match config.dns_resolver {
                    Some(resolver) => Authenticator::with_resolver(resolver),
                    None => Authenticator::system(),
                }
                .map_err(|error| error.to_string())?,
            ),
        };
        let limits = SessionLimits {
            max_message_bytes: config.max_message_bytes,
            ..SessionLimits::default()
        };
        let status = Arc::new(MailListenerStatus::new(tls.clone()));
        Ok(Self {
            hostname,
            intake,
            repo,
            auth,
            tls,
            limiter: ConnLimiter::default(),
            limits,
            gateways: default_gateways(),
            status,
            config,
        })
    }

    #[must_use]
    pub fn status(&self) -> Arc<MailListenerStatus> {
        Arc::clone(&self.status)
    }

    /// Replaces the direct mode checker (tests use fixed DNS answers).
    pub fn set_authenticator(&mut self, authenticator: Authenticator) {
        if self.config.mode == MailMode::Direct {
            self.auth = Some(authenticator);
        }
    }

    /// Binds `ORBIT_MAIL_SMTP_BIND:ORBIT_MAIL_SMTP_PORT`.
    pub async fn bind(config: &MailConfig) -> io::Result<TcpListener> {
        TcpListener::bind(SocketAddr::new(config.smtp_bind, config.smtp_port)).await
    }

    /// Serves until `cancel`: then stops accepting, sends `421 4.3.2` to the open sessions and
    /// waits up to `drain` for them to end.
    pub async fn run(
        self,
        listener: TcpListener,
        cancel: CancellationToken,
        drain: Duration,
    ) -> Result<(), String> {
        let address = listener.local_addr().map_err(|error| error.to_string())?;
        let this = Arc::new(self);
        let sessions = TaskTracker::new();
        let watcher = this
            .tls
            .as_ref()
            .map(|tls| Arc::clone(tls).spawn_watcher(CERT_POLL, cancel.clone()));
        this.status.set_running(Some(address));
        tracing::info!(%address, mode = ?this.config.mode, "mail listener started");
        loop {
            let accepted = tokio::select! {
                () = cancel.cancelled() => break,
                accepted = listener.accept() => accepted,
            };
            match accepted {
                Ok((stream, peer)) => {
                    let this = Arc::clone(&this);
                    let cancel = cancel.clone();
                    sessions.spawn(async move { this.connection(stream, peer, &cancel).await });
                }
                Err(error) => {
                    // Usually out of file descriptors; give the open sessions time to end.
                    tracing::warn!(%error, "mail listener accept failed");
                    tokio::select! {
                        () = cancel.cancelled() => break,
                        () = tokio::time::sleep(Duration::from_millis(100)) => {}
                    }
                }
            }
        }
        drop(listener);
        this.status.set_running(None);
        sessions.close();
        if tokio::time::timeout(drain, sessions.wait()).await.is_err() {
            tracing::warn!("mail sessions did not end within the drain time");
        }
        if let Some(watcher) = watcher {
            let _ = watcher.await;
        }
        Ok(())
    }

    async fn connection(
        &self,
        mut stream: TcpStream,
        peer: SocketAddr,
        cancel: &CancellationToken,
    ) {
        let _ = stream.set_nodelay(true);
        let mut ip = peer.ip().to_canonical();
        if self
            .config
            .proxy_cidrs
            .iter()
            .any(|cidr| cidr.contains(&ip))
        {
            match read_proxy_v2(&mut stream, PROXY_TIMEOUT).await {
                Ok(Some(client)) => ip = client.ip().to_canonical(),
                Ok(None) => {}
                Err(error) => {
                    tracing::debug!(%peer, %error, "mail connection without a valid PROXY v2 header");
                    return;
                }
            }
        }
        if self
            .status
            .record_peer(ip, is_docker_bridge(ip, &self.gateways))
        {
            tracing::warn!(
                %ip,
                "mail client connected from a Docker bridge gateway: client addresses are lost; \
                 use network_mode: host or userland-proxy: false"
            );
        }
        if self.config.mode == MailMode::Relay
            && !self
                .config
                .relay_cidrs
                .iter()
                .any(|cidr| cidr.contains(&ip))
        {
            refuse(stream, NOT_A_RELAY).await;
            return;
        }
        let Some(_permit) = self.limiter.try_acquire(ip) else {
            refuse(stream, TOO_MANY_CONNECTIONS).await;
            return;
        };
        let ctx = SessionContext {
            peer_ip: ip,
            helo: None,
            tls: None,
        };
        let result = match &self.tls {
            Some(tls) => {
                run_session(
                    stream,
                    ctx,
                    self,
                    &self.limits,
                    &self.hostname,
                    Some(&**tls),
                    cancel,
                )
                .await
            }
            None => {
                run_session(
                    stream,
                    ctx,
                    self,
                    &self.limits,
                    &self.hostname,
                    None::<&NoTls>,
                    cancel,
                )
                .await
            }
        };
        if let Err(error) = result {
            tracing::debug!(%ip, %error, "mail session ended with an error");
        }
    }

    /// RFC 5321 §4.5.1: bare `postmaster` must be accepted. It means the postmaster of the mail
    /// host: `postmaster@<ORBIT_MAIL_HOSTNAME>`, else `postmaster@<its parent domain>`.
    async fn postmaster(&self, now: TimestampMillis) -> Result<RecipientLookup, String> {
        let mut lookup = self
            .lookup(&format!("postmaster@{}", self.hostname), now)
            .await?;
        if matches!(lookup, RecipientLookup::NotLocal)
            && let Some((_, parent)) = self.hostname.split_once('.')
            && parent.contains('.')
        {
            lookup = self.lookup(&format!("postmaster@{parent}"), now).await?;
        }
        Ok(lookup)
    }

    async fn lookup(&self, rcpt: &str, now: TimestampMillis) -> Result<RecipientLookup, String> {
        self.repo
            .lookup_recipient(rcpt, now)
            .await
            .map_err(|error| error.to_string())
    }

    /// SPF, DKIM and DMARC: the relay's pinned header in relay mode, Orbit's checks in direct
    /// mode. `None` is a DNS temporary error.
    async fn authenticate(
        &self,
        ctx: &SessionContext,
        envelope: &Envelope,
        raw: &[u8],
    ) -> Option<AuthOutcome> {
        let Some(auth) = &self.auth else {
            let relay_id = self.config.relay_authserv_id.as_deref().unwrap_or_default();
            return Some(trusted_relay_results(raw, relay_id).unwrap_or_else(|| {
                tracing::warn!(
                    relay_id,
                    "relayed message without the relay's Authentication-Results header"
                );
                unchecked()
            }));
        };
        let helo = ctx.helo.as_deref().unwrap_or_default();
        let check = auth.check(raw, ctx.peer_ip, helo, envelope.mail_from.as_deref());
        match tokio::time::timeout(AUTH_TIMEOUT, check).await {
            Ok(outcome) if !outcome.temp_error => Some(outcome),
            _ => None,
        }
    }
}

impl SessionHandler for MailListener {
    async fn rcpt(&self, _ctx: &SessionContext, rcpt: &str) -> RcptDecision {
        let now = TimestampMillis::now();
        let lookup = if rcpt.eq_ignore_ascii_case("postmaster") {
            self.postmaster(now).await
        } else {
            self.lookup(rcpt, now).await
        };
        let accept = |address_id: String, workspace_id: String| {
            RcptDecision::Accept(AcceptedRecipient {
                address_id,
                workspace_id,
                rcpt: rcpt.to_owned(),
            })
        };
        match lookup {
            Ok(RecipientLookup::Accept {
                address_id,
                workspace_id,
                ..
            }) => accept(address_id.to_string(), workspace_id.to_string()),
            Ok(RecipientLookup::HealthTest { test_id }) => {
                accept(format!("{HEALTH_TEST}{test_id}"), String::new())
            }
            Ok(RecipientLookup::UnknownUser) => RcptDecision::Reject(Reply::USER_UNKNOWN),
            Ok(RecipientLookup::NotLocal) => RcptDecision::Reject(Reply::RELAY_DENIED),
            Ok(RecipientLookup::DomainUnavailable) => {
                RcptDecision::Reject(Reply::DOMAIN_UNAVAILABLE)
            }
            Ok(RecipientLookup::MailboxSuspended) => RcptDecision::Reject(Reply::MAILBOX_SUSPENDED),
            Ok(RecipientLookup::Invalid) => RcptDecision::Reject(Reply::BAD_RECIPIENT),
            Err(error) => {
                tracing::warn!(%error, "mail recipient lookup failed");
                RcptDecision::Reject(Reply::STORAGE_TEMPORARY)
            }
        }
    }

    async fn data(&self, ctx: &SessionContext, envelope: &Envelope, raw: Vec<u8>) -> DataDecision {
        let raw = strip_auth_results(&raw, &self.hostname);
        let Some(auth) = self.authenticate(ctx, envelope, &raw).await else {
            return DataDecision::Reject(Reply::DNS_TEMPORARY);
        };
        let Some(recipients) = envelope
            .recipients
            .iter()
            .map(recipient)
            .collect::<Option<Vec<_>>>()
        else {
            tracing::error!("mail session returned a recipient the listener did not accept");
            return DataDecision::Reject(Reply::STORAGE_TEMPORARY);
        };

        let mut message = trace_headers(&self.hostname, ctx, &auth).into_bytes();
        let trace_len = message.len();
        message.extend_from_slice(&raw);
        let inbound = InboundMessage {
            source: Source::Smtp,
            raw: message,
            trace_len,
            mail_from: envelope.mail_from.clone(),
            recipients,
            peer_ip: ctx.peer_ip,
            helo: ctx.helo.clone().unwrap_or_default(),
            tls: ctx.tls.clone(),
            auth,
        };
        match self.intake.accept(inbound).await {
            Ok(_) => DataDecision::Accepted,
            Err(IntakeError::Temporary(error)) => {
                tracing::warn!(%error, "received mail could not be stored");
                DataDecision::Reject(Reply::STORAGE_TEMPORARY)
            }
            Err(IntakeError::QuotaExceeded) => DataDecision::Reject(Reply::QUOTA_EXCEEDED),
            Err(IntakeError::TooLarge) => DataDecision::Reject(Reply::MESSAGE_TOO_BIG),
        }
    }
}

/// Maps a session recipient back to the ids [`MailListener::rcpt`] gave it.
fn recipient(accepted: &AcceptedRecipient) -> Option<Recipient> {
    let rcpt = accepted.rcpt.clone();
    if let Some(test_id) = accepted.address_id.strip_prefix(HEALTH_TEST) {
        return Some(Recipient::HealthTest {
            test_id: test_id.parse().ok()?,
            rcpt,
        });
    }
    Some(Recipient::Mailbox {
        workspace_id: accepted.workspace_id.parse::<Id>().ok()?,
        address_id: accepted.address_id.parse::<Id>().ok()?,
        rcpt,
    })
}

/// No results: a relayed message without the relay's header.
fn unchecked() -> AuthOutcome {
    AuthOutcome {
        spf: Verdict::None,
        dkim: Verdict::None,
        dmarc: DmarcVerdict::None,
        aligned_domain: None,
        temp_error: false,
        spf_domain: None,
        dkim_domain: None,
        from_domain: None,
    }
}

/// The `Received` (RFC 5321 §4.4) and `Authentication-Results` (RFC 8601) fields Orbit puts on
/// top of the message, CRLF-terminated and folded.
fn trace_headers(hostname: &str, ctx: &SessionContext, auth: &AuthOutcome) -> String {
    let helo = ctx
        .helo
        .as_deref()
        .filter(|helo| {
            !helo.is_empty()
                && helo.len() <= 255
                && helo.bytes().all(|b| {
                    b.is_ascii_alphanumeric()
                        || matches!(b, b'.' | b'-' | b'_' | b':' | b'[' | b']')
                })
        })
        .unwrap_or("unknown");
    let ip = match ctx.peer_ip {
        IpAddr::V4(ip) => ip.to_string(),
        IpAddr::V6(ip) => format!("IPv6:{ip}"),
    };
    let protocol = match &ctx.tls {
        Some(tls) => format!(
            "ESMTPS ({} {})",
            safe_text(&tls.version),
            safe_text(&tls.cipher)
        ),
        None => "ESMTP".to_owned(),
    };
    let date = chrono::Utc::now().to_rfc2822();
    let results = authentication_results(hostname, auth).replace("; ", ";\r\n\t");
    format!(
        "Received: from {helo} ([{ip}])\r\n\tby {hostname} (Orbit) with {protocol};\r\n\t{date}\r\n\
         Authentication-Results: {results}\r\n"
    )
}

fn safe_text(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        .collect()
}

async fn refuse(mut stream: TcpStream, reply: Reply) {
    let line = format!("{reply}\r\n");
    let write = async {
        stream.write_all(line.as_bytes()).await?;
        stream.shutdown().await
    };
    let _ = tokio::time::timeout(Duration::from_secs(10), write).await;
}

/// The gateways of this host's IPv4 default routes (`/proc/net/route`). Inside a container on a
/// bridge network that is the bridge gateway, so clients behind Docker's userland proxy show up
/// as it. Empty when the table cannot be read.
fn default_gateways() -> Vec<IpAddr> {
    let Ok(table) = std::fs::read_to_string("/proc/net/route") else {
        return Vec::new();
    };
    table
        .lines()
        .skip(1)
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let (destination, gateway) = (*fields.get(1)?, *fields.get(2)?);
            if destination != "00000000" {
                return None;
            }
            // The address in network byte order, printed as a host-order number.
            let raw = u32::from_str_radix(gateway, 16)
                .ok()
                .filter(|raw| *raw != 0)?;
            Some(IpAddr::V4(Ipv4Addr::from(raw.to_ne_bytes())))
        })
        .collect()
}
