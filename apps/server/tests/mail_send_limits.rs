//! Send limits, pause and promotion (spec §5.8).

use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_mail::events::{ParsedEvent, ProviderEvent};
use orbit_mail::{DsnAction, DsnRecipient};
use orbit_platform::{
    AttachmentMutationCoordinator, AuthenticatedUser, Id, LocalBlobStore, MailMode,
    PasswordService, TestDatabase, TimestampMillis, UploadLimits, UploadService,
};
use orbit_server::auth_routes::CookieMode;
use orbit_server::mail_routes::{MailSettings, MailState, mail_router};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::instance_settings::InstanceSettingsRepository;
use orbit_server::repositories::mail::{
    AttemptResult, BounceKind, Limits, MailRepository, SendTier,
};
use serde_json::{Value, json};
use tower::ServiceExt;

const APP_KEY: [u8; 32] = [9; 32];
const MINUTE: i64 = 60_000;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;

struct Fixture {
    _root: tempfile::TempDir,
    database: TestDatabase,
    identity: Arc<IdentityRepository>,
    mail: Arc<MailRepository>,
    workspace: Id,
    root_user: Id,
    root_cookie: String,
    owner_membership: Id,
    mailbox: Id,
}

impl Fixture {
    async fn new() -> Self {
        Self::with_cloud(false).await
    }

    async fn with_cloud(cloud: bool) -> Self {
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
            UploadLimits::default(),
        );
        let mail =
            Arc::new(MailRepository::new((*database).clone(), uploads, APP_KEY).with_cloud(cloud));
        let owner_membership: String = sqlx::query_scalar(
            "SELECT id FROM memberships WHERE workspace_id = $1 AND user_id = $2",
        )
        .bind(setup.workspace_id.to_string())
        .bind(setup.user_id.to_string())
        .fetch_one(database.pool())
        .await
        .unwrap();
        let mailbox = Id::new_v7();
        sqlx::query(
            "INSERT INTO mailboxes (id, workspace_id, kind, name, owner_membership_id, status, \
             created_at, updated_at) VALUES ($1, $2, 'shared', 'Team', NULL, 'active', $3, $3)",
        )
        .bind(mailbox.to_string())
        .bind(setup.workspace_id.to_string())
        .bind(now.as_millis())
        .execute(database.pool())
        .await
        .unwrap();
        Self {
            _root: root,
            database,
            identity,
            mail,
            workspace: setup.workspace_id,
            root_user: setup.user_id,
            root_cookie: format!("__Host-orbit_session={}", setup.session.token),
            owner_membership: owner_membership.parse().unwrap(),
            mailbox,
        }
    }

    async fn sql(&self, sql: &str) {
        sqlx::query(sqlx::AssertSqlSafe(sql.to_owned()))
            .execute(self.database.pool())
            .await
            .unwrap();
    }

    async fn policy(&self, tier: &str, extra: &str) {
        self.sql(&format!(
            "INSERT INTO mail_send_policies (workspace_id, tier, first_release_at, created_at, \
             updated_at) VALUES ('{}', '{tier}', {}, 1, 1)",
            self.workspace,
            TimestampMillis::now().as_millis() - 20 * DAY
        ))
        .await;
        if !extra.is_empty() {
            self.sql(&format!(
                "UPDATE mail_send_policies SET {extra} WHERE workspace_id = '{}'",
                self.workspace
            ))
            .await;
        }
    }

    /// A user with a session; a member of the workspace with `role`.
    async fn member(&self, name: &str, role: &str) -> (Id, String) {
        let id = Id::new_v7();
        let membership = Id::new_v7();
        let now = TimestampMillis::now();
        let email = format!("{name}@example.com");
        sqlx::query(
            "INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at) \
             VALUES ($1, $2, $2, $3, 'unused', $4, $4)",
        )
        .bind(id.to_string())
        .bind(&email)
        .bind(name)
        .bind(now.as_millis())
        .execute(self.database.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, 0, $5, $5)",
        )
        .bind(membership.to_string())
        .bind(self.workspace.to_string())
        .bind(id.to_string())
        .bind(role)
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
                    display_name: name.to_owned(),
                },
                now,
            )
            .await
            .unwrap();
        (
            membership,
            format!("__Host-orbit_session={}", session.token),
        )
    }

    /// One outbound row (`status`) with `count` recipients released at `at` by `membership`;
    /// the first `hard` of them bounced hard. Returns the outbound id and the recipient ids.
    async fn sent(
        &self,
        membership: Id,
        count: usize,
        hard: usize,
        at: i64,
        status: &str,
    ) -> (Id, Vec<Id>) {
        let outbound = Id::new_v7();
        self.sql(&format!(
            "INSERT INTO mail_outbound (id, workspace_id, membership_id, mailbox_id, status, \
             release_at, created_at, updated_at) VALUES ('{outbound}', '{}', '{membership}', '{}', \
             '{status}', {at}, {at}, {at})",
            self.workspace, self.mailbox
        ))
        .await;
        let mut ids = Vec::new();
        for i in 0..count {
            let id = Id::new_v7();
            let (state, kind) = if i < hard {
                ("bounced", "'hard'")
            } else {
                ("delivered", "NULL")
            };
            self.sql(&format!(
                "INSERT INTO mail_outbound_recipients (id, outbound_id, workspace_id, membership_id, \
                 address, kind, status, bounce_kind, released_at, created_at, updated_at) \
                 VALUES ('{id}', '{outbound}', '{}', '{membership}', 'r{i}-{outbound}@out.test', \
                 'to', '{state}', {kind}, {at}, {at}, {at})",
                self.workspace
            ))
            .await;
            ids.push(id);
        }
        (outbound, ids)
    }

    async fn allows(&self, membership: Id, count: i64, now: i64) -> Option<i64> {
        let mut transaction = self.database.immediate_transaction().await.unwrap();
        let result = self
            .mail
            .window_allows(
                &mut transaction,
                self.workspace,
                membership,
                count,
                TimestampMillis::from_millis(now),
                None,
            )
            .await
            .unwrap();
        transaction.commit().await.unwrap();
        result.map(TimestampMillis::as_millis)
    }

    async fn check_pause(&self, now: i64) -> bool {
        let mut transaction = self.database.immediate_transaction().await.unwrap();
        let paused = self
            .mail
            .check_pause(
                &mut transaction,
                self.workspace,
                TimestampMillis::from_millis(now),
            )
            .await
            .unwrap();
        transaction.commit().await.unwrap();
        paused
    }

    async fn paused_at(&self) -> Option<i64> {
        sqlx::query_scalar("SELECT paused_at FROM mail_send_policies WHERE workspace_id = $1")
            .bind(self.workspace.to_string())
            .fetch_one(self.database.pool())
            .await
            .unwrap()
    }

    async fn tier(&self) -> String {
        sqlx::query_scalar("SELECT tier FROM mail_send_policies WHERE workspace_id = $1")
            .bind(self.workspace.to_string())
            .fetch_one(self.database.pool())
            .await
            .unwrap()
    }

    /// Marks `ids` as complained about (a provider's complaint) at `at`.
    async fn complained(&self, ids: &[Id], at: i64) {
        for id in ids {
            self.sql(&format!(
                "UPDATE mail_outbound_recipients SET complained_at = {at} WHERE id = '{id}'"
            ))
            .await;
        }
    }

    /// A send route of `kind` postmark that the outbound `outbound` went through.
    async fn route(&self, outbound: Id) -> Id {
        let route = Id::new_v7();
        sqlx::query(
            "INSERT INTO mail_send_routes (id, workspace_id, kind, name, secrets, settings, \
             created_at, updated_at) VALUES ($1, $2, 'postmark', 'Route', $3, '{}', 1, 1)",
        )
        .bind(route.to_string())
        .bind(self.workspace.to_string())
        .bind(vec![0_u8])
        .execute(self.database.pool())
        .await
        .unwrap();
        self.sql(&format!(
            "UPDATE mail_outbound SET route_id = '{route}' WHERE id = '{outbound}'"
        ))
        .await;
        route
    }

    async fn event(&self, route: Id, recipient: Id, event: ProviderEvent) {
        let address: String =
            sqlx::query_scalar("SELECT address FROM mail_outbound_recipients WHERE id = $1")
                .bind(recipient.to_string())
                .fetch_one(self.database.pool())
                .await
                .unwrap();
        self.mail
            .apply_provider_event(
                route,
                &ParsedEvent {
                    provider_message_id: None,
                    correlation: Some(recipient.to_string()),
                    message_id: None,
                    recipient: address,
                    at_ms: None,
                    event,
                },
                TimestampMillis::now(),
            )
            .await
            .unwrap();
    }

    async fn audits(&self, action: &str) -> i64 {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM audit_events WHERE workspace_id = $1 AND action = $2",
        )
        .bind(self.workspace.to_string())
        .bind(action)
        .fetch_one(self.database.pool())
        .await
        .unwrap()
    }

    async fn first_paused_at(&self) -> Option<i64> {
        sqlx::query_scalar("SELECT first_paused_at FROM mail_send_policies WHERE workspace_id = $1")
            .bind(self.workspace.to_string())
            .fetch_one(self.database.pool())
            .await
            .unwrap()
    }

    async fn call(
        &self,
        cookie: &str,
        method: &str,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let app = mail_router(MailState::new(
            Arc::clone(&self.identity),
            Arc::clone(&self.mail),
            CookieMode::secure(),
            MailSettings {
                enabled: true,
                hostname: Some("mx.orbit.test".to_owned()),
                mode: MailMode::Direct,
            },
        ));
        let builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::COOKIE, cookie);
        let request = match body {
            Some(value) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(value.to_string())),
            None => builder.body(Body::empty()),
        }
        .unwrap();
        let response = app.oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }
}

fn now() -> i64 {
    TimestampMillis::now().as_millis()
}

#[tokio::test]
async fn cloud_starts_new_selfhosted_standard() {
    let cloud = Fixture::with_cloud(true).await;
    assert_eq!(
        cloud.mail.limits_for(cloud.workspace).await.unwrap(),
        Limits::NEW
    );
    let view = cloud
        .mail
        .send_policy(cloud.workspace, TimestampMillis::now())
        .await
        .unwrap();
    assert_eq!(view.tier, SendTier::New);
    let hosted = Fixture::new().await;
    assert_eq!(
        hosted.mail.limits_for(hosted.workspace).await.unwrap(),
        Limits::STANDARD
    );
    let view = hosted
        .mail
        .send_policy(hosted.workspace, TimestampMillis::now())
        .await
        .unwrap();
    assert_eq!(view.tier, SendTier::Standard);
}

#[tokio::test]
async fn user_hour_limit_holds() {
    let f = Fixture::new().await;
    f.policy("new", "").await;
    let at = now() - 10 * MINUTE;
    f.sent(f.owner_membership, 30, 0, at, "done").await;
    assert_eq!(
        f.allows(f.owner_membership, 1, now()).await,
        Some(at + HOUR)
    );
    // another member is not limited by the first one's hour (the workspace has room)
    let (other, _) = f.member("ann", "member").await;
    assert_eq!(f.allows(other, 1, now()).await, None);
}

#[tokio::test]
async fn workspace_day_limit_holds() {
    let f = Fixture::new().await;
    f.policy("new", "").await;
    let (ann, _) = f.member("ann", "member").await;
    let (bob, _) = f.member("bob", "member").await;
    let (cy, _) = f.member("cy", "member").await;
    let at = now() - 5 * HOUR;
    f.sent(ann, 100, 0, at, "done").await;
    f.sent(bob, 100, 0, at, "done").await;
    // 200 in the workspace's day: a third member waits for the oldest to leave the day
    assert_eq!(f.allows(cy, 1, now()).await, Some(at + DAY));
    assert_eq!(f.allows(cy, 1, at + DAY + 1).await, None);
}

#[tokio::test]
async fn custom_limits_apply() {
    let f = Fixture::new().await;
    let custom = Limits {
        per_user_hour: 2,
        per_user_day: 5,
        per_ws_hour: 10,
        per_ws_day: 50,
        max_recipients: 3,
    };
    let (status, _) = f
        .call(
            &f.root_cookie,
            "PUT",
            &format!("/api/v1/admin/mail/send-policies/{}", f.workspace),
            Some(json!({"tier": "custom", "custom_limits": custom})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(f.mail.limits_for(f.workspace).await.unwrap(), custom);
    let at = now() - MINUTE;
    f.sent(f.owner_membership, 2, 0, at, "done").await;
    assert_eq!(
        f.allows(f.owner_membership, 1, now()).await,
        Some(at + HOUR)
    );
    // custom needs its numbers, and a tier drops them
    let (status, _) = f
        .call(
            &f.root_cookie,
            "PUT",
            &format!("/api/v1/admin/mail/send-policies/{}", f.workspace),
            Some(json!({"tier": "custom"})),
        )
        .await;
    assert_ne!(status, StatusCode::OK);
    let (status, body) = f
        .call(
            &f.root_cookie,
            "PUT",
            &format!("/api/v1/admin/mail/send-policies/{}", f.workspace),
            Some(json!({"tier": "standard"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        f.mail.limits_for(f.workspace).await.unwrap(),
        Limits::STANDARD
    );
}

#[tokio::test]
async fn pause_at_5_percent_hard_bounces_with_100_recipients() {
    let f = Fixture::new().await;
    f.policy("standard", "").await;
    // 5 of 100 is exactly 5 %: not over
    f.sent(f.owner_membership, 100, 5, now() - HOUR, "done")
        .await;
    assert!(!f.check_pause(now()).await);
    assert_eq!(f.paused_at().await, None);
    // a sixth is over
    f.sql("UPDATE mail_outbound_recipients SET status = 'bounced', bounce_kind = 'hard' WHERE id = (SELECT id FROM mail_outbound_recipients WHERE bounce_kind IS NULL LIMIT 1)").await;
    assert!(f.check_pause(now()).await);
    assert!(f.paused_at().await.is_some());
    let reason: String =
        sqlx::query_scalar("SELECT pause_reason FROM mail_send_policies WHERE workspace_id = $1")
            .bind(f.workspace.to_string())
            .fetch_one(f.database.pool())
            .await
            .unwrap();
    assert!(reason.contains("6 of 100"), "{reason}");
    // other bounce kinds do not count
    let g = Fixture::new().await;
    g.policy("standard", "").await;
    let (_, ids) = g
        .sent(g.owner_membership, 100, 0, now() - HOUR, "done")
        .await;
    for id in ids.iter().take(20) {
        g.sql(&format!("UPDATE mail_outbound_recipients SET status = 'bounced', bounce_kind = 'gave_up' WHERE id = '{id}'")).await;
    }
    assert!(!g.check_pause(now()).await);
}

#[tokio::test]
async fn no_pause_under_100_recipients() {
    let f = Fixture::new().await;
    f.policy("standard", "").await;
    let (_, ids) = f
        .sent(f.owner_membership, 99, 50, now() - HOUR, "done")
        .await;
    f.complained(&ids[..5], now()).await;
    assert!(!f.check_pause(now()).await);
    // recipients older than 7 days do not count toward the 100
    f.sent(f.owner_membership, 100, 0, now() - 8 * DAY, "done")
        .await;
    assert!(!f.check_pause(now()).await);
    assert_eq!(f.paused_at().await, None);
}

#[tokio::test]
async fn pause_at_complaints_over_0_3_percent() {
    let f = Fixture::new().await;
    f.policy("standard", "").await;
    let (_, ids) = f
        .sent(f.owner_membership, 1000, 0, now() - HOUR, "done")
        .await;
    f.complained(&ids[..3], now()).await;
    assert!(!f.check_pause(now()).await, "0.3 % is not over");
    f.complained(&ids[3..4], now() - MINUTE).await;
    assert!(f.check_pause(now()).await, "4 of 1000 is over 0.3 %");
    let reason: String =
        sqlx::query_scalar("SELECT pause_reason FROM mail_send_policies WHERE workspace_id = $1")
            .bind(f.workspace.to_string())
            .fetch_one(f.database.pool())
            .await
            .unwrap();
    assert!(reason.starts_with("Complaints"), "{reason}");
}

#[tokio::test]
async fn dsn_pauses_through_process_dsn() {
    let f = Fixture::new().await;
    f.policy("standard", "").await;
    let (_, ids) = f
        .sent(f.owner_membership, 100, 5, now() - HOUR, "done")
        .await;
    let victim = ids[50];
    let address: String =
        sqlx::query_scalar("SELECT address FROM mail_outbound_recipients WHERE id = $1")
            .bind(victim.to_string())
            .fetch_one(f.database.pool())
            .await
            .unwrap();
    let changed = f
        .mail
        .process_dsn(
            victim,
            &[DsnRecipient {
                address,
                original: None,
                action: DsnAction::Failed,
                status: "5.1.1".to_owned(),
                diagnostic: None,
            }],
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    assert!(changed);
    assert!(f.paused_at().await.is_some());
}

#[tokio::test]
async fn paused_refuses_send_and_resume_requeues() {
    let f = Fixture::new().await;
    f.policy("standard", "paused_at = 5, pause_reason = 'test'")
        .await;
    let (outbound, _) = f
        .sent(f.owner_membership, 1, 0, now() - HOUR, "released")
        .await;
    sqlx::query("UPDATE mail_outbound_recipients SET status = 'queued', bounce_kind = NULL WHERE outbound_id = $1")
        .bind(outbound.to_string())
        .execute(f.database.pool())
        .await
        .unwrap();
    let t = TimestampMillis::now();
    assert_eq!(f.mail.sweep_deliveries(t).await.unwrap(), 0);
    let (status, view) = f
        .call(
            &f.root_cookie,
            "POST",
            &format!("/api/v1/admin/mail/send-policies/{}/resume", f.workspace),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert!(view["paused_at"].is_null());
    let resumed_by: Option<String> =
        sqlx::query_scalar("SELECT resumed_by FROM mail_send_policies WHERE workspace_id = $1")
            .bind(f.workspace.to_string())
            .fetch_one(f.database.pool())
            .await
            .unwrap();
    assert_eq!(resumed_by, Some(f.root_user.to_string()));
    assert_eq!(f.mail.sweep_deliveries(t).await.unwrap(), 1);
}

#[tokio::test]
async fn only_root_resumes() {
    let f = Fixture::new().await;
    f.policy("standard", "paused_at = 5, pause_reason = 'test'")
        .await;
    let (_, admin) = f.member("ann", "admin").await;
    let uri = format!("/api/v1/admin/mail/send-policies/{}/resume", f.workspace);
    let (status, _) = f.call(&admin, "POST", &uri, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(f.paused_at().await.is_some());
    let (status, _) = f
        .call(&admin, "GET", "/api/v1/admin/mail/send-policies", None)
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    // a workspace admin reads the policy but cannot change it
    let (status, view) = f
        .call(
            &admin,
            "GET",
            &format!("/api/v1/workspaces/{}/mail/send-policy", f.workspace),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["pause_reason"], "test");
    let (status, _) = f.call(&f.root_cookie, "POST", &uri, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(f.paused_at().await, None);
    let (status, list) = f
        .call(
            &f.root_cookie,
            "GET",
            "/api/v1/admin/mail/send-policies",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn promotion_after_14_clean_days() {
    let f = Fixture::new().await;
    f.policy("new", "").await;
    f.sent(f.owner_membership, 100, 1, now() - DAY, "done")
        .await;
    assert_eq!(
        f.mail
            .run_send_policy(TimestampMillis::now())
            .await
            .unwrap(),
        1
    );
    assert_eq!(f.tier().await, "standard");
    // too early: first release 13 days ago
    let g = Fixture::new().await;
    g.policy("new", &format!("first_release_at = {}", now() - 13 * DAY))
        .await;
    assert_eq!(
        g.mail
            .run_send_policy(TimestampMillis::now())
            .await
            .unwrap(),
        0
    );
    assert_eq!(g.tier().await, "new");
    // 2 % hard bounces is not under 2 %
    let h = Fixture::new().await;
    h.policy("new", "").await;
    h.sent(h.owner_membership, 100, 2, now() - DAY, "done")
        .await;
    assert_eq!(
        h.mail
            .run_send_policy(TimestampMillis::now())
            .await
            .unwrap(),
        0
    );
    // a complaint rate of 0.1 % is not under 0.1 %
    let c = Fixture::new().await;
    c.policy("new", "").await;
    let (_, ids) = c
        .sent(c.owner_membership, 1000, 0, now() - DAY, "done")
        .await;
    c.complained(&ids[..1], now()).await;
    assert_eq!(
        c.mail
            .run_send_policy(TimestampMillis::now())
            .await
            .unwrap(),
        0
    );
    // a custom tier stays
    let k = Fixture::new().await;
    k.policy("custom", "").await;
    assert_eq!(
        k.mail
            .run_send_policy(TimestampMillis::now())
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn no_promotion_if_ever_paused() {
    let f = Fixture::new().await;
    f.policy("new", "first_paused_at = 5")
        .await;
    assert_eq!(
        f.mail
            .run_send_policy(TimestampMillis::now())
            .await
            .unwrap(),
        0
    );
    assert_eq!(f.tier().await, "new");
    // pausing then resuming keeps the history
    let g = Fixture::new().await;
    g.policy("standard", "").await;
    g.sent(g.owner_membership, 100, 10, now() - HOUR, "done")
        .await;
    g.sql(&format!(
        "UPDATE mail_send_policies SET tier = 'new' WHERE workspace_id = '{}'",
        g.workspace
    ))
    .await;
    assert!(g.check_pause(now()).await);
    g.mail
        .resume_sending(g.workspace, g.root_user, "request", TimestampMillis::now())
        .await
        .unwrap();
    g.sql("UPDATE mail_outbound_recipients SET bounce_kind = NULL, status = 'delivered'")
        .await;
    assert_eq!(
        g.mail
            .run_send_policy(TimestampMillis::now())
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn smtp_time_hard_bounce_pauses() {
    let f = Fixture::new().await;
    f.policy("standard", "").await;
    let (outbound, ids) = f
        .sent(f.owner_membership, 100, 5, now() - HOUR, "released")
        .await;
    let attempt = Id::new_v7();
    f.sql(&format!(
        "UPDATE mail_outbound_recipients SET status = 'sending', attempt_id = '{attempt}', \
         attempts = 1 WHERE id = '{}'",
        ids[60]
    ))
    .await;
    f.mail
        .record_outcomes(
            outbound,
            attempt,
            &[(
                ids[60],
                AttemptResult::Bounced {
                    response: "550 5.1.1 no such user".to_owned(),
                    kind: BounceKind::Hard,
                },
            )],
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    assert!(f.paused_at().await.is_some());
    assert!(f.first_paused_at().await.is_some());
}

#[tokio::test]
async fn provider_hard_bounce_pauses_without_a_policy_row() {
    let f = Fixture::with_cloud(true).await;
    let (outbound, ids) = f
        .sent(f.owner_membership, 100, 5, now() - HOUR, "done")
        .await;
    let route = f.route(outbound).await;
    f.event(
        route,
        ids[70],
        ProviderEvent::Bounced {
            hard: true,
            diagnostic: Some("550 5.1.1 unknown".to_owned()),
        },
    )
    .await;
    // the row was made with the Cloud default tier, and paused
    assert_eq!(f.tier().await, "new");
    assert!(f.paused_at().await.is_some());
}

#[tokio::test]
async fn provider_complaints_pause_even_for_suppressed_addresses() {
    let f = Fixture::new().await;
    f.policy("standard", "").await;
    let (outbound, ids) = f
        .sent(f.owner_membership, 1000, 0, now() - HOUR, "done")
        .await;
    let route = f.route(outbound).await;
    // the first four addresses are on the list already
    for id in &ids[..4] {
        f.sql(&format!(
            "INSERT INTO mail_suppressions (workspace_id, address, reason, created_at) \
             SELECT workspace_id, address, 'hard_bounce', 1 FROM mail_outbound_recipients \
             WHERE id = '{id}'"
        ))
        .await;
    }
    for id in &ids[..3] {
        f.event(route, *id, ProviderEvent::Complained).await;
    }
    assert_eq!(f.paused_at().await, None, "3 of 1000 is not over 0.3 %");
    // a repeated event counts once
    f.event(route, ids[0], ProviderEvent::Complained).await;
    assert_eq!(f.paused_at().await, None);
    f.event(route, ids[3], ProviderEvent::Complained).await;
    assert!(f.paused_at().await.is_some());
}

#[tokio::test]
async fn pause_without_a_policy_row_makes_it() {
    let f = Fixture::new().await;
    f.sent(f.owner_membership, 100, 10, now() - HOUR, "done")
        .await;
    assert!(f.check_pause(now()).await);
    assert_eq!(f.tier().await, "standard");
    assert!(f.paused_at().await.is_some());
}

#[tokio::test]
async fn admin_changes_are_audited() {
    let f = Fixture::new().await;
    let uri = format!("/api/v1/admin/mail/send-policies/{}", f.workspace);
    let (status, _) = f
        .call(&f.root_cookie, "PUT", &uri, Some(json!({"tier": "new"})))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(f.audits("instance.mail_send_policy_updated").await, 1);
    f.sql(&format!(
        "UPDATE mail_send_policies SET paused_at = 5 WHERE workspace_id = '{}'",
        f.workspace
    ))
    .await;
    let (status, _) = f
        .call(&f.root_cookie, "POST", &format!("{uri}/resume"), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(f.audits("instance.mail_sending_resumed").await, 1);
}

#[tokio::test]
async fn an_outsider_gets_not_found_for_the_workspace_policy() {
    let f = Fixture::new().await;
    let id = Id::new_v7();
    let now = TimestampMillis::now();
    sqlx::query(
        "INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at) \
         VALUES ($1, 'out@example.com', 'out@example.com', 'Out', 'unused', $2, $2)",
    )
    .bind(id.to_string())
    .bind(now.as_millis())
    .execute(f.database.pool())
    .await
    .unwrap();
    let session = f
        .identity
        .create_session(
            &AuthenticatedUser {
                id,
                email: "out@example.com".to_owned(),
                display_name: "Out".to_owned(),
            },
            now,
        )
        .await
        .unwrap();
    let (status, _) = f
        .call(
            &format!("__Host-orbit_session={}", session.token),
            "GET",
            &format!("/api/v1/workspaces/{}/mail/send-policy", f.workspace),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
