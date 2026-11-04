//! The STARTTLS acceptor with a reloadable certificate (spec §4.5, direct mode).
//!
//! The certificate chain and key are PEM files (`ORBIT_MAIL_TLS_CERT` / `ORBIT_MAIL_TLS_KEY`). A watcher polls their
//! modification times and reloads them when they change; a pair that cannot be read or does not match keeps the last
//! good configuration. rustls with the `ring` provider, TLS 1.2 and 1.3 only.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError, RwLock};
use std::time::{Duration, SystemTime};

use rustls::ServerConfig;
use rustls::pki_types::pem::{self, PemObject};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::task::JoinHandle;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::server::TlsStream;
use tokio_util::sync::CancellationToken;
use x509_parser::extensions::GeneralName;

use super::{StartTls, TlsInfo};

/// How long before `not_after` Orbit starts to warn (spec §4.5).
pub const CERT_EXPIRY_WARNING: Duration = Duration::from_secs(7 * 24 * 60 * 60);

#[derive(Debug, Error)]
pub enum TlsError {
    #[error("cannot read {path}: {source}")]
    Read { path: PathBuf, source: io::Error },
    #[error("cannot parse {path}: {source}")]
    Pem { path: PathBuf, source: pem::Error },
    #[error("no certificate in {0}")]
    NoCertificate(PathBuf),
    #[error("no private key in {0}")]
    NoKey(PathBuf),
    #[error("bad certificate in {path}: {reason}")]
    Certificate { path: PathBuf, reason: String },
    #[error("TLS configuration refused: {0}")]
    Rustls(#[from] rustls::Error),
}

/// What the loaded leaf certificate covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CertInfo {
    /// DNS names from the subject alternative names (the subject CN when there are none), lowercased.
    pub names: Vec<String>,
    /// Unix seconds.
    pub not_after: i64,
}

impl CertInfo {
    /// Whether one of the names matches `hostname`; `*.example.com` matches exactly one label.
    pub fn covers(&self, hostname: &str) -> bool {
        let host = hostname.trim_end_matches('.').to_ascii_lowercase();
        self.names.iter().any(|name| match name.strip_prefix("*.") {
            Some(parent) => host
                .split_once('.')
                .is_some_and(|(label, rest)| !label.is_empty() && rest == parent),
            None => *name == host,
        })
    }

    /// Whether the certificate expires within [`CERT_EXPIRY_WARNING`] of `now` (Unix seconds), or already has.
    pub fn expires_soon(&self, now: i64) -> bool {
        self.not_after - now <= CERT_EXPIRY_WARNING.as_secs() as i64
    }
}

struct Loaded {
    config: Arc<ServerConfig>,
    info: CertInfo,
}

/// The rustls acceptor behind `STARTTLS`. Cheap to read on every handshake; [`reload`](Self::reload) swaps the
/// configuration for new connections only.
pub struct TlsAcceptorHandle {
    cert_path: PathBuf,
    key_path: PathBuf,
    current: RwLock<Loaded>,
}

impl std::fmt::Debug for TlsAcceptorHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TlsAcceptorHandle")
            .field("cert_path", &self.cert_path)
            .field("key_path", &self.key_path)
            .field("info", &self.info())
            .finish()
    }
}

impl TlsAcceptorHandle {
    /// Reads the PEM certificate chain (leaf first) and private key.
    pub fn load(cert: &Path, key: &Path) -> Result<Self, TlsError> {
        let loaded = read_pair(cert, key)?;
        Ok(Self {
            cert_path: cert.to_owned(),
            key_path: key.to_owned(),
            current: RwLock::new(loaded),
        })
    }

    /// Reads the files again. On error the current configuration stays in use.
    pub fn reload(&self) -> Result<CertInfo, TlsError> {
        let loaded = read_pair(&self.cert_path, &self.key_path)?;
        let info = loaded.info.clone();
        *self.current.write().unwrap_or_else(PoisonError::into_inner) = loaded;
        Ok(info)
    }

    pub fn info(&self) -> CertInfo {
        self.read().info.clone()
    }

    /// Polls the files' modification times every `every` (the listener uses 60 s) and reloads when either
    /// changes. Failures are logged and the last good certificate stays; the watcher stops on `cancel`.
    pub fn spawn_watcher(
        self: Arc<Self>,
        every: Duration,
        cancel: CancellationToken,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            warn_if_expiring(&self.info());
            let mut seen = self.mtimes();
            let mut tick = tokio::time::interval(every);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            tick.tick().await;
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => return,
                    _ = tick.tick() => {}
                }
                let handle = self.clone();
                let Ok((mtimes, outcome)) = tokio::task::spawn_blocking(move || {
                    let mtimes = handle.mtimes();
                    let outcome = (mtimes != seen).then(|| handle.reload());
                    (mtimes, outcome)
                })
                .await
                else {
                    continue;
                };
                // Remember the attempt either way: a broken file is retried once it changes again, not every poll.
                seen = mtimes;
                match outcome {
                    None => {}
                    Some(Ok(info)) => {
                        tracing::info!(names = ?info.names, not_after = info.not_after, "mail TLS certificate reloaded");
                        warn_if_expiring(&info);
                    }
                    Some(Err(error)) => {
                        tracing::warn!(%error, "mail TLS certificate not reloaded, keeping the previous one");
                    }
                }
            }
        })
    }

    fn mtimes(&self) -> [Option<SystemTime>; 2] {
        [&self.cert_path, &self.key_path]
            .map(|path| fs::metadata(path).and_then(|m| m.modified()).ok())
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, Loaded> {
        self.current.read().unwrap_or_else(PoisonError::into_inner)
    }
}

impl StartTls for TlsAcceptorHandle {
    type Stream<S>
        = TlsStream<S>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send;

    async fn accept<S>(&self, stream: S) -> io::Result<(TlsStream<S>, TlsInfo)>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send,
    {
        let acceptor = TlsAcceptor::from(self.read().config.clone());
        let stream = acceptor.accept(stream).await?;
        let (_, conn) = stream.get_ref();
        let version = match conn.protocol_version() {
            Some(rustls::ProtocolVersion::TLSv1_3) => "TLSv1.3".to_owned(),
            Some(rustls::ProtocolVersion::TLSv1_2) => "TLSv1.2".to_owned(),
            other => format!("{other:?}"),
        };
        let cipher = conn
            .negotiated_cipher_suite()
            .map(|suite| {
                let suite = suite.suite();
                suite
                    .as_str()
                    .map_or_else(|| format!("{suite:?}"), str::to_owned)
            })
            .unwrap_or_default();
        Ok((stream, TlsInfo { version, cipher }))
    }
}

fn warn_if_expiring(info: &CertInfo) {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    if info.expires_soon(now) {
        tracing::warn!(names = ?info.names, not_after = info.not_after, "mail TLS certificate expires soon");
    }
}

fn read_pair(cert_path: &Path, key_path: &Path) -> Result<Loaded, TlsError> {
    let cert_pem = read_file(cert_path)?;
    let key_pem = read_file(key_path)?;
    let chain = CertificateDer::pem_slice_iter(&cert_pem)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| TlsError::Pem {
            path: cert_path.to_owned(),
            source,
        })?;
    let Some(leaf) = chain.first() else {
        return Err(TlsError::NoCertificate(cert_path.to_owned()));
    };
    let info = cert_info(leaf).map_err(|reason| TlsError::Certificate {
        path: cert_path.to_owned(),
        reason,
    })?;
    let key = PrivateKeyDer::from_pem_slice(&key_pem).map_err(|source| match source {
        pem::Error::NoItemsFound => TlsError::NoKey(key_path.to_owned()),
        source => TlsError::Pem {
            path: key_path.to_owned(),
            source,
        },
    })?;

    // `with_single_cert` also checks that the key belongs to the leaf certificate.
    let config =
        ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])?
            .with_no_client_auth()
            .with_single_cert(chain, key)?;
    Ok(Loaded {
        config: Arc::new(config),
        info,
    })
}

fn read_file(path: &Path) -> Result<Vec<u8>, TlsError> {
    fs::read(path).map_err(|source| TlsError::Read {
        path: path.to_owned(),
        source,
    })
}

fn cert_info(der: &CertificateDer<'_>) -> Result<CertInfo, String> {
    let (_, cert) = x509_parser::parse_x509_certificate(der).map_err(|e| e.to_string())?;
    let mut names: Vec<String> = match cert.subject_alternative_name().map_err(|e| e.to_string())? {
        Some(san) => san
            .value
            .general_names
            .iter()
            .filter_map(|name| match name {
                GeneralName::DNSName(dns) => Some(dns.to_ascii_lowercase()),
                _ => None,
            })
            .collect(),
        None => Vec::new(),
    };
    if names.is_empty() {
        names = cert
            .subject()
            .iter_common_name()
            .filter_map(|cn| cn.as_str().ok())
            .map(str::to_ascii_lowercase)
            .collect();
    }
    Ok(CertInfo {
        names,
        not_after: cert.validity().not_after.timestamp(),
    })
}
