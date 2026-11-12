//! The SMTP listener over the real server (spec §4.5, §6, §9): `App::build` + `serve` with the
//! listener on a random port, `lettre` (or a raw client) as the sender, fixed DNS answers.

use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use std::time::Duration;

use lettre::address::Envelope;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::transport::smtp::extension::ClientId;
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};
use orbit_mail::{Authenticator, StaticDns};
use orbit_platform::{
    AttachmentMutationCoordinator, Config, EnvironmentMode, Id, LocalBlobStore, MailMode,
    PasswordService, TimestampMillis, UploadLimits, UploadService,
};
use orbit_server::app::App;
use orbit_server::mail_listener::MailListenerStatus;
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::mail::{MailRepository, open_blob, receipt_aad};
use serde_json::Value;
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;

const HOSTNAME: &str = "mx.one.test";
const APP_KEY: [u8; 32] = [1; 32];
const RELAY_ID: &str = "relay.test";

enum Mode {
    /// Direct mode with a self-signed certificate for [`HOSTNAME`] and these DNS answers.
    Direct(StaticDns),
    /// Relay mode: relays in `relay`, PROXY v2 senders in `proxy`.
    Relay {
        relay: &'static str,
        proxy: Option<&'static str>,
    },
}

struct Server {
    database: orbit_platform::Database,
    status: Arc<MailListenerStatus>,
    port: u16,
    shutdown: CancellationToken,
    task: tokio::task::JoinHandle<()>,
    workspace: Id,
    root: TempDir,
}

impl Server {
    async fn start(mode: Mode) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut config = Config {
            environment: EnvironmentMode::Development,
            app_key: Some(orbit_platform::AppKey::new(APP_KEY)),
            ..Config::default()
        };
        config.data.database = root.path().join("data/orbit.sqlite");
        config.data.attachments = root.path().join("data/attachments");
        config.data.backups = root.path().join("backups");
        config.mail.enabled = true;
        config.mail.hostname = Some(HOSTNAME.to_owned());
        config.mail.smtp_bind = IpAddr::V4(Ipv4Addr::LOCALHOST);
        config.mail.smtp_port = 0;
        let mut dns = None;
        match mode {
            Mode::Direct(answers) => {
                let key = rcgen::KeyPair::generate().unwrap();
                let cert = rcgen::CertificateParams::new(vec![HOSTNAME.to_owned()])
                    .unwrap()
                    .self_signed(&key)
                    .unwrap();
                let cert_path = root.path().join("mail-cert.pem");
                let key_path = root.path().join("mail-key.pem");
                std::fs::write(&cert_path, cert.pem()).unwrap();
                std::fs::write(&key_path, key.serialize_pem()).unwrap();
                config.mail.mode = MailMode::Direct;
                config.mail.tls_cert = Some(cert_path);
                config.mail.tls_key = Some(key_path);
                dns = Some(answers);
            }
            Mode::Relay { relay, proxy } => {
                config.mail.mode = MailMode::Relay;
                config.mail.relay_cidrs = vec![relay.parse().unwrap()];
                config.mail.relay_authserv_id = Some(RELAY_ID.to_owned());
                config.mail.proxy_cidrs = proxy.map(|p| p.parse().unwrap()).into_iter().collect();
            }
        }
        let mut app = App::build(config).await.unwrap();
        if let Some(dns) = dns {
            app = app.with_mail_authenticator(Authenticator::with_static_dns(dns).unwrap());
        }
        let database = app.database().clone();
        let status = app.mail_listener_status();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let shutdown = CancellationToken::new();
        let token = shutdown.clone();
        let task = tokio::spawn(async move {
            app.serve(listener, token).await.unwrap();
        });
        let mut port = None;
        for _ in 0..500 {
            if let Some(address) = status.local_addr() {
                port = Some(address.port());
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let mut server = Self {
            database,
            status,
            port: port.expect("the mail listener did not start"),
            shutdown,
            task,
            workspace: Id::new_v7(),
            root,
        };
        server.workspace = server.setup().await;
        server
    }

    async fn setup(&self) -> Id {
        let identity = IdentityRepository::new(self.database.clone());
        let now = TimestampMillis::now();
        identity
            .store_setup_token(
                "mail-setup",
                TimestampMillis::from_millis(now.as_millis() + 60_000),
            )
            .await
            .unwrap();
        identity
            .complete_setup(
                SetupRequest {
                    token: "mail-setup".to_owned(),
                    email: "owner@example.com".to_owned(),
                    display_name: "Owner".to_owned(),
                    password_hash: PasswordService::default()
                        .hash("correct horse battery")
                        .unwrap(),
                    workspace_name: "Orbit".to_owned(),
                    project_name: "General".to_owned(),
                },
                now,
            )
            .await
            .unwrap()
            .workspace_id
    }

    async fn stop(self) {
        self.shutdown.cancel();
        self.task.await.unwrap();
    }

    async fn execute(&self, sql: &str, binds: &[&str]) {
        let mut query = sqlx::query(sqlx::AssertSqlSafe(sql.to_owned()));
        for bind in binds {
            query = query.bind(*bind);
        }
        query.execute(self.database.pool()).await.unwrap();
    }

    /// The verified domain `one.test` with a shared mailbox `box@` and the alias `postmaster@`.
    async fn mailbox(&self) {
        let now = TimestampMillis::now().as_millis().to_string();
        let workspace = self.workspace.to_string();
        let domain = Id::new_v7().to_string();
        let mailbox = Id::new_v7().to_string();
        self.execute(
            &format!(
                "INSERT INTO mail_domains (id, workspace_id, name, status, verify_token, created_at, updated_at) \
                 VALUES ($1, $2, 'one.test', 'verified', 'token', {now}, {now})"
            ),
            &[&domain, &workspace],
        )
        .await;
        self.execute(
            &format!(
                "INSERT INTO mailboxes (id, workspace_id, kind, name, owner_membership_id, status, created_at, updated_at) \
                 VALUES ($1, $2, 'shared', 'Box', NULL, 'active', {now}, {now})"
            ),
            &[&mailbox, &workspace],
        )
        .await;
        for (local, kind) in [("box", "primary"), ("postmaster", "alias")] {
            self.execute(
                &format!(
                    "INSERT INTO mail_addresses (id, domain_id, mailbox_id, local_part, kind, created_at) \
                     VALUES ($1, $2, $3, $4, $5, {now})"
                ),
                &[&Id::new_v7().to_string(), &domain, &mailbox, local, kind],
            )
            .await;
        }
    }

    fn transport(&self, tls: Tls) -> AsyncSmtpTransport<Tokio1Executor> {
        AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous("127.0.0.1")
            .port(self.port)
            .hello_name(ClientId::Domain("client.sender.test".to_owned()))
            .timeout(Some(Duration::from_secs(30)))
            .tls(tls)
            .build()
    }

    async fn send(&self, from: &str, to: &str, raw: &[u8]) -> Result<(), String> {
        let envelope =
            Envelope::new(Some(from.parse().unwrap()), vec![to.parse().unwrap()]).unwrap();
        self.transport(Tls::None)
            .send_raw(&envelope, raw)
            .await
            .map(|response| assert_eq!(response.code().to_string(), "250"))
            .map_err(|error| error.to_string())
    }

    /// The workspace's receipts, oldest first.
    async fn receipts(&self) -> Vec<Receipt> {
        let rows: Vec<ReceiptRow> = sqlx::query_as(
            "SELECT id, peer_ip, helo, mail_from, auth, tls, blob_id FROM mail_receipts \
                 WHERE workspace_id = $1 ORDER BY received_at",
        )
        .bind(self.workspace.to_string())
        .fetch_all(self.database.pool())
        .await
        .unwrap();
        rows.into_iter()
            .map(
                |(id, peer_ip, helo, mail_from, auth, tls, blob_id)| Receipt {
                    id: id.parse().unwrap(),
                    peer_ip,
                    helo,
                    mail_from,
                    auth: serde_json::from_str(&auth).unwrap(),
                    tls: tls.map(|tls| serde_json::from_str(&tls).unwrap()),
                    blob_id,
                },
            )
            .collect()
    }

    /// The stored raw message of a receipt, opened with the workspace key.
    async fn raw(&self, receipt: &Receipt) -> String {
        let uploads = UploadService::new(
            self.database.clone(),
            Arc::new(LocalBlobStore::new(
                self.root.path().join("data/attachments"),
            )),
            AttachmentMutationCoordinator::default(),
            UploadLimits::default(),
        );
        let repo = MailRepository::new(self.database.clone(), uploads, APP_KEY);
        let storage_key: String =
            sqlx::query_scalar("SELECT storage_key FROM attachment_blobs WHERE id = $1")
                .bind(&receipt.blob_id)
                .fetch_one(self.database.pool())
                .await
                .unwrap();
        let sealed = repo.uploads().read_blob(&storage_key).await.unwrap();
        let key = repo.keys().get(self.workspace, 1).await.unwrap();
        String::from_utf8(open_blob(&key, &receipt_aad(receipt.id), &sealed).unwrap()).unwrap()
    }
}

/// (id, peer_ip, helo, mail_from, auth, tls, blob_id)
type ReceiptRow = (
    String,
    String,
    String,
    Option<String>,
    String,
    Option<String>,
    String,
);

struct Receipt {
    id: Id,
    peer_ip: String,
    helo: String,
    mail_from: Option<String>,
    auth: Value,
    tls: Option<Value>,
    blob_id: String,
}

const MESSAGE: &[u8] =
    b"From: Ann <ann@sender.test>\r\nTo: box@one.test\r\nSubject: Hello\r\n\r\nHi there\r\n";

/// A line-based SMTP client for what `lettre` cannot send (PROXY headers, bare `postmaster`).
struct RawClient {
    stream: BufReader<TcpStream>,
}

impl RawClient {
    async fn connect(port: u16, proxy_source: Option<Ipv4Addr>) -> Self {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        if let Some(source) = proxy_source {
            stream.write_all(&proxy_v2(source)).await.unwrap();
        }
        Self {
            stream: BufReader::new(stream),
        }
    }

    /// One reply (all its lines); empty when the server closed the connection.
    async fn reply(&mut self) -> String {
        let mut reply = String::new();
        loop {
            let mut line = String::new();
            let read =
                tokio::time::timeout(Duration::from_secs(30), self.stream.read_line(&mut line))
                    .await
                    .expect("no reply within 30 s")
                    .unwrap();
            if read == 0 {
                return reply;
            }
            reply.push_str(&line);
            if line.as_bytes().get(3) != Some(&b'-') {
                return reply;
            }
        }
    }

    async fn command(&mut self, line: &str) -> String {
        self.stream
            .get_mut()
            .write_all(format!("{line}\r\n").as_bytes())
            .await
            .unwrap();
        self.reply().await
    }
}

/// A PROXY v2 header for a TCP/IPv4 connection from `source:40000` to `127.0.0.1:25`.
fn proxy_v2(source: Ipv4Addr) -> Vec<u8> {
    let mut header = vec![
        0x0D, 0x0A, 0x0D, 0x0A, 0x00, 0x0D, 0x0A, 0x51, 0x55, 0x49, 0x54, 0x0A, 0x21, 0x11, 0x00,
        0x0C,
    ];
    header.extend_from_slice(&source.octets());
    header.extend_from_slice(&Ipv4Addr::LOCALHOST.octets());
    header.extend_from_slice(&40000u16.to_be_bytes());
    header.extend_from_slice(&25u16.to_be_bytes());
    header
}

fn count(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

#[tokio::test]
async fn delivers_to_a_verified_mailbox() {
    let server = Server::start(Mode::Direct(StaticDns::new())).await;
    server.mailbox().await;

    server
        .send("ann@sender.test", "box+news@one.test", MESSAGE)
        .await
        .unwrap();

    let receipts = server.receipts().await;
    assert_eq!(receipts.len(), 1);
    let receipt = &receipts[0];
    assert_eq!(receipt.peer_ip, "127.0.0.1");
    assert_eq!(receipt.helo, "client.sender.test");
    assert_eq!(receipt.mail_from.as_deref(), Some("ann@sender.test"));
    assert_eq!(receipt.auth["dmarc"]["result"], "none");
    assert!(receipt.tls.is_none());
    let rcpt: String =
        sqlx::query_scalar("SELECT rcpt FROM mail_receipt_recipients WHERE receipt_id = $1")
            .bind(receipt.id.to_string())
            .fetch_one(server.database.pool())
            .await
            .unwrap();
    assert_eq!(rcpt, "box+news@one.test");

    // Orbit's trace headers come first, then the message as sent.
    let raw = server.raw(receipt).await;
    assert!(
        raw.starts_with("Received: from client.sender.test ([127.0.0.1])"),
        "{raw}"
    );
    assert!(raw.contains(&format!("by {HOSTNAME}")), "{raw}");
    assert!(
        raw.contains(&format!("\r\nAuthentication-Results: {HOSTNAME};")),
        "{raw}"
    );
    // (lettre writes CRLF before the final dot, so the stored text has one more line end.)
    assert!(
        raw.ends_with(&format!("{}\r\n", std::str::from_utf8(MESSAGE).unwrap())),
        "{raw}"
    );

    assert!(server.status.running());
    assert_eq!(
        server.status.last_peer_ip(),
        Some(IpAddr::V4(Ipv4Addr::LOCALHOST))
    );
    assert_eq!(server.status.bridge_peers(), 0);
    assert!(server.status.cert().unwrap().covers(HOSTNAME));
    server.stop().await;
}

#[tokio::test]
async fn starttls_is_recorded_on_the_receipt() {
    let server = Server::start(Mode::Direct(StaticDns::new())).await;
    server.mailbox().await;
    let tls = TlsParameters::builder(HOSTNAME.to_owned())
        .dangerous_accept_invalid_certs(true)
        .build()
        .unwrap();
    let envelope = Envelope::new(
        Some("ann@sender.test".parse().unwrap()),
        vec!["box@one.test".parse().unwrap()],
    )
    .unwrap();
    server
        .transport(Tls::Required(tls))
        .send_raw(&envelope, MESSAGE)
        .await
        .unwrap();

    let receipts = server.receipts().await;
    assert_eq!(receipts.len(), 1);
    let tls = receipts[0].tls.as_ref().unwrap();
    assert_eq!(tls["version"], "TLSv1.3");
    let raw = server.raw(&receipts[0]).await;
    assert!(raw.contains("with ESMTPS"), "{raw}");
    server.stop().await;
}

#[tokio::test]
async fn refuses_unknown_and_non_local_recipients() {
    let server = Server::start(Mode::Direct(StaticDns::new())).await;
    server.mailbox().await;

    let error = server
        .send("ann@sender.test", "nobody@one.test", MESSAGE)
        .await
        .unwrap_err();
    assert!(
        error.contains("(550)") && error.contains("5.1.1"),
        "{error}"
    );
    let error = server
        .send("ann@sender.test", "someone@elsewhere.test", MESSAGE)
        .await
        .unwrap_err();
    assert!(
        error.contains("(554)") && error.contains("5.7.1"),
        "{error}"
    );
    assert!(server.receipts().await.is_empty());
    server.stop().await;
}

#[tokio::test]
async fn accepts_bare_postmaster() {
    let server = Server::start(Mode::Direct(StaticDns::new())).await;
    server.mailbox().await;

    let mut client = RawClient::connect(server.port, None).await;
    assert!(client.reply().await.starts_with("220 "));
    assert!(
        client
            .command("EHLO client.sender.test")
            .await
            .starts_with("250")
    );
    assert!(
        client
            .command("MAIL FROM:<ann@sender.test>")
            .await
            .starts_with("250")
    );
    let reply = client.command("RCPT TO:<Postmaster>").await;
    assert!(reply.starts_with("250"), "{reply}");
    assert!(client.command("DATA").await.starts_with("354"));
    let reply = client
        .command("From: ann@sender.test\r\nSubject: Abuse\r\n\r\nReport\r\n.")
        .await;
    assert!(reply.starts_with("250"), "{reply}");
    client.command("QUIT").await;

    let receipts = server.receipts().await;
    assert_eq!(receipts.len(), 1);
    server.stop().await;
}

#[tokio::test]
async fn removes_forged_authentication_results() {
    let server = Server::start(Mode::Direct(StaticDns::new())).await;
    server.mailbox().await;
    let forged = b"Authentication-Results: mx.one.test; dmarc=pass header.from=bank.test\r\n\
        Authentication-Results: MX.ONE.TEST; spf=pass\r\n\
        Authentication-Results: other.test; spf=pass\r\n\
        From: Ann <ann@sender.test>\r\nSubject: Hello\r\n\r\nHi there\r\n";

    server
        .send("ann@sender.test", "box@one.test", forged)
        .await
        .unwrap();

    let receipts = server.receipts().await;
    let raw = server.raw(&receipts[0]).await;
    assert_eq!(
        count(
            &raw.to_ascii_lowercase(),
            "authentication-results: mx.one.test;"
        ),
        1,
        "{raw}"
    );
    assert!(!raw.contains("header.from=bank.test"), "{raw}");
    assert!(
        raw.contains("Authentication-Results: other.test; spf=pass"),
        "{raw}"
    );
    server.stop().await;
}

#[tokio::test]
async fn accepts_dmarc_failure_with_reject_policy() {
    let dns = StaticDns::new()
        .txt("sender.test", "v=spf1 -all")
        .txt("_dmarc.sender.test", "v=DMARC1; p=reject");
    let server = Server::start(Mode::Direct(dns)).await;
    server.mailbox().await;

    server
        .send("ann@sender.test", "box@one.test", MESSAGE)
        .await
        .unwrap();

    let receipts = server.receipts().await;
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].auth["spf"], "fail");
    assert_eq!(receipts[0].auth["dmarc"]["result"], "fail");
    assert_eq!(receipts[0].auth["dmarc"]["policy"], "reject");
    let raw = server.raw(&receipts[0]).await;
    assert!(raw.contains("dmarc=fail"), "{raw}");
    server.stop().await;
}

#[tokio::test]
async fn dns_temporary_error_gives_451() {
    let dns = StaticDns::new().temp_error("sender.test");
    let server = Server::start(Mode::Direct(dns)).await;
    server.mailbox().await;

    let error = server
        .send("ann@sender.test", "box@one.test", MESSAGE)
        .await
        .unwrap_err();
    assert!(
        error.contains("(451)") && error.contains("4.4.3"),
        "{error}"
    );
    assert!(server.receipts().await.is_empty());
    server.stop().await;
}

#[tokio::test]
async fn relay_mode_refuses_peers_outside_the_relay_cidrs() {
    let server = Server::start(Mode::Relay {
        relay: "192.0.2.0/24",
        proxy: None,
    })
    .await;
    server.mailbox().await;

    let error = server
        .send("ann@sender.test", "box@one.test", MESSAGE)
        .await
        .unwrap_err();
    assert!(
        error.contains("(554)") && error.contains("5.7.1"),
        "{error}"
    );
    assert!(server.receipts().await.is_empty());
    server.stop().await;
}

#[tokio::test]
async fn relay_mode_trusts_the_relay_behind_proxy_v2() {
    let server = Server::start(Mode::Relay {
        relay: "192.0.2.0/24",
        proxy: Some("127.0.0.1/32"),
    })
    .await;
    server.mailbox().await;

    let mut client = RawClient::connect(server.port, Some(Ipv4Addr::new(192, 0, 2, 7))).await;
    assert!(client.reply().await.starts_with("220 "));
    assert!(client.command("EHLO relay.test").await.starts_with("250"));
    assert!(
        client
            .command("MAIL FROM:<ann@sender.test>")
            .await
            .starts_with("250")
    );
    assert!(
        client
            .command("RCPT TO:<box@one.test>")
            .await
            .starts_with("250")
    );
    assert!(client.command("DATA").await.starts_with("354"));
    let reply = client
        .command(
            "Authentication-Results: relay.test; spf=pass smtp.mailfrom=sender.test;\r\n \
             dkim=pass header.d=sender.test; dmarc=pass header.from=sender.test\r\n\
             From: Ann <ann@sender.test>\r\nSubject: Relayed\r\n\r\nHi\r\n.",
        )
        .await;
    assert!(reply.starts_with("250"), "{reply}");
    client.command("QUIT").await;

    let receipts = server.receipts().await;
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].peer_ip, "192.0.2.7");
    assert_eq!(receipts[0].auth["dmarc"]["result"], "pass");
    assert_eq!(receipts[0].auth["spf"], "pass");
    let raw = server.raw(&receipts[0]).await;
    assert!(
        raw.contains(&format!("Authentication-Results: {HOSTNAME};")),
        "{raw}"
    );
    assert!(raw.contains("Authentication-Results: relay.test;"), "{raw}");

    // A peer behind Docker's userland proxy is counted for the health card (and refused here,
    // since it is not a relay).
    let mut client = RawClient::connect(server.port, Some(Ipv4Addr::new(172, 17, 0, 1))).await;
    let reply = client.reply().await;
    assert!(reply.starts_with("554 5.7.1"), "{reply}");
    assert_eq!(server.status.bridge_peers(), 1);
    assert_eq!(
        server.status.last_peer_ip(),
        Some(IpAddr::V4(Ipv4Addr::new(172, 17, 0, 1)))
    );
    server.stop().await;
}

#[tokio::test]
async fn shutdown_sends_421_to_open_sessions() {
    let server = Server::start(Mode::Direct(StaticDns::new())).await;
    let mut client = RawClient::connect(server.port, None).await;
    assert!(client.reply().await.starts_with("220 "));
    assert!(
        client
            .command("EHLO client.sender.test")
            .await
            .starts_with("250")
    );

    let status = Arc::clone(&server.status);
    let stop = tokio::spawn(server.stop());
    let reply = client.reply().await;
    assert!(reply.starts_with("421 4.3.2"), "{reply}");
    stop.await.unwrap();
    assert!(!status.running());
}
