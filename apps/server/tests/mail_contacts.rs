//! Contacts and address autocomplete (spec §5.12).

#![allow(dead_code)]
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_mail::{AuthOutcome, DmarcVerdict, Verdict};
use orbit_platform::{
    AttachmentMutationCoordinator, AuthenticatedUser, Id, LocalBlobStore, MailMode,
    PasswordService, TestDatabase, TimestampMillis, UploadLimits, UploadService,
};
use orbit_server::auth_routes::CookieMode;
use orbit_server::mail_intake::{InboundMessage, MailIntake, Recipient, Source};
use orbit_server::mail_routes::{MailSettings, MailState, mail_router};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::instance_settings::InstanceSettingsRepository;
use orbit_server::repositories::mail::{IngestOutcome, MailRepository, MailboxKind, NewMailbox};
use serde_json::{Value, json};
use tower::ServiceExt;

const APP_KEY: [u8; 32] = [32; 32];

#[derive(Clone)]
struct Member {
    cookie: String,
    membership: Id,
}

struct Fixture {
    _root: tempfile::TempDir,
    database: TestDatabase,
    identity: Arc<IdentityRepository>,
    mail: Arc<MailRepository>,
    intake: MailIntake,
    workspace: Id,
    owner: Member,
    ann: Member,
    bob: Member,
    domain: Id,
    team: Id,
    team_primary: Id,
    team_alias: Id,
    sales: Id,
    sales_primary: Id,
    ann_personal: Id,
    ann_primary: Id,
}

impl Fixture {
    async fn new() -> Self {
        Self::with_limits(UploadLimits::default()).await
    }

    async fn with_limits(limits: UploadLimits) -> Self {
        Self::build(limits, false).await
    }

    async fn build(limits: UploadLimits, cloud: bool) -> Self {
        let database = TestDatabase::new().await.unwrap();
        let identity = Arc::new(IdentityRepository::new((*database).clone()));
        let now = TimestampMillis::now();
        identity
            .store_setup_token(
                "operator-secret",
                TimestampMillis::from_millis(now.as_millis() + 60_000),
            )
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
            limits,
        );
        let mail =
            Arc::new(MailRepository::new((*database).clone(), uploads, APP_KEY).with_cloud(cloud));
        let intake = MailIntake::new(Arc::clone(&mail), 52_428_800);
        let owner_membership: String = sqlx::query_scalar(
            "SELECT id FROM memberships WHERE workspace_id = $1 AND user_id = $2",
        )
        .bind(setup.workspace_id.to_string())
        .bind(setup.user_id.to_string())
        .fetch_one(database.pool())
        .await
        .unwrap();
        let owner = Member {
            cookie: format!("__Host-orbit_session={}", setup.session.token),
            membership: owner_membership.parse().unwrap(),
        };
        let domain = Id::new_v7();
        sqlx::query(
            "INSERT INTO mail_domains (id, workspace_id, name, status, verify_token, created_at, updated_at) \
             VALUES ($1, $2, 'one.test', 'verified', 'token', $3, $4)",
        )
        .bind(domain.to_string())
        .bind(setup.workspace_id.to_string())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(database.pool())
        .await
        .unwrap();
        let mut fixture = Self {
            _root: root,
            database,
            identity,
            mail,
            intake,
            workspace: setup.workspace_id,
            ann: owner.clone(),
            bob: owner.clone(),
            owner,
            domain,
            team: Id::new_v7(),
            team_primary: Id::new_v7(),
            team_alias: Id::new_v7(),
            sales: Id::new_v7(),
            sales_primary: Id::new_v7(),
            ann_personal: Id::new_v7(),
            ann_primary: Id::new_v7(),
        };
        fixture.ready().await;
        fixture.ann = fixture.member("Ann Lee").await;
        fixture.bob = fixture.member("Bob").await;
        let team = fixture
            .mailbox(
                MailboxKind::Shared,
                "Team",
                "team",
                None,
                vec![fixture.owner.membership, fixture.ann.membership],
            )
            .await;
        fixture.team = team.id;
        fixture.team_primary = team.addresses[0].id;
        let team = fixture
            .mail
            .add_alias(
                fixture.workspace,
                user_of(&fixture, &fixture.owner).await,
                fixture.team,
                domain,
                "support",
                None,
                "request",
            )
            .await
            .unwrap();
        fixture.team_alias = team
            .addresses
            .iter()
            .find(|address| address.local_part == "support")
            .unwrap()
            .id;
        let sales = fixture
            .mailbox(
                MailboxKind::Shared,
                "Sales",
                "sales",
                None,
                vec![fixture.owner.membership],
            )
            .await;
        fixture.sales = sales.id;
        fixture.sales_primary = sales.addresses[0].id;
        let personal = fixture
            .mailbox(
                MailboxKind::Personal,
                "Ann Lee",
                "ann",
                Some(fixture.ann.membership),
                vec![],
            )
            .await;
        fixture.ann_personal = personal.id;
        fixture.ann_primary = personal.addresses[0].id;
        fixture
    }

    async fn mailbox(
        &self,
        kind: MailboxKind,
        name: &str,
        local_part: &str,
        owner_membership_id: Option<Id>,
        member_ids: Vec<Id>,
    ) -> orbit_server::repositories::mail::Mailbox {
        self.mail
            .create_mailbox(
                self.workspace,
                user_of(self, &self.owner).await,
                NewMailbox {
                    kind,
                    name: name.to_owned(),
                    domain_id: self.domain,
                    local_part: local_part.to_owned(),
                    owner_membership_id,
                    member_ids,
                },
                "request",
            )
            .await
            .unwrap()
    }

    async fn member(&self, display_name: &str) -> Member {
        let id = Id::new_v7();
        let membership_id = Id::new_v7();
        let now = TimestampMillis::now();
        let email = format!("{id}@example.com");
        sqlx::query(
            "INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, 'unused', $5, $6)",
        )
        .bind(id.to_string())
        .bind(&email)
        .bind(&email)
        .bind(display_name)
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(self.database.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at) \
             VALUES ($1, $2, $3, 'member', 0, $4, $5)",
        )
        .bind(membership_id.to_string())
        .bind(self.workspace.to_string())
        .bind(id.to_string())
        .bind(now.as_millis())
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
            membership: membership_id,
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

    async fn call(
        &self,
        member: &Member,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let builder = Request::builder()
            .method(method)
            .uri(format!("/api/v1/workspaces/{}/mail{path}", self.workspace))
            .header(header::COOKIE, &member.cookie);
        let request = match body {
            Some(value) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(value.to_string())),
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

    /// Delivers a message to the mailbox of `address_id`, sent to `rcpt`; returns its message id
    /// and thread id.
    async fn deliver(&self, rcpt: &str, address_id: Id) -> (Id, Id) {
        let raw = format!(
            "From: Carl <carl@sender.test>\r\nTo: {rcpt}\r\nSubject: Hello\r\n\
             Message-ID: <m1@sender.test>\r\n\r\nHi\r\n"
        );
        self.deliver_raw(raw, rcpt, address_id).await
    }

    async fn deliver_raw(&self, raw: String, rcpt: &str, address_id: Id) -> (Id, Id) {
        let ids = self
            .intake
            .accept(InboundMessage {
                source: Source::Smtp,
                trace_len: 0,
                raw: raw.into_bytes(),
                mail_from: Some("carl@sender.test".to_owned()),
                recipients: vec![Recipient::Mailbox {
                    workspace_id: self.workspace,
                    address_id,
                    rcpt: rcpt.to_owned(),
                }],
                peer_ip: IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)),
                helo: "mx.sender.test".to_owned(),
                tls: None,
                auth: AuthOutcome {
                    spf: Verdict::Pass,
                    dkim: Verdict::Pass,
                    dmarc: DmarcVerdict::Pass,
                    aligned_domain: Some("sender.test".to_owned()),
                    temp_error: false,
                    spf_domain: Some("sender.test".to_owned()),
                    dkim_domain: Some("sender.test".to_owned()),
                    from_domain: Some("sender.test".to_owned()),
                },
            })
            .await
            .unwrap();
        let IngestOutcome::Ingested { message_id, .. } = self
            .mail
            .ingest(ids[0], TimestampMillis::now())
            .await
            .unwrap()
        else {
            panic!("not ingested");
        };
        let thread: String =
            sqlx::query_scalar("SELECT thread_id FROM mail_thread_messages WHERE message_id = $1")
                .bind(message_id.to_string())
                .fetch_one(self.database.pool())
                .await
                .unwrap();
        (message_id, thread.parse().unwrap())
    }
}

/// The user id behind a member (the mailbox API takes users).
async fn user_of(fixture: &Fixture, member: &Member) -> Id {
    let user: String = sqlx::query_scalar("SELECT user_id FROM memberships WHERE id = $1")
        .bind(member.membership.to_string())
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    user.parse().unwrap()
}

impl Fixture {
    /// Makes the domain able to send: MX live, an active DKIM key and a route.
    async fn ready(&self) {
        let now = TimestampMillis::now().as_millis();
        let route = Id::new_v7();
        sqlx::query(
            "INSERT INTO mail_send_routes (id, workspace_id, kind, name, secrets, settings, \
             created_at, updated_at) VALUES ($1, NULL, 'smtp', 'Relay', $2, '{}', $3, $3)",
        )
        .bind(route.to_string())
        .bind(b"sealed".to_vec())
        .bind(now)
        .execute(self.database.pool())
        .await
        .unwrap();
        sqlx::query(
            "UPDATE mail_domains SET send_route_id = $1, dns_report = $2 WHERE id = $3",
        )
        .bind(route.to_string())
        .bind(
            json!({"verify": {"status": "ok"}, "mx": {"status": "ok"}, "dkim": {"status": "ok"},
                   "spf": {"status": "ok"}, "dmarc": {"status": "ok"}})
            .to_string(),
        )
        .bind(self.domain.to_string())
        .execute(self.database.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO mail_dkim_keys (id, domain_id, selector, public_key, private_sealed, \
             status, created_at, updated_at) VALUES ($1, $2, 'orbit1', 'key', $3, 'active', $4, $4)",
        )
        .bind(Id::new_v7().to_string())
        .bind(self.domain.to_string())
        .bind(b"sealed".to_vec())
        .bind(now)
        .execute(self.database.pool())
        .await
        .unwrap();
    }

    async fn sql(&self, sql: &str) {
        sqlx::query(sqlx::AssertSqlSafe(sql.to_owned()))
            .execute(self.database.pool())
            .await
            .unwrap();
    }

    /// A new draft of Ann's with the given recipients.
    async fn draft(&self, to: &[&str]) -> String {
        let (status, draft) = self
            .call(
                &self.ann,
                "POST",
                "/drafts",
                Some(
                    json!({"mode": "new", "from_address_id": self.ann_primary.to_string(),
                            "to": to, "subject": "Hello", "body_html": "<p>Hi</p>"}),
                ),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{draft}");
        draft["id"].as_str().unwrap().to_owned()
    }

    async fn send(&self, draft: &str, body: Value) -> (StatusCode, Value) {
        self.call(
            &self.ann,
            "POST",
            &format!("/drafts/{draft}/send"),
            Some(body),
        )
        .await
    }

    async fn outbound_row(&self, id: &str) -> (String, i64, i64) {
        let row =
            sqlx::query("SELECT status, release_at, created_at FROM mail_outbound WHERE id = $1")
                .bind(id)
                .fetch_one(self.database.pool())
                .await
                .unwrap();
        use sqlx::Row;
        (
            row.get("status"),
            row.get("release_at"),
            row.get("created_at"),
        )
    }

    async fn jobs(&self) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM jobs WHERE kind = 'mail.send'")
            .fetch_one(self.database.pool())
            .await
            .unwrap()
    }
}

fn at(millis: i64) -> TimestampMillis {
    TimestampMillis::from_millis(millis)
}
impl Fixture {
    /// Gives the domain a real active DKIM key (the fixture's is a placeholder); returns its
    /// selector and its TXT record.
    async fn signing(&self) -> (String, String) {
        let key = self
            .mail
            .rotate_dkim(
                self.workspace,
                user_of(self, &self.owner).await,
                self.domain,
                "request",
                TimestampMillis::now(),
            )
            .await
            .unwrap();
        self.sql(&format!(
            "DELETE FROM mail_dkim_keys WHERE domain_id = '{}' AND selector <> '{}'",
            self.domain, key.selector
        ))
        .await;
        self.sql(&format!(
            "UPDATE mail_dkim_keys SET status = 'active' WHERE domain_id = '{}'",
            self.domain
        ))
        .await;
        (
            key.selector,
            format!("v=DKIM1; k=rsa; p={}", key.public_key),
        )
    }
}

const WEEK_MS: i64 = 7 * 24 * 60 * 60 * 1000;

impl Fixture {
    /// Delivers `raw` from `mail_from` to `rcpt`; returns the message and its thread.
    async fn deliver_as(
        &self,
        raw: &str,
        mail_from: &str,
        rcpt: &str,
        address_id: Id,
        at_ms: TimestampMillis,
    ) -> (Id, Id) {
        let ids = self
            .intake
            .accept(InboundMessage {
                source: Source::Smtp,
                trace_len: 0,
                raw: raw.as_bytes().to_vec(),
                mail_from: Some(mail_from.to_owned()),
                recipients: vec![Recipient::Mailbox {
                    workspace_id: self.workspace,
                    address_id,
                    rcpt: rcpt.to_owned(),
                }],
                peer_ip: IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)),
                helo: "mx.sender.test".to_owned(),
                tls: None,
                auth: AuthOutcome {
                    spf: Verdict::Pass,
                    dkim: Verdict::Pass,
                    dmarc: DmarcVerdict::Pass,
                    aligned_domain: Some("sender.test".to_owned()),
                    temp_error: false,
                    spf_domain: Some("sender.test".to_owned()),
                    dkim_domain: Some("sender.test".to_owned()),
                    from_domain: Some("sender.test".to_owned()),
                },
            })
            .await
            .unwrap();
        let IngestOutcome::Ingested { message_id, .. } =
            self.mail.ingest(ids[0], at_ms).await.unwrap()
        else {
            panic!("not ingested");
        };
        let thread: String =
            sqlx::query_scalar("SELECT thread_id FROM mail_thread_messages WHERE message_id = $1")
                .bind(message_id.to_string())
                .fetch_one(self.database.pool())
                .await
                .unwrap();
        (message_id, thread.parse().unwrap())
    }

    /// A message from Carl to `rcpt` with extra header lines.
    async fn mail(&self, id: &str, rcpt: &str, address_id: Id, extra: &str) -> (Id, Id) {
        let raw = format!(
            "From: Carl <carl@sender.test>\r\nTo: {rcpt}\r\nSubject: Hello\r\n\
             Message-ID: <{id}@sender.test>\r\n{extra}\r\nHi\r\n"
        );
        self.deliver_as(
            &raw,
            "carl@sender.test",
            rcpt,
            address_id,
            TimestampMillis::now(),
        )
        .await
    }

    async fn away(&self, member: &Member, mailbox: Id, extra: Value) -> (StatusCode, Value) {
        let mut body = json!({"enabled": true, "body_html": "<p>I am away.</p>"});
        for (key, value) in extra.as_object().unwrap() {
            body[key] = value.clone();
        }
        self.call(
            member,
            "PUT",
            &format!("/mailboxes/{mailbox}/vacation"),
            Some(body),
        )
        .await
    }

    async fn auto_outbound(&self, mailbox: Id) -> Vec<Id> {
        let rows: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM mail_outbound WHERE mailbox_id = $1 AND auto_reply IS NOT NULL \
             ORDER BY created_at, id",
        )
        .bind(mailbox.to_string())
        .fetch_all(self.database.pool())
        .await
        .unwrap();
        rows.iter().map(|id| id.parse().unwrap()).collect()
    }
}

impl Fixture {
    async fn contact(
        &self,
        member: &Member,
        address: &str,
        name: Option<&str>,
        count: i64,
        last: i64,
    ) {
        sqlx::query(
            "INSERT INTO mail_contacts (workspace_id, membership_id, address, name, send_count, \
             last_sent_at, hidden, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, 0, 1, 1)",
        )
        .bind(self.workspace.to_string())
        .bind(member.membership.to_string())
        .bind(address)
        .bind(name)
        .bind(count)
        .bind(last)
        .execute(self.database.pool())
        .await
        .unwrap();
    }

    async fn search(&self, member: &Member, q: &str) -> Vec<(String, String)> {
        let (status, hits) = self
            .call(member, "GET", &format!("/contacts?q={q}"), None)
            .await;
        assert_eq!(status, StatusCode::OK, "{hits}");
        hits.as_array()
            .unwrap()
            .iter()
            .map(|hit| {
                (
                    hit["address"].as_str().unwrap().to_owned(),
                    hit["kind"].as_str().unwrap().to_owned(),
                )
            })
            .collect()
    }
}

fn pair(address: &str, kind: &str) -> (String, String) {
    (address.to_owned(), kind.to_owned())
}

#[tokio::test]
async fn members_first_then_by_score() {
    let f = Fixture::new().await;
    let now = TimestampMillis::now().as_millis();
    let day = 24 * 60 * 60 * 1000;
    f.contact(&f.owner, "annabel@out.test", Some("Annabel"), 1, now)
        .await;
    // Ann's own list: many sends but long ago, one send today, a few sends this week.
    f.contact(&f.ann, "ann.old@out.test", None, 10, now - 120 * day)
        .await;
    f.contact(&f.ann, "ann.once@out.test", None, 1, now).await;
    f.contact(
        &f.ann,
        "ann.week@out.test",
        Some("Ann Week"),
        6,
        now - 7 * day,
    )
    .await;
    let hits = f.search(&f.ann, "ann").await;
    assert_eq!(
        hits,
        vec![
            pair("ann@one.test", "member"),
            pair("ann.week@out.test", "contact"),
            pair("ann.once@out.test", "contact"),
            pair("ann.old@out.test", "contact"),
        ],
        "the member's mailbox address, then by count and recency; another member's contacts stay private"
    );
    // A name matches too; a member without a mailbox is listed by their sign-in address.
    let hits = f.search(&f.ann, "bob").await;
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].1, "member");
}

#[tokio::test]
async fn limit_8() {
    let f = Fixture::new().await;
    let now = TimestampMillis::now().as_millis();
    for index in 0..12 {
        f.contact(
            &f.ann,
            &format!("zed{index:02}@out.test"),
            None,
            1 + index,
            now,
        )
        .await;
    }
    let hits = f.search(&f.ann, "zed").await;
    assert_eq!(hits.len(), 8);
    assert_eq!(hits[0].0, "zed11@out.test");
}

#[tokio::test]
async fn hidden_contact_not_returned() {
    let f = Fixture::new().await;
    let now = TimestampMillis::now().as_millis();
    f.contact(&f.ann, "gone@out.test", Some("Gone"), 3, now)
        .await;
    f.contact(&f.ann, "kept@out.test", Some("Kept"), 1, now)
        .await;
    f.mail("h1", "ann@one.test", f.ann_primary, "").await;
    assert_eq!(f.search(&f.ann, "out.test").await.len(), 2);
    let (status, _) = f
        .call(&f.ann, "DELETE", "/contacts/gone%40out.test", None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        f.search(&f.ann, "out.test").await,
        vec![pair("kept@out.test", "contact")]
    );
    // A seen address can be removed too.
    assert_eq!(
        f.search(&f.ann, "carl").await,
        vec![pair("carl@sender.test", "seen")]
    );
    let (status, _) = f
        .call(&f.ann, "DELETE", "/contacts/carl%40sender.test", None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(f.search(&f.ann, "carl").await.is_empty());
    // Only for the caller.
    f.contact(&f.owner, "gone@out.test", None, 1, now).await;
    assert_eq!(
        f.search(&f.owner, "gone").await,
        vec![pair("gone@out.test", "contact")]
    );
    // Writing to it again brings it back.
    f.signing().await;
    let draft = f.draft(&["gone@out.test"]).await;
    let (_, sent) = f.send(&draft, json!({})).await;
    let id: Id = sent["id"].as_str().unwrap().parse().unwrap();
    let (_, release_at, _) = f.outbound_row(&id.to_string()).await;
    f.mail
        .release(id, None, at(release_at))
        .await
        .unwrap()
        .expect("released");
    assert_eq!(
        f.search(&f.ann, "gone").await,
        vec![pair("gone@out.test", "contact")]
    );
}

#[tokio::test]
async fn release_records_recipients_with_names() {
    let f = Fixture::new().await;
    f.signing().await;
    // Carl's name is known from his mail.
    f.mail("r1", "ann@one.test", f.ann_primary, "").await;
    let (status, draft) = f
        .call(
            &f.ann,
            "POST",
            "/drafts",
            Some(
                json!({"mode": "new", "from_address_id": f.ann_primary.to_string(),
                        "to": ["Carl@Sender.test"], "cc": ["dee@out.test"], "bcc": ["eve@out.test"],
                        "subject": "Hi", "body_html": "<p>Hi</p>"}),
            ),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{draft}");
    let (_, sent) = f.send(draft["id"].as_str().unwrap(), json!({})).await;
    let id: Id = sent["id"].as_str().unwrap().parse().unwrap();
    let (_, release_at, _) = f.outbound_row(&id.to_string()).await;
    f.mail
        .release(id, None, at(release_at))
        .await
        .unwrap()
        .expect("released");
    let rows: Vec<(String, Option<String>, i64)> = sqlx::query_as(
        "SELECT address, name, send_count FROM mail_contacts WHERE membership_id = $1 ORDER BY address",
    )
    .bind(f.ann.membership.to_string())
    .fetch_all(f.database.pool())
    .await
    .unwrap();
    assert_eq!(
        rows,
        vec![
            ("carl@sender.test".to_owned(), Some("Carl".to_owned()), 1),
            ("dee@out.test".to_owned(), None, 1),
            ("eve@out.test".to_owned(), None, 1),
        ]
    );
    // Contacts rank above a seen address of the same person.
    assert_eq!(
        f.search(&f.ann, "carl").await,
        vec![pair("carl@sender.test", "contact")]
    );
}

#[tokio::test]
async fn seen_addresses_from_own_mailboxes_only() {
    let f = Fixture::new().await;
    f.mail("n1", "ann@one.test", f.ann_primary, "").await;
    let raw = "From: Eve <eve@sender.test>\r\nTo: sales@one.test\r\nSubject: Quote\r\n\
               Message-ID: <n2@sender.test>\r\n\r\nHi\r\n";
    f.deliver_as(
        raw,
        "eve@sender.test",
        "sales@one.test",
        f.sales_primary,
        TimestampMillis::now(),
    )
    .await;
    // Ann reads her own mailbox and Team, not Sales; the owner reads Team and Sales.
    assert_eq!(
        f.search(&f.ann, "sender.test").await,
        vec![pair("carl@sender.test", "seen")]
    );
    assert_eq!(
        f.search(&f.owner, "sender.test").await,
        vec![pair("eve@sender.test", "seen")]
    );
    assert!(f.search(&f.bob, "sender.test").await.is_empty());
}

impl Fixture {
    async fn raw_of(&self, message: Id) -> Vec<u8> {
        self.mail
            .message_raw(self.workspace, user_of(self, &self.ann).await, message)
            .await
            .unwrap()
            .0
    }

    async fn text(&self, sql: &str, id: impl ToString) -> Option<String> {
        sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
            .bind(id.to_string())
            .fetch_one(self.database.pool())
            .await
            .unwrap()
    }

    async fn number(&self, sql: &str, id: impl ToString) -> Option<i64> {
        sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
            .bind(id.to_string())
            .fetch_one(self.database.pool())
            .await
            .unwrap()
    }

    async fn thread_ids(&self, member: &Member, query: &str) -> Vec<String> {
        let (status, page) = self
            .call(member, "GET", &format!("/threads?{query}"), None)
            .await;
        assert_eq!(status, StatusCode::OK, "{page}");
        page["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["id"].as_str().unwrap().to_owned())
            .collect()
    }

    async fn unread(&self, member: &Member, thread: Id) -> bool {
        let (status, detail) = self
            .call(member, "GET", &format!("/threads/{thread}"), None)
            .await;
        assert_eq!(status, StatusCode::OK, "{detail}");
        detail["thread"]["unread"].as_bool().unwrap()
    }
}

#[tokio::test]
async fn seen_addresses_skip_spam_and_trash() {
    let f = Fixture::new().await;
    let (_, thread) = f.mail("sp1", "ann@one.test", f.ann_primary, "").await;
    assert_eq!(
        f.search(&f.ann, "carl").await,
        vec![pair("carl@sender.test", "seen")]
    );
    f.sql(&format!(
        "UPDATE mail_threads SET folder = 'spam' WHERE id = '{thread}'"
    ))
    .await;
    assert!(f.search(&f.ann, "carl").await.is_empty());
    f.sql(&format!(
        "UPDATE mail_threads SET folder = 'trash', trashed_at = 1 WHERE id = '{thread}'"
    ))
    .await;
    assert!(f.search(&f.ann, "carl").await.is_empty());
}
