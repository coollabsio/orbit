//! TLS with certificate reload, PROXY protocol v2, connection limits and the Docker bridge check.

use std::fs;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use orbit_mail::{
    AcceptedRecipient, CertInfo, ConnLimiter, DataDecision, Envelope, ProxyError, RcptDecision,
    SessionContext, SessionHandler, SessionLimits, TlsAcceptorHandle, TlsError, is_docker_bridge,
    read_proxy_v2, run_session,
};
use rcgen::{CertificateParams, KeyPair, date_time_ymd};
use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{ClientConfig, RootCertStore, SupportedProtocolVersion};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio_rustls::TlsConnector;
use tokio_util::sync::CancellationToken;

const HOST: &str = "mx.orbit.test";
/// 2030-01-01T00:00:00Z.
const NOT_AFTER_2030: i64 = 1_893_456_000;

// ---------------------------------------------------------------------------------------------------------------
// Certificates

struct Cert {
    cert_pem: String,
    key_pem: String,
    der: CertificateDer<'static>,
}

fn make_cert(names: &[&str], year: i32) -> Cert {
    let key = KeyPair::generate().unwrap();
    let mut params =
        CertificateParams::new(names.iter().map(|n| n.to_string()).collect::<Vec<_>>()).unwrap();
    params.not_after = date_time_ymd(year, 1, 1);
    let cert = params.self_signed(&key).unwrap();
    Cert {
        cert_pem: cert.pem(),
        key_pem: key.serialize_pem(),
        der: cert.der().clone(),
    }
}

struct CertDir {
    dir: PathBuf,
    cert: PathBuf,
    key: PathBuf,
}

impl CertDir {
    fn new(cert: &Cert) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "orbit-mail-tls-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let this = Self {
            cert: dir.join("cert.pem"),
            key: dir.join("key.pem"),
            dir,
        };
        this.write(cert);
        this
    }

    fn write(&self, cert: &Cert) {
        fs::write(&self.cert, &cert.cert_pem).unwrap();
        fs::write(&self.key, &cert.key_pem).unwrap();
    }

    fn load(&self) -> Arc<TlsAcceptorHandle> {
        Arc::new(TlsAcceptorHandle::load(&self.cert, &self.key).unwrap())
    }
}

impl Drop for CertDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

// ---------------------------------------------------------------------------------------------------------------
// STARTTLS end to end

#[derive(Default)]
struct Handler {
    messages: Mutex<Vec<(Vec<u8>, SessionContext)>>,
}

impl SessionHandler for Handler {
    async fn rcpt(&self, _ctx: &SessionContext, rcpt: &str) -> RcptDecision {
        RcptDecision::Accept(AcceptedRecipient {
            address_id: "a".into(),
            workspace_id: "w".into(),
            rcpt: rcpt.to_owned(),
        })
    }

    async fn data(&self, ctx: &SessionContext, _env: &Envelope, raw: Vec<u8>) -> DataDecision {
        self.messages.lock().unwrap().push((raw, ctx.clone()));
        DataDecision::Accepted
    }
}

async fn reply<S: AsyncRead + Unpin>(io: &mut BufReader<S>) -> Vec<String> {
    let mut lines = Vec::new();
    loop {
        let mut line = String::new();
        assert!(
            io.read_line(&mut line).await.unwrap() > 0,
            "closed after {lines:?}"
        );
        let line = line.trim_end().to_owned();
        let last = line.as_bytes().get(3) != Some(&b'-');
        lines.push(line);
        if last {
            return lines;
        }
    }
}

async fn cmd<S: AsyncRead + AsyncWrite + Unpin>(io: &mut BufReader<S>, line: &str) -> Vec<String> {
    io.get_mut()
        .write_all(format!("{line}\r\n").as_bytes())
        .await
        .unwrap();
    reply(io).await
}

fn client_config(
    root: &CertificateDer<'static>,
    versions: &[&'static SupportedProtocolVersion],
) -> ClientConfig {
    let mut roots = RootCertStore::empty();
    roots.add(root.clone()).unwrap();
    ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_protocol_versions(versions)
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth()
}

/// Runs a session with `handle`, upgrades with STARTTLS, sends one message and returns what the handler saw.
async fn deliver_over_tls(
    handle: Arc<TlsAcceptorHandle>,
    root: &CertificateDer<'static>,
    versions: &[&'static SupportedProtocolVersion],
) -> (Vec<u8>, SessionContext) {
    let (client, server) = tokio::io::duplex(1 << 16);
    let handler = Arc::new(Handler::default());
    let task = {
        let handler = handler.clone();
        tokio::spawn(async move {
            let limits = SessionLimits {
                greeting_delay: Duration::from_millis(10),
                ..SessionLimits::default()
            };
            let ctx = SessionContext {
                peer_ip: IpAddr::V4(Ipv4Addr::new(192, 0, 2, 7)),
                helo: None,
                tls: None,
            };
            let cancel = CancellationToken::new();
            run_session(
                server,
                ctx,
                &*handler,
                &limits,
                HOST,
                Some(&*handle),
                &cancel,
            )
            .await
            .unwrap();
        })
    };

    let mut io = BufReader::new(client);
    assert!(reply(&mut io).await[0].starts_with("220 mx.orbit.test"));
    let ehlo = cmd(&mut io, "EHLO client.example").await;
    assert!(ehlo.iter().any(|l| l.ends_with("STARTTLS")), "{ehlo:?}");
    assert!(cmd(&mut io, "STARTTLS").await[0].starts_with("220 2.0.0"));
    assert!(io.buffer().is_empty());

    let connector = TlsConnector::from(Arc::new(client_config(root, versions)));
    let tls = connector
        .connect(ServerName::try_from(HOST).unwrap(), io.into_inner())
        .await
        .unwrap();
    let mut io = BufReader::new(tls);
    let ehlo = cmd(&mut io, "EHLO client.example").await;
    assert!(!ehlo.iter().any(|l| l.contains("STARTTLS")));
    assert!(cmd(&mut io, "MAIL FROM:<ann@example.com>").await[0].starts_with("250"));
    assert!(cmd(&mut io, "RCPT TO:<bob@orbit.test>").await[0].starts_with("250"));
    assert!(cmd(&mut io, "DATA").await[0].starts_with("354"));
    let done = cmd(&mut io, "Subject: hi\r\n\r\nover TLS\r\n.").await;
    assert!(done[0].starts_with("250 2.0.0"), "{done:?}");
    assert!(cmd(&mut io, "QUIT").await[0].starts_with("221"));
    task.await.unwrap();

    let mut messages = handler.messages.lock().unwrap();
    assert_eq!(messages.len(), 1);
    messages.pop().unwrap()
}

#[tokio::test]
async fn starttls_handshake_over_duplex() {
    let cert = make_cert(&[HOST], 2030);
    let dir = CertDir::new(&cert);
    let (raw, ctx) = deliver_over_tls(dir.load(), &cert.der, rustls::ALL_VERSIONS).await;
    assert_eq!(raw, b"Subject: hi\r\n\r\nover TLS\r\n");
    let tls = ctx.tls.unwrap();
    assert_eq!(tls.version, "TLSv1.3");
    assert!(tls.cipher.starts_with("TLS13_"), "{}", tls.cipher);
    assert_eq!(ctx.helo.as_deref(), Some("client.example"));
}

#[tokio::test]
async fn starttls_accepts_tls12() {
    let cert = make_cert(&[HOST], 2030);
    let dir = CertDir::new(&cert);
    let (_, ctx) = deliver_over_tls(dir.load(), &cert.der, &[&rustls::version::TLS12]).await;
    let tls = ctx.tls.unwrap();
    assert_eq!(tls.version, "TLSv1.2");
    assert!(tls.cipher.starts_with("TLS_ECDHE_"), "{}", tls.cipher);
}

// ---------------------------------------------------------------------------------------------------------------
// Loading and reload

#[test]
fn load_reports_names_and_not_after() {
    let cert = make_cert(&[HOST, "Mail.Orbit.Test"], 2030);
    let dir = CertDir::new(&cert);
    let handle = dir.load();
    let info = handle.info();
    assert_eq!(
        info.names,
        vec![HOST.to_owned(), "mail.orbit.test".to_owned()]
    );
    assert_eq!(info.not_after, NOT_AFTER_2030);
}

#[test]
fn cert_info_covers_names_and_wildcards() {
    let info = CertInfo {
        names: vec!["*.orbit.test".into(), "orbit.example".into()],
        not_after: 0,
    };
    assert!(info.covers("mx.orbit.test"));
    assert!(info.covers("MX.Orbit.Test."));
    assert!(info.covers("orbit.example"));
    assert!(!info.covers("orbit.test"));
    assert!(!info.covers("a.mx.orbit.test"));
    assert!(!info.covers("mx.orbit.example"));
}

#[test]
fn cert_info_expiry_warning() {
    let info = CertInfo {
        names: vec![],
        not_after: NOT_AFTER_2030,
    };
    let day = 24 * 60 * 60;
    assert!(!info.expires_soon(NOT_AFTER_2030 - 8 * day));
    assert!(info.expires_soon(NOT_AFTER_2030 - 6 * day));
    assert!(info.expires_soon(NOT_AFTER_2030 + 1));
}

#[test]
fn load_refuses_missing_and_broken_files() {
    let cert = make_cert(&[HOST], 2030);
    let dir = CertDir::new(&cert);
    let missing = TlsAcceptorHandle::load(&dir.dir.join("nope.pem"), &dir.key);
    assert!(matches!(missing, Err(TlsError::Read { .. })), "{missing:?}");

    fs::write(&dir.cert, "not a certificate").unwrap();
    let broken = TlsAcceptorHandle::load(&dir.cert, &dir.key);
    assert!(
        matches!(broken, Err(TlsError::NoCertificate(_))),
        "{broken:?}"
    );

    dir.write(&cert);
    fs::write(&dir.key, "not a key").unwrap();
    assert!(TlsAcceptorHandle::load(&dir.cert, &dir.key).is_err());
}

#[test]
fn reload_picks_up_a_new_certificate() {
    let dir = CertDir::new(&make_cert(&[HOST], 2030));
    let handle = dir.load();
    dir.write(&make_cert(&["mx2.orbit.test"], 2031));
    let info = handle.reload().unwrap();
    assert_eq!(info.names, vec!["mx2.orbit.test".to_owned()]);
    assert_eq!(handle.info(), info);
}

#[tokio::test]
async fn reload_with_a_broken_file_keeps_the_old_certificate() {
    let cert = make_cert(&[HOST], 2030);
    let dir = CertDir::new(&cert);
    let handle = dir.load();
    let before = handle.info();

    fs::write(
        &dir.cert,
        "-----BEGIN CERTIFICATE-----\ngarbage\n-----END CERTIFICATE-----\n",
    )
    .unwrap();
    assert!(handle.reload().is_err());
    assert_eq!(handle.info(), before);

    // A new certificate with the old key (a half-finished renewal) is refused too.
    let other = make_cert(&["mx2.orbit.test"], 2031);
    fs::write(&dir.cert, &other.cert_pem).unwrap();
    let mismatch = handle.reload();
    assert!(matches!(mismatch, Err(TlsError::Rustls(_))), "{mismatch:?}");
    assert_eq!(handle.info(), before);

    // Handshakes still use the old certificate.
    let (_, ctx) = deliver_over_tls(handle, &cert.der, rustls::ALL_VERSIONS).await;
    assert!(ctx.tls.is_some());
}

#[tokio::test]
async fn watcher_reloads_when_the_files_change() {
    let dir = CertDir::new(&make_cert(&[HOST], 2030));
    let handle = dir.load();
    let cancel = CancellationToken::new();
    let watcher = handle
        .clone()
        .spawn_watcher(Duration::from_millis(20), cancel.clone());

    // Make sure the new files get a different mtime even on coarse clocks.
    tokio::time::sleep(Duration::from_millis(30)).await;
    let renewed = make_cert(&["mx2.orbit.test"], 2031);
    dir.write(&renewed);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while handle.info().names != ["mx2.orbit.test"] {
        assert!(
            tokio::time::Instant::now() < deadline,
            "watcher did not reload"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    // A broken write is ignored; the renewed certificate stays.
    fs::write(&dir.cert, "broken").unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(handle.info().names, ["mx2.orbit.test"]);

    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(5), watcher)
        .await
        .unwrap()
        .unwrap();
}

// ---------------------------------------------------------------------------------------------------------------
// PROXY protocol v2

const SIG: [u8; 12] = [
    0x0D, 0x0A, 0x0D, 0x0A, 0x00, 0x0D, 0x0A, 0x51, 0x55, 0x49, 0x54, 0x0A,
];

fn header(ver_cmd: u8, fam: u8, body: &[u8]) -> Vec<u8> {
    let mut out = SIG.to_vec();
    out.push(ver_cmd);
    out.push(fam);
    out.extend_from_slice(&(body.len() as u16).to_be_bytes());
    out.extend_from_slice(body);
    out
}

fn v4_body() -> Vec<u8> {
    let mut body = vec![203, 0, 113, 9, 10, 0, 0, 2];
    body.extend_from_slice(&40000u16.to_be_bytes());
    body.extend_from_slice(&25u16.to_be_bytes());
    body
}

const TIMEOUT: Duration = Duration::from_secs(3);

async fn parse(bytes: &[u8]) -> (Result<Option<SocketAddr>, ProxyError>, Vec<u8>) {
    let mut stream = bytes;
    let result = read_proxy_v2(&mut stream, TIMEOUT).await;
    (result, stream.to_vec())
}

#[tokio::test]
async fn proxy_v2_tcp4() {
    let mut bytes = header(0x21, 0x11, &v4_body());
    bytes.extend_from_slice(b"EHLO x\r\n");
    let (result, rest) = parse(&bytes).await;
    assert_eq!(result.unwrap(), Some("203.0.113.9:40000".parse().unwrap()));
    // Exactly the header is consumed.
    assert_eq!(rest, b"EHLO x\r\n");
}

#[tokio::test]
async fn proxy_v2_tcp6() {
    let src: Ipv6Addr = "2001:db8::1".parse().unwrap();
    let dst: Ipv6Addr = "2001:db8::2".parse().unwrap();
    let mut body = src.octets().to_vec();
    body.extend_from_slice(&dst.octets());
    body.extend_from_slice(&50000u16.to_be_bytes());
    body.extend_from_slice(&25u16.to_be_bytes());
    let (result, rest) = parse(&header(0x21, 0x21, &body)).await;
    assert_eq!(
        result.unwrap(),
        Some("[2001:db8::1]:50000".parse().unwrap())
    );
    assert!(rest.is_empty());
}

#[tokio::test]
async fn proxy_v2_local_and_unspec_use_the_socket_address() {
    // LOCAL (a health check) may still carry an address block; it is skipped.
    let mut bytes = header(0x20, 0x11, &v4_body());
    bytes.extend_from_slice(b"QUIT\r\n");
    let (result, rest) = parse(&bytes).await;
    assert_eq!(result.unwrap(), None);
    assert_eq!(rest, b"QUIT\r\n");

    let (result, _) = parse(&header(0x21, 0x00, &[])).await;
    assert_eq!(result.unwrap(), None);
    // UNIX sockets carry no IP.
    let (result, _) = parse(&header(0x21, 0x31, &[0; 216])).await;
    assert_eq!(result.unwrap(), None);
}

#[tokio::test]
async fn proxy_v2_skips_tlvs() {
    let mut body = v4_body();
    body.extend_from_slice(&[0x04, 0x00, 0x03, 1, 2, 3]); // NOOP
    body.extend_from_slice(&[0xE0, 0x00, 0x00]); // application type, empty
    let (result, rest) = parse(&header(0x21, 0x11, &body)).await;
    assert_eq!(result.unwrap(), Some("203.0.113.9:40000".parse().unwrap()));
    assert!(rest.is_empty());
}

/// CRC-32C (Castagnoli), bitwise, as an independent reference for the test.
fn crc32c(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0x82F6_3B78
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

#[tokio::test]
async fn proxy_v2_checks_crc32c() {
    assert_eq!(crc32c(b"123456789"), 0xE306_9283);
    let mut body = v4_body();
    body.extend_from_slice(&[0x03, 0x00, 0x04, 0, 0, 0, 0]);
    let mut bytes = header(0x21, 0x11, &body);
    let crc = crc32c(&bytes);
    let at = bytes.len() - 4;
    bytes[at..].copy_from_slice(&crc.to_be_bytes());
    let (result, _) = parse(&bytes).await;
    assert_eq!(result.unwrap(), Some("203.0.113.9:40000".parse().unwrap()));

    bytes[at] ^= 0xFF;
    let (result, _) = parse(&bytes).await;
    assert!(matches!(result, Err(ProxyError::Checksum)), "{result:?}");
}

#[tokio::test]
async fn proxy_v2_refuses_garbage() {
    // Plain SMTP, PROXY v1 text and a wrong signature.
    for bytes in [
        b"EHLO client.example\r\n".to_vec(),
        b"PROXY TCP4 203.0.113.9 10.0.0.2 40000 25\r\n".to_vec(),
        {
            let mut b = header(0x21, 0x11, &v4_body());
            b[5] = 0x01;
            b
        },
    ] {
        let (result, _) = parse(&bytes).await;
        assert!(matches!(result, Err(ProxyError::Signature)), "{result:?}");
    }

    let cases: [(Vec<u8>, &str); 5] = [
        (header(0x11, 0x11, &v4_body()), "version"),
        (header(0x22, 0x11, &v4_body()), "command"),
        (header(0x21, 0x41, &v4_body()), "family"),
        (header(0x21, 0x13, &v4_body()), "family"),
        (header(0x21, 0x11, &[1, 2, 3, 4]), "truncated"),
    ];
    for (bytes, what) in cases {
        let (result, _) = parse(&bytes).await;
        let ok = match what {
            "version" => matches!(result, Err(ProxyError::Version(1))),
            "command" => matches!(result, Err(ProxyError::Command(2))),
            "family" => matches!(result, Err(ProxyError::Family(_))),
            _ => matches!(result, Err(ProxyError::Truncated)),
        };
        assert!(ok, "{what}: {result:?}");
    }

    // A TLV that runs past the end of the header.
    let mut body = v4_body();
    body.extend_from_slice(&[0x04, 0x00, 0x09, 1]);
    let (result, _) = parse(&header(0x21, 0x11, &body)).await;
    assert!(matches!(result, Err(ProxyError::Truncated)), "{result:?}");
}

#[tokio::test]
async fn proxy_v2_refuses_a_partial_header() {
    let bytes = header(0x21, 0x11, &v4_body());
    let (result, _) = parse(&bytes[..20]).await;
    assert!(matches!(result, Err(ProxyError::Io(_))), "{result:?}");
}

#[tokio::test(start_paused = true)]
async fn proxy_v2_times_out() {
    let (mut client, mut server) = tokio::io::duplex(1024);
    client.write_all(&SIG[..6]).await.unwrap();
    let started = tokio::time::Instant::now();
    let result = read_proxy_v2(&mut server, TIMEOUT).await;
    assert!(matches!(result, Err(ProxyError::Timeout)), "{result:?}");
    assert_eq!(started.elapsed(), TIMEOUT);
    // The stream is still usable by the caller (it will just be closed).
    let mut buf = [0u8; 1];
    client.write_all(b"x").await.unwrap();
    server.read_exact(&mut buf).await.unwrap();
}

// ---------------------------------------------------------------------------------------------------------------
// Connection limits

fn v4(last: u8) -> IpAddr {
    IpAddr::V4(Ipv4Addr::new(198, 51, 100, last))
}

#[test]
fn limiter_per_ip_and_release_on_drop() {
    let limiter = ConnLimiter::default();
    let mut permits: Vec<_> = (0..10)
        .map(|_| limiter.try_acquire(v4(1)).unwrap())
        .collect();
    assert!(limiter.try_acquire(v4(1)).is_none());
    // Another address is not affected.
    let other = limiter.try_acquire(v4(2)).unwrap();
    assert_eq!(limiter.active(), 11);
    drop(permits.pop());
    assert_eq!(limiter.active(), 10);
    assert!(limiter.try_acquire(v4(1)).is_some());
    drop(other);
}

#[test]
fn limiter_per_ipv6_64() {
    let limiter = ConnLimiter::default();
    let ip = |n: u16| IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 1, 0, 0, 0, n));
    let mut permits = Vec::new();
    for n in 0..20 {
        permits.push(limiter.try_acquire(ip(n)).unwrap());
    }
    // A fresh address in the same /64 is refused, one in the next /64 is not.
    assert!(limiter.try_acquire(ip(100)).is_none());
    let next = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 2, 0, 0, 0, 1));
    assert!(limiter.try_acquire(next).is_some());
    // The per-address limit also applies inside a /64.
    permits.clear();
    let held: Vec<_> = (0..10)
        .map(|_| limiter.try_acquire(ip(7)).unwrap())
        .collect();
    assert!(limiter.try_acquire(ip(7)).is_none());
    assert!(limiter.try_acquire(ip(8)).is_some());
    drop(held);
    assert_eq!(limiter.active(), 0);
}

#[test]
fn limiter_global() {
    let limiter = ConnLimiter::new(10, 20, 3);
    let held: Vec<_> = (1..=3)
        .map(|n| limiter.try_acquire(v4(n)).unwrap())
        .collect();
    assert!(limiter.try_acquire(v4(9)).is_none());
    drop(held);
    assert!(limiter.try_acquire(v4(9)).is_some());
}

#[test]
fn limiter_counts_ipv4_mapped_as_ipv4() {
    let limiter = ConnLimiter::default();
    let mapped = IpAddr::V6(Ipv4Addr::new(198, 51, 100, 1).to_ipv6_mapped());
    let held: Vec<_> = (0..5)
        .flat_map(|_| [limiter.try_acquire(v4(1)), limiter.try_acquire(mapped)])
        .map(Option::unwrap)
        .collect();
    assert!(limiter.try_acquire(mapped).is_none());
    assert!(limiter.try_acquire(v4(1)).is_none());
    // Mapped addresses are not one big IPv6 /64 either.
    assert!(
        limiter
            .try_acquire(IpAddr::V6(Ipv4Addr::new(198, 51, 100, 2).to_ipv6_mapped()))
            .is_some()
    );
    drop(held);
}

#[test]
fn permits_move_across_threads() {
    let limiter = ConnLimiter::new(1, 1, 1);
    let permit = limiter.try_acquire(v4(1)).unwrap();
    std::thread::spawn(move || drop(permit)).join().unwrap();
    assert_eq!(limiter.active(), 0);
}

// ---------------------------------------------------------------------------------------------------------------
// Docker bridge

#[test]
fn docker_bridge_gateways() {
    assert!(is_docker_bridge("172.17.0.1".parse().unwrap(), &[]));
    assert!(is_docker_bridge("::ffff:172.17.0.1".parse().unwrap(), &[]));
    assert!(!is_docker_bridge("172.17.0.5".parse().unwrap(), &[]));
    assert!(!is_docker_bridge("203.0.113.9".parse().unwrap(), &[]));
    let compose: IpAddr = "172.20.0.1".parse().unwrap();
    assert!(is_docker_bridge(compose, &[compose]));
    assert!(is_docker_bridge(
        "::ffff:172.20.0.1".parse().unwrap(),
        &[compose]
    ));
    assert!(!is_docker_bridge(compose, &[]));
}
