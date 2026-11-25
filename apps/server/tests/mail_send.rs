//! Send, undo, scheduled send and the limits gate (spec §5.3, §5.8, §5.9).

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
    uploads: UploadService,
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
        let mail = Arc::new(
            MailRepository::new((*database).clone(), uploads.clone(), APP_KEY).with_cloud(cloud),
        );
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
            uploads,
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

#[tokio::test]
async fn send_creates_held_outbound_with_undo_delay() {
    let f = Fixture::new().await;
    let draft = f.draft(&["x@out.test"]).await;
    let (status, sent) = f.send(&draft, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{sent}");
    assert_eq!(sent["status"], "held");
    assert_eq!(sent["scheduled"], false);
    let (status, release, created) = f.outbound_row(sent["id"].as_str().unwrap()).await;
    assert_eq!(status, "held");
    assert_eq!(release - created, 10_000, "The default undo time is 10 s");
    let queued: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM jobs WHERE kind = 'mail.send' AND available_at = $1",
    )
    .bind(release)
    .fetch_one(f.database.pool())
    .await
    .unwrap();
    assert_eq!(queued, 1);
    // The draft stays for Undo.
    let (status, _) = f
        .call(&f.ann, "GET", &format!("/drafts/{draft}"), None)
        .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn zero_delay_releases_now() {
    let f = Fixture::new().await;
    f.sql(&format!(
        "INSERT INTO mail_member_settings (membership_id, undo_send_seconds, updated_at) \
         VALUES ('{}', 0, 0)",
        f.ann.membership
    ))
    .await;
    let draft = f.draft(&["x@out.test"]).await;
    let (_, sent) = f.send(&draft, json!({})).await;
    let id: Id = sent["id"].as_str().unwrap().parse().unwrap();
    let (_, release, created) = f.outbound_row(&id.to_string()).await;
    assert_eq!(release, created);
    assert!(f.mail.claim_release(id, at(release)).await.unwrap());
}

#[tokio::test]
async fn send_twice_same_outbound() {
    let f = Fixture::new().await;
    let draft = f.draft(&["x@out.test"]).await;
    let (_, first) = f.send(&draft, json!({})).await;
    let (status, second) = f.send(&draft, json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first["id"], second["id"]);
    assert_eq!(f.jobs().await, 1, "No second job");
}

#[tokio::test]
async fn cancel_returns_draft() {
    let f = Fixture::new().await;
    let draft = f.draft(&["x@out.test"]).await;
    let (_, sent) = f.send(&draft, json!({})).await;
    let id = sent["id"].as_str().unwrap();
    let (status, back) = f
        .call(&f.ann, "POST", &format!("/outbound/{id}/cancel"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{back}");
    assert_eq!(back["id"], draft.as_str());
    assert_eq!(f.outbound_row(id).await.0, "cancelled");
    // Sent again after the edit: the same row is held again.
    let (status, again) = f.send(&draft, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["id"], id);
    assert_eq!(again["status"], "held");
    assert_eq!(f.jobs().await, 2);
    // Someone else cannot undo it.
    let (status, _) = f
        .call(&f.bob, "POST", &format!("/outbound/{id}/cancel"), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn cancel_after_release_conflicts() {
    let f = Fixture::new().await;
    let draft = f.draft(&["x@out.test"]).await;
    let (_, sent) = f.send(&draft, json!({})).await;
    let id = sent["id"].as_str().unwrap();
    let (_, release, _) = f.outbound_row(id).await;
    assert!(
        !f.mail
            .claim_release(id.parse().unwrap(), at(release - 1))
            .await
            .unwrap()
    );
    assert!(
        f.mail
            .claim_release(id.parse().unwrap(), at(release))
            .await
            .unwrap()
    );
    let (status, body) = f
        .call(&f.ann, "POST", &format!("/outbound/{id}/cancel"), None)
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "already_sent");
}

#[tokio::test]
async fn cancel_races_release_one_outcome() {
    let f = Fixture::new().await;
    let (membership, now) = (f.ann.membership, at(TimestampMillis::now().as_millis()));
    let (mut cancels, mut claims) = (0, 0);
    let mut rounds = 0;
    // 50 rounds, and more (up to 400) until each side has won once.
    while rounds < 50 || ((cancels == 0 || claims == 0) && rounds < 400) {
        rounds += 1;
        let draft = f.draft(&["x@out.test"]).await;
        let sent = f
            .mail
            .send_draft(membership, draft.parse().unwrap(), None, now)
            .await
            .unwrap();
        let (cancel, claim) = {
            let (a, b) = (Arc::clone(&f.mail), Arc::clone(&f.mail));
            let id = sent.id;
            let due = sent.release_at;
            let gate = Arc::new(tokio::sync::Barrier::new(2));
            let (g1, g2) = (Arc::clone(&gate), gate);
            let cancel = tokio::spawn(async move {
                g1.wait().await;
                a.cancel_outbound(membership, id, due).await
            });
            let claim = tokio::spawn(async move {
                g2.wait().await;
                b.claim_release(id, due).await
            });
            (cancel.await.unwrap(), claim.await.unwrap().unwrap())
        };
        let cancelled = cancel.is_ok();
        if cancelled {
            cancels += 1;
        } else {
            claims += 1;
        }
        assert_ne!(cancelled, claim, "Exactly one of cancel and release wins");
        if !cancelled {
            assert!(matches!(
                cancel,
                Err(orbit_server::repositories::mail::MailError::Conflict(
                    "already_sent"
                ))
            ));
        }
        let (status, ..) = f.outbound_row(&sent.id.to_string()).await;
        assert_eq!(status, if cancelled { "cancelled" } else { "released" });
    }
    assert!(
        cancels > 0 && claims > 0,
        "cancels {cancels}, releases {claims}"
    );
}

#[tokio::test]
async fn scheduled_send_holds_until_time() {
    let f = Fixture::new().await;
    let draft = f.draft(&["x@out.test"]).await;
    let when = TimestampMillis::now().as_millis() + 3_600_000;
    let (status, sent) = f.send(&draft, json!({"scheduled_at": at(when)})).await;
    assert_eq!(status, StatusCode::OK, "{sent}");
    assert_eq!(sent["scheduled"], true);
    let id = sent["id"].as_str().unwrap();
    assert_eq!(f.outbound_row(id).await.1, when);
    let (_, list) = f
        .call(&f.ann, "GET", "/outbound?view=scheduled", None)
        .await;
    assert_eq!(list.as_array().unwrap().len(), 1, "{list}");
    assert_eq!(list[0]["subject"], "Hello");
    let (_, outbox) = f.call(&f.ann, "GET", "/outbound?view=outbox", None).await;
    assert_eq!(outbox, json!([]));
    assert!(
        !f.mail
            .claim_release(id.parse().unwrap(), at(when - 1))
            .await
            .unwrap()
    );
    assert!(
        f.mail
            .claim_release(id.parse().unwrap(), at(when))
            .await
            .unwrap()
    );
    // A time in the past is refused.
    let other = f.draft(&["y@out.test"]).await;
    let (status, _) = f
        .send(&other, json!({"scheduled_at": at(when - 7_200_000)}))
        .await;
    assert!(status.is_client_error());
}

#[tokio::test]
async fn refused_when_not_ready() {
    let f = Fixture::new().await;
    let draft = f.draft(&["x@out.test"]).await;
    let cases = [
        (
            "UPDATE mail_domains SET status = 'pending'",
            "UPDATE mail_domains SET status = 'verified'",
            "domain_unverified",
        ),
        (
            "UPDATE mail_domains SET dns_report = '{\"verify\":{\"status\":\"ok\"},\"mx\":{\"status\":\"missing\"},\"dkim\":{\"status\":\"ok\"},\"spf\":{\"status\":\"ok\"},\"dmarc\":{\"status\":\"ok\"}}'",
            "UPDATE mail_domains SET dns_report = '{\"verify\":{\"status\":\"ok\"},\"mx\":{\"status\":\"ok\"},\"dkim\":{\"status\":\"ok\"},\"spf\":{\"status\":\"ok\"},\"dmarc\":{\"status\":\"ok\"}}'",
            "inbound_not_live",
        ),
        (
            "UPDATE mail_dkim_keys SET status = 'pending'",
            "UPDATE mail_dkim_keys SET status = 'active'",
            "dkim_inactive",
        ),
        (
            "UPDATE mail_domains SET send_route_id = NULL",
            "UPDATE mail_domains SET send_route_id = (SELECT id FROM mail_send_routes LIMIT 1)",
            "no_route",
        ),
        (
            "UPDATE mailboxes SET status = 'suspended'",
            "UPDATE mailboxes SET status = 'active'",
            "mailbox_suspended",
        ),
    ];
    for (break_it, fix_it, reason) in cases {
        f.sql(break_it).await;
        let (status, body) = f.send(&draft, json!({})).await;
        assert_eq!(status, StatusCode::CONFLICT, "{reason}: {body}");
        assert_eq!(body["code"], "send_not_ready");
        assert_eq!(body["reason"], reason, "{body}");
        f.sql(fix_it).await;
    }
    let (status, body) = f.send(&draft, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[tokio::test]
async fn refused_when_paused() {
    let f = Fixture::new().await;
    f.sql(&format!(
        "INSERT INTO mail_send_policies (workspace_id, tier, paused_at, pause_reason, created_at, updated_at) \
         VALUES ('{}', 'standard', 1, 'bounces', 1, 1)",
        f.workspace
    ))
    .await;
    let draft = f.draft(&["x@out.test"]).await;
    let (status, body) = f.send(&draft, json!({})).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "sending_paused");
}

#[tokio::test]
async fn own_route_skips_pause_and_cap() {
    let f = Fixture::new().await;
    f.sql(&format!(
        "INSERT INTO mail_send_policies (workspace_id, tier, paused_at, pause_reason, created_at, updated_at) \
         VALUES ('{}', 'new', 1, 'bounces', 1, 1)",
        f.workspace
    ))
    .await;
    f.sql(&format!(
        "UPDATE mail_send_routes SET workspace_id = '{}'",
        f.workspace
    ))
    .await;
    // the workspace's own relay: no pause, and more than the 50 recipients of the new tier
    let many: Vec<String> = (0..51).map(|i| format!("o{i}@out.test")).collect();
    let many: Vec<&str> = many.iter().map(String::as_str).collect();
    let draft = f.draft(&many).await;
    let (status, body) = f.send(&draft, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[tokio::test]
async fn recipient_cap_new_tier_50() {
    let f = Fixture::new().await;
    f.sql(&format!(
        "INSERT INTO mail_send_policies (workspace_id, tier, created_at, updated_at) \
         VALUES ('{}', 'new', 1, 1)",
        f.workspace
    ))
    .await;
    let limits = f.mail.limits_for(f.workspace).await.unwrap().unwrap();
    assert_eq!((limits.per_user_hour, limits.max_recipients), (30, 50));
    let many: Vec<String> = (0..51).map(|i| format!("r{i}@out.test")).collect();
    let many: Vec<&str> = many.iter().map(String::as_str).collect();
    let draft = f.draft(&many).await;
    let (status, body) = f.send(&draft, json!({})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["code"], "too_many_recipients");
    let draft = f.draft(&many[..50]).await;
    let (status, body) = f.send(&draft, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // Without a policy row a self-hosted workspace has no send limits.
    f.sql("DELETE FROM mail_send_policies").await;
    assert_eq!(f.mail.limits_for(f.workspace).await.unwrap(), None);
    let many: Vec<String> = (0..100).map(|i| format!("u{i}@out.test")).collect();
    let many: Vec<&str> = many.iter().map(String::as_str).collect();
    let draft = f.draft(&many).await;
    let (status, body) = f.send(&draft, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[tokio::test]
async fn over_the_window_waits_in_the_outbox() {
    let f = Fixture::new().await;
    f.sql(&format!(
        "INSERT INTO mail_send_policies (workspace_id, tier, created_at, updated_at) \
         VALUES ('{}', 'new', 1, 1)",
        f.workspace
    ))
    .await;
    // 30 recipients went 10 minutes ago: the user's hour is full.
    let now = TimestampMillis::now().as_millis();
    let earlier = now - 600_000;
    let old = Id::new_v7();
    f.sql(&format!(
        "INSERT INTO mail_outbound (id, workspace_id, membership_id, mailbox_id, status, release_at, \
         created_at, updated_at) VALUES ('{old}', '{}', '{}', '{}', 'done', {earlier}, {earlier}, {earlier})",
        f.workspace, f.ann.membership, f.ann_personal
    ))
    .await;
    for i in 0..30 {
        f.sql(&format!(
            "INSERT INTO mail_outbound_recipients (id, outbound_id, workspace_id, membership_id, address, \
             kind, status, released_at, created_at, updated_at) VALUES ('{}', '{old}', '{}', '{}', \
             'r{i}@out.test', 'to', 'delivered', {earlier}, {earlier}, {earlier})",
            Id::new_v7(),
            f.workspace,
            f.ann.membership
        ))
        .await;
    }
    let draft = f.draft(&["x@out.test"]).await;
    let (status, sent) = f.send(&draft, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{sent}");
    assert_eq!(sent["hold_reason"], "rate_limit");
    let (_, release, _) = f.outbound_row(sent["id"].as_str().unwrap()).await;
    assert_eq!(release, earlier + 3_600_000);
    let (_, outbox) = f.call(&f.ann, "GET", "/outbound?view=outbox", None).await;
    assert_eq!(outbox.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn size_over_route_limit_refused() {
    let f = Fixture::new().await;
    f.sql("UPDATE mail_send_routes SET settings = '{\"max_message_bytes\": 10000}'")
        .await;
    let (status, draft) = f
        .call(
            &f.ann,
            "POST",
            "/drafts",
            Some(json!({"mode": "new", "from_address_id": f.ann_primary.to_string(),
                        "to": ["x@out.test"], "body_html": format!("<p>{}</p>", "a".repeat(20_000))})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{draft}");
    let (status, body) = f.send(draft["id"].as_str().unwrap(), json!({})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["code"], "message_too_large");
    assert_eq!(body["limit_bytes"], 10_000);
    assert!(body["size_bytes"].as_i64().unwrap() > 20_000, "{body}");
}

#[tokio::test]
async fn from_addresses_carry_the_route_size_limit() {
    let f = Fixture::new().await;
    let (status, list) = f.call(&f.ann, "GET", "/from-addresses", None).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(list[0]["max_message_bytes"], 25_000_000, "smtp default");
    f.sql("UPDATE mail_send_routes SET settings = '{\"max_message_bytes\": 10000}'")
        .await;
    let (_, list) = f.call(&f.ann, "GET", "/from-addresses", None).await;
    assert_eq!(list[0]["max_message_bytes"], 10_000);
    f.sql("UPDATE mail_domains SET send_route_id = NULL").await;
    let (_, list) = f.call(&f.ann, "GET", "/from-addresses", None).await;
    assert_eq!(list[0]["max_message_bytes"], Value::Null);
}

#[tokio::test]
async fn inline_image_is_sent_as_multipart_related() {
    let f = Fixture::new().await;
    f.signing().await;
    let (status, draft) = f
        .call(
            &f.ann,
            "POST",
            "/drafts",
            Some(
                json!({"mode": "new", "from_address_id": f.ann_primary.to_string(),
                        "to": ["x@out.test"], "subject": "Pic"}),
            ),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{draft}");
    let id = draft["id"].as_str().unwrap();
    let png = b"\x89PNG\r\n\x1a\n0000000000";
    let (status, file) = f
        .upload_with(&f.ann, id, "pic.png", png, "?inline=true")
        .await;
    assert_eq!(status, StatusCode::CREATED, "{file}");
    let content_id = file["content_id"].as_str().unwrap().to_owned();
    let (status, body) = f
        .call(
            &f.ann,
            "PATCH",
            &format!("/drafts/{id}"),
            Some(json!({"version": 1,
                        "body_html": format!("<p>Look <img src=\"cid:{content_id}\"></p>")})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, released, _) = f.send_and_release(id).await;
    let raw = f.raw_of(released.message_id).await;
    let text = String::from_utf8_lossy(&raw).into_owned();
    assert!(text.contains("multipart/related"), "{text}");
    assert!(
        text.contains(&format!("Content-ID: <{content_id}>")),
        "{text}"
    );
    assert!(text.contains(&format!("cid:{content_id}")), "{text}");
    assert!(text.contains("Content-Disposition: inline"), "{text}");
}

#[tokio::test]
async fn bad_recipients_are_refused() {
    let f = Fixture::new().await;
    let draft = f.draft(&["not an address"]).await;
    let (status, body) = f.send(&draft, json!({})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["code"], "invalid_address");
    let empty = f.draft(&[]).await;
    let (status, body) = f.send(&empty, json!({})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["code"], "no_recipients");
}

#[tokio::test]
async fn outsider_cannot_send() {
    let f = Fixture::new().await;
    let draft = f.draft(&["x@out.test"]).await;
    // Bob has the workspace but not the draft.
    let (status, _) = f
        .call(
            &f.bob,
            "POST",
            &format!("/drafts/{draft}/send"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // The draft id of another workspace: the owner is in both, the draft is in one.
    let second =
        orbit_server::repositories::workspaces::WorkspaceRepository::new((*f.database).clone())
            .create(
                user_of(&f, &f.owner).await,
                "Second".to_owned(),
                "request",
                TimestampMillis::now(),
            )
            .await
            .unwrap()
            .id;
    let request = Request::builder()
        .method("POST")
        .uri(format!(
            "/api/v1/workspaces/{second}/mail/drafts/{draft}/send"
        ))
        .header(header::COOKIE, &f.owner.cookie)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let response = f.app().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    // A member who left the mailbox cannot send from it any more: 403, the draft is still theirs.
    f.sql(&format!(
        "DELETE FROM mailbox_members WHERE membership_id = '{}'",
        f.ann.membership
    ))
    .await;
    let (status, body) = f.send(&draft, json!({})).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["code"], "send_as_denied");
}

#[tokio::test]
async fn reply_from_another_mailbox_is_denied() {
    let f = Fixture::new().await;
    let (_, thread) = f.deliver("team@one.test", f.team_primary).await;
    let (status, draft) = f
        .call(
            &f.ann,
            "POST",
            "/drafts",
            Some(json!({"mode": "reply", "thread_id": thread.to_string(), "to": ["x@out.test"]})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{draft}");
    let id = draft["id"].as_str().unwrap();
    // The From moves to another mailbox behind the API's back (D13).
    f.sql(&format!(
        "UPDATE mail_drafts SET from_address_id = '{}', mailbox_id = '{}' WHERE id = '{id}'",
        f.ann_primary, f.ann_personal
    ))
    .await;
    let (status, body) = f.send(id, json!({})).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["code"], "send_as_denied");
}

#[tokio::test]
async fn a_held_draft_cannot_be_changed() {
    let f = Fixture::new().await;
    let draft = f.draft(&["x@out.test"]).await;
    let (_, sent) = f.send(&draft, json!({})).await;
    let outbound = sent["id"].as_str().unwrap();
    let path = format!("/drafts/{draft}");
    let (status, body) = f
        .call(
            &f.ann,
            "PATCH",
            &path,
            Some(json!({"version": 1, "subject": "Changed"})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "draft_sending");
    let (status, body) = f.call(&f.ann, "DELETE", &path, None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "draft_sending");
    let (status, body) = f.upload(&f.ann, &draft, "a.txt", b"hello").await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "draft_sending");
    // After Cancel the draft is editable again, files included.
    let (status, _) = f
        .call(
            &f.ann,
            "POST",
            &format!("/outbound/{outbound}/cancel"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = f.upload(&f.ann, &draft, "a.txt", b"hello").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let file = body["id"].as_str().unwrap().to_owned();
    let (status, _) = f
        .call(
            &f.ann,
            "PATCH",
            &format!("{path}/files/{file}"),
            Some(json!({"as_link": true})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = f
        .call(
            &f.ann,
            "PATCH",
            &path,
            Some(json!({"version": 1, "subject": "Changed"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // Held again: removing the file is refused too.
    let (_, again) = f.send(&draft, json!({})).await;
    assert_eq!(again["status"], "held");
    let (status, body) = f
        .call(&f.ann, "DELETE", &format!("{path}/files/{file}"), None)
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    let (status, _) = f
        .call(
            &f.ann,
            "PATCH",
            &format!("{path}/files/{file}"),
            Some(json!({"as_link": false})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn malformed_tiers_fail_closed_to_new() {
    let f = Fixture::new().await;
    f.sql(&format!(
        "INSERT INTO mail_send_policies (workspace_id, tier, created_at, updated_at) \
         VALUES ('{}', 'standard', 1, 1)",
        f.workspace
    ))
    .await;
    let limits = || async { f.mail.limits_for(f.workspace).await.unwrap().unwrap() };
    f.sql("UPDATE instance_settings SET mail_send_tiers = '{\"new\": 1}'")
        .await;
    assert_eq!(limits().await.max_recipients, 50);
    f.sql("UPDATE instance_settings SET mail_send_tiers = '[]'")
        .await;
    assert_eq!(limits().await.per_user_hour, 30);
    f.sql(&format!(
        "UPDATE mail_send_policies SET tier = 'custom', custom_limits = '{{\"x\": 1}}' \
         WHERE workspace_id = '{}'",
        f.workspace
    ))
    .await;
    assert_eq!(limits().await.max_recipients, 50);
}

fn multipart(name: &str, bytes: &[u8]) -> (String, Vec<u8>) {
    let boundary = "orbit-test-boundary";
    let mut body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\n\
         Content-Type: application/octet-stream\r\n\r\n"
    )
    .into_bytes();
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

impl Fixture {
    async fn upload(
        &self,
        member: &Member,
        draft: &str,
        name: &str,
        bytes: &[u8],
    ) -> (StatusCode, Value) {
        self.upload_with(member, draft, name, bytes, "").await
    }

    /// An upload with a query string such as `?inline=true`.
    async fn upload_with(
        &self,
        member: &Member,
        draft: &str,
        name: &str,
        bytes: &[u8],
        query: &str,
    ) -> (StatusCode, Value) {
        let (content_type, body) = multipart(name, bytes);
        let request = Request::builder()
            .method("POST")
            .uri(format!(
                "/api/v1/workspaces/{}/mail/drafts/{draft}/files{query}",
                self.workspace
            ))
            .header(header::COOKIE, &member.cookie)
            .header(header::CONTENT_TYPE, content_type)
            .body(Body::from(body))
            .unwrap();
        let response = self.app().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn reply(&self, member: &Member, mode: &str, thread: Id) -> (StatusCode, Value) {
        self.call(
            member,
            "POST",
            "/drafts",
            Some(json!({"mode": mode, "thread_id": thread.to_string()})),
        )
        .await
    }
}

#[tokio::test]
async fn files_added_after_the_checks_are_sized_when_holding() {
    let f = Fixture::new().await;
    f.sql("UPDATE mail_send_routes SET settings = '{\"max_message_bytes\": 10000}'")
        .await;
    let draft = f.draft(&["x@out.test"]).await;
    let sent = f
        .mail
        .send_draft_between(
            f.ann.membership,
            draft.parse().unwrap(),
            None,
            at(TimestampMillis::now().as_millis()),
            async {
                // Lands after the read checks; the draft's version does not change.
                let (status, body) = f.upload(&f.ann, &draft, "big.bin", &[7u8; 20_000]).await;
                assert_eq!(status, StatusCode::CREATED, "{body}");
            },
        )
        .await;
    assert!(matches!(
        sent,
        Err(orbit_server::repositories::mail::MailError::MessageTooLarge { .. })
    ));
    let held: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM mail_outbound")
        .fetch_one(f.database.pool())
        .await
        .unwrap();
    assert_eq!(held, 0);
}

#[tokio::test]
async fn another_members_held_draft_is_not_found() {
    let f = Fixture::new().await;
    let draft = f.draft(&["x@out.test"]).await;
    f.send(&draft, json!({})).await;
    let (status, _) = f
        .call(&f.bob, "DELETE", &format!("/drafts/{draft}"), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = f
        .call(
            &f.bob,
            "PATCH",
            &format!("/drafts/{draft}"),
            Some(json!({"version": 1})),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = f
        .call(
            &f.bob,
            "PATCH",
            &format!("/drafts/{draft}/files/{}", Id::new_v7()),
            Some(json!({"as_link": true})),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = f
        .call(
            &f.bob,
            "DELETE",
            &format!("/drafts/{draft}/files/{}", Id::new_v7()),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---- Release (Task 12): build, sign, file into Sent and the thread ----

use orbit_mail::{Authenticator, ParseLimits, StaticDns};
use orbit_server::live::LiveHub;
use orbit_server::mail_dns::FakeDns;
use orbit_server::mail_send::{DeliveryContext, SendJobState, run_send_job};
use orbit_server::repositories::mail::{OutboundView, Released};

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

    async fn new_draft(&self, from: Id, to: &[&str], cc: &[&str], bcc: &[&str]) -> String {
        let (status, draft) = self
            .call(
                &self.ann,
                "POST",
                "/drafts",
                Some(json!({"mode": "new", "from_address_id": from.to_string(), "to": to,
                            "cc": cc, "bcc": bcc, "subject": "Hello",
                            "body_html": "<p onclick=\"x()\">Hi <img src=\"cid:logo@one.test\"></p>"})),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{draft}");
        draft["id"].as_str().unwrap().to_owned()
    }

    async fn thread_draft(&self, mode: &str, thread: Id) -> String {
        let (status, draft) = self
            .call(
                &self.ann,
                "POST",
                "/drafts",
                Some(json!({"mode": mode, "thread_id": thread.to_string(),
                            "body_html": "<p>Thanks</p>"})),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{draft}");
        draft["id"].as_str().unwrap().to_owned()
    }

    /// Sends Ann's draft and releases it at its time.
    async fn send_and_release(&self, draft: &str) -> (Id, Released, i64) {
        let (status, sent) = self.send(draft, json!({})).await;
        assert_eq!(status, StatusCode::OK, "{sent}");
        let id: Id = sent["id"].as_str().unwrap().parse().unwrap();
        let (_, release_at, _) = self.outbound_row(&id.to_string()).await;
        let released = self
            .mail
            .release(id, Some("mx.orbit.test"), at(release_at))
            .await
            .unwrap()
            .expect("released");
        (id, released, release_at)
    }

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

    /// Ann sends at once (no undo time), so the release happens now and mail delivered after it
    /// is newer.
    async fn no_undo(&self) {
        self.sql(&format!(
            "INSERT INTO mail_member_settings (membership_id, undo_send_seconds, updated_at) \
             VALUES ('{}', 0, 0)",
            self.ann.membership
        ))
        .await;
    }

    fn job_state(&self) -> SendJobState {
        SendJobState {
            mail: Arc::clone(&self.mail),
            delivery: DeliveryContext::new(
                Some("mx.orbit.test".to_owned()),
                Arc::new(FakeDns::new()),
                false,
            ),
            instance_delivery: DeliveryContext::new(
                Some("mx.orbit.test".to_owned()),
                Arc::new(FakeDns::new()),
                true,
            ),
            intake: self.intake.clone(),
            live: LiveHub::of(&self.database),
            send_timeout: std::time::Duration::from_secs(1200),
        }
    }
}

fn parsed(raw: &[u8]) -> orbit_mail::ParsedMessage {
    orbit_mail::parse(raw, &ParseLimits::default()).unwrap()
}

#[tokio::test]
async fn release_files_reply_in_thread_and_sent() {
    let f = Fixture::new().await;
    f.signing().await;
    let (_, thread) = f.deliver("ann@one.test", f.ann_primary).await;
    let (status, body) = f
        .call(
            &f.ann,
            "PATCH",
            &format!("/threads/{thread}"),
            Some(json!({"folder": "archive"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let draft = f.thread_draft("reply", thread).await;
    let (outbound, released, release_at) = f.send_and_release(&draft).await;
    assert_eq!(released.thread_id, thread);
    assert!(!released.new_thread);

    // Appended without moving the thread.
    let row = sqlx::query(
        "SELECT folder, message_count, last_sent_at, last_incoming_at FROM mail_threads WHERE id = $1",
    )
    .bind(thread.to_string())
    .fetch_one(f.database.pool())
    .await
    .unwrap();
    use sqlx::Row;
    assert_eq!(row.get::<String, _>("folder"), "archive");
    assert_eq!(row.get::<i64, _>("message_count"), 2);
    assert_eq!(row.get::<Option<i64>, _>("last_sent_at"), Some(release_at));
    assert!(row.get::<Option<i64>, _>("last_incoming_at").is_some());
    assert_eq!(
        f.number(
            "SELECT outgoing FROM mail_thread_messages WHERE message_id = $1",
            released.message_id
        )
        .await,
        Some(1)
    );
    // Filed like a received message, from source `sent`.
    let receipt = sqlx::query(
        "SELECT r.source, r.status, r.peer_ip, r.helo, r.auth, m.in_reply_to FROM mail_messages m \
         JOIN mail_receipts r ON r.id = m.receipt_id WHERE m.id = $1",
    )
    .bind(released.message_id.to_string())
    .fetch_one(f.database.pool())
    .await
    .unwrap();
    assert_eq!(receipt.get::<String, _>("source"), "sent");
    assert_eq!(receipt.get::<String, _>("status"), "ingested");
    assert_eq!(receipt.get::<String, _>("peer_ip"), "");
    assert_eq!(receipt.get::<String, _>("helo"), "mx.orbit.test");
    assert_eq!(receipt.get::<String, _>("auth"), "{}");
    assert_eq!(
        receipt.get::<Option<String>, _>("in_reply_to").as_deref(),
        Some("m1@sender.test")
    );
    // One queued recipient; the outbound row points at the message; the draft is gone.
    let recipients = sqlx::query(
        "SELECT address, kind, status FROM mail_outbound_recipients WHERE outbound_id = $1",
    )
    .bind(outbound.to_string())
    .fetch_all(f.database.pool())
    .await
    .unwrap();
    assert_eq!(recipients.len(), 1);
    assert_eq!(
        recipients[0].get::<String, _>("address"),
        "carl@sender.test"
    );
    assert_eq!(recipients[0].get::<String, _>("kind"), "to");
    assert_eq!(recipients[0].get::<String, _>("status"), "queued");
    let row = sqlx::query("SELECT status, message_id, draft_id FROM mail_outbound WHERE id = $1")
        .bind(outbound.to_string())
        .fetch_one(f.database.pool())
        .await
        .unwrap();
    assert_eq!(row.get::<String, _>("status"), "released");
    assert_eq!(
        row.get::<Option<String>, _>("message_id"),
        Some(released.message_id.to_string())
    );
    assert_eq!(row.get::<Option<String>, _>("draft_id"), None);
    let (status, _) = f
        .call(&f.ann, "GET", &format!("/drafts/{draft}"), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Sent and Archive both list it; the Outbox reads the sent message now.
    assert!(
        f.thread_ids(&f.ann, "sent=true")
            .await
            .contains(&thread.to_string())
    );
    assert!(
        f.thread_ids(&f.ann, "folder=archive")
            .await
            .contains(&thread.to_string())
    );
    let outbox = f
        .mail
        .list_outbound(f.ann.membership, OutboundView::Outbox, None)
        .await
        .unwrap();
    assert_eq!(outbox.len(), 1);
    assert_eq!(outbox[0].subject, "Re: Hello");
    assert_eq!(outbox[0].to, vec!["carl@sender.test".to_owned()]);
    // The quoted original is under the reply.
    let message = parsed(&f.raw_of(released.message_id).await);
    assert!(
        message.body_text.contains("wrote:"),
        "{}",
        message.body_text
    );
}

#[tokio::test]
async fn new_message_thread_in_archive_hidden_from_archive_list_and_in_sent() {
    let f = Fixture::new().await;
    f.signing().await;
    let draft = f.new_draft(f.team_primary, &["x@out.test"], &[], &[]).await;
    let (_, released, _) = f.send_and_release(&draft).await;
    assert!(released.new_thread);
    let thread = released.thread_id.to_string();
    assert_eq!(
        f.text("SELECT folder FROM mail_threads WHERE id = $1", &thread)
            .await
            .as_deref(),
        Some("archive")
    );
    assert_eq!(
        f.number(
            "SELECT last_incoming_at FROM mail_threads WHERE id = $1",
            &thread
        )
        .await,
        None
    );
    let team = f.team;
    for member in [&f.owner, &f.ann] {
        assert!(
            !f.thread_ids(member, &format!("mailbox_id={team}&folder=archive"))
                .await
                .contains(&thread)
        );
        assert!(
            !f.thread_ids(member, &format!("mailbox_id={team}"))
                .await
                .contains(&thread)
        );
        assert_eq!(
            f.thread_ids(member, &format!("mailbox_id={team}&sent=true"))
                .await,
            vec![thread.clone()]
        );
    }
    // The other member's unread count of Archive leaves it out too.
    let (status, counts) = f.call(&f.owner, "GET", "/counts", None).await;
    assert_eq!(status, StatusCode::OK, "{counts}");
    let team_counts = counts["mailboxes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|mailbox| mailbox["mailbox_id"] == f.team.to_string())
        .unwrap()
        .clone();
    assert_eq!(team_counts["archive"], 0);
    // Sent is a view, not a folder.
    let (status, _) = f
        .call(&f.ann, "GET", "/threads?sent=true&folder=inbox", None)
        .await;
    assert!(status.is_client_error());
}

#[tokio::test]
async fn release_signs_with_active_key() {
    let f = Fixture::new().await;
    let (selector, txt) = f.signing().await;
    let draft = f
        .new_draft(
            f.ann_primary,
            &["x@out.test"],
            &["y@out.test"],
            &["z@hidden.test"],
        )
        .await;
    let (outbound, released, _) = f.send_and_release(&draft).await;
    let raw = f.raw_of(released.message_id).await;
    let dns = StaticDns::new().txt(&format!("{selector}._domainkey.one.test"), &txt);
    let outcome = Authenticator::with_static_dns(dns)
        .unwrap()
        .check(
            &raw,
            IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)),
            "mx.orbit.test",
            Some("ann@one.test"),
        )
        .await;
    assert_eq!(outcome.dkim, Verdict::Pass);
    let message = parsed(&raw);
    let from = message.from.unwrap();
    assert_eq!(from.address, "ann@one.test");
    assert_eq!(from.name.as_deref(), Some("Ann Lee"));
    // Bcc only in the envelope; the HTML is sanitized again, inline `cid:` images kept.
    let text = String::from_utf8_lossy(&raw).into_owned();
    assert!(!text.contains("z@hidden.test"), "{text}");
    assert!(!text.to_ascii_lowercase().contains("bcc:"), "{text}");
    let html = String::from_utf8(orbit_mail::part_bytes(&raw, message.html_part.unwrap()).unwrap())
        .unwrap();
    assert!(!html.contains("onclick"), "{html}");
    assert!(html.contains("cid:logo@one.test"), "{html}");
    let kinds: Vec<(String, String)> = sqlx::query_as(
        "SELECT address, kind FROM mail_outbound_recipients WHERE outbound_id = $1 ORDER BY address",
    )
    .bind(outbound.to_string())
    .fetch_all(f.database.pool())
    .await
    .unwrap();
    assert_eq!(
        kinds,
        vec![
            ("x@out.test".to_owned(), "to".to_owned()),
            ("y@out.test".to_owned(), "cc".to_owned()),
            ("z@hidden.test".to_owned(), "bcc".to_owned()),
        ]
    );
}

#[tokio::test]
async fn release_without_active_key_stays_held() {
    let f = Fixture::new().await;
    let draft = f.new_draft(f.ann_primary, &["x@out.test"], &[], &[]).await;
    // Sent while the domain had its key (D8); the key is gone at release.
    let (status, sent) = f.send(&draft, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{sent}");
    f.sql("DELETE FROM mail_dkim_keys").await;
    let id: Id = sent["id"].as_str().unwrap().parse().unwrap();
    let (_, release_at, _) = f.outbound_row(&id.to_string()).await;
    let result = run_send_job(&f.job_state(), id, at(release_at)).await;
    assert!(
        matches!(result, Err(orbit_platform::JobError::Permanent(_))),
        "{result:?}"
    );
    let row =
        sqlx::query("SELECT status, hold_reason, message_id FROM mail_outbound WHERE id = $1")
            .bind(id.to_string())
            .fetch_one(f.database.pool())
            .await
            .unwrap();
    use sqlx::Row;
    assert_eq!(row.get::<String, _>("status"), "held");
    assert_eq!(
        row.get::<Option<String>, _>("hold_reason").as_deref(),
        Some("dkim_inactive")
    );
    assert_eq!(row.get::<Option<String>, _>("message_id"), None);
    let sent_receipts: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM mail_receipts WHERE source = 'sent'")
            .fetch_one(f.database.pool())
            .await
            .unwrap();
    assert_eq!(sent_receipts, 0, "Never sent unsigned");
    // The author's Outbox hears of the hold (the new message has no thread yet).
    let frames: Vec<String> = sqlx::query_scalar("SELECT frame FROM live_events")
        .fetch_all(f.database.pool())
        .await
        .unwrap();
    assert!(
        frames
            .iter()
            .any(|frame| frame.contains("outbound.updated") && frame.contains(&id.to_string())),
        "{frames:?}"
    );
    // Undo still brings the draft back.
    let (status, _) = f
        .call(&f.ann, "POST", &format!("/outbound/{id}/cancel"), None)
        .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn release_skips_cancelled() {
    let f = Fixture::new().await;
    f.signing().await;
    let draft = f.new_draft(f.ann_primary, &["x@out.test"], &[], &[]).await;
    let (_, sent) = f.send(&draft, json!({})).await;
    let id: Id = sent["id"].as_str().unwrap().parse().unwrap();
    let (_, first_release, _) = f.outbound_row(&id.to_string()).await;
    // Not due yet.
    assert!(
        f.mail
            .release(id, None, at(first_release - 1))
            .await
            .unwrap()
            .is_none()
    );
    let (status, _) = f
        .call(&f.ann, "POST", &format!("/outbound/{id}/cancel"), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        f.mail
            .release(id, None, at(first_release))
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(f.outbound_row(&id.to_string()).await.0, "cancelled");
    // Sent again later: the first job, at the first time, releases nothing.
    f.sql(&format!(
        "INSERT INTO mail_member_settings (membership_id, undo_send_seconds, updated_at) \
         VALUES ('{}', 30, 0)",
        f.ann.membership
    ))
    .await;
    let (status, _) = f.send(&draft, json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        f.mail
            .release(id, None, at(first_release))
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(f.outbound_row(&id.to_string()).await.0, "held");
    let (status, _) = f
        .call(&f.ann, "GET", &format!("/drafts/{draft}"), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    let sent_receipts: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM mail_receipts WHERE source = 'sent'")
            .fetch_one(f.database.pool())
            .await
            .unwrap();
    assert_eq!(sent_receipts, 0);
}

#[tokio::test]
async fn sent_message_read_for_sender() {
    let f = Fixture::new().await;
    f.signing().await;
    f.no_undo().await;
    let (_, thread) = f.deliver("team@one.test", f.team_primary).await;
    let draft = f.thread_draft("reply", thread).await;
    f.send_and_release(&draft).await;
    assert!(!f.unread(&f.ann, thread).await, "Read for the sender");
    assert!(
        f.unread(&f.owner, thread).await,
        "Unread for the other member"
    );

    // A message that arrives after the reply was written stays unread.
    let (_, second) = f
        .deliver_raw(
            "From: Carl <carl@sender.test>\r\nTo: team@one.test\r\nSubject: Re: Hello\r\n\
             Message-ID: <m2@sender.test>\r\nIn-Reply-To: <m1@sender.test>\r\n\r\nMore\r\n"
                .to_owned(),
            "team@one.test",
            f.team_primary,
        )
        .await;
    assert_eq!(second, thread);
    let draft = f.thread_draft("reply", thread).await;
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    f.deliver_raw(
        "From: Carl <carl@sender.test>\r\nTo: team@one.test\r\nSubject: Re: Hello\r\n\
         Message-ID: <m3@sender.test>\r\nIn-Reply-To: <m2@sender.test>\r\n\r\nAnd more\r\n"
            .to_owned(),
        "team@one.test",
        f.team_primary,
    )
    .await;
    f.send_and_release(&draft).await;
    assert!(
        f.unread(&f.ann, thread).await,
        "The newer message is unread"
    );
}

#[tokio::test]
async fn reply_to_self_dedupes() {
    let f = Fixture::new().await;
    f.signing().await;
    let draft = f
        .new_draft(f.ann_primary, &["ann@one.test"], &[], &[])
        .await;
    let (_, released, _) = f.send_and_release(&draft).await;
    let raw = f.raw_of(released.message_id).await;
    // The copy delivered to the own mailbox has the same bytes: the mailbox keeps one.
    let ids = f
        .intake
        .accept(InboundMessage {
            source: Source::Smtp,
            trace_len: 0,
            raw,
            mail_from: Some("ann@one.test".to_owned()),
            recipients: vec![Recipient::Mailbox {
                workspace_id: f.workspace,
                address_id: f.ann_primary,
                rcpt: "ann@one.test".to_owned(),
            }],
            peer_ip: IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            helo: "mx.orbit.test".to_owned(),
            tls: None,
            auth: AuthOutcome {
                spf: Verdict::Pass,
                dkim: Verdict::Pass,
                dmarc: DmarcVerdict::Pass,
                aligned_domain: Some("one.test".to_owned()),
                temp_error: false,
                spf_domain: Some("one.test".to_owned()),
                dkim_domain: Some("one.test".to_owned()),
                from_domain: Some("one.test".to_owned()),
            },
        })
        .await
        .unwrap();
    let IngestOutcome::Ingested { deliveries, .. } =
        f.mail.ingest(ids[0], TimestampMillis::now()).await.unwrap()
    else {
        panic!("not ingested");
    };
    assert!(deliveries.is_empty(), "{deliveries:?}");
    assert_eq!(
        f.number(
            "SELECT message_count FROM mail_threads WHERE id = $1",
            released.thread_id
        )
        .await,
        Some(1)
    );
}

#[tokio::test]
async fn references_trimmed_on_long_chain() {
    let f = Fixture::new().await;
    f.signing().await;
    let refs: Vec<String> = (1..=40).map(|n| format!("<r{n}@sender.test>")).collect();
    let (_, thread) = f
        .deliver_raw(
            format!(
                "From: Carl <carl@sender.test>\r\nTo: ann@one.test\r\nSubject: Hello\r\n\
                 Message-ID: <p@sender.test>\r\nReferences: {}\r\n\r\nHi\r\n",
                refs.join(" ")
            ),
            "ann@one.test",
            f.ann_primary,
        )
        .await;
    let draft = f.thread_draft("reply", thread).await;
    let (_, released, _) = f.send_and_release(&draft).await;
    let message = parsed(&f.raw_of(released.message_id).await);
    assert_eq!(message.in_reply_to, vec!["p@sender.test".to_owned()]);
    assert_eq!(message.references.len(), 20);
    assert_eq!(message.references[0], "r1@sender.test");
    assert_eq!(message.references[1], "r23@sender.test");
    assert_eq!(message.references[18], "r40@sender.test");
    assert_eq!(message.references[19], "p@sender.test");
    // The answer to the reply still finds the thread.
    let sent_id = message.message_id.unwrap();
    let mut chain = message
        .references
        .iter()
        .map(|id| format!("<{id}>"))
        .collect::<Vec<_>>();
    chain.push(format!("<{sent_id}>"));
    let (_, answer) = f
        .deliver_raw(
            format!(
                "From: Carl <carl@sender.test>\r\nTo: ann@one.test\r\nSubject: Re: Hello\r\n\
                 Message-ID: <a@sender.test>\r\nIn-Reply-To: <{sent_id}>\r\nReferences: {}\r\n\r\nOk\r\n",
                chain.join(" ")
            ),
            "ann@one.test",
            f.ann_primary,
        )
        .await;
    assert_eq!(answer, thread);
}

#[tokio::test]
async fn incoming_reply_moves_thread_to_inbox() {
    let f = Fixture::new().await;
    f.signing().await;
    f.no_undo().await;
    let draft = f
        .new_draft(f.ann_primary, &["carl@sender.test"], &[], &[])
        .await;
    let (_, released, _) = f.send_and_release(&draft).await;
    let sent_id = parsed(&f.raw_of(released.message_id).await)
        .message_id
        .unwrap();
    let (message, thread) = f
        .deliver_raw(
            format!(
                "From: Carl <carl@sender.test>\r\nTo: ann@one.test\r\nSubject: Re: Hello\r\n\
                 Message-ID: <answer@sender.test>\r\nIn-Reply-To: <{sent_id}>\r\n\r\nSure\r\n"
            ),
            "ann@one.test",
            f.ann_primary,
        )
        .await;
    assert_eq!(thread, released.thread_id);
    let row = sqlx::query(
        "SELECT folder, message_count, last_incoming_at, last_sent_at FROM mail_threads WHERE id = $1",
    )
    .bind(thread.to_string())
    .fetch_one(f.database.pool())
    .await
    .unwrap();
    use sqlx::Row;
    assert_eq!(row.get::<String, _>("folder"), "inbox");
    assert_eq!(row.get::<i64, _>("message_count"), 2);
    assert!(row.get::<Option<i64>, _>("last_incoming_at").is_some());
    assert!(row.get::<Option<i64>, _>("last_sent_at").is_some());
    // The person written to is not an outsider in this thread.
    assert_eq!(
        f.number(
            "SELECT external FROM mail_thread_messages WHERE message_id = $1",
            message
        )
        .await,
        Some(0)
    );
    assert!(f.unread(&f.ann, thread).await);
    assert!(f.thread_ids(&f.ann, "").await.contains(&thread.to_string()));
}

#[tokio::test]
async fn forwards_carry_the_original() {
    let f = Fixture::new().await;
    f.signing().await;
    let raw = "From: Carl <carl@sender.test>\r\nTo: ann@one.test\r\nSubject: Report\r\n\
               Message-ID: <rep@sender.test>\r\nMIME-Version: 1.0\r\n\
               Content-Type: multipart/mixed; boundary=\"b\"\r\n\r\n\
               --b\r\nContent-Type: text/html\r\n\r\n<p>See <b>attached</b></p>\r\n\
               --b\r\nContent-Type: application/pdf\r\nContent-Disposition: attachment; filename=\"r.pdf\"\r\n\
               Content-Transfer-Encoding: base64\r\n\r\nJVBERi0xLjQK\r\n--b--\r\n";
    let (_, thread) = f
        .deliver_raw(raw.to_owned(), "ann@one.test", f.ann_primary)
        .await;
    let draft = f.thread_draft("forward", thread).await;
    let (status, body) = f
        .call(
            &f.ann,
            "PATCH",
            &format!("/drafts/{draft}"),
            Some(json!({"version": 1, "to": ["x@out.test"]})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = f.upload(&f.ann, &draft, "notes.txt", b"my notes").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (_, released, _) = f.send_and_release(&draft).await;
    let raw = f.raw_of(released.message_id).await;
    let message = parsed(&raw);
    assert!(
        message.body_text.contains("Forwarded message")
            && message.body_text.contains("See attached"),
        "{}",
        message.body_text
    );
    let names: Vec<Option<String>> = message
        .attachments
        .iter()
        .map(|attachment| attachment.filename.clone())
        .collect();
    assert!(names.contains(&Some("r.pdf".to_owned())), "{names:?}");
    assert!(names.contains(&Some("notes.txt".to_owned())), "{names:?}");
    let pdf = message
        .attachments
        .iter()
        .find(|attachment| attachment.filename.as_deref() == Some("r.pdf"))
        .unwrap();
    assert_eq!(
        orbit_mail::part_bytes(&raw, pdf.part_id).unwrap(),
        b"%PDF-1.4\n".to_vec()
    );

    // Forward as attachment: the original as message/rfc822.
    let (_, other) = f.deliver("ann@one.test", f.ann_primary).await;
    let draft = f.thread_draft("forward_attached", other).await;
    f.call(
        &f.ann,
        "PATCH",
        &format!("/drafts/{draft}"),
        Some(json!({"version": 1, "to": ["x@out.test"]})),
    )
    .await;
    let (_, released, _) = f.send_and_release(&draft).await;
    let raw = f.raw_of(released.message_id).await;
    let message = parsed(&raw);
    assert!(
        message
            .attachments
            .iter()
            .any(|attachment| attachment.declared_type == "message/rfc822"),
        "{:?}",
        message.attachments
    );
}

#[tokio::test]
async fn send_job_releases_due_message() {
    let f = Fixture::new().await;
    f.signing().await;
    let draft = f.new_draft(f.ann_primary, &["x@out.test"], &[], &[]).await;
    let (_, sent) = f.send(&draft, json!({})).await;
    let id: Id = sent["id"].as_str().unwrap().parse().unwrap();
    let (_, release_at, _) = f.outbound_row(&id.to_string()).await;
    let state = f.job_state();
    run_send_job(&state, id, at(release_at - 1)).await.unwrap();
    assert_eq!(f.outbound_row(&id.to_string()).await.0, "held");
    run_send_job(&state, id, at(release_at)).await.unwrap();
    assert_eq!(f.outbound_row(&id.to_string()).await.0, "released");
    // A second run (an old job) does nothing.
    run_send_job(&state, id, at(release_at + 1)).await.unwrap();
    let sent_receipts: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM mail_receipts WHERE source = 'sent'")
            .fetch_one(f.database.pool())
            .await
            .unwrap();
    assert_eq!(sent_receipts, 1);
}

// ---- Release fix round 1 ----

impl Fixture {
    async fn outbound_reason(&self, id: Id) -> (String, Option<String>) {
        let row = sqlx::query("SELECT status, hold_reason FROM mail_outbound WHERE id = $1")
            .bind(id.to_string())
            .fetch_one(self.database.pool())
            .await
            .unwrap();
        use sqlx::Row;
        (row.get("status"), row.get("hold_reason"))
    }

    async fn queued_send_jobs(&self, id: Id) -> i64 {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM jobs WHERE kind = 'mail.send' AND state = 'queued' \
             AND payload_json = $1",
        )
        .bind(json!({"outbound_id": id.to_string()}).to_string())
        .fetch_one(self.database.pool())
        .await
        .unwrap()
    }

    async fn external_of(&self, message: Id) -> Option<i64> {
        self.number(
            "SELECT external FROM mail_thread_messages WHERE message_id = $1",
            message,
        )
        .await
    }
}

#[tokio::test]
async fn external_rule_in_sent_threads() {
    let f = Fixture::new().await;
    f.signing().await;
    f.no_undo().await;
    let draft = f
        .new_draft(f.ann_primary, &["carl@sender.test"], &[], &[])
        .await;
    let (_, released, _) = f.send_and_release(&draft).await;
    let sent_id = parsed(&f.raw_of(released.message_id).await)
        .message_id
        .unwrap();
    let reply = |from: &str, id: &str| {
        format!(
            "From: {from}\r\nTo: ann@one.test\r\nSubject: Re: Hello\r\n\
             Message-ID: <{id}@other.test>\r\nIn-Reply-To: <{sent_id}>\r\n\r\nHi\r\n"
        )
    };
    // An outsider Ann never wrote to: external, also the second time.
    for id in ["e1", "e2"] {
        let (message, thread) = f
            .deliver_raw(reply("eve@evil.test", id), "ann@one.test", f.ann_primary)
            .await;
        assert_eq!(thread, released.thread_id);
        assert_eq!(f.external_of(message).await, Some(1), "{id}");
    }
    // The person Ann wrote to is not.
    let (message, thread) = f
        .deliver_raw(
            reply("Carl <carl@sender.test>", "c1"),
            "ann@one.test",
            f.ann_primary,
        )
        .await;
    assert_eq!(thread, released.thread_id);
    assert_eq!(f.external_of(message).await, Some(0));
}

#[tokio::test]
async fn sweep_requeues_failed_release() {
    let f = Fixture::new().await;
    f.signing().await;
    let draft = f.new_draft(f.ann_primary, &["x@out.test"], &[], &[]).await;
    let (_, sent) = f.send(&draft, json!({})).await;
    let id: Id = sent["id"].as_str().unwrap().parse().unwrap();
    let (_, release_at, _) = f.outbound_row(&id.to_string()).await;
    // The job gave up: no job left, the row held with `release_failed`.
    f.mail
        .hold_release(id, "release_failed", at(release_at))
        .await
        .unwrap();
    f.sql("DELETE FROM jobs WHERE kind = 'mail.send'").await;
    // Not before its time.
    assert_eq!(f.mail.sweep_releases(at(release_at - 1)).await.unwrap(), 0);
    assert_eq!(f.mail.sweep_releases(at(release_at)).await.unwrap(), 1);
    assert_eq!(f.queued_send_jobs(id).await, 1);
    // Its job is queued: the next sweep leaves it alone.
    assert_eq!(f.mail.sweep_releases(at(release_at + 1)).await.unwrap(), 0);
    assert_eq!(f.queued_send_jobs(id).await, 1);
    run_send_job(&f.job_state(), id, at(release_at + 1))
        .await
        .unwrap();
    assert_eq!(f.outbound_reason(id).await, ("released".to_owned(), None));
}

#[tokio::test]
async fn dkim_inactive_released_after_key_returns() {
    let f = Fixture::new().await;
    let draft = f.new_draft(f.ann_primary, &["x@out.test"], &[], &[]).await;
    let (_, sent) = f.send(&draft, json!({})).await;
    let id: Id = sent["id"].as_str().unwrap().parse().unwrap();
    let (_, release_at, _) = f.outbound_row(&id.to_string()).await;
    f.sql("DELETE FROM mail_dkim_keys").await;
    assert!(
        run_send_job(&f.job_state(), id, at(release_at))
            .await
            .is_err()
    );
    assert_eq!(
        f.outbound_reason(id).await,
        ("held".to_owned(), Some("dkim_inactive".to_owned()))
    );
    f.sql("DELETE FROM jobs WHERE kind = 'mail.send'").await;
    // No key yet: nothing to queue.
    assert_eq!(f.mail.sweep_releases(at(release_at + 1)).await.unwrap(), 0);
    f.signing().await;
    assert_eq!(f.mail.sweep_releases(at(release_at + 2)).await.unwrap(), 1);
    run_send_job(&f.job_state(), id, at(release_at + 3))
        .await
        .unwrap();
    assert_eq!(f.outbound_reason(id).await, ("released".to_owned(), None));
}

#[tokio::test]
async fn missing_file_blob_holds_at_once() {
    let f = Fixture::new().await;
    f.signing().await;
    let draft = f.new_draft(f.ann_primary, &["x@out.test"], &[], &[]).await;
    let (status, body) = f.upload(&f.ann, &draft, "notes.txt", b"my notes").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let key: String = sqlx::query_scalar(
        "SELECT b.storage_key FROM mail_draft_files f \
         JOIN attachment_blobs b ON b.id = f.blob_id WHERE f.draft_id = $1",
    )
    .bind(&draft)
    .fetch_one(f.database.pool())
    .await
    .unwrap();
    std::fs::remove_file(f._root.path().join("attachments").join(&key)).unwrap();
    let (_, sent) = f.send(&draft, json!({})).await;
    let id: Id = sent["id"].as_str().unwrap().parse().unwrap();
    let (_, release_at, _) = f.outbound_row(&id.to_string()).await;
    let result = run_send_job(&f.job_state(), id, at(release_at)).await;
    assert!(
        matches!(result, Err(orbit_platform::JobError::Permanent(_))),
        "{result:?}"
    );
    assert_eq!(
        f.outbound_reason(id).await,
        ("held".to_owned(), Some("file_missing".to_owned()))
    );
}

#[tokio::test]
async fn bcc_only_uses_undisclosed_recipients() {
    let f = Fixture::new().await;
    f.signing().await;
    let draft = f
        .new_draft(f.ann_primary, &[], &[], &["z@hidden.test"])
        .await;
    let (outbound, released, _) = f.send_and_release(&draft).await;
    let raw = f.raw_of(released.message_id).await;
    let text = String::from_utf8_lossy(&raw).into_owned();
    assert!(text.contains("To: undisclosed-recipients:;\r\n"), "{text}");
    assert!(!text.contains("z@hidden.test"), "{text}");
    assert!(!text.contains("To: ann@one.test"), "{text}");
    assert!(parsed(&raw).to.is_empty());
    let recipients: Vec<String> =
        sqlx::query_scalar("SELECT address FROM mail_outbound_recipients WHERE outbound_id = $1")
            .bind(outbound.to_string())
            .fetch_all(f.database.pool())
            .await
            .unwrap();
    assert_eq!(recipients, vec!["z@hidden.test".to_owned()]);
}

#[tokio::test]
async fn references_from_in_reply_to_when_parent_has_none() {
    let f = Fixture::new().await;
    f.signing().await;
    let (_, thread) = f
        .deliver_raw(
            "From: Carl <carl@sender.test>\r\nTo: ann@one.test\r\nSubject: Re: Plans\r\n\
             Message-ID: <m1@sender.test>\r\nIn-Reply-To: <orig@one.test>\r\n\r\nHi\r\n"
                .to_owned(),
            "ann@one.test",
            f.ann_primary,
        )
        .await;
    let draft = f.thread_draft("reply", thread).await;
    let (_, released, _) = f.send_and_release(&draft).await;
    let message = parsed(&f.raw_of(released.message_id).await);
    assert_eq!(message.in_reply_to, vec!["m1@sender.test".to_owned()]);
    assert_eq!(
        message.references,
        vec!["orig@one.test".to_owned(), "m1@sender.test".to_owned()]
    );
}

#[tokio::test]
async fn undo_during_build_releases_nothing() {
    let f = Fixture::new().await;
    f.signing().await;
    let draft = f.new_draft(f.ann_primary, &["x@out.test"], &[], &[]).await;
    let (_, sent) = f.send(&draft, json!({})).await;
    let id: Id = sent["id"].as_str().unwrap().parse().unwrap();
    let (_, release_at, _) = f.outbound_row(&id.to_string()).await;
    let released = f
        .mail
        .release_between(id, None, at(release_at), async {
            // Undo, then Send again at once, while the message was being built.
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            let (status, back) = f
                .call(&f.ann, "POST", &format!("/outbound/{id}/cancel"), None)
                .await;
            assert_eq!(status, StatusCode::OK, "{back}");
            f.no_undo().await;
            let (status, again) = f.send(&draft, json!({})).await;
            assert_eq!(status, StatusCode::OK, "{again}");
        })
        .await
        .unwrap();
    assert!(released.is_none());
    assert_eq!(f.outbound_reason(id).await, ("held".to_owned(), None));
    let (status, _) = f
        .call(&f.ann, "GET", &format!("/drafts/{draft}"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "The draft is back");
    let sent_receipts: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM mail_receipts WHERE source = 'sent'")
            .fetch_one(f.database.pool())
            .await
            .unwrap();
    assert_eq!(sent_receipts, 0);
    // The new send releases (its own job would).
    assert!(
        f.mail
            .release(id, None, at(release_at))
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn permanent_release_errors_are_not_swept() {
    use orbit_server::mail_send::release_failure;
    use orbit_server::repositories::mail::MailError;
    // Only a passing store error is retried; an error without a code is `release_error`.
    assert_eq!(
        release_failure(&MailError::Unavailable(sqlx::Error::PoolTimedOut)),
        ("release_failed", true)
    );
    assert_eq!(
        release_failure(&MailError::Conflict("already_filed")),
        ("release_error", false)
    );
    assert_eq!(
        release_failure(&MailError::NotFound),
        ("release_error", false)
    );
    let f = Fixture::new().await;
    let draft = f.new_draft(f.ann_primary, &["x@out.test"], &[], &[]).await;
    let (_, sent) = f.send(&draft, json!({})).await;
    let id: Id = sent["id"].as_str().unwrap().parse().unwrap();
    let (_, release_at, _) = f.outbound_row(&id.to_string()).await;
    f.sql("DELETE FROM jobs WHERE kind = 'mail.send'").await;
    for reason in [
        "release_error",
        "build_failed",
        "file_missing",
        "send_as_denied",
        "storage_quota",
        "decrypt_failed",
    ] {
        f.mail
            .hold_release(id, reason, at(release_at))
            .await
            .unwrap();
        assert_eq!(
            f.mail.sweep_releases(at(release_at + 1)).await.unwrap(),
            0,
            "{reason}"
        );
    }
    assert_eq!(f.queued_send_jobs(id).await, 0);
}

#[tokio::test]
async fn cloud_workspace_is_new_after_its_first_release() {
    let f = Fixture::build(UploadLimits::default(), true).await;
    f.signing().await;
    let none: Option<String> =
        sqlx::query_scalar("SELECT tier FROM mail_send_policies WHERE workspace_id = $1")
            .bind(f.workspace.to_string())
            .fetch_optional(f.database.pool())
            .await
            .unwrap();
    assert_eq!(none, None);
    let draft = f.new_draft(f.ann_primary, &["x@out.test"], &[], &[]).await;
    f.send_and_release(&draft).await;
    let row = sqlx::query(
        "SELECT tier, first_release_at FROM mail_send_policies WHERE workspace_id = $1",
    )
    .bind(f.workspace.to_string())
    .fetch_one(f.database.pool())
    .await
    .unwrap();
    use sqlx::Row;
    assert_eq!(row.get::<String, _>("tier"), "new");
    assert!(row.get::<Option<i64>, _>("first_release_at").is_some());
}

impl Fixture {
    async fn outbound_with(&self, status: &str, at: i64) -> Id {
        let id = Id::new_v7();
        self.sql(&format!(
            "INSERT INTO mail_outbound (id, workspace_id, membership_id, mailbox_id, status, release_at, \
             created_at, updated_at) VALUES ('{id}', '{}', '{}', '{}', '{status}', {at}, {at}, {at})",
            self.workspace, self.ann.membership, self.ann_personal
        ))
        .await;
        id
    }

    async fn recipient_with(&self, outbound: Id, status: &str, released: i64) {
        self.sql(&format!(
            "INSERT INTO mail_outbound_recipients (id, outbound_id, workspace_id, membership_id, address, \
             kind, status, last_response, released_at, created_at, updated_at) VALUES ('{}', '{outbound}', \
             '{}', '{}', 'r@out.test', 'to', '{status}', '250 ok', {released}, {released}, {released})",
            Id::new_v7(),
            self.workspace,
            self.ann.membership
        ))
        .await;
    }

    async fn link_with(
        &self,
        outbound: Id,
        bytes: &[u8],
        expires: i64,
        revoked: Option<i64>,
    ) -> String {
        let finalization = self.uploads.begin_finalization().await;
        let prepared = finalization
            .prepare_bytes(self.workspace, bytes)
            .await
            .unwrap();
        let mut tx = self.database.immediate_transaction().await.unwrap();
        let blob = finalization
            .finalize_bytes_in_transaction(&mut tx, &prepared)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let id = Id::new_v7();
        let revoked = revoked.map_or("NULL".to_owned(), |at| at.to_string());
        self.sql(&format!(
            "INSERT INTO mail_file_links (id, workspace_id, outbound_id, blob_id, filename, size, \
             token_sha256, expires_at, revoked_at, downloads, created_at) VALUES ('{id}', '{}', \
             '{outbound}', '{}', 'a.txt', 5, '{}', {expires}, {revoked}, 0, 1)",
            self.workspace,
            blob.id,
            format!("{:0>64}", id.to_string().replace('-', ""))
        ))
        .await;
        id.to_string()
    }

    async fn total(&self, sql: &str) -> i64 {
        sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
            .fetch_one(self.database.pool())
            .await
            .unwrap()
    }
}

#[tokio::test]
async fn purge_send_rows() {
    let f = Fixture::new().await;
    let now = TimestampMillis::now().as_millis();
    let day = 86_400_000;

    // Cancelled messages: one over a day old goes, a fresh one stays; so do held and released ones.
    let old_cancelled = f.outbound_with("cancelled", now - 2 * day).await;
    f.recipient_with(old_cancelled, "cancelled", now - 2 * day)
        .await;
    let new_cancelled = f.outbound_with("cancelled", now - 3_600_000).await;
    let held = f.outbound_with("held", now - 10 * day).await;
    let released = f.outbound_with("released", now - 10 * day).await;
    f.recipient_with(released, "deferred", now - 100 * day)
        .await;

    // File links: expired and long-revoked go (with their blobs); live and just-revoked stay.
    let sent = f.outbound_with("done", now - 10 * day).await;
    f.recipient_with(sent, "delivered", now - 10 * day).await;
    f.link_with(sent, b"expired", now - 1_000, None).await;
    f.link_with(sent, b"revoked long ago", now + day, Some(now - 2 * day))
        .await;
    let live = f.link_with(sent, b"live", now + day, None).await;
    let revoked_now = f
        .link_with(sent, b"revoked now", now + day, Some(now - 1_000))
        .await;

    // Vacation replies.
    f.sql(&format!(
        "INSERT INTO mail_vacation_replies (mailbox_id, sender, replied_at) VALUES \
         ('{0}', 'old@out.test', {1}), ('{0}', 'new@out.test', {2})",
        f.ann_personal,
        now - 8 * day,
        now - 6 * day
    ))
    .await;

    // Recipients over 90 days: summarized and removed; a recent one and an open one stay.
    let ancient = f.outbound_with("done", now - 120 * day).await;
    f.recipient_with(ancient, "delivered", now - 100 * day)
        .await;
    f.sql(&format!(
        "INSERT INTO mail_outbound_recipients (id, outbound_id, workspace_id, membership_id, address, \
         kind, status, last_response, bounce_kind, released_at, created_at, updated_at) VALUES ('{}', \
         '{ancient}', '{}', '{}', 'bad@out.test', 'cc', 'bounced', '550 no such user', 'hard', {3}, {3}, {3})",
        Id::new_v7(),
        f.workspace,
        f.ann.membership,
        now - 100 * day
    ))
    .await;

    f.mail.purge(at(now)).await.unwrap();

    assert_eq!(
        f.total(&format!(
            "SELECT COUNT(*) FROM mail_outbound WHERE id = '{old_cancelled}'"
        ))
        .await,
        0
    );
    assert_eq!(
        f.total(&format!(
            "SELECT COUNT(*) FROM mail_outbound_recipients WHERE outbound_id = '{old_cancelled}'"
        ))
        .await,
        0
    );
    for kept in [new_cancelled, held, released, sent] {
        assert_eq!(
            f.total(&format!(
                "SELECT COUNT(*) FROM mail_outbound WHERE id = '{kept}'"
            ))
            .await,
            1,
            "{kept}"
        );
    }
    assert_eq!(
        f.total(&format!(
            "SELECT COUNT(*) FROM mail_outbound_recipients WHERE outbound_id = '{released}'"
        ))
        .await,
        1
    );
    let links: Vec<String> = sqlx::query_scalar("SELECT id FROM mail_file_links ORDER BY id")
        .fetch_all(f.database.pool())
        .await
        .unwrap();
    let mut expected = vec![live, revoked_now];
    expected.sort();
    assert_eq!(links, expected);
    // the blobs of the removed links go on the next reconcile
    let far = now + 40 * day;
    f.uploads.reconcile(far).await.unwrap();
    assert_eq!(
        f.total("SELECT COUNT(*) FROM attachment_blobs WHERE deleted_at IS NULL")
            .await,
        2
    );
    assert_eq!(
        f.total("SELECT COUNT(*) FROM mail_vacation_replies").await,
        1
    );
    assert_eq!(
        f.total("SELECT COUNT(*) FROM mail_vacation_replies WHERE sender = 'new@out.test'")
            .await,
        1
    );

    // 90 days: the rows are gone, the summary keeps each recipient's final status.
    assert_eq!(
        f.total(&format!(
            "SELECT COUNT(*) FROM mail_outbound_recipients WHERE outbound_id = '{ancient}'"
        ))
        .await,
        0
    );
    assert_eq!(
        f.total(&format!(
            "SELECT COUNT(*) FROM mail_outbound_recipients WHERE outbound_id = '{sent}'"
        ))
        .await,
        1
    );
    let summary: String =
        sqlx::query_scalar("SELECT delivery_summary FROM mail_outbound WHERE id = $1")
            .bind(ancient.to_string())
            .fetch_one(f.database.pool())
            .await
            .unwrap();
    let summary: Value = serde_json::from_str(&summary).unwrap();
    let rows = summary.as_array().unwrap();
    assert_eq!(rows.len(), 2);
    let bad = rows
        .iter()
        .find(|row| row["address"] == "bad@out.test")
        .unwrap();
    assert_eq!(bad["status"], "bounced");
    assert_eq!(bad["bounce_kind"], "hard");
    assert_eq!(bad["kind"], "cc");
    // the status view reads the summary once the rows are gone
    let statuses = f.mail.outbound_recipients(ancient, at(now)).await.unwrap();
    assert_eq!(statuses.len(), 2);
    assert!(
        statuses
            .iter()
            .any(|status| status.address == "bad@out.test"
                && status.status == "bounced"
                && status.last_response.as_deref() == Some("550 no such user"))
    );
}

// ---- Delivery status in the thread view, the Outbox filter and Sent recipients (Task G2) ----

#[tokio::test]
async fn message_view_has_delivery_for_outgoing_only() {
    let f = Fixture::new().await;
    f.signing().await;
    let (_, thread) = f.deliver("ann@one.test", f.ann_primary).await;
    let draft = f.thread_draft("reply", thread).await;
    let (outbound, _, _) = f.send_and_release(&draft).await;
    let (status, detail) = f
        .call(&f.ann, "GET", &format!("/threads/{thread}"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    let messages = detail["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 2, "{detail}");
    assert!(messages[0]["delivery"].is_null(), "{detail}");
    let delivery = &messages[1]["delivery"];
    assert_eq!(delivery["outbound_id"], outbound.to_string(), "{detail}");
    let recipients = delivery["recipients"].as_array().unwrap();
    assert_eq!(recipients.len(), 1, "{detail}");
    assert_eq!(recipients[0]["address"], "carl@sender.test");
    assert_eq!(recipients[0]["kind"], "to");
    assert_eq!(recipients[0]["status"], "queued");
    assert_eq!(recipients[0]["retryable"], false);

    // After the rows are purged the summary answers.
    f.sql(&format!(
        "UPDATE mail_outbound SET delivery_summary = '[{{\"address\":\"carl@sender.test\",\
         \"kind\":\"to\",\"status\":\"delivered\"}}]' WHERE id = '{outbound}'"
    ))
    .await;
    f.sql(&format!(
        "DELETE FROM mail_outbound_recipients WHERE outbound_id = '{outbound}'"
    ))
    .await;
    let (_, detail) = f
        .call(&f.ann, "GET", &format!("/threads/{thread}"), None)
        .await;
    let recipients = &detail["messages"][1]["delivery"]["recipients"];
    assert_eq!(recipients[0]["status"], "delivered", "{detail}");
    assert_eq!(recipients[0]["retryable"], false);

    // Not a member of the mailbox: the thread is not found, as before.
    let (status, _) = f
        .call(&f.owner, "GET", &format!("/threads/{thread}"), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn outbound_list_filters_by_mailbox() {
    let f = Fixture::new().await;
    let personal = f.new_draft(f.ann_primary, &["x@out.test"], &[], &[]).await;
    let team = f.new_draft(f.team_primary, &["y@out.test"], &[], &[]).await;
    for draft in [&personal, &team] {
        let (status, sent) = f.send(draft, json!({})).await;
        assert_eq!(status, StatusCode::OK, "{sent}");
    }
    let (_, all) = f.call(&f.ann, "GET", "/outbound?view=outbox", None).await;
    assert_eq!(all.as_array().unwrap().len(), 2, "{all}");
    let (_, only) = f
        .call(
            &f.ann,
            "GET",
            &format!("/outbound?view=outbox&mailbox_id={}", f.team),
            None,
        )
        .await;
    let only = only.as_array().unwrap();
    assert_eq!(only.len(), 1, "{only:?}");
    assert_eq!(only[0]["mailbox_id"], f.team.to_string());
    assert_eq!(only[0]["to"], json!(["y@out.test"]));
    // A mailbox the member is not in lists nothing.
    let (_, none) = f
        .call(
            &f.owner,
            "GET",
            &format!("/outbound?view=outbox&mailbox_id={}", f.ann_personal),
            None,
        )
        .await;
    assert_eq!(none, json!([]));
}

#[tokio::test]
async fn sent_summary_has_recipients() {
    let f = Fixture::new().await;
    f.signing().await;
    let draft = f
        .new_draft(
            f.team_primary,
            &["a@out.test", "b@out.test", "c@out.test"],
            &["d@out.test", "e@out.test", "f@out.test"],
            &[],
        )
        .await;
    f.send_and_release(&draft).await;
    let (status, page) = f
        .call(
            &f.ann,
            "GET",
            &format!("/threads?mailbox_id={}&sent=true", f.team),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let item = &page["items"][0];
    assert_eq!(
        item["sent_to"],
        json!([
            "a@out.test",
            "b@out.test",
            "c@out.test",
            "d@out.test",
            "e@out.test"
        ]),
        "{page}"
    );
    assert_eq!(item["sent_to_more"], 1);
}

// ---- Held drafts are not drafts; own outbound rows survive leaving the mailbox (G2 fix) ----

impl Fixture {
    async fn draft_ids(&self) -> Vec<String> {
        let (status, list) = self.call(&self.ann, "GET", "/drafts", None).await;
        assert_eq!(status, StatusCode::OK, "{list}");
        list.as_array()
            .unwrap()
            .iter()
            .map(|draft| draft["id"].as_str().unwrap().to_owned())
            .collect()
    }
}

#[tokio::test]
async fn held_drafts_are_not_listed() {
    let f = Fixture::new().await;
    let draft = f.new_draft(f.ann_primary, &["x@out.test"], &[], &[]).await;
    assert_eq!(f.draft_ids().await, vec![draft.clone()]);
    // In the undo window: not a draft.
    let (status, sent) = f.send(&draft, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{sent}");
    assert!(f.draft_ids().await.is_empty());
    // Cancelled: back.
    let outbound = sent["id"].as_str().unwrap();
    let (status, body) = f
        .call(
            &f.ann,
            "POST",
            &format!("/outbound/{outbound}/cancel"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(f.draft_ids().await, vec![draft.clone()]);
    // Scheduled: not a draft either.
    let when = TimestampMillis::now().as_millis() + 3_600_000;
    let (status, sent) = f.send(&draft, json!({"scheduled_at": at(when)})).await;
    assert_eq!(status, StatusCode::OK, "{sent}");
    assert!(f.draft_ids().await.is_empty());
}

#[tokio::test]
async fn own_outbound_stays_after_leaving_the_mailbox() {
    let f = Fixture::new().await;
    let draft = f.new_draft(f.team_primary, &["x@out.test"], &[], &[]).await;
    let when = TimestampMillis::now().as_millis() + 3_600_000;
    let (status, sent) = f.send(&draft, json!({"scheduled_at": at(when)})).await;
    assert_eq!(status, StatusCode::OK, "{sent}");
    f.sql(&format!(
        "DELETE FROM mailbox_members WHERE mailbox_id = '{}' AND membership_id = '{}'",
        f.team, f.ann.membership
    ))
    .await;
    let (_, list) = f
        .call(&f.ann, "GET", "/outbound?view=scheduled", None)
        .await;
    assert_eq!(list.as_array().unwrap().len(), 1, "{list}");
    let (_, filtered) = f
        .call(
            &f.ann,
            "GET",
            &format!("/outbound?view=scheduled&mailbox_id={}", f.ann_personal),
            None,
        )
        .await;
    assert_eq!(filtered, json!([]));
    let id = sent["id"].as_str().unwrap();
    let (status, body) = f
        .call(&f.ann, "POST", &format!("/outbound/{id}/cancel"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[tokio::test]
async fn no_reply_draft_while_it_is_being_sent() {
    let f = Fixture::new().await;
    f.signing().await;
    let (_, thread) = f.deliver("ann@one.test", f.ann_primary).await;
    let draft = f.thread_draft("reply", thread).await;
    let (status, sent) = f.send(&draft, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{sent}");
    let outbound = sent["id"].as_str().unwrap();
    let body = json!({"mode": "reply", "thread_id": thread.to_string(), "body_html": "<p>x</p>"});
    let (status, problem) = f.call(&f.ann, "POST", "/drafts", Some(body.clone())).await;
    assert_eq!(status, StatusCode::CONFLICT, "{problem}");
    assert_eq!(problem["code"], "draft_sending");
    assert_eq!(problem["outbound_id"], outbound);
    let (status, cancelled) = f
        .call(
            &f.ann,
            "POST",
            &format!("/outbound/{outbound}/cancel"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{cancelled}");
    let (status, again) = f.call(&f.ann, "POST", "/drafts", Some(body)).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["id"], draft);
}

// ---- Final review fixes ----

const HOUR_MS: i64 = 3_600_000;
const DAY_MS: i64 = 24 * HOUR_MS;

impl Fixture {
    /// Ann sends a new message; its job is dropped (the job store lost it). Returns its id and
    /// release time.
    async fn held_without_job(&self) -> (Id, i64) {
        let draft = self
            .new_draft(self.ann_primary, &["x@out.test"], &[], &[])
            .await;
        let (status, sent) = self.send(&draft, json!({})).await;
        assert_eq!(status, StatusCode::OK, "{sent}");
        let id: Id = sent["id"].as_str().unwrap().parse().unwrap();
        let (_, release_at, _) = self.outbound_row(&id.to_string()).await;
        self.sql("DELETE FROM jobs WHERE kind = 'mail.send'").await;
        (id, release_at)
    }
}

#[tokio::test]
async fn sweep_requeues_never_released_rows() {
    let f = Fixture::new().await;
    f.signing().await;
    // No reason: the job of a due message was lost.
    let (id, release_at) = f.held_without_job().await;
    assert_eq!(f.mail.sweep_releases(at(release_at - 1)).await.unwrap(), 0);
    assert_eq!(f.mail.sweep_releases(at(release_at)).await.unwrap(), 1);
    assert_eq!(f.queued_send_jobs(id).await, 1);
    assert_eq!(f.mail.sweep_releases(at(release_at + 1)).await.unwrap(), 0);
    // Waiting for the limits (`rate_limit`), its job lost too.
    f.sql("DELETE FROM jobs WHERE kind = 'mail.send'").await;
    f.sql(&format!(
        "UPDATE mail_outbound SET hold_reason = 'rate_limit' WHERE id = '{id}'"
    ))
    .await;
    assert_eq!(f.mail.sweep_releases(at(release_at + 2)).await.unwrap(), 1);
    run_send_job(&f.job_state(), id, at(release_at + 3))
        .await
        .unwrap();
    assert_eq!(f.outbound_reason(id).await.0, "released");
}

#[tokio::test]
async fn release_failed_is_swept_for_a_day_then_given_up() {
    let f = Fixture::new().await;
    f.signing().await;
    let (id, release_at) = f.held_without_job().await;
    f.mail
        .hold_release(id, "release_failed", at(release_at))
        .await
        .unwrap();
    // A later failure keeps the time of the first.
    f.mail
        .hold_release(id, "release_failed", at(release_at + HOUR_MS))
        .await
        .unwrap();
    assert_eq!(
        f.number(
            "SELECT release_failed_at FROM mail_outbound WHERE id = $1",
            id
        )
        .await,
        Some(release_at)
    );
    assert_eq!(
        f.mail
            .sweep_releases(at(release_at + DAY_MS - 1))
            .await
            .unwrap(),
        1
    );
    f.sql("DELETE FROM jobs WHERE kind = 'mail.send'").await;
    // A day after the first failure: held as `release_error`, never swept again.
    assert_eq!(
        f.mail
            .sweep_releases(at(release_at + DAY_MS))
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        f.outbound_reason(id).await,
        ("held".to_owned(), Some("release_error".to_owned()))
    );
    assert_eq!(
        f.mail
            .sweep_releases(at(release_at + 2 * DAY_MS))
            .await
            .unwrap(),
        0
    );
    assert_eq!(f.queued_send_jobs(id).await, 0);
    // Another reason clears the time.
    f.mail
        .hold_release(id, "build_failed", at(release_at + 2 * DAY_MS))
        .await
        .unwrap();
    assert_eq!(
        f.number(
            "SELECT release_failed_at FROM mail_outbound WHERE id = $1",
            id
        )
        .await,
        None
    );
}

#[tokio::test]
async fn release_holds_when_the_domain_or_mailbox_stopped_being_ready() {
    let f = Fixture::new().await;
    f.signing().await;
    let state = f.job_state();
    // The domain was suspended after Send.
    let (id, release_at) = f.held_without_job().await;
    f.sql(&format!(
        "UPDATE mail_domains SET status = 'suspended', suspended_at = {release_at} WHERE id = '{}'",
        f.domain
    ))
    .await;
    assert!(run_send_job(&state, id, at(release_at)).await.is_err());
    assert_eq!(
        f.outbound_reason(id).await,
        ("held".to_owned(), Some("domain_unverified".to_owned()))
    );
    let sent: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM mail_receipts WHERE source = 'sent'")
        .fetch_one(f.database.pool())
        .await
        .unwrap();
    assert_eq!(sent, 0, "nothing was built or sent");
    f.sql("DELETE FROM jobs WHERE kind = 'mail.send'").await;
    // Still not ready: the sweep leaves it.
    assert_eq!(f.mail.sweep_releases(at(release_at + 1)).await.unwrap(), 0);
    // Verified again: swept and sent.
    f.sql(&format!(
        "UPDATE mail_domains SET status = 'verified', suspended_at = NULL WHERE id = '{}'",
        f.domain
    ))
    .await;
    assert_eq!(f.mail.sweep_releases(at(release_at + 2)).await.unwrap(), 1);
    run_send_job(&state, id, at(release_at + 3)).await.unwrap();
    assert_eq!(f.outbound_reason(id).await, ("released".to_owned(), None));

    // The mailbox was suspended after Send.
    let (id, release_at) = f.held_without_job().await;
    f.sql(&format!(
        "UPDATE mailboxes SET status = 'suspended' WHERE id = '{}'",
        f.ann_personal
    ))
    .await;
    assert!(run_send_job(&state, id, at(release_at)).await.is_err());
    assert_eq!(
        f.outbound_reason(id).await,
        ("held".to_owned(), Some("mailbox_suspended".to_owned()))
    );
    f.sql("DELETE FROM jobs WHERE kind = 'mail.send'").await;
    assert_eq!(f.mail.sweep_releases(at(release_at + 1)).await.unwrap(), 0);
    f.sql(&format!(
        "UPDATE mailboxes SET status = 'active' WHERE id = '{}'",
        f.ann_personal
    ))
    .await;
    assert_eq!(f.mail.sweep_releases(at(release_at + 2)).await.unwrap(), 1);
}

#[tokio::test]
async fn held_message_of_a_removed_member_is_cancelled_at_release() {
    let f = Fixture::new().await;
    f.signing().await;
    let (id, release_at) = f.held_without_job().await;
    let version: i64 = sqlx::query_scalar("SELECT version FROM memberships WHERE id = $1")
        .bind(f.ann.membership.to_string())
        .fetch_one(f.database.pool())
        .await
        .unwrap();
    orbit_server::repositories::workspaces::WorkspaceRepository::new((*f.database).clone())
        .remove_member(
            f.workspace,
            f.ann.membership,
            user_of(&f, &f.owner).await,
            u64::try_from(version).unwrap(),
            "request",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    // The row stays (no cascade), without its author and its draft.
    let row =
        sqlx::query("SELECT membership_id, draft_id, status FROM mail_outbound WHERE id = $1")
            .bind(id.to_string())
            .fetch_one(f.database.pool())
            .await
            .unwrap();
    use sqlx::Row;
    assert_eq!(row.get::<Option<String>, _>("membership_id"), None);
    assert_eq!(row.get::<Option<String>, _>("draft_id"), None);
    assert_eq!(row.get::<String, _>("status"), "held");
    // The sweep finds it stranded; its release cancels it as `draft_missing`.
    assert_eq!(f.mail.sweep_releases(at(release_at)).await.unwrap(), 1);
    run_send_job(&f.job_state(), id, at(release_at))
        .await
        .unwrap();
    assert_eq!(
        f.outbound_reason(id).await,
        ("cancelled".to_owned(), Some("draft_missing".to_owned()))
    );
}

#[tokio::test]
async fn readiness_sweep_is_not_starved_by_rows_still_not_ready() {
    let f = Fixture::new().await;
    f.signing().await;
    // Three held messages of a mailbox that is still suspended: oldest first.
    let mut last = 0;
    for _ in 0..3 {
        let (_, release_at) = f.held_without_job().await;
        last = last.max(release_at);
    }
    f.sql(&format!(
        "UPDATE mailboxes SET status = 'suspended' WHERE id = '{}'",
        f.ann_personal
    ))
    .await;
    // A newer message of another mailbox, held while it was not ready and ready again now.
    let draft = f.new_draft(f.team_primary, &["x@out.test"], &[], &[]).await;
    let (status, sent) = f.send(&draft, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{sent}");
    let ready: Id = sent["id"].as_str().unwrap().parse().unwrap();
    let (_, release_at, _) = f.outbound_row(&ready.to_string()).await;
    last = last.max(release_at);
    f.sql("DELETE FROM jobs WHERE kind = 'mail.send'").await;
    f.sql("UPDATE mail_outbound SET hold_reason = 'mailbox_suspended' WHERE status = 'held'")
        .await;
    // With pages of two rows, the two oldest are not ready: the sweep reads on and finds the ready one.
    assert_eq!(
        f.mail.sweep_releases_with_batch(at(last), 2).await.unwrap(),
        1
    );
    assert_eq!(f.queued_send_jobs(ready).await, 1);
}
