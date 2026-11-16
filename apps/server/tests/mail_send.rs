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
        let mail = Arc::new(MailRepository::new((*database).clone(), uploads, APP_KEY));
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
async fn recipient_cap_new_tier_50() {
    let f = Fixture::new().await;
    f.sql(&format!(
        "INSERT INTO mail_send_policies (workspace_id, tier, created_at, updated_at) \
         VALUES ('{}', 'new', 1, 1)",
        f.workspace
    ))
    .await;
    let limits = f.mail.limits_for(f.workspace).await.unwrap();
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
    // Without a policy row a workspace is standard.
    f.sql("DELETE FROM mail_send_policies").await;
    assert_eq!(
        f.mail.limits_for(f.workspace).await.unwrap().max_recipients,
        100
    );
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
    f.sql("UPDATE instance_settings SET mail_send_tiers = '{\"new\": 1}'")
        .await;
    assert_eq!(
        f.mail.limits_for(f.workspace).await.unwrap().max_recipients,
        50
    );
    f.sql("UPDATE instance_settings SET mail_send_tiers = '[]'")
        .await;
    assert_eq!(
        f.mail.limits_for(f.workspace).await.unwrap().per_user_hour,
        30
    );
    f.sql(&format!(
        "INSERT INTO mail_send_policies (workspace_id, tier, custom_limits, created_at, updated_at) \
         VALUES ('{}', 'custom', '{{\"x\": 1}}', 1, 1)",
        f.workspace
    ))
    .await;
    assert_eq!(
        f.mail.limits_for(f.workspace).await.unwrap().max_recipients,
        50
    );
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
        let (content_type, body) = multipart(name, bytes);
        let request = Request::builder()
            .method("POST")
            .uri(format!(
                "/api/v1/workspaces/{}/mail/drafts/{draft}/files",
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
        Err(orbit_server::repositories::mail::MailError::Unprocessable(
            "message_too_large"
        ))
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
            live: LiveHub::of(&self.database),
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
        .list_outbound(f.ann.membership, OutboundView::Outbox)
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
