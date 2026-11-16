//! Bounces (spec §5.7): the bounce address at the listener and at provider intake, delivery
//! status notifications, the suppression list and **Retry**.

#![allow(dead_code)]
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use futures_util::future::BoxFuture;
use lettre::address::Envelope;
use lettre::transport::smtp::client::Tls;
use lettre::transport::smtp::extension::ClientId;
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};
use orbit_mail::inbound::{Delivery as ProviderDelivery, SpfSource};
use orbit_mail::{Authenticator, DsnAction, DsnRecipient, StaticDns, bounce_local_part};
use orbit_platform::{
    AttachmentMutationCoordinator, AuthenticatedUser, Id, LocalBlobStore, MailConfig, MailMode,
    PasswordService, TestDatabase, TimestampMillis, UploadLimits, UploadService,
};
use orbit_server::auth_routes::CookieMode;
use orbit_server::live::LiveHub;
use orbit_server::mail_delivery::{DeliveryResult, ProviderIntake};
use orbit_server::mail_dns::FakeDns;
use orbit_server::mail_intake::{InboundMessage, MailIntake, Recipient, Source};
use orbit_server::mail_listener::MailListener;
use orbit_server::mail_routes::{MailSettings, MailState, mail_router};
use orbit_server::mail_send::{
    BounceKind, Delivery, DeliveryContext, Outgoing, RecipientOutcome, SendJobState, run_send_job,
};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::instance_settings::InstanceSettingsRepository;
use orbit_server::repositories::mail::{
    DraftMode, MailError, MailRepository, MailboxKind, NewDraft, NewMailbox, NewProvider, NewRoute,
    ProviderKind, ProviderScope, ProviderSettings, RecipientLookup, RouteSecrets, RouteSettings,
    SendRouteKind, SuppressionReason, bounce_codes, suppresses, suppresses_response,
};
use orbit_server::repositories::workspaces::WorkspaceRepository;
use serde_json::{Value, json};
use sqlx::Row;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

const APP_KEY: [u8; 32] = [32; 32];
const DAY: i64 = 24 * 3_600_000;

/// An adapter that accepts everything except the addresses in its table.
#[derive(Default)]
struct Fake {
    outcomes: Mutex<HashMap<String, RecipientOutcome>>,
    /// The envelope sender of each call, by recipient.
    senders: Mutex<HashMap<String, String>>,
}

impl Fake {
    fn answer(&self, address: &str, outcome: RecipientOutcome) {
        self.outcomes
            .lock()
            .unwrap()
            .insert(address.to_owned(), outcome);
    }
}

impl Delivery for Fake {
    fn deliver<'a>(
        &'a self,
        outgoing: &'a Outgoing<'a>,
    ) -> BoxFuture<'a, Vec<(String, RecipientOutcome)>> {
        Box::pin(async move {
            let table = self.outcomes.lock().unwrap();
            let mut senders = self.senders.lock().unwrap();
            outgoing
                .recipients
                .iter()
                .map(|address| {
                    senders.insert(address.clone(), outgoing.mail_from.to_owned());
                    (
                        address.clone(),
                        table
                            .get(address)
                            .cloned()
                            .unwrap_or(RecipientOutcome::Accepted {
                                provider_message_id: None,
                            }),
                    )
                })
                .collect()
        })
    }
}

fn at(millis: i64) -> TimestampMillis {
    TimestampMillis::from_millis(millis)
}

fn report() -> String {
    json!({"verify": {"status": "ok"}, "mx": {"status": "ok"}, "dkim": {"status": "ok"},
           "spf": {"status": "ok"}, "dmarc": {"status": "ok"}})
    .to_string()
}

/// A DSN (RFC 3464) for `address` with `action`, `status` and an SMTP diagnostic.
fn dsn(to: &str, address: &str, action: &str, status: &str, diagnostic: &str) -> Vec<u8> {
    format!(
        "From: Mail Delivery System <MAILER-DAEMON@mx.out.test>\r\n\
         To: {to}\r\n\
         Subject: Undelivered Mail Returned to Sender\r\n\
         MIME-Version: 1.0\r\n\
         Content-Type: multipart/report; report-type=delivery-status; boundary=\"b1\"\r\n\
         \r\n\
         --b1\r\n\
         Content-Type: text/plain\r\n\
         \r\n\
         Your message could not be delivered.\r\n\
         --b1\r\n\
         Content-Type: message/delivery-status\r\n\
         \r\n\
         Reporting-MTA: dns; mx.out.test\r\n\
         \r\n\
         Final-Recipient: rfc822; {address}\r\n\
         Action: {action}\r\n\
         Status: {status}\r\n\
         Diagnostic-Code: smtp; {diagnostic}\r\n\
         \r\n\
         --b1--\r\n"
    )
    .into_bytes()
}

#[derive(Clone)]
struct Member {
    cookie: String,
    membership: Id,
    user: Id,
}

struct Fixture {
    _root: tempfile::TempDir,
    database: TestDatabase,
    identity: Arc<IdentityRepository>,
    mail: Arc<MailRepository>,
    intake: MailIntake,
    workspace: Id,
    owner: Member,
    domain: Id,
    mailbox: Id,
    address: Id,
    fake: Arc<Fake>,
}

impl Fixture {
    async fn new() -> Self {
        let database = TestDatabase::new().await.unwrap();
        let identity = Arc::new(IdentityRepository::new((*database).clone()));
        let now = TimestampMillis::now();
        identity
            .store_setup_token("operator-secret", at(now.as_millis() + 60_000))
            .await
            .unwrap();
        let setup = identity
            .complete_setup(
                SetupRequest {
                    token: "operator-secret".to_owned(),
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
            .unwrap();
        InstanceSettingsRepository::new((*database).clone())
            .set_mail_enabled(setup.user_id, true, "request", now)
            .await
            .unwrap();
        let root = tempfile::tempdir().unwrap();
        let uploads = UploadService::new(
            (*database).clone(),
            Arc::new(LocalBlobStore::new(root.path().join("attachments"))),
            AttachmentMutationCoordinator::default(),
            UploadLimits::default(),
        );
        let mail = Arc::new(MailRepository::new((*database).clone(), uploads, APP_KEY));
        let intake = MailIntake::new(Arc::clone(&mail), 52_428_800);
        let membership: String = sqlx::query_scalar(
            "SELECT id FROM memberships WHERE workspace_id = $1 AND user_id = $2",
        )
        .bind(setup.workspace_id.to_string())
        .bind(setup.user_id.to_string())
        .fetch_one(database.pool())
        .await
        .unwrap();
        let membership: Id = membership.parse().unwrap();
        let domain = Id::new_v7();
        sqlx::query(
            "INSERT INTO mail_domains (id, workspace_id, name, status, verify_token, dns_report, \
             created_at, updated_at) VALUES ($1, $2, 'one.test', 'verified', 'token', $3, $4, $4)",
        )
        .bind(domain.to_string())
        .bind(setup.workspace_id.to_string())
        .bind(report())
        .bind(now.as_millis())
        .execute(database.pool())
        .await
        .unwrap();
        let route = mail
            .create_route(
                ProviderScope::Instance,
                setup.user_id,
                NewRoute {
                    kind: SendRouteKind::Direct,
                    name: "Direct".to_owned(),
                    provider_id: None,
                    secrets: RouteSecrets::None,
                    settings: RouteSettings::default(),
                },
                Some("mx.orbit.test"),
                "request",
            )
            .await
            .unwrap();
        sqlx::query("UPDATE mail_domains SET send_route_id = $1 WHERE id = $2")
            .bind(route.id.to_string())
            .bind(domain.to_string())
            .execute(database.pool())
            .await
            .unwrap();
        mail.rotate_dkim(setup.workspace_id, setup.user_id, domain, "request", now)
            .await
            .unwrap();
        sqlx::query("UPDATE mail_dkim_keys SET status = 'active' WHERE domain_id = $1")
            .bind(domain.to_string())
            .execute(database.pool())
            .await
            .unwrap();
        let mailbox = mail
            .create_mailbox(
                setup.workspace_id,
                setup.user_id,
                NewMailbox {
                    kind: MailboxKind::Shared,
                    name: "Team".to_owned(),
                    domain_id: domain,
                    local_part: "team".to_owned(),
                    owner_membership_id: None,
                    member_ids: vec![membership],
                },
                "request",
            )
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO mail_member_settings (membership_id, undo_send_seconds, updated_at) \
             VALUES ($1, 0, 0)",
        )
        .bind(membership.to_string())
        .execute(database.pool())
        .await
        .unwrap();
        Self {
            _root: root,
            database,
            identity,
            mail,
            intake,
            workspace: setup.workspace_id,
            owner: Member {
                cookie: format!("__Host-orbit_session={}", setup.session.token),
                membership,
                user: setup.user_id,
            },
            domain,
            mailbox: mailbox.id,
            address: mailbox.addresses[0].id,
            fake: Arc::default(),
        }
    }

    fn state(&self) -> SendJobState {
        let context = |private| {
            DeliveryContext::new(
                Some("mx.orbit.test".to_owned()),
                Arc::new(FakeDns::new()),
                private,
            )
            .with_adapter(Arc::clone(&self.fake) as Arc<dyn Delivery>)
        };
        SendJobState {
            mail: Arc::clone(&self.mail),
            delivery: context(false),
            instance_delivery: context(true),
            intake: self.intake.clone(),
            live: LiveHub::of(&self.database),
            send_timeout: Duration::from_secs(20 * 60),
        }
    }

    async fn sql(&self, sql: &str, binds: &[&str]) {
        let mut query = sqlx::query(sqlx::AssertSqlSafe(sql.to_owned()));
        for bind in binds {
            query = query.bind(*bind);
        }
        query.execute(self.database.pool()).await.unwrap();
    }

    async fn count(&self, sql: &str, binds: &[&str]) -> i64 {
        let mut query = sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(sql.to_owned()));
        for bind in binds {
            query = query.bind(*bind);
        }
        query.fetch_one(self.database.pool()).await.unwrap()
    }

    /// Sends a message from team@one.test now and runs its first delivery. Returns its id.
    async fn send(&self, to: &[&str]) -> Id {
        let now = TimestampMillis::now();
        let (draft, _) = self
            .mail
            .create_draft(
                self.owner.membership,
                self.workspace,
                NewDraft {
                    mode: DraftMode::New,
                    thread_id: None,
                    parent_message_id: None,
                    from_address_id: Some(self.address),
                    to: Some(to.iter().map(|item| (*item).to_owned()).collect()),
                    cc: None,
                    bcc: None,
                    subject: Some("Hello".to_owned()),
                    body_html: Some("<p>Hi</p>".to_owned()),
                },
            )
            .await
            .unwrap();
        let outbound = self
            .mail
            .send_draft(self.owner.membership, draft.id, None, now)
            .await
            .unwrap();
        self.sql("DELETE FROM jobs WHERE kind = 'mail.send'", &[])
            .await;
        run_send_job(&self.state(), outbound.id, outbound.release_at)
            .await
            .unwrap();
        outbound.id
    }

    async fn recipient(&self, outbound: Id, address: &str) -> Id {
        let id: String = sqlx::query_scalar(
            "SELECT id FROM mail_outbound_recipients WHERE outbound_id = $1 AND address = $2",
        )
        .bind(outbound.to_string())
        .bind(address)
        .fetch_one(self.database.pool())
        .await
        .unwrap();
        id.parse().unwrap()
    }

    /// (status, bounce_kind, attempts, last_response).
    async fn state_of(&self, recipient: Id) -> (String, Option<String>, i64, Option<String>) {
        let row = sqlx::query(
            "SELECT status, bounce_kind, attempts, last_response FROM mail_outbound_recipients \
             WHERE id = $1",
        )
        .bind(recipient.to_string())
        .fetch_one(self.database.pool())
        .await
        .unwrap();
        (
            row.get("status"),
            row.get("bounce_kind"),
            row.get("attempts"),
            row.get("last_response"),
        )
    }

    /// The bounce address of a recipient, as delivery puts it in the envelope.
    fn bounce_address(&self, recipient: Id) -> String {
        format!(
            "{}@one.test",
            bounce_local_part(&recipient.to_bytes(), &self.mail.bounce_key())
        )
    }

    async fn suppressed(&self, address: &str) -> Option<(String, Option<String>)> {
        sqlx::query_as(
            "SELECT reason, recipient_id FROM mail_suppressions WHERE workspace_id = $1 \
             AND address = $2",
        )
        .bind(self.workspace.to_string())
        .bind(address)
        .fetch_optional(self.database.pool())
        .await
        .unwrap()
    }

    /// (folder, read by the owner) of the sent message's thread.
    async fn thread_state(&self, outbound: Id) -> (String, bool) {
        let row = sqlx::query(
            "SELECT t.folder, s.last_read_at, t.last_message_at FROM mail_outbound o \
             JOIN mail_thread_messages tm ON tm.message_id = o.message_id \
             JOIN mail_threads t ON t.id = tm.thread_id \
             LEFT JOIN mail_thread_user_state s ON s.thread_id = t.id AND s.membership_id = $1 \
             WHERE o.id = $2",
        )
        .bind(self.owner.membership.to_string())
        .bind(outbound.to_string())
        .fetch_one(self.database.pool())
        .await
        .unwrap();
        let read: Option<i64> = row.get("last_read_at");
        let last: i64 = row.get("last_message_at");
        (row.get("folder"), read.is_some_and(|read| read >= last))
    }

    async fn thread_updates(&self) -> usize {
        let frames: Vec<String> =
            sqlx::query_scalar("SELECT frame FROM live_events WHERE workspace_id = $1")
                .bind(self.workspace.to_string())
                .fetch_all(self.database.pool())
                .await
                .unwrap();
        frames
            .iter()
            .filter(|frame| frame.contains("thread.updated"))
            .count()
    }

    /// Hands a message for `rcpts` to intake as the listener does after `RCPT TO`.
    async fn receive(&self, rcpts: &[&str], raw: Vec<u8>) -> Vec<Id> {
        let now = TimestampMillis::now();
        let mut recipients = Vec::new();
        for rcpt in rcpts {
            match self.mail.lookup_recipient(rcpt, now).await.unwrap() {
                RecipientLookup::Bounce { recipient_id } => recipients.push(Recipient::Bounce {
                    recipient_id,
                    rcpt: (*rcpt).to_owned(),
                }),
                RecipientLookup::Accept {
                    address_id,
                    workspace_id,
                    ..
                } => recipients.push(Recipient::Mailbox {
                    workspace_id,
                    address_id,
                    rcpt: (*rcpt).to_owned(),
                }),
                other => panic!("{rcpt} refused: {other:?}"),
            }
        }
        self.intake
            .accept(InboundMessage {
                source: Source::Smtp,
                raw,
                trace_len: 0,
                mail_from: None,
                recipients,
                peer_ip: IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)),
                helo: "mx.out.test".to_owned(),
                tls: None,
                auth: orbit_mail::AuthOutcome {
                    spf: orbit_mail::Verdict::None,
                    dkim: orbit_mail::Verdict::None,
                    dmarc: orbit_mail::DmarcVerdict::None,
                    aligned_domain: None,
                    temp_error: false,
                    spf_domain: None,
                    dkim_domain: None,
                    from_domain: None,
                },
            })
            .await
            .unwrap()
    }

    async fn member(&self, display_name: &str) -> Member {
        let id = Id::new_v7();
        let membership = Id::new_v7();
        let now = TimestampMillis::now();
        let email = format!("{id}@example.com");
        sqlx::query(
            "INSERT INTO users (id, email, normalized_email, display_name, password_hash, \
             created_at, updated_at) VALUES ($1, $2, $3, $4, 'unused', $5, $5)",
        )
        .bind(id.to_string())
        .bind(&email)
        .bind(&email)
        .bind(display_name)
        .bind(now.as_millis())
        .execute(self.database.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, \
             updated_at) VALUES ($1, $2, $3, 'member', 0, $4, $4)",
        )
        .bind(membership.to_string())
        .bind(self.workspace.to_string())
        .bind(id.to_string())
        .bind(now.as_millis())
        .execute(self.database.pool())
        .await
        .unwrap();
        let session = self
            .identity
            .create_session(
                &AuthenticatedUser {
                    id,
                    email,
                    display_name: display_name.to_owned(),
                },
                now,
            )
            .await
            .unwrap();
        Member {
            cookie: format!("__Host-orbit_session={}", session.token),
            membership,
            user: id,
        }
    }

    fn app(&self) -> axum::Router {
        mail_router(MailState::new(
            Arc::clone(&self.identity),
            Arc::clone(&self.mail),
            CookieMode::secure(),
            MailSettings {
                enabled: true,
                hostname: Some("mx.orbit.test".to_owned()),
                mode: MailMode::Direct,
            },
        ))
    }

    async fn call(&self, member: &Member, method: &str, path: &str) -> (StatusCode, Value) {
        self.call_in(self.workspace, member, method, path, None)
            .await
    }

    async fn call_in(
        &self,
        workspace: Id,
        member: &Member,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let builder = Request::builder()
            .method(method)
            .uri(format!("/api/v1/workspaces/{workspace}/mail{path}"))
            .header(header::COOKIE, &member.cookie);
        let request = match body {
            Some(body) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string())),
            None => builder.body(Body::empty()),
        }
        .unwrap();
        let response = self.app().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    /// Runs this instance's SMTP listener (relay mode, from 127.0.0.1) on a free port.
    async fn listener(&self) -> (u16, CancellationToken) {
        let config = MailConfig {
            enabled: true,
            hostname: Some("mx.orbit.test".to_owned()),
            smtp_bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
            smtp_port: 0,
            mode: MailMode::Relay,
            relay_cidrs: vec!["127.0.0.1/32".parse().unwrap()],
            relay_authserv_id: Some("relay.test".to_owned()),
            ..MailConfig::default()
        };
        let listener =
            MailListener::new(config, self.intake.clone(), Arc::clone(&self.mail)).unwrap();
        let socket = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = socket.local_addr().unwrap().port();
        let cancel = CancellationToken::new();
        let token = cancel.clone();
        tokio::spawn(async move {
            listener
                .run(socket, token, Duration::from_secs(1))
                .await
                .unwrap();
        });
        (port, cancel)
    }
}

async fn smtp_send(port: u16, to: &str, raw: &[u8]) -> Result<(), String> {
    let transport = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous("127.0.0.1")
        .port(port)
        .hello_name(ClientId::Domain("mx.out.test".to_owned()))
        .timeout(Some(Duration::from_secs(30)))
        .tls(Tls::None)
        .build();
    let envelope = Envelope::new(None, vec![to.parse().unwrap()]).unwrap();
    transport
        .send_raw(&envelope, raw)
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[test]
fn suppression_rule() {
    // Bad addresses.
    assert!(suppresses_response(
        BounceKind::Hard,
        "permanent error (550): 5.1.1 No such user"
    ));
    assert!(suppresses_response(
        BounceKind::Hard,
        "550 5.2.1 Mailbox disabled"
    ));
    assert!(suppresses_response(
        BounceKind::Hard,
        "5.1.10 out.test accepts no mail (null MX)"
    ));
    assert!(suppresses_response(
        BounceKind::Hard,
        "permanent error (553): mailbox name invalid"
    ));
    assert!(suppresses_response(BounceKind::Hard, "551 user not local"));
    // Policy, content and size refusals bounce but do not suppress.
    assert!(!suppresses_response(
        BounceKind::Hard,
        "550 5.7.1 Rejected by policy"
    ));
    assert!(!suppresses_response(
        BounceKind::Hard,
        "552 5.3.4 Message too big"
    ));
    assert!(!suppresses_response(
        BounceKind::Hard,
        "554 5.6.0 Bad content"
    ));
    assert!(!suppresses_response(
        BounceKind::Hard,
        "554 Transaction failed"
    ));
    assert!(!suppresses_response(
        BounceKind::Hard,
        "550 5.2.2 Mailbox full"
    ));
    // Only hard bounces.
    for kind in [
        BounceKind::Rejected,
        BounceKind::GaveUp,
        BounceKind::Route,
        BounceKind::Unreadable,
    ] {
        assert!(
            !suppresses_response(kind, "550 5.1.1 No such user"),
            "{kind:?}"
        );
    }
    assert!(suppresses(BounceKind::Hard, Some("5.1.1"), None));
    assert!(!suppresses(BounceKind::Hard, Some("5.0.0"), Some(550)));
    assert!(suppresses(BounceKind::Hard, None, Some(550)));
    assert_eq!(
        bounce_codes("permanent error (550): 5.1.1 <a@b.test>: unknown"),
        (Some(550), Some("5.1.1".to_owned()))
    );
}

#[tokio::test]
async fn smtp_time_bounces_follow_the_rule() {
    let f = Fixture::new().await;
    f.fake.answer(
        "gone@out.test",
        RecipientOutcome::bounced("permanent error (550): 5.1.1 No such user"),
    );
    f.fake.answer(
        "policy@out.test",
        RecipientOutcome::bounced("permanent error (550): 5.7.1 Spam"),
    );
    f.fake.answer(
        "big@out.test",
        RecipientOutcome::rejected("5.3.4 The message is larger than the route allows"),
    );
    let id = f
        .send(&["gone@out.test", "policy@out.test", "big@out.test"])
        .await;
    let gone = f.recipient(id, "gone@out.test").await;
    assert_eq!(f.state_of(gone).await.0, "bounced");
    let (reason, recipient) = f.suppressed("gone@out.test").await.expect("suppressed");
    assert_eq!(reason, SuppressionReason::HardBounce.as_str());
    assert_eq!(recipient, Some(gone.to_string()));
    let policy = f.recipient(id, "policy@out.test").await;
    assert_eq!(f.state_of(policy).await.1.as_deref(), Some("hard"));
    assert_eq!(f.suppressed("policy@out.test").await, None);
    assert_eq!(f.suppressed("big@out.test").await, None);
}

#[tokio::test]
async fn listener_accepts_valid_bounce_address() {
    let f = Fixture::new().await;
    let id = f.send(&["bob@out.test"]).await;
    let bob = f.recipient(id, "bob@out.test").await;
    assert_eq!(f.state_of(bob).await.0, "delivered");
    let bounce = f.bounce_address(bob);
    assert_eq!(
        f.fake.senders.lock().unwrap().get("bob@out.test"),
        Some(&bounce),
        "delivery uses this bounce address"
    );
    let receipts = f
        .count("SELECT CAST(COUNT(*) AS BIGINT) FROM mail_receipts", &[])
        .await;
    let (port, stop) = f.listener().await;
    smtp_send(
        port,
        &bounce.to_uppercase().replacen("@ONE.TEST", "@one.test", 1),
        &dsn(
            &bounce,
            "bob@out.test",
            "failed",
            "5.1.1",
            "550 5.1.1 User unknown",
        ),
    )
    .await
    .expect("the bounce address takes the DSN");
    stop.cancel();
    let (status, kind, _, response) = f.state_of(bob).await;
    assert_eq!(status, "bounced");
    assert_eq!(kind.as_deref(), Some("hard"));
    assert_eq!(response.as_deref(), Some("550 5.1.1 User unknown"));
    assert!(f.suppressed("bob@out.test").await.is_some());
    assert_eq!(
        f.count("SELECT CAST(COUNT(*) AS BIGINT) FROM mail_receipts", &[])
            .await,
        receipts,
        "no receipt"
    );
}

#[tokio::test]
async fn listener_refuses_forged_token() {
    let f = Fixture::new().await;
    let id = f.send(&["bob@out.test"]).await;
    let bob = f.recipient(id, "bob@out.test").await;
    let forged = format!("{}@one.test", bounce_local_part(&bob.to_bytes(), &[7; 32]));
    let (port, stop) = f.listener().await;
    let error = smtp_send(
        port,
        &forged,
        &dsn(
            &forged,
            "bob@out.test",
            "failed",
            "5.1.1",
            "550 5.1.1 User unknown",
        ),
    )
    .await
    .expect_err("a forged token is refused");
    assert!(error.contains("550"), "{error}");
    // Not a `+tag` of a mailbox either, and garbage after `bounce+` is refused the same way.
    let error = smtp_send(port, "bounce+nonsense@one.test", b"Subject: x\r\n\r\nx\r\n")
        .await
        .expect_err("garbage is refused");
    assert!(error.contains("550"), "{error}");
    stop.cancel();
    assert_eq!(f.state_of(bob).await.0, "delivered");
    assert_eq!(f.suppressed("bob@out.test").await, None);
}

#[tokio::test]
async fn old_token_refused() {
    let f = Fixture::new().await;
    let id = f.send(&["bob@out.test"]).await;
    let bob = f.recipient(id, "bob@out.test").await;
    let local = bounce_local_part(&bob.to_bytes(), &f.mail.bounce_key());
    let now = TimestampMillis::now().as_millis();
    f.sql(
        &format!(
            "UPDATE mail_outbound_recipients SET released_at = {} WHERE id = $1",
            now - 6 * DAY
        ),
        &[&bob.to_string()],
    )
    .await;
    assert_eq!(
        f.mail.bounce_recipient(&local, at(now)).await.unwrap(),
        Some(bob)
    );
    f.sql(
        &format!(
            "UPDATE mail_outbound_recipients SET released_at = {} WHERE id = $1",
            now - 8 * DAY
        ),
        &[&bob.to_string()],
    )
    .await;
    assert_eq!(
        f.mail.bounce_recipient(&local, at(now)).await.unwrap(),
        None
    );
    assert_eq!(
        f.mail
            .lookup_recipient(&format!("{local}@one.test"), at(now))
            .await
            .unwrap(),
        RecipientLookup::BadBounce
    );
    // A recipient that is gone.
    let unknown = bounce_local_part(&Id::new_v7().to_bytes(), &f.mail.bounce_key());
    assert_eq!(
        f.mail.bounce_recipient(&unknown, at(now)).await.unwrap(),
        None
    );
}

#[tokio::test]
async fn hard_dsn_bounces_and_suppresses() {
    let f = Fixture::new().await;
    let id = f.send(&["bob@out.test", "carl@out.test"]).await;
    let bob = f.recipient(id, "bob@out.test").await;
    let carl = f.recipient(id, "carl@out.test").await;
    assert_eq!(f.thread_state(id).await.0, "archive");
    let updates = f.thread_updates().await;

    // A DSN to Bob's bounce address about Carl changes nothing.
    f.receive(
        &[&f.bounce_address(bob)],
        dsn(
            "x",
            "carl@out.test",
            "failed",
            "5.1.1",
            "550 5.1.1 User unknown",
        ),
    )
    .await;
    assert_eq!(f.state_of(carl).await.0, "delivered");
    assert_eq!(f.state_of(bob).await.0, "delivered");

    f.receive(
        &[&f.bounce_address(bob)],
        dsn(
            "x",
            "BOB@out.test",
            "failed",
            "5.1.1",
            "550 5.1.1 User unknown",
        ),
    )
    .await;
    let (status, kind, _, _) = f.state_of(bob).await;
    assert_eq!(status, "bounced");
    assert_eq!(kind.as_deref(), Some("hard"));
    assert_eq!(
        f.suppressed("bob@out.test").await,
        Some(("hard_bounce".to_owned(), Some(bob.to_string())))
    );
    assert_eq!(f.thread_state(id).await, ("inbox".to_owned(), false));
    assert!(f.thread_updates().await > updates, "thread.updated");
    let summary: String =
        sqlx::query_scalar("SELECT delivery_summary FROM mail_outbound WHERE id = $1")
            .bind(id.to_string())
            .fetch_one(f.database.pool())
            .await
            .unwrap();
    assert!(summary.contains("User unknown"), "{summary}");

    // A policy refusal bounces Carl but does not suppress the address.
    f.receive(
        &[&f.bounce_address(carl)],
        dsn(
            "x",
            "carl@out.test",
            "failed",
            "5.7.1",
            "550 5.7.1 Message rejected as spam",
        ),
    )
    .await;
    let (status, kind, _, _) = f.state_of(carl).await;
    assert_eq!(status, "bounced");
    assert_eq!(kind.as_deref(), Some("hard"));
    assert_eq!(f.suppressed("carl@out.test").await, None);

    // Applying a DSN again changes nothing.
    assert!(
        !f.mail
            .process_dsn(
                bob,
                &[DsnRecipient {
                    address: "bob@out.test".to_owned(),
                    original: None,
                    action: DsnAction::Failed,
                    status: "5.1.1".to_owned(),
                    diagnostic: None,
                }],
                TimestampMillis::now(),
            )
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn delayed_dsn_keeps_deferred() {
    let f = Fixture::new().await;
    f.fake.answer(
        "slow@out.test",
        RecipientOutcome::deferred("421 4.4.2 Try again later"),
    );
    let id = f.send(&["slow@out.test"]).await;
    let slow = f.recipient(id, "slow@out.test").await;
    assert_eq!(f.state_of(slow).await.0, "deferred");
    f.receive(
        &[&f.bounce_address(slow)],
        dsn(
            "x",
            "slow@out.test",
            "delayed",
            "4.4.7",
            "421 4.4.7 Delivery delayed",
        ),
    )
    .await;
    let (status, kind, attempts, _) = f.state_of(slow).await;
    assert_eq!(status, "deferred");
    assert_eq!(kind, None);
    assert_eq!(attempts, 1);
    assert_eq!(f.suppressed("slow@out.test").await, None);
    assert_eq!(f.thread_state(id).await.0, "archive");
}

#[tokio::test]
async fn dsn_not_filed_in_any_mailbox() {
    let f = Fixture::new().await;
    let id = f.send(&["bob@out.test"]).await;
    let bob = f.recipient(id, "bob@out.test").await;
    let receipts = f
        .count("SELECT CAST(COUNT(*) AS BIGINT) FROM mail_receipts", &[])
        .await;
    let blobs = f
        .count("SELECT CAST(COUNT(*) AS BIGINT) FROM attachment_blobs", &[])
        .await;
    let messages = f
        .count("SELECT CAST(COUNT(*) AS BIGINT) FROM mail_messages", &[])
        .await;
    let ingests = f
        .count(
            "SELECT CAST(COUNT(*) AS BIGINT) FROM jobs WHERE kind = 'mail.ingest'",
            &[],
        )
        .await;
    let stored = f
        .receive(
            &[&f.bounce_address(bob)],
            dsn(
                "x",
                "bob@out.test",
                "failed",
                "5.1.1",
                "550 5.1.1 User unknown",
            ),
        )
        .await;
    assert!(stored.is_empty(), "no receipt");
    assert_eq!(f.state_of(bob).await.0, "bounced");
    for (table, before) in [
        ("mail_receipts", receipts),
        ("attachment_blobs", blobs),
        ("mail_messages", messages),
    ] {
        assert_eq!(
            f.count(
                &format!("SELECT CAST(COUNT(*) AS BIGINT) FROM {table}"),
                &[]
            )
            .await,
            before,
            "{table}"
        );
    }
    assert_eq!(
        f.count(
            "SELECT CAST(COUNT(*) AS BIGINT) FROM jobs WHERE kind = 'mail.ingest'",
            &[]
        )
        .await,
        ingests
    );

    // With a mailbox recipient as well, only the mailbox gets a receipt.
    let stored = f
        .receive(
            &[&f.bounce_address(bob), "team@one.test"],
            dsn(
                "x",
                "bob@out.test",
                "failed",
                "5.1.1",
                "550 5.1.1 User unknown",
            ),
        )
        .await;
    assert_eq!(stored.len(), 1);
}

#[tokio::test]
async fn provider_intake_drops_bad_bounce_and_applies_valid() {
    let f = Fixture::new().await;
    let id = f.send(&["bob@out.test"]).await;
    let bob = f.recipient(id, "bob@out.test").await;
    let created = f
        .mail
        .create_provider(
            ProviderScope::Instance,
            f.owner.user,
            NewProvider {
                kind: ProviderKind::Cloudflare,
                name: "Cloudflare".to_owned(),
                secrets: None,
                settings: ProviderSettings::default(),
            },
            "request",
        )
        .await
        .unwrap();
    f.mail
        .set_domain_route(
            f.workspace,
            f.owner.user,
            f.domain,
            Some(created.provider.id),
            "request",
        )
        .await
        .unwrap();
    let intake = ProviderIntake::new(
        Arc::clone(&f.mail),
        52_428_800,
        Arc::new(Authenticator::with_static_dns(StaticDns::new()).unwrap()),
        "mx.orbit.test".to_owned(),
    );
    let forged = format!("{}@one.test", bounce_local_part(&bob.to_bytes(), &[7; 32]));
    let delivery = |to: &str| ProviderDelivery {
        raw: dsn(
            to,
            "bob@out.test",
            "failed",
            "5.1.1",
            "550 5.1.1 User unknown",
        ),
        mail_from: None,
        recipients: vec![to.to_owned()],
        peer_ip: None,
        helo: None,
        spf: SpfSource::None,
    };
    let result = intake
        .deliver(&created.provider, delivery(&forged), TimestampMillis::now())
        .await;
    assert!(
        matches!(
            result,
            DeliveryResult::Stored {
                receipts: 0,
                rejected: 0
            }
        ),
        "{result:?}"
    );
    assert_eq!(f.state_of(bob).await.0, "delivered");
    assert_eq!(
        f.count(
            "SELECT CAST(COUNT(*) AS BIGINT) FROM mail_rejected_deliveries",
            &[]
        )
        .await,
        0,
        "nothing recorded"
    );

    let result = intake
        .deliver(
            &created.provider,
            delivery(&f.bounce_address(bob)),
            TimestampMillis::now(),
        )
        .await;
    assert!(
        matches!(result, DeliveryResult::Stored { receipts: 0, .. }),
        "{result:?}"
    );
    assert_eq!(f.state_of(bob).await.0, "bounced");
}

#[tokio::test]
async fn retry_removes_suppression_and_requeues() {
    let f = Fixture::new().await;
    f.fake.answer(
        "gone@out.test",
        RecipientOutcome::bounced("permanent error (550): 5.1.1 No such user"),
    );
    let id = f.send(&["gone@out.test"]).await;
    let gone = f.recipient(id, "gone@out.test").await;
    assert!(f.suppressed("gone@out.test").await.is_some());
    let done: String = sqlx::query_scalar("SELECT status FROM mail_outbound WHERE id = $1")
        .bind(id.to_string())
        .fetch_one(f.database.pool())
        .await
        .unwrap();
    assert_eq!(done, "done");

    // Someone who is neither the author nor a member of the mailbox cannot retry.
    let outsider = f.member("Dan").await;
    let path = format!("/outbound/{id}/recipients/{gone}/retry");
    let (status, _) = f.call(&outsider, "POST", &path).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // The address takes mail now.
    f.fake.answer(
        "gone@out.test",
        RecipientOutcome::Accepted {
            provider_message_id: None,
        },
    );

    let (status, body) = f.call(&f.owner, "POST", &path).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "queued");
    assert_eq!(body["attempts"], 0);
    assert_eq!(body["bounce_kind"], Value::Null);
    assert_eq!(f.suppressed("gone@out.test").await, None);
    let (status, kind, attempts, response) = f.state_of(gone).await;
    assert_eq!(
        (status.as_str(), kind, attempts, response),
        ("queued", None, 0, None)
    );
    let row = sqlx::query("SELECT status FROM mail_outbound WHERE id = $1")
        .bind(id.to_string())
        .fetch_one(f.database.pool())
        .await
        .unwrap();
    assert_eq!(row.get::<String, _>("status"), "released");
    let jobs = f
        .count(
            "SELECT CAST(COUNT(*) AS BIGINT) FROM jobs WHERE kind = 'mail.send' AND payload_json = $1",
            &[&json!({"outbound_id": id.to_string()}).to_string()],
        )
        .await;
    assert_eq!(jobs, 1);

    // The run sends it again.
    run_send_job(&f.state(), id, TimestampMillis::now())
        .await
        .unwrap();
    assert_eq!(f.state_of(gone).await.0, "delivered");
    // A delivered recipient is not retried.
    let (status, _) = f.call(&f.owner, "POST", &path).await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn member_cannot_list_suppressions() {
    let f = Fixture::new().await;
    let mut transaction = f.database.immediate_transaction().await.unwrap();
    for (index, address) in ["Gone@Out.test", "old@out.test", "x@other.test"]
        .iter()
        .enumerate()
    {
        MailRepository::add_suppression(
            &mut transaction,
            f.workspace,
            address,
            SuppressionReason::HardBounce,
            Some("550 5.1.1 No such user"),
            None,
            at(1_000 + i64::try_from(index).unwrap()),
        )
        .await
        .unwrap();
    }
    transaction.commit().await.unwrap();
    let member = f.member("Ann").await;
    let (status, body) = f.call(&member, "GET", "/suppressions").await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["code"], "mail_forbidden");
    let (status, _) = f
        .call(&member, "DELETE", "/suppressions/gone@out.test")
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, body) = f.call(&f.owner, "GET", "/suppressions").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let addresses: Vec<&str> = body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["address"].as_str().unwrap())
        .collect();
    assert_eq!(addresses, ["x@other.test", "old@out.test", "gone@out.test"]);
    assert_eq!(body["items"][0]["reason"], "hard_bounce");
    assert_eq!(body["cursor"], Value::Null);
    let (_, body) = f.call(&f.owner, "GET", "/suppressions?q=OUT.test").await;
    assert_eq!(body["items"].as_array().unwrap().len(), 2);
    let (_, body) = f.call(&f.owner, "GET", "/suppressions?q=%25").await;
    assert_eq!(body["items"].as_array().unwrap().len(), 0, "% is literal");

    let (status, _) = f
        .call(&f.owner, "DELETE", "/suppressions/Gone%40out.test")
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = f
        .call(&f.owner, "DELETE", "/suppressions/gone@out.test")
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        !f.mail
            .is_suppressed(f.workspace, "gone@out.test")
            .await
            .unwrap()
    );
    assert!(
        f.mail
            .is_suppressed(f.workspace, "OLD@out.test")
            .await
            .unwrap()
    );
}

/// A DSN from a server where an alias expanded the address: `Final-Recipient` is the inner
/// address, `Original-Recipient` the one Orbit sent to.
fn alias_dsn(original: &str, final_recipient: &str) -> Vec<u8> {
    format!(
        "From: MAILER-DAEMON@mx.out.test\r\n\
         To: x\r\n\
         MIME-Version: 1.0\r\n\
         Content-Type: multipart/report; report-type=delivery-status; boundary=\"b1\"\r\n\
         \r\n\
         --b1\r\n\
         Content-Type: message/delivery-status\r\n\
         \r\n\
         Reporting-MTA: dns; mx.out.test\r\n\
         \r\n\
         Original-Recipient: rfc822;<{original}>\r\n\
         Final-Recipient: rfc822; <{final_recipient}>\r\n\
         Action: failed\r\n\
         Status: 5.1.1\r\n\
         Diagnostic-Code: smtp; 550 5.1.1 User unknown\r\n\
         \r\n\
         --b1--\r\n"
    )
    .into_bytes()
}

#[tokio::test]
async fn alias_expanded_dsn_matches_original_recipient() {
    let f = Fixture::new().await;
    let id = f.send(&["bob@out.test", "carl@out.test"]).await;
    let bob = f.recipient(id, "bob@out.test").await;
    // An original recipient that is another address does not bounce Bob.
    f.receive(
        &[&f.bounce_address(bob)],
        alias_dsn("carl@out.test", "bob.smith@inner.out.test"),
    )
    .await;
    assert_eq!(f.state_of(bob).await.0, "delivered");
    f.receive(
        &[&f.bounce_address(bob)],
        alias_dsn("Bob@out.test", "bob.smith@inner.out.test"),
    )
    .await;
    let (status, kind, _, _) = f.state_of(bob).await;
    assert_eq!(status, "bounced");
    assert_eq!(kind.as_deref(), Some("hard"));
    assert!(f.suppressed("bob@out.test").await.is_some());
    assert_eq!(f.suppressed("bob.smith@inner.out.test").await, None);
}

#[tokio::test]
async fn failed_dsn_with_temporary_status_gives_up_without_suppressing() {
    let f = Fixture::new().await;
    let id = f.send(&["bob@out.test"]).await;
    let bob = f.recipient(id, "bob@out.test").await;
    f.receive(
        &[&f.bounce_address(bob)],
        dsn(
            "x",
            "bob@out.test",
            "failed",
            "4.4.7",
            "421 4.4.7 Message expired",
        ),
    )
    .await;
    let (status, kind, _, _) = f.state_of(bob).await;
    assert_eq!(status, "bounced");
    assert_eq!(kind.as_deref(), Some("gave_up"));
    assert_eq!(f.suppressed("bob@out.test").await, None);
    assert_eq!(f.thread_state(id).await, ("inbox".to_owned(), false));
}

#[tokio::test]
async fn late_dsn_after_retry_is_ignored() {
    let f = Fixture::new().await;
    f.fake.answer(
        "bob@out.test",
        RecipientOutcome::bounced("permanent error (550): 5.1.1 No such user"),
    );
    let id = f.send(&["bob@out.test"]).await;
    let bob = f.recipient(id, "bob@out.test").await;
    f.fake.answer(
        "bob@out.test",
        RecipientOutcome::Accepted {
            provider_message_id: None,
        },
    );
    let path = format!("/outbound/{id}/recipients/{bob}/retry");
    let (status, _) = f.call(&f.owner, "POST", &path).await;
    assert_eq!(status, StatusCode::OK);
    let late = dsn(
        "x",
        "bob@out.test",
        "failed",
        "5.1.1",
        "550 5.1.1 User unknown",
    );
    let reports = orbit_mail::parse_dsn(&late).unwrap();
    // While queued (its bounce address is not valid then either).
    assert!(
        !f.mail
            .process_dsn(bob, &reports, TimestampMillis::now())
            .await
            .unwrap()
    );
    assert_eq!(
        f.mail
            .lookup_recipient(&f.bounce_address(bob), TimestampMillis::now())
            .await
            .unwrap(),
        RecipientLookup::BadBounce
    );
    run_send_job(&f.state(), id, TimestampMillis::now())
        .await
        .unwrap();
    assert_eq!(f.state_of(bob).await.0, "delivered");
    // After the new send: a DSN of the first send is ignored.
    f.receive(&[&f.bounce_address(bob)], late).await;
    assert_eq!(f.state_of(bob).await.0, "delivered");
    assert_eq!(f.suppressed("bob@out.test").await, None);
}

#[tokio::test]
async fn mailbox_member_can_retry_and_other_workspace_cannot() {
    let f = Fixture::new().await;
    f.fake.answer(
        "gone@out.test",
        RecipientOutcome::bounced("permanent error (550): 5.1.1 No such user"),
    );
    let id = f.send(&["gone@out.test"]).await;
    let gone = f.recipient(id, "gone@out.test").await;
    let path = format!("/outbound/{id}/recipients/{gone}/retry");

    // The same caller through another workspace of theirs.
    let other = WorkspaceRepository::new((*f.database).clone())
        .create(
            f.owner.user,
            "Second".to_owned(),
            "request",
            TimestampMillis::now(),
        )
        .await
        .unwrap()
        .id;
    let (status, _) = f.call_in(other, &f.owner, "POST", &path, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // A member of the mailbox who did not write the message.
    let ann = f.member("Ann").await;
    let (status, _) = f.call(&ann, "POST", &path).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "not a member of the mailbox yet"
    );
    f.mail
        .add_member(
            f.workspace,
            f.owner.user,
            f.mailbox,
            ann.membership,
            "request",
        )
        .await
        .unwrap();
    let (status, body) = f.call(&ann, "POST", &path).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "queued");
    assert_eq!(f.suppressed("gone@out.test").await, None);
}

#[tokio::test]
async fn bounce_local_part_is_reserved() {
    let f = Fixture::new().await;
    let created = f
        .mail
        .create_mailbox(
            f.workspace,
            f.owner.user,
            NewMailbox {
                kind: MailboxKind::Shared,
                name: "Bounces".to_owned(),
                domain_id: f.domain,
                local_part: "Bounce".to_owned(),
                owner_membership_id: None,
                member_ids: vec![],
            },
            "request",
        )
        .await;
    assert!(
        matches!(created, Err(MailError::Unprocessable("reserved_address"))),
        "{created:?}"
    );
    let (status, body) = f
        .call_in(
            f.workspace,
            &f.owner,
            "POST",
            &format!("/mailboxes/{}/addresses", f.mailbox),
            Some(json!({ "domain_id": f.domain.to_string(), "local_part": "bounce" })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["code"], "reserved_address");
    // Other local parts that start with it are fine.
    let (status, body) = f
        .call_in(
            f.workspace,
            &f.owner,
            "POST",
            &format!("/mailboxes/{}/addresses", f.mailbox),
            Some(json!({ "domain_id": f.domain.to_string(), "local_part": "bounces" })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}
