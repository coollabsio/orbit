//! Provider events of send routes (spec §5.5, §5.7): `POST /api/v1/mail/events/{kind}/{route_id}`
//! authenticates with the route's secret, finds the recipient of that route, and bounces,
//! suppresses, defers or delivers it.

#![allow(dead_code)]
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use futures_util::future::BoxFuture;
use hmac::{Hmac, Mac};
use orbit_platform::{
    AttachmentMutationCoordinator, Id, LocalBlobStore, MailMode, PasswordService, TestDatabase,
    TimestampMillis, UploadLimits, UploadService,
};
use orbit_server::auth_routes::CookieMode;
use orbit_server::live::LiveHub;
use orbit_server::mail_dns::FakeDns;
use orbit_server::mail_intake::MailIntake;
use orbit_server::mail_routes::{MailSettings, MailState, mail_router};
use orbit_server::mail_send::{
    Delivery, DeliveryContext, Outgoing, RecipientOutcome, SendJobState, run_send_job,
};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::instance_settings::InstanceSettingsRepository;
use orbit_server::repositories::mail::{
    AttemptResult, DeferCause, DraftMode, MailRepository, MailboxKind, NewDraft, NewMailbox,
    NewRoute, PROVIDER_DEFER_GIVE_UP_MS, ProviderScope, RouteChange, RouteSecrets, RouteSettings,
    SendRouteKind,
};
use orbit_server::repositories::workspaces::WorkspaceRepository;
use serde_json::{Value, json};
use sha2::Sha256;
use sqlx::Row;
use tower::ServiceExt;

const APP_KEY: [u8; 32] = [32; 32];
const MAILGUN_KEY: &str = "mailgun-webhook-signing-key";

/// An adapter that accepts everything, with the provider message ID of its table.
#[derive(Default)]
struct Fake {
    ids: Mutex<HashMap<String, String>>,
}

impl Delivery for Fake {
    fn deliver<'a>(
        &'a self,
        outgoing: &'a Outgoing<'a>,
    ) -> BoxFuture<'a, Vec<(String, RecipientOutcome)>> {
        Box::pin(async move {
            let ids = self.ids.lock().unwrap();
            outgoing
                .recipients
                .iter()
                .map(|address| {
                    (
                        address.clone(),
                        RecipientOutcome::Accepted {
                            provider_message_id: ids.get(address).cloned(),
                        },
                    )
                })
                .collect()
        })
    }
}

fn hmac_hex(key: &[u8], message: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).unwrap();
    mac.update(message);
    mac.finalize()
        .into_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn report() -> String {
    json!({"verify": {"status": "ok"}, "mx": {"status": "ok"}, "dkim": {"status": "ok"},
           "spf": {"status": "ok"}, "dmarc": {"status": "ok"}})
    .to_string()
}

/// A Mailgun event body (the docs' shape), signed now with `key`.
fn mailgun_event(key: &str, data: &Value) -> String {
    let timestamp = TimestampMillis::now().as_millis() / 1000;
    let token = format!("token-{}", Id::new_v7());
    json!({
        "signature": {
            "timestamp": timestamp.to_string(),
            "token": token,
            "signature": hmac_hex(key.as_bytes(), format!("{timestamp}{token}").as_bytes()),
        },
        "event-data": data,
    })
    .to_string()
}

/// Mailgun's `failed` event for `recipient` of the message `message_id`.
fn mailgun_failed(
    recipient: &str,
    message_id: &str,
    severity: &str,
    response: &str,
    at_secs: f64,
) -> Value {
    json!({
        "event": "failed",
        "id": "2kFItcrLQuKTdp-Ia2Xr7w",
        "timestamp": at_secs,
        "message": { "headers": { "message-id": message_id } },
        "recipient": recipient,
        "reason": "bounce",
        "severity": severity,
        "delivery-status": { "code": 550, "message": response },
        "user-variables": []
    })
}

fn now_secs() -> f64 {
    #[allow(clippy::cast_precision_loss)]
    let secs = TimestampMillis::now().as_millis() as f64 / 1000.0;
    secs
}

struct Fixture {
    _root: tempfile::TempDir,
    database: TestDatabase,
    identity: Arc<IdentityRepository>,
    mail: Arc<MailRepository>,
    intake: MailIntake,
    workspace: Id,
    user: Id,
    membership: Id,
    domain: Id,
    address: Id,
    route: Id,
    fake: Arc<Fake>,
}

impl Fixture {
    /// A workspace whose domain one.test sends through an instance route of `kind`.
    async fn new(kind: SendRouteKind, secrets: RouteSecrets) -> Self {
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
                    kind,
                    name: "Provider".to_owned(),
                    provider_id: None,
                    secrets,
                    settings: RouteSettings {
                        region: (kind == SendRouteKind::Ses).then(|| "us-east-1".to_owned()),
                        ..RouteSettings::default()
                    },
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
            user: setup.user_id,
            membership,
            domain,
            address: mailbox.addresses[0].id,
            route: route.id,
            fake: Arc::default(),
        }
    }

    async fn mailgun() -> Self {
        Self::new(
            SendRouteKind::Mailgun,
            RouteSecrets::Mailgun {
                api_key: "key-api".to_owned(),
                webhook_signing_key: MAILGUN_KEY.to_owned(),
            },
        )
        .await
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

    /// Posts `body` to the events URL of `kind` and `route` with `headers`.
    async fn post(
        &self,
        kind: &str,
        route: Id,
        headers: &[(&str, String)],
        body: String,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder()
            .method("POST")
            .uri(format!("/api/v1/mail/events/{kind}/{route}"))
            .header("content-type", "application/json");
        for (name, value) in headers {
            builder = builder.header(*name, value);
        }
        let response = self
            .app()
            .oneshot(builder.body(Body::from(body)).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn mailgun_post(&self, route: Id, key: &str, data: &Value) -> (StatusCode, Value) {
        self.post("mailgun", route, &[], mailgun_event(key, data))
            .await
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
                self.membership,
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
            .send_draft(self.membership, draft.id, None, now)
            .await
            .unwrap();
        sqlx::query("DELETE FROM jobs WHERE kind = 'mail.send'")
            .execute(self.database.pool())
            .await
            .unwrap();
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

    /// (status, bounce_kind, last_response, next_attempt_at).
    async fn state_of(
        &self,
        recipient: Id,
    ) -> (String, Option<String>, Option<String>, Option<i64>) {
        let row = sqlx::query(
            "SELECT status, bounce_kind, last_response, next_attempt_at \
             FROM mail_outbound_recipients WHERE id = $1",
        )
        .bind(recipient.to_string())
        .fetch_one(self.database.pool())
        .await
        .unwrap();
        (
            row.get("status"),
            row.get("bounce_kind"),
            row.get("last_response"),
            row.get("next_attempt_at"),
        )
    }

    async fn suppressed(&self, workspace: Id, address: &str) -> Option<(String, Option<String>)> {
        sqlx::query_as(
            "SELECT reason, recipient_id FROM mail_suppressions WHERE workspace_id = $1 \
             AND address = $2",
        )
        .bind(workspace.to_string())
        .bind(address)
        .fetch_optional(self.database.pool())
        .await
        .unwrap()
    }

    async fn outbound_status(&self, outbound: Id) -> (String, Option<String>) {
        let row = sqlx::query("SELECT status, delivery_summary FROM mail_outbound WHERE id = $1")
            .bind(outbound.to_string())
            .fetch_one(self.database.pool())
            .await
            .unwrap();
        (row.get("status"), row.get("delivery_summary"))
    }

    async fn thread_folder(&self, outbound: Id) -> String {
        sqlx::query_scalar(
            "SELECT t.folder FROM mail_outbound o \
             JOIN mail_thread_messages tm ON tm.message_id = o.message_id \
             JOIN mail_threads t ON t.id = tm.thread_id WHERE o.id = $1",
        )
        .bind(outbound.to_string())
        .fetch_one(self.database.pool())
        .await
        .unwrap()
    }

    fn answer_id(&self, address: &str, id: &str) {
        self.fake
            .ids
            .lock()
            .unwrap()
            .insert(address.to_owned(), id.to_owned());
    }
}

#[tokio::test]
async fn hard_bounce_event_suppresses() {
    let f = Fixture::mailgun().await;
    // Mailgun answers `<id>`; its events name `id`
    f.answer_id("gone@out.test", "<20261027.1@one.test>");
    f.answer_id("policy@out.test", "<20261027.1@one.test>");
    f.answer_id("ok@out.test", "<20261027.1@one.test>");
    let id = f
        .send(&["gone@out.test", "policy@out.test", "ok@out.test"])
        .await;
    let gone = f.recipient(id, "gone@out.test").await;
    let policy = f.recipient(id, "policy@out.test").await;
    let ok = f.recipient(id, "ok@out.test").await;
    assert_eq!(f.outbound_status(id).await.0, "done");

    let event = mailgun_failed(
        "gone@out.test",
        "20261027.1@one.test",
        "permanent",
        "5.1.1 The email account that you tried to reach does not exist",
        now_secs(),
    );
    let (status, body) = f.mailgun_post(f.route, MAILGUN_KEY, &event).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["applied"], 1);
    let (state, kind, response, _) = f.state_of(gone).await;
    assert_eq!((state.as_str(), kind.as_deref()), ("bounced", Some("hard")));
    assert_eq!(
        response.as_deref(),
        Some("550 5.1.1 The email account that you tried to reach does not exist")
    );
    assert_eq!(
        f.suppressed(f.workspace, "gone@out.test").await,
        Some(("hard_bounce".to_owned(), Some(gone.to_string())))
    );
    assert_eq!(f.thread_folder(id).await, "inbox");
    let (outbound, summary) = f.outbound_status(id).await;
    assert_eq!(outbound, "done");
    assert!(summary.unwrap().contains("\"bounced\""));
    // the other recipients of the same provider message are untouched
    assert_eq!(f.state_of(ok).await.0, "delivered");

    // a policy refusal bounces hard but is not a bad address
    let event = mailgun_failed(
        "policy@out.test",
        "20261027.1@one.test",
        "permanent",
        "5.7.1 Message rejected as spam",
        now_secs(),
    );
    let (status, _) = f.mailgun_post(f.route, MAILGUN_KEY, &event).await;
    assert_eq!(status, StatusCode::OK);
    let (state, kind, ..) = f.state_of(policy).await;
    assert_eq!((state.as_str(), kind.as_deref()), ("bounced", Some("hard")));
    assert_eq!(f.suppressed(f.workspace, "policy@out.test").await, None);

    // the same event again changes nothing
    let event = mailgun_failed(
        "gone@out.test",
        "20261027.1@one.test",
        "permanent",
        "5.1.1 again",
        now_secs(),
    );
    let (status, body) = f.mailgun_post(f.route, MAILGUN_KEY, &event).await;
    assert_eq!(
        (status, body["ignored"].clone()),
        (StatusCode::OK, json!(1))
    );
}

#[tokio::test]
async fn soft_event_defers_without_a_run_and_delivery_clears_it() {
    let f = Fixture::mailgun().await;
    f.answer_id("slow@out.test", "<slow.1@one.test>");
    let id = f.send(&["slow@out.test"]).await;
    let slow = f.recipient(id, "slow@out.test").await;
    tokio::time::sleep(Duration::from_millis(5)).await;
    let event = mailgun_failed(
        "slow@out.test",
        "slow.1@one.test",
        "temporary",
        "4.2.2 Mailbox full",
        now_secs(),
    );
    let (status, body) = f.mailgun_post(f.route, MAILGUN_KEY, &event).await;
    assert_eq!(
        (status, body["applied"].clone()),
        (StatusCode::OK, json!(1))
    );
    let (state, kind, response, next) = f.state_of(slow).await;
    assert_eq!(state, "deferred");
    assert_eq!(kind, None);
    assert_eq!(response.as_deref(), Some("550 4.2.2 Mailbox full"));
    assert_eq!(next, None, "the provider retries, not Orbit");
    assert_eq!(
        f.count(
            "SELECT CAST(COUNT(*) AS BIGINT) FROM jobs WHERE kind = 'mail.send'",
            &[]
        )
        .await,
        0
    );
    assert_eq!(f.outbound_status(id).await.0, "done");

    tokio::time::sleep(Duration::from_millis(5)).await;
    let delivered = json!({
        "event": "delivered",
        "timestamp": now_secs(),
        "message": { "headers": { "message-id": "slow.1@one.test" } },
        "recipient": "slow@out.test",
        "delivery-status": { "code": 250, "message": "OK" }
    });
    let (status, body) = f.mailgun_post(f.route, MAILGUN_KEY, &delivered).await;
    assert_eq!(
        (status, body["applied"].clone()),
        (StatusCode::OK, json!(1))
    );
    let (state, _, response, _) = f.state_of(slow).await;
    assert_eq!((state.as_str(), response), ("delivered", None));

    // the provider sends the earlier deferral again (out of order, or a retry of its webhook):
    // it is older than the delivery, so it changes nothing
    let (status, body) = f.mailgun_post(f.route, MAILGUN_KEY, &event).await;
    assert_eq!(
        (status, body["ignored"].clone()),
        (StatusCode::OK, json!(1))
    );
    assert_eq!(f.state_of(slow).await.0, "delivered");
    // an event without a time changes no status
    let mut timeless = event.clone();
    timeless.as_object_mut().unwrap().remove("timestamp");
    let (status, body) = f.mailgun_post(f.route, MAILGUN_KEY, &timeless).await;
    assert_eq!(
        (status, body["ignored"].clone()),
        (StatusCode::OK, json!(1))
    );
    assert_eq!(f.state_of(slow).await.0, "delivered");
}

#[tokio::test]
async fn delivered_event_wins_over_orbit_deferral_and_attempt_in_flight() {
    let f = Fixture::mailgun().await;
    let id = f.send(&["ann@out.test", "bob@out.test"]).await;
    let ann = f.recipient(id, "ann@out.test").await;
    let bob = f.recipient(id, "bob@out.test").await;
    // Orbit's call timed out after the provider took the message: deferred to a next attempt,
    // without the provider's message ID
    let now = TimestampMillis::now().as_millis();
    sqlx::query(
        "UPDATE mail_outbound_recipients SET status = 'deferred', next_attempt_at = $1, \
         last_response = 'Timed out', provider_message_id = NULL, updated_at = $2 WHERE id = $3",
    )
    .bind(now + 60_000)
    .bind(now)
    .bind(ann.to_string())
    .execute(f.database.pool())
    .await
    .unwrap();
    sqlx::query("UPDATE mail_outbound SET status = 'released' WHERE id = $1")
        .bind(id.to_string())
        .execute(f.database.pool())
        .await
        .unwrap();
    // the provider's delivery happened before Orbit recorded the timeout: it still wins
    #[allow(clippy::cast_precision_loss)]
    let earlier = (now - 3_600_000) as f64 / 1000.0;
    let delivered = |address: &str| {
        json!({
            "event": "delivered",
            "timestamp": earlier,
            "message": { "headers": { "message-id": "unknown@mailgun" } },
            "recipient": address,
            "user-variables": { "orbit": id.to_string() },
            "delivery-status": { "code": 250, "message": "OK" }
        })
    };
    let (status, body) = f
        .mailgun_post(f.route, MAILGUN_KEY, &delivered("ann@out.test"))
        .await;
    assert_eq!(
        (status, body["applied"].clone()),
        (StatusCode::OK, json!(1))
    );
    let (state, _, response, next) = f.state_of(ann).await;
    assert_eq!((state.as_str(), response, next), ("delivered", None, None));
    assert_eq!(f.outbound_status(id).await.0, "done");
    // no further claim: a run (even after the old next attempt) sends nothing
    sqlx::query("UPDATE mail_outbound SET status = 'released' WHERE id = $1")
        .bind(id.to_string())
        .execute(f.database.pool())
        .await
        .unwrap();
    let attempts = |recipient: Id| {
        let pool = f.database.pool().clone();
        async move {
            sqlx::query_scalar::<_, i64>(
                "SELECT attempts FROM mail_outbound_recipients WHERE id = $1",
            )
            .bind(recipient.to_string())
            .fetch_one(&pool)
            .await
            .unwrap()
        }
    };
    let before = attempts(ann).await;
    run_send_job(&f.state(), id, TimestampMillis::from_millis(now + 120_000))
        .await
        .unwrap();
    assert_eq!(attempts(ann).await, before);
    assert_eq!(f.state_of(ann).await.0, "delivered");

    // Orbit's own attempt is in flight for bob: the provider's delivery applies, and the
    // attempt's late result changes nothing
    let attempt = Id::new_v7();
    sqlx::query(
        "UPDATE mail_outbound_recipients SET status = 'sending', attempt_id = $1 WHERE id = $2",
    )
    .bind(attempt.to_string())
    .bind(bob.to_string())
    .execute(f.database.pool())
    .await
    .unwrap();
    let (status, body) = f
        .mailgun_post(f.route, MAILGUN_KEY, &delivered("bob@out.test"))
        .await;
    assert_eq!(
        (status, body["applied"].clone()),
        (StatusCode::OK, json!(1))
    );
    assert_eq!(f.state_of(bob).await.0, "delivered");
    f.mail
        .record_outcomes(
            id,
            attempt,
            &[(
                bob,
                AttemptResult::Deferred {
                    response: "451 4.4.1 Timed out".to_owned(),
                    cause: DeferCause::Temporary,
                },
            )],
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    let (state, _, response, next) = f.state_of(bob).await;
    assert_eq!((state.as_str(), response, next), ("delivered", None, None));
}

/// Defers `address` of the message by a provider event now (`deferred`, no next attempt).
async fn provider_defer(f: &Fixture, address: &str, message_id: &str) {
    tokio::time::sleep(Duration::from_millis(5)).await;
    let event = mailgun_failed(
        address,
        message_id,
        "temporary",
        "4.2.2 Mailbox full",
        now_secs(),
    );
    let (status, body) = f.mailgun_post(f.route, MAILGUN_KEY, &event).await;
    assert_eq!(
        (status, body["applied"].clone()),
        (StatusCode::OK, json!(1))
    );
}

#[tokio::test]
async fn provider_deferred_rows_are_left_to_the_provider() {
    let f = Fixture::mailgun().await;
    f.answer_id("ann@out.test", "<left.1@one.test>");
    f.answer_id("bob@out.test", "<left.1@one.test>");
    let id = f.send(&["ann@out.test", "bob@out.test"]).await;
    let ann = f.recipient(id, "ann@out.test").await;
    let bob = f.recipient(id, "bob@out.test").await;
    provider_defer(&f, "ann@out.test", "left.1@one.test").await;

    // the message runs again (bob queued) while ann's address is on the list: ann is neither
    // suppressed nor sent again
    sqlx::query(
        "INSERT INTO mail_suppressions (workspace_id, address, reason, created_at) \
         VALUES ($1, 'ann@out.test', 'manual', 0)",
    )
    .bind(f.workspace.to_string())
    .execute(f.database.pool())
    .await
    .unwrap();
    let requeue = |bob: Id| {
        let pool = f.database.pool().clone();
        async move {
            sqlx::query("UPDATE mail_outbound_recipients SET status = 'queued' WHERE id = $1")
                .bind(bob.to_string())
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query("UPDATE mail_outbound SET status = 'released' WHERE id = $1")
                .bind(id.to_string())
                .execute(&pool)
                .await
                .unwrap();
        }
    };
    requeue(bob).await;
    run_send_job(&f.state(), id, TimestampMillis::now())
        .await
        .unwrap();
    let (state, _, response, next) = f.state_of(ann).await;
    assert_eq!(
        (state.as_str(), response.as_deref(), next),
        ("deferred", Some("550 4.2.2 Mailbox full"), None)
    );
    assert_eq!(f.state_of(bob).await.0, "delivered");
    // the provider-deferred row does not hold the message open
    assert_eq!(f.outbound_status(id).await.0, "done");

    // the sent copy is gone: Orbit cancels what it still has to send, not ann
    requeue(bob).await;
    sqlx::query("UPDATE mail_outbound SET message_id = NULL WHERE id = $1")
        .bind(id.to_string())
        .execute(f.database.pool())
        .await
        .unwrap();
    run_send_job(&f.state(), id, TimestampMillis::now())
        .await
        .unwrap();
    assert_eq!(f.state_of(bob).await.0, "cancelled");
    assert_eq!(f.state_of(ann).await.0, "deferred");
}

#[tokio::test]
async fn provider_deferred_rows_give_up_after_five_days() {
    let f = Fixture::mailgun().await;
    f.answer_id("ann@out.test", "<late.1@one.test>");
    f.answer_id("bob@out.test", "<late.1@one.test>");
    let id = f.send(&["ann@out.test", "bob@out.test"]).await;
    let ann = f.recipient(id, "ann@out.test").await;
    let bob = f.recipient(id, "bob@out.test").await;
    provider_defer(&f, "ann@out.test", "late.1@one.test").await;
    provider_defer(&f, "bob@out.test", "late.1@one.test").await;
    let now = TimestampMillis::now().as_millis();
    // ann went 5 days and a minute ago, bob 4 days ago
    for (recipient, ago) in [
        (ann, PROVIDER_DEFER_GIVE_UP_MS + 60_000),
        (bob, 4 * 24 * 3_600_000),
    ] {
        sqlx::query("UPDATE mail_outbound_recipients SET released_at = $1 WHERE id = $2")
            .bind(now - ago)
            .bind(recipient.to_string())
            .execute(f.database.pool())
            .await
            .unwrap();
    }
    f.mail
        .sweep_deliveries(TimestampMillis::now())
        .await
        .unwrap();
    let (state, kind, response, _) = f.state_of(ann).await;
    assert_eq!(
        (state.as_str(), kind.as_deref()),
        ("bounced", Some("gave_up"))
    );
    assert_eq!(
        response.as_deref(),
        Some("Gave up after 5 days: 550 4.2.2 Mailbox full")
    );
    assert_eq!(f.suppressed(f.workspace, "ann@out.test").await, None);
    assert_eq!(f.thread_folder(id).await, "inbox");
    assert_eq!(f.state_of(bob).await.0, "deferred");
    let (_, summary) = f.outbound_status(id).await;
    assert!(summary.unwrap().contains("gave_up"));
    // no run is queued for either
    assert_eq!(
        f.count(
            "SELECT CAST(COUNT(*) AS BIGINT) FROM jobs WHERE kind = 'mail.send'",
            &[]
        )
        .await,
        0
    );
}

#[tokio::test]
async fn complaint_event_suppresses() {
    let f = Fixture::new(
        SendRouteKind::Postmark,
        RouteSecrets::Postmark {
            server_token: "server-token".to_owned(),
            webhook_user: "orbit".to_owned(),
            webhook_password: "hook-password".to_owned(),
        },
    )
    .await;
    let id = f.send(&["ann@out.test", "bob@out.test"]).await;
    let ann = f.recipient(id, "ann@out.test").await;
    // Postmark's spam complaint webhook, with Orbit's metadata (the outbound's ID: one call for
    // all recipients)
    let body = json!({
        "RecordType": "SpamComplaint",
        "MessageStream": "outbound",
        "ID": 42,
        "Type": "SpamComplaint",
        "TypeCode": 100_001,
        "MessageID": "00000000-0000-0000-0000-000000000000",
        "Metadata": { "orbit": id.to_string() },
        "Email": "Ann@out.test",
        "BouncedAt": "2026-10-27T08:52:00Z",
    })
    .to_string();
    let auth = |password: &str| {
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!("orbit:{password}"))
        )
    };
    let (status, _) = f
        .post(
            "postmark",
            f.route,
            &[("authorization", auth("guess"))],
            body.clone(),
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(f.suppressed(f.workspace, "ann@out.test").await, None);

    let (status, body) = f
        .post(
            "postmark",
            f.route,
            &[("authorization", auth("hook-password"))],
            body,
        )
        .await;
    assert_eq!(
        (status, body["applied"].clone()),
        (StatusCode::OK, json!(1))
    );
    assert_eq!(
        f.suppressed(f.workspace, "ann@out.test").await,
        Some(("complaint".to_owned(), Some(ann.to_string())))
    );
    // the recipient stays as it is
    assert_eq!(f.state_of(ann).await.0, "delivered");
    assert_eq!(f.suppressed(f.workspace, "bob@out.test").await, None);
}

#[tokio::test]
async fn unknown_message_ok_200() {
    let f = Fixture::mailgun().await;
    f.answer_id("ann@out.test", "<known.1@one.test>");
    let id = f.send(&["ann@out.test"]).await;
    let ann = f.recipient(id, "ann@out.test").await;
    for event in [
        // another message
        mailgun_failed(
            "ann@out.test",
            "unknown.9@one.test",
            "permanent",
            "5.1.1 x",
            now_secs(),
        ),
        // the message, another address
        mailgun_failed(
            "zed@out.test",
            "known.1@one.test",
            "permanent",
            "5.1.1 x",
            now_secs(),
        ),
    ] {
        let (status, body) = f.mailgun_post(f.route, MAILGUN_KEY, &event).await;
        assert_eq!(
            (status, body["ignored"].clone()),
            (StatusCode::OK, json!(1))
        );
    }
    // an event type Orbit does not use
    let opened = json!({ "event": "opened", "recipient": "ann@out.test", "timestamp": now_secs() });
    let (status, body) = f.mailgun_post(f.route, MAILGUN_KEY, &opened).await;
    assert_eq!(
        (status, body["applied"].clone()),
        (StatusCode::OK, json!(0))
    );
    assert_eq!(f.state_of(ann).await.0, "delivered");

    // a bad signature, no such route, a route of another kind
    let event = mailgun_failed(
        "ann@out.test",
        "known.1@one.test",
        "permanent",
        "5.1.1 x",
        now_secs(),
    );
    let (status, _) = f.mailgun_post(f.route, "another-key", &event).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = f.mailgun_post(Id::new_v7(), MAILGUN_KEY, &event).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = f
        .post("postmark", f.route, &[], mailgun_event(MAILGUN_KEY, &event))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = f
        .post("sendgrid", f.route, &[], mailgun_event(MAILGUN_KEY, &event))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(f.state_of(ann).await.0, "delivered");
    assert_eq!(f.suppressed(f.workspace, "ann@out.test").await, None);

    // a bounce of a recipient still being sent: the provider sends it again later
    sqlx::query("UPDATE mail_outbound_recipients SET status = 'sending' WHERE id = $1")
        .bind(ann.to_string())
        .execute(f.database.pool())
        .await
        .unwrap();
    let (status, _) = f.mailgun_post(f.route, MAILGUN_KEY, &event).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(f.state_of(ann).await.0, "sending");
}

#[tokio::test]
async fn event_for_other_workspace_route_ignored() {
    let f = Fixture::mailgun().await;
    f.answer_id("ann@out.test", "<shared.1@one.test>");
    let id = f.send(&["ann@out.test"]).await;
    let ann = f.recipient(id, "ann@out.test").await;

    // another workspace's own Mailgun route, with its own signing key
    InstanceSettingsRepository::new((*f.database).clone())
        .set_mail_workspace_providers_enabled(f.user, true, "request", TimestampMillis::now())
        .await
        .unwrap();
    let other = WorkspaceRepository::new((*f.database).clone())
        .create(
            f.user,
            "Second".to_owned(),
            "request",
            TimestampMillis::now(),
        )
        .await
        .unwrap()
        .id;
    let theirs = f
        .mail
        .create_route(
            ProviderScope::Workspace(other),
            f.user,
            NewRoute {
                kind: SendRouteKind::Mailgun,
                name: "Theirs".to_owned(),
                provider_id: None,
                secrets: RouteSecrets::Mailgun {
                    api_key: "key-theirs".to_owned(),
                    webhook_signing_key: "their-signing-key".to_owned(),
                },
                settings: RouteSettings::default(),
            },
            None,
            "request",
        )
        .await
        .unwrap();
    // a valid event of their route, naming our message (by its ID and by its correlation)
    let mut event = mailgun_failed(
        "ann@out.test",
        "shared.1@one.test",
        "permanent",
        "5.1.1 x",
        now_secs(),
    );
    event["user-variables"] = json!({ "orbit": id.to_string() });
    let (status, body) = f.mailgun_post(theirs.id, "their-signing-key", &event).await;
    assert_eq!(
        (status, body["ignored"].clone()),
        (StatusCode::OK, json!(1))
    );
    assert_eq!(f.state_of(ann).await.0, "delivered");
    assert_eq!(f.suppressed(f.workspace, "ann@out.test").await, None);
    assert_eq!(f.suppressed(other, "ann@out.test").await, None);
    // our route's key on their URL is refused
    let (status, _) = f.mailgun_post(theirs.id, MAILGUN_KEY, &event).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn event_older_than_a_retry_is_ignored() {
    let f = Fixture::mailgun().await;
    f.answer_id("ann@out.test", "<retry.1@one.test>");
    let id = f.send(&["ann@out.test"]).await;
    let ann = f.recipient(id, "ann@out.test").await;
    let before = now_secs() - 60.0;
    let (status, _) = f
        .mailgun_post(
            f.route,
            MAILGUN_KEY,
            &mailgun_failed(
                "ann@out.test",
                "retry.1@one.test",
                "permanent",
                "5.1.1 x",
                before,
            ),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(f.state_of(ann).await.0, "bounced");

    // Retry: the address leaves the list and is sent again with the same provider IDs
    f.mail
        .retry_recipient(
            f.workspace,
            f.membership,
            id,
            ann,
            "request",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    run_send_job(&f.state(), id, TimestampMillis::now())
        .await
        .unwrap();
    assert_eq!(f.state_of(ann).await.0, "delivered");

    // the provider sends the old bounce again: it is about the send before the retry
    let (status, body) = f
        .mailgun_post(
            f.route,
            MAILGUN_KEY,
            &mailgun_failed(
                "ann@out.test",
                "retry.1@one.test",
                "permanent",
                "5.1.1 x",
                before,
            ),
        )
        .await;
    assert_eq!(
        (status, body["ignored"].clone()),
        (StatusCode::OK, json!(1))
    );
    assert_eq!(f.state_of(ann).await.0, "delivered");
    assert_eq!(f.suppressed(f.workspace, "ann@out.test").await, None);

    // a bounce after the retry applies
    let (status, body) = f
        .mailgun_post(
            f.route,
            MAILGUN_KEY,
            &mailgun_failed(
                "ann@out.test",
                "retry.1@one.test",
                "permanent",
                "5.1.1 x",
                now_secs() + 1.0,
            ),
        )
        .await;
    assert_eq!(
        (status, body["applied"].clone()),
        (StatusCode::OK, json!(1))
    );
    assert_eq!(f.state_of(ann).await.0, "bounced");
}

#[tokio::test]
async fn resend_event_matches_by_message_id_header() {
    const SVIX_KEY: &[u8] = b"resend-svix-key-0123456789abcdef";
    let secret = format!(
        "whsec_{}",
        base64::engine::general_purpose::STANDARD.encode(SVIX_KEY)
    );
    let f = Fixture::new(
        SendRouteKind::Resend,
        RouteSecrets::Resend {
            api_key: "re_key".to_owned(),
            webhook_secret: secret,
        },
    )
    .await;
    let id = f.send(&["ann@out.test"]).await;
    let ann = f.recipient(id, "ann@out.test").await;
    let message_id: String = sqlx::query_scalar(
        "SELECT m.message_id_header FROM mail_outbound o JOIN mail_messages m ON m.id = o.message_id \
         WHERE o.id = $1",
    )
    .bind(id.to_string())
    .fetch_one(f.database.pool())
    .await
    .unwrap();
    // Resend over SMTP has no tags: its event names the email and the Message-ID header
    let body = json!({
        "type": "email.bounced",
        "created_at": "2099-01-01T00:00:00.000Z",
        "data": {
            "email_id": "56761188-7520-42d8-8898-ff6fc54ce618",
            "message_id": format!("<{message_id}>"),
            "from": "team@one.test",
            "to": ["ann@out.test"],
            "bounce": {
                "diagnosticCode": ["smtp; 550 5.1.1 user unknown"],
                "message": "The recipient does not exist.",
                "subType": "General",
                "type": "Permanent"
            }
        }
    })
    .to_string();
    let timestamp = TimestampMillis::now().as_millis() / 1000;
    let mut mac = Hmac::<Sha256>::new_from_slice(SVIX_KEY).unwrap();
    mac.update(format!("msg_1.{timestamp}.{body}").as_bytes());
    let signature = format!(
        "v1,{}",
        base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes())
    );
    let (status, reply) = f
        .post(
            "resend",
            f.route,
            &[
                ("svix-id", "msg_1".to_owned()),
                ("svix-timestamp", timestamp.to_string()),
                ("svix-signature", signature),
            ],
            body,
        )
        .await;
    assert_eq!(
        (status, reply["applied"].clone()),
        (StatusCode::OK, json!(1))
    );
    let (state, kind, response, _) = f.state_of(ann).await;
    assert_eq!(
        (state.as_str(), kind.as_deref(), response.as_deref()),
        ("bounced", Some("hard"), Some("550 5.1.1 user unknown"))
    );
    assert_eq!(
        f.suppressed(f.workspace, "ann@out.test")
            .await
            .map(|entry| entry.0),
        Some("hard_bounce".to_owned())
    );
    // unsigned: refused
    let (status, _) = f.post("resend", f.route, &[], "{}".to_owned()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn ses_route_needs_its_topic_and_refuses_other_topics() {
    let f = Fixture::new(
        SendRouteKind::Ses,
        RouteSecrets::Ses {
            access_key_id: "AKIA".to_owned(),
            secret_access_key: "secret".to_owned(),
        },
    )
    .await;
    // without a topic on the route, every SES event is refused before its certificate is fetched
    let body = json!({
        "Type": "Notification", "Message": "{}", "MessageId": "m", "Timestamp": "2026-10-27T08:52:00Z",
        "TopicArn": "arn:aws:sns:us-east-1:123456789012:x", "Signature": "AA==",
        "SignatureVersion": "2",
        "SigningCertURL": "https://sns.us-east-1.amazonaws.com/SimpleNotificationService-abc.pem"
    })
    .to_string();
    let (status, _) = f.post("ses", f.route, &[], body.clone()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // the topic must be an SNS ARN
    let error = f
        .mail
        .update_route(
            ProviderScope::Instance,
            f.user,
            f.route,
            RouteChange {
                settings: Some(RouteSettings {
                    region: Some("us-east-1".to_owned()),
                    topic_arn: Some("sns-topic".to_owned()),
                    ..RouteSettings::default()
                }),
                ..RouteChange::default()
            },
            None,
            "request",
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("topic_arn"), "{error}");
    // every AWS partition
    for (arn, ok) in [
        ("arn:aws-cn:sns:cn-north-1:123456789012:t", true),
        ("arn:aws-us-gov:sns:us-gov-west-1:123456789012:t", true),
        ("arn:aws-xx:sns:us-east-1:123456789012:t", false),
        ("arn:aws:sqs:us-east-1:123456789012:t", false),
    ] {
        let result = f
            .mail
            .update_route(
                ProviderScope::Instance,
                f.user,
                f.route,
                RouteChange {
                    settings: Some(RouteSettings {
                        region: Some("us-east-1".to_owned()),
                        topic_arn: Some(arn.to_owned()),
                        ..RouteSettings::default()
                    }),
                    ..RouteChange::default()
                },
                None,
                "request",
            )
            .await;
        assert_eq!(result.is_ok(), ok, "{arn}");
    }
    f.mail
        .update_route(
            ProviderScope::Instance,
            f.user,
            f.route,
            RouteChange {
                settings: Some(RouteSettings {
                    region: Some("us-east-1".to_owned()),
                    topic_arn: Some(" arn:aws:sns:us-east-1:123456789012:orbit-events ".to_owned()),
                    ..RouteSettings::default()
                }),
                ..RouteChange::default()
            },
            None,
            "request",
        )
        .await
        .unwrap();
    // with the route's topic set, a notification of another topic is refused before anything is
    // fetched; this state has no intake, so a fetch answers 503
    let (status, _) = f.post("ses", f.route, &[], body.clone()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = f
        .post("ses", f.route, &[], body.replace(":x\"", ":orbit-events\""))
        .await;
    assert_eq!(
        status,
        StatusCode::SERVICE_UNAVAILABLE,
        "the certificate fetch needs intake"
    );
}
