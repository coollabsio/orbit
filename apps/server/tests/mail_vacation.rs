//! Vacation reply (spec §5.11).

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

#[tokio::test]
async fn replies_once_per_7_days() {
    let f = Fixture::new().await;
    let (status, body) = f.away(&f.ann, f.ann_personal, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (first, thread) = f.mail("a1", "ann@one.test", f.ann_primary, "").await;
    assert_eq!(f.auto_outbound(f.ann_personal).await.len(), 1);
    // The reply is queued now, with no draft and no undo delay.
    let row = sqlx::query(
        "SELECT status, release_at, created_at, draft_id FROM mail_outbound WHERE auto_reply IS NOT NULL",
    )
    .fetch_one(f.database.pool())
    .await
    .unwrap();
    use sqlx::Row;
    assert_eq!(row.get::<String, _>("status"), "held");
    assert_eq!(
        row.get::<i64, _>("release_at"),
        row.get::<i64, _>("created_at")
    );
    assert!(row.get::<Option<String>, _>("draft_id").is_none());
    assert_eq!(f.jobs().await, 1);

    f.mail("a2", "ann@one.test", f.ann_primary, "").await;
    assert_eq!(f.auto_outbound(f.ann_personal).await.len(), 1, "once");
    let now = TimestampMillis::now().as_millis();
    // Inside the 7 days: no. After them: yes.
    assert!(
        !f.mail
            .maybe_vacation_reply(f.ann_personal, thread, first, at(now + WEEK_MS - 60_000))
            .await
            .unwrap()
    );
    assert!(
        f.mail
            .maybe_vacation_reply(f.ann_personal, thread, first, at(now + WEEK_MS + 60_000))
            .await
            .unwrap()
    );
    assert_eq!(f.auto_outbound(f.ann_personal).await.len(), 2);
}

#[tokio::test]
async fn concurrent_messages_get_one_reply() {
    let f = Fixture::new().await;
    // Two messages filed before either reply is decided.
    let (m1, t1) = f.mail("c1", "ann@one.test", f.ann_primary, "").await;
    let (m2, t2) = f.mail("c2", "ann@one.test", f.ann_primary, "").await;
    f.away(&f.ann, f.ann_personal, json!({})).await;
    let now = TimestampMillis::now();
    let (a, b) = tokio::join!(
        f.mail.maybe_vacation_reply(f.ann_personal, t1, m1, now),
        f.mail.maybe_vacation_reply(f.ann_personal, t2, m2, now)
    );
    assert_eq!(
        [a.unwrap(), b.unwrap()]
            .iter()
            .filter(|sent| **sent)
            .count(),
        1
    );
    assert_eq!(f.auto_outbound(f.ann_personal).await.len(), 1);
}

#[tokio::test]
async fn outside_dates_no_reply() {
    let f = Fixture::new().await;
    // Ended.
    let (status, _) = f
        .away(
            &f.ann,
            f.ann_personal,
            json!({"starts_at": "2020-01-01", "ends_at": "2020-02-01"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    f.mail("d1", "ann@one.test", f.ann_primary, "").await;
    // Not started.
    f.away(
        &f.ann,
        f.ann_personal,
        json!({"starts_at": "2099-01-01", "ends_at": null}),
    )
    .await;
    f.mail("d2", "ann@one.test", f.ann_primary, "").await;
    // Off.
    f.away(
        &f.ann,
        f.ann_personal,
        json!({"enabled": false, "starts_at": null}),
    )
    .await;
    f.mail("d3", "ann@one.test", f.ann_primary, "").await;
    assert!(f.auto_outbound(f.ann_personal).await.is_empty());
    // Inside the dates.
    f.away(
        &f.ann,
        f.ann_personal,
        json!({"starts_at": "2020-01-01", "ends_at": "2099-01-01"}),
    )
    .await;
    f.mail("d4", "ann@one.test", f.ann_primary, "").await;
    assert_eq!(f.auto_outbound(f.ann_personal).await.len(), 1);
}

#[tokio::test]
async fn dates_use_the_owners_time_zone() {
    let f = Fixture::new().await;
    f.sql(&format!(
        "UPDATE users SET timezone = 'Asia/Tokyo' WHERE id IN \
         (SELECT user_id FROM memberships WHERE id = '{}')",
        f.ann.membership
    ))
    .await;
    let (status, body) = f
        .away(
            &f.ann,
            f.ann_personal,
            json!({"starts_at": "2026-11-20T09:00", "ends_at": "2026-11-25"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["timezone"], "Asia/Tokyo");
    assert!(
        body["starts_at"]
            .as_str()
            .unwrap()
            .starts_with("2026-11-20T00:00:00"),
        "{body}"
    );
    assert!(
        body["ends_at"]
            .as_str()
            .unwrap()
            .starts_with("2026-11-24T15:00:00"),
        "{body}"
    );
    let (status, got) = f
        .call(
            &f.ann,
            "GET",
            &format!("/mailboxes/{}/vacation", f.ann_personal),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(got["starts_at"], body["starts_at"]);
    // The end is after the start; a bad date is refused.
    let (status, _) = f
        .away(
            &f.ann,
            f.ann_personal,
            json!({"starts_at": "2026-11-25", "ends_at": "2026-11-20"}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = f
        .away(&f.ann, f.ann_personal, json!({"starts_at": "next week"}))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn contacts_only() {
    let f = Fixture::new().await;
    f.away(&f.ann, f.ann_personal, json!({"contacts_only": true}))
        .await;
    f.mail("e1", "ann@one.test", f.ann_primary, "").await;
    assert!(f.auto_outbound(f.ann_personal).await.is_empty());
    f.sql(&format!(
        "INSERT INTO mail_contacts (workspace_id, membership_id, address, name, send_count, \
         last_sent_at, hidden, created_at, updated_at) \
         VALUES ('{}', '{}', 'carl@sender.test', 'Carl', 2, 1, 0, 1, 1)",
        f.workspace, f.ann.membership
    ))
    .await;
    f.mail("e2", "ann@one.test", f.ann_primary, "").await;
    assert_eq!(f.auto_outbound(f.ann_personal).await.len(), 1);
}

#[tokio::test]
async fn no_reply_to_list_mail() {
    let f = Fixture::new().await;
    f.away(&f.ann, f.ann_personal, json!({})).await;
    for (index, header) in [
        "List-Id: <news.sender.test>\r\n",
        "List-Unsubscribe: <mailto:u@sender.test>\r\n",
        "Precedence: bulk\r\n",
        "Auto-Submitted: auto-generated\r\n",
    ]
    .iter()
    .enumerate()
    {
        f.mail(&format!("l{index}"), "ann@one.test", f.ann_primary, header)
            .await;
    }
    // Only in Bcc: the mailbox is not in To or Cc.
    let raw = "From: Carl <carl@sender.test>\r\nTo: other@sender.test\r\nSubject: Hello\r\n\
               Message-ID: <l9@sender.test>\r\n\r\nHi\r\n";
    f.deliver_as(
        raw,
        "carl@sender.test",
        "ann@one.test",
        f.ann_primary,
        TimestampMillis::now(),
    )
    .await;
    // A mailbox of the workspace.
    let raw = "From: Team <team@one.test>\r\nTo: ann@one.test\r\nSubject: Hi\r\n\
               Message-ID: <l10@one.test>\r\n\r\nHi\r\n";
    f.deliver_as(
        raw,
        "team@one.test",
        "ann@one.test",
        f.ann_primary,
        TimestampMillis::now(),
    )
    .await;
    assert!(f.auto_outbound(f.ann_personal).await.is_empty());
}

#[tokio::test]
async fn reply_is_signed_and_in_sent() {
    let f = Fixture::new().await;
    let (selector, _) = f.signing().await;
    f.away(&f.ann, f.ann_personal, json!({})).await;
    let (message, thread) = f.mail("s1", "ann@one.test", f.ann_primary, "").await;
    let outbound = f.auto_outbound(f.ann_personal).await.remove(0);
    let now = TimestampMillis::now();
    let released = f
        .mail
        .release(outbound, Some("mx.orbit.test"), now)
        .await
        .unwrap()
        .expect("released");
    assert_eq!(released.thread_id, thread);
    let raw = String::from_utf8(f.raw_of(released.message_id).await).unwrap();
    assert!(raw.contains("Auto-Submitted: auto-replied\r\n"), "{raw}");
    assert!(raw.contains("In-Reply-To: <s1@sender.test>"), "{raw}");
    assert!(raw.contains("References: <s1@sender.test>"), "{raw}");
    assert!(raw.contains("Subject: Re: Hello\r\n"), "{raw}");
    assert!(
        raw.contains("To: <carl@sender.test>") || raw.contains("To: carl@sender.test"),
        "{raw}"
    );
    assert!(
        raw.contains(&format!("s={selector}")) && raw.contains("DKIM-Signature:"),
        "{raw}"
    );
    assert!(raw.contains("I am away."));
    assert!(!raw.contains("wrote:"), "no quote of the original");
    // Filed in Sent, with one recipient, counted; the thread stays unread; no draft was made.
    assert!(
        f.thread_ids(&f.ann, "sent=true")
            .await
            .contains(&thread.to_string())
    );
    assert!(f.unread(&f.ann, thread).await);
    let recipients: Vec<String> =
        sqlx::query_scalar("SELECT address FROM mail_outbound_recipients WHERE outbound_id = $1")
            .bind(outbound.to_string())
            .fetch_all(f.database.pool())
            .await
            .unwrap();
    assert_eq!(recipients, vec!["carl@sender.test".to_owned()]);
    // The auto reply does not count as a contact.
    let contacts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM mail_contacts")
        .fetch_one(f.database.pool())
        .await
        .unwrap();
    assert_eq!(contacts, 0);
    let _ = message;
}

#[tokio::test]
async fn from_is_the_receiving_address_without_tag_and_subject_is_configured() {
    let f = Fixture::new().await;
    f.signing().await;
    f.away(
        &f.owner,
        f.team,
        json!({"subject": "Out of office", "body_html": "<p>Back soon</p><script>x()</script>"}),
    )
    .await;
    let (_, _) = f
        .mail("t1", "Support <support+vip@one.test>", f.team_alias, "")
        .await;
    let outbound = f.auto_outbound(f.team).await.remove(0);
    let released = f
        .mail
        .release(outbound, Some("mx.orbit.test"), TimestampMillis::now())
        .await
        .unwrap()
        .expect("released");
    let raw = String::from_utf8(f.raw_of(released.message_id).await).unwrap();
    assert!(raw.contains("<support@one.test>"), "{raw}");
    assert!(!raw.contains("+vip"), "{raw}");
    assert!(raw.contains("Subject: Out of office\r\n"), "{raw}");
    assert!(
        raw.contains("Back soon") && !raw.contains("<script"),
        "{raw}"
    );
}

#[tokio::test]
async fn member_cannot_set_shared_vacation() {
    let f = Fixture::new().await;
    // Ann is a member of Team, not a manager.
    let (status, _) = f.away(&f.ann, f.team, json!({})).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = f
        .call(
            &f.ann,
            "GET",
            &format!("/mailboxes/{}/vacation", f.team),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "members read it");
    // The workspace owner manages mail.
    let (status, body) = f.away(&f.owner, f.team, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["enabled"], true);
    // Bob is not in Sales or Ann's personal mailbox: not found.
    let (status, _) = f.away(&f.bob, f.sales, json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = f.away(&f.bob, f.ann_personal, json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // Nobody but the owner sets a personal mailbox.
    let (status, _) = f.away(&f.owner, f.ann_personal, json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // An enabled vacation needs a body.
    let (status, _) = f
        .away(&f.ann, f.ann_personal, json!({"body_html": "  "}))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
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
