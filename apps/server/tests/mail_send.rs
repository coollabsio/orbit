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
