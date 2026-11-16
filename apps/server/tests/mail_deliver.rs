//! Delivery of released messages (spec §5.3 steps 5–7, §5.4): retries, local delivery, the
//! suppression list, limits and pauses, envelopes, and live status.

#![allow(dead_code)]
use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

use futures_util::future::BoxFuture;
use orbit_platform::{
    AttachmentMutationCoordinator, Id, LocalBlobStore, PasswordService, TestDatabase,
    TimestampMillis, UploadLimits, UploadService,
};
use orbit_server::live::LiveHub;
use orbit_server::mail_dns::FakeDns;
use orbit_server::mail_intake::MailIntake;
use orbit_server::mail_send::{
    Delivery, DeliveryContext, NextRun, Outgoing, RETRY_STEPS_MS, RecipientOutcome,
    SENDING_LEASE_MS, SendJobState, deliver_due, run_send_job,
};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::instance_settings::InstanceSettingsRepository;
use orbit_server::repositories::mail::{
    DraftMode, IngestOutcome, MailAuthResult, MailRepository, MailboxKind, NewDraft, NewMailbox,
    NewRoute, ProviderScope, RouteSecrets, RouteSettings, SendRouteKind,
};
use orbit_server::repositories::workspaces::WorkspaceRepository;
use serde_json::{Value, json};
use sqlx::Row;

const APP_KEY: [u8; 32] = [32; 32];
const HOUR: i64 = 3_600_000;

/// What one adapter call got.
#[derive(Clone, Debug)]
struct Call {
    raw: String,
    mail_from: String,
    recipients: Vec<String>,
    correlation: String,
    from_domain: String,
}

/// An adapter that records its calls and answers from a table (accepted by default).
#[derive(Default)]
struct Fake {
    calls: Mutex<Vec<Call>>,
    outcomes: Mutex<HashMap<String, RecipientOutcome>>,
}

impl Fake {
    fn answer(&self, address: &str, outcome: RecipientOutcome) {
        self.outcomes
            .lock()
            .unwrap()
            .insert(address.to_owned(), outcome);
    }

    fn calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap().clone()
    }

    fn recipients(&self) -> BTreeSet<String> {
        self.calls()
            .into_iter()
            .flat_map(|call| call.recipients)
            .collect()
    }
}

impl Delivery for Fake {
    fn deliver<'a>(
        &'a self,
        outgoing: &'a Outgoing<'a>,
    ) -> BoxFuture<'a, Vec<(String, RecipientOutcome)>> {
        Box::pin(async move {
            self.calls.lock().unwrap().push(Call {
                raw: String::from_utf8_lossy(outgoing.raw).into_owned(),
                mail_from: outgoing.mail_from.to_owned(),
                recipients: outgoing.recipients.to_vec(),
                correlation: outgoing.correlation.to_owned(),
                from_domain: outgoing.from_domain.to_owned(),
            });
            let table = self.outcomes.lock().unwrap();
            outgoing
                .recipients
                .iter()
                .map(|address| {
                    (
                        address.clone(),
                        table
                            .get(address)
                            .cloned()
                            .unwrap_or(RecipientOutcome::Accepted {
                                provider_message_id: Some(format!("pm-{address}")),
                            }),
                    )
                })
                .collect()
        })
    }
}

fn deferred(response: &str) -> RecipientOutcome {
    RecipientOutcome::Deferred {
        response: response.to_owned(),
    }
}

fn at(millis: i64) -> TimestampMillis {
    TimestampMillis::from_millis(millis)
}

struct Fixture {
    _root: tempfile::TempDir,
    database: TestDatabase,
    mail: Arc<MailRepository>,
    intake: MailIntake,
    workspace: Id,
    user: Id,
    membership: Id,
    domain: Id,
    route: Id,
    mailbox: Id,
    address: Id,
    /// The adapter of direct sending and workspace routes (`allow_private = false`).
    fake: Arc<Fake>,
    /// The adapter of instance routes other than direct (`allow_private = true`).
    instance_fake: Arc<Fake>,
}

impl Fixture {
    async fn new() -> Self {
        let database = TestDatabase::new().await.unwrap();
        let identity = IdentityRepository::new((*database).clone());
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
        .bind(report("ok"))
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
            mail,
            intake,
            workspace: setup.workspace_id,
            user: setup.user_id,
            membership,
            domain,
            route: route.id,
            mailbox: mailbox.id,
            address: mailbox.addresses[0].id,
            fake: Arc::default(),
            instance_fake: Arc::default(),
        }
    }

    fn state(&self) -> SendJobState {
        SendJobState {
            mail: Arc::clone(&self.mail),
            delivery: DeliveryContext::new(
                Some("mx.orbit.test".to_owned()),
                Arc::new(FakeDns::new()),
                false,
            )
            .with_adapter(Arc::clone(&self.fake) as Arc<dyn Delivery>),
            instance_delivery: DeliveryContext::new(
                Some("mx.orbit.test".to_owned()),
                Arc::new(FakeDns::new()),
                true,
            )
            .with_adapter(Arc::clone(&self.instance_fake) as Arc<dyn Delivery>),
            intake: self.intake.clone(),
            live: LiveHub::of(&self.database),
        }
    }

    async fn sql(&self, sql: &str, binds: &[&str]) {
        let mut query = sqlx::query(sqlx::AssertSqlSafe(sql.to_owned()));
        for bind in binds {
            query = query.bind(*bind);
        }
        query.execute(self.database.pool()).await.unwrap();
    }

    /// Sends a message from team@one.test at `now` (no undo time, so it is due at once) and
    /// clears the queued jobs, so a test sees only the runs delivery queues. Returns its id and
    /// release time.
    async fn send_at(
        &self,
        to: &[&str],
        cc: &[&str],
        bcc: &[&str],
        now: TimestampMillis,
    ) -> (Id, i64) {
        let list = |items: &[&str]| Some(items.iter().map(|item| (*item).to_owned()).collect());
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
                    to: list(to),
                    cc: list(cc),
                    bcc: list(bcc),
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
        self.sql("DELETE FROM jobs WHERE kind = 'mail.send'", &[])
            .await;
        (outbound.id, outbound.release_at.as_millis())
    }

    async fn send(&self, to: &[&str], cc: &[&str], bcc: &[&str]) -> (Id, i64) {
        self.send_at(to, cc, bcc, TimestampMillis::now()).await
    }

    async fn run(&self, id: Id, now: i64) {
        run_send_job(&self.state(), id, at(now)).await.unwrap();
    }

    /// (address, status, attempts, next_attempt_at, last_response, provider_message_id).
    async fn recipients(
        &self,
        id: Id,
    ) -> Vec<(
        String,
        String,
        i64,
        Option<i64>,
        Option<String>,
        Option<String>,
    )> {
        let rows = sqlx::query(
            "SELECT address, status, attempts, next_attempt_at, last_response, \
             provider_message_id FROM mail_outbound_recipients WHERE outbound_id = $1 \
             ORDER BY address",
        )
        .bind(id.to_string())
        .fetch_all(self.database.pool())
        .await
        .unwrap();
        rows.iter()
            .map(|row| {
                (
                    row.get("address"),
                    row.get("status"),
                    row.get("attempts"),
                    row.get("next_attempt_at"),
                    row.get("last_response"),
                    row.get("provider_message_id"),
                )
            })
            .collect()
    }

    async fn status_of(&self, id: Id, address: &str) -> String {
        sqlx::query_scalar(
            "SELECT status FROM mail_outbound_recipients WHERE outbound_id = $1 AND address = $2",
        )
        .bind(id.to_string())
        .bind(address)
        .fetch_one(self.database.pool())
        .await
        .unwrap()
    }

    /// (status, hold_reason, delivery_summary).
    async fn outbound(&self, id: Id) -> (String, Option<String>, Option<String>) {
        let row = sqlx::query(
            "SELECT status, hold_reason, delivery_summary FROM mail_outbound WHERE id = $1",
        )
        .bind(id.to_string())
        .fetch_one(self.database.pool())
        .await
        .unwrap();
        (
            row.get("status"),
            row.get("hold_reason"),
            row.get("delivery_summary"),
        )
    }

    /// The times of the queued `mail.send` runs of the message.
    async fn runs(&self, id: Id) -> Vec<i64> {
        sqlx::query_scalar(
            "SELECT available_at FROM jobs WHERE kind = 'mail.send' AND state = 'queued' \
             AND payload_json = $1 ORDER BY available_at",
        )
        .bind(json!({"outbound_id": id.to_string()}).to_string())
        .fetch_all(self.database.pool())
        .await
        .unwrap()
    }

    async fn clear_runs(&self) {
        self.sql("DELETE FROM jobs WHERE kind = 'mail.send'", &[])
            .await;
    }

    /// The thread of the message's sent copy.
    async fn thread_of(&self, id: Id) -> String {
        sqlx::query_scalar(
            "SELECT tm.thread_id FROM mail_outbound o \
             JOIN mail_thread_messages tm ON tm.message_id = o.message_id WHERE o.id = $1",
        )
        .bind(id.to_string())
        .fetch_one(self.database.pool())
        .await
        .unwrap()
    }

    /// (folder, read by the sender).
    async fn thread_state(&self, thread: &str) -> (String, bool) {
        let row = sqlx::query(
            "SELECT t.folder, s.last_read_at, t.last_message_at FROM mail_threads t \
             LEFT JOIN mail_thread_user_state s ON s.thread_id = t.id AND s.membership_id = $1 \
             WHERE t.id = $2",
        )
        .bind(self.membership.to_string())
        .bind(thread)
        .fetch_one(self.database.pool())
        .await
        .unwrap();
        let read: Option<i64> = row.get("last_read_at");
        let last: i64 = row.get("last_message_at");
        (row.get("folder"), read.is_some_and(|read| read >= last))
    }

    /// The `thread.updated` events of the workspace, as (recipients, event).
    async fn mail_events(&self) -> Vec<(Value, Value)> {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT recipients_json, frame FROM live_events WHERE workspace_id = $1 ORDER BY seq",
        )
        .bind(self.workspace.to_string())
        .fetch_all(self.database.pool())
        .await
        .unwrap();
        rows.into_iter()
            .map(|(recipients, frame)| {
                (
                    serde_json::from_str::<Value>(&recipients).unwrap(),
                    serde_json::from_str::<Value>(&frame).unwrap(),
                )
            })
            .filter(|(_, frame)| frame["topic"] == "mail")
            .map(|(recipients, frame)| (recipients, frame["event"].clone()))
            .collect()
    }

    /// A second workspace with a verified domain `two.test` (MX `mx`: `ok` points here) and the
    /// shared mailbox of `carl@two.test`, read by the owner. Returns (workspace, address).
    async fn second_workspace(&self, mx: &str) -> (Id, Id) {
        let now = TimestampMillis::now().as_millis();
        let second = WorkspaceRepository::new((*self.database).clone())
            .create(self.user, "Second".to_owned(), "request", at(now))
            .await
            .unwrap()
            .id;
        let membership: String = sqlx::query_scalar(
            "SELECT id FROM memberships WHERE workspace_id = $1 AND user_id = $2",
        )
        .bind(second.to_string())
        .bind(self.user.to_string())
        .fetch_one(self.database.pool())
        .await
        .unwrap();
        let domain = Id::new_v7();
        let mailbox = Id::new_v7();
        let address = Id::new_v7();
        let now = now.to_string();
        self.sql(
            &format!(
                "INSERT INTO mail_domains (id, workspace_id, name, status, verify_token, dns_report, \
                 created_at, updated_at) VALUES ($1, $2, 'two.test', 'verified', 'token', $3, {now}, {now})"
            ),
            &[&domain.to_string(), &second.to_string(), &report(mx)],
        )
        .await;
        self.sql(
            &format!(
                "INSERT INTO mailboxes (id, workspace_id, kind, name, owner_membership_id, status, \
                 created_at, updated_at) VALUES ($1, $2, 'shared', 'Carl', NULL, 'active', {now}, {now})"
            ),
            &[&mailbox.to_string(), &second.to_string()],
        )
        .await;
        self.sql(
            &format!(
                "INSERT INTO mail_addresses (id, domain_id, mailbox_id, local_part, kind, created_at) \
                 VALUES ($1, $2, $3, 'carl', 'primary', {now})"
            ),
            &[&address.to_string(), &domain.to_string(), &mailbox.to_string()],
        )
        .await;
        self.sql(
            &format!(
                "INSERT INTO mailbox_members (mailbox_id, membership_id, created_at) \
                 VALUES ($1, $2, {now})"
            ),
            &[&mailbox.to_string(), &membership],
        )
        .await;
        (second, address)
    }
}

/// A DNS report with the MX result `mx` (`ok`, or another domain's MX).
fn report(mx: &str) -> String {
    let mx = if mx == "ok" {
        json!({"status": "ok"})
    } else {
        json!({"status": "wrong", "found": mx})
    };
    json!({"verify": {"status": "ok"}, "mx": mx, "dkim": {"status": "ok"},
           "spf": {"status": "ok"}, "dmarc": {"status": "ok"}})
    .to_string()
}

#[tokio::test]
async fn all_delivered_marks_done() {
    let f = Fixture::new().await;
    let (id, t0) = f.send(&["a@out.test"], &["b@out.test"], &[]).await;
    f.run(id, t0).await;

    let recipients = f.recipients(id).await;
    assert_eq!(recipients.len(), 2);
    for (address, status, attempts, next, _, provider) in &recipients {
        assert_eq!(status, "delivered", "{address}");
        assert_eq!(*attempts, 1);
        assert_eq!(*next, None);
        assert_eq!(provider.as_deref(), Some(format!("pm-{address}").as_str()));
    }
    let (status, hold, summary) = f.outbound(id).await;
    assert_eq!(status, "done");
    assert_eq!(hold, None);
    let summary: Value = serde_json::from_str(&summary.unwrap()).unwrap();
    let mut listed: Vec<(String, String, String)> = summary
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            (
                item["address"].as_str().unwrap().to_owned(),
                item["kind"].as_str().unwrap().to_owned(),
                item["status"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    listed.sort();
    assert_eq!(
        listed,
        vec![
            (
                "a@out.test".to_owned(),
                "to".to_owned(),
                "delivered".to_owned()
            ),
            (
                "b@out.test".to_owned(),
                "cc".to_owned(),
                "delivered".to_owned()
            ),
        ]
    );
    assert!(f.runs(id).await.is_empty(), "nothing left to run");
    let last_sent: Option<i64> =
        sqlx::query_scalar("SELECT last_sent_at FROM mail_send_routes WHERE id = $1")
            .bind(f.route.to_string())
            .fetch_one(f.database.pool())
            .await
            .unwrap();
    assert_eq!(last_sent, Some(t0));

    // A later run (an old job) sends nothing again.
    f.run(id, t0 + 1000).await;
    assert_eq!(f.fake.calls().len(), 2);
}

#[tokio::test]
async fn deferred_retries_on_schedule() {
    let f = Fixture::new().await;
    f.fake
        .answer("a@out.test", deferred("451 4.7.1 Try again later"));
    let (id, t0) = f.send(&["a@out.test"], &[], &[]).await;

    f.run(id, t0).await;
    let recipient = f.recipients(id).await.remove(0);
    assert_eq!(recipient.1, "deferred");
    assert_eq!(recipient.2, 1);
    assert_eq!(recipient.3, Some(t0 + RETRY_STEPS_MS[0]));
    assert_eq!(recipient.4.as_deref(), Some("451 4.7.1 Try again later"));
    assert_eq!(f.runs(id).await, vec![t0 + 60_000]);
    assert_eq!(f.outbound(id).await.0, "released");

    // Not due yet: nothing is sent.
    f.clear_runs().await;
    f.run(id, t0 + 30_000).await;
    assert_eq!(f.fake.calls().len(), 1);
    assert_eq!(f.runs(id).await, vec![t0 + 60_000]);

    f.clear_runs().await;
    let t1 = t0 + 60_000;
    f.run(id, t1).await;
    let recipient = f.recipients(id).await.remove(0);
    assert_eq!(recipient.2, 2);
    assert_eq!(recipient.3, Some(t1 + 5 * 60_000));
    assert_eq!(f.runs(id).await, vec![t1 + 300_000]);
    assert_eq!(f.fake.calls().len(), 2);

    // A route error is deferred too, never a bounce, and the route shows it.
    f.fake.answer(
        "a@out.test",
        deferred("Route error: Postmark refused the route's credentials"),
    );
    f.clear_runs().await;
    let t2 = t1 + 300_000;
    f.run(id, t2).await;
    let recipient = f.recipients(id).await.remove(0);
    assert_eq!(recipient.1, "deferred");
    assert_eq!(recipient.3, Some(t2 + 15 * 60_000));
    let last_error: Option<String> =
        sqlx::query_scalar("SELECT last_error FROM mail_send_routes WHERE id = $1")
            .bind(f.route.to_string())
            .fetch_one(f.database.pool())
            .await
            .unwrap();
    assert_eq!(
        last_error.as_deref(),
        Some("Route error: Postmark refused the route's credentials")
    );
}

#[tokio::test]
async fn gives_up_after_last_step_and_moves_thread_to_inbox() {
    let f = Fixture::new().await;
    f.fake.answer("a@out.test", deferred("451 4.3.0 Busy"));
    let (id, t0) = f.send(&["a@out.test"], &[], &[]).await;
    f.run(id, t0).await;
    let thread = f.thread_of(id).await;
    assert_eq!(f.thread_state(&thread).await, ("archive".to_owned(), true));

    // The last step was waited for.
    let t1 = t0 + 1000;
    f.sql(
        &format!(
            "UPDATE mail_outbound_recipients SET attempts = {}, next_attempt_at = {t1} \
             WHERE outbound_id = $1",
            RETRY_STEPS_MS.len()
        ),
        &[&id.to_string()],
    )
    .await;
    f.clear_runs().await;
    f.run(id, t1).await;
    let recipient = f.recipients(id).await.remove(0);
    assert_eq!(recipient.1, "bounced");
    assert_eq!(recipient.2, 13);
    assert!(
        recipient
            .4
            .as_deref()
            .unwrap()
            .starts_with("Gave up after 5 days"),
        "{recipient:?}"
    );
    assert_eq!(f.outbound(id).await.0, "done");
    assert!(f.runs(id).await.is_empty());
    assert_eq!(f.thread_state(&thread).await, ("inbox".to_owned(), false));
}

#[tokio::test]
async fn smtp_bounce_moves_thread_to_inbox() {
    let f = Fixture::new().await;
    f.fake.answer(
        "gone@out.test",
        RecipientOutcome::Bounced {
            response: "permanent error (550): 5.1.1 No such user".to_owned(),
        },
    );
    let (id, t0) = f.send(&["gone@out.test", "a@out.test"], &[], &[]).await;
    f.run(id, t0).await;
    assert_eq!(f.status_of(id, "gone@out.test").await, "bounced");
    assert_eq!(f.status_of(id, "a@out.test").await, "delivered");
    let (status, _, summary) = f.outbound(id).await;
    assert_eq!(status, "done");
    assert!(summary.unwrap().contains("5.1.1 No such user"));
    let thread = f.thread_of(id).await;
    assert_eq!(f.thread_state(&thread).await, ("inbox".to_owned(), false));
}

#[tokio::test]
async fn delayed_flag_after_4h() {
    let f = Fixture::new().await;
    f.fake.answer("a@out.test", deferred("421 4.4.2 Timeout"));
    let (id, t0) = f.send(&["a@out.test"], &[], &[]).await;
    f.run(id, t0).await;
    let soon = f.mail.outbound_recipients(id, at(t0 + HOUR)).await.unwrap();
    assert_eq!(soon[0].status, "deferred");
    assert!(!soon[0].delayed);
    let late = f
        .mail
        .outbound_recipients(id, at(t0 + 4 * HOUR + 1))
        .await
        .unwrap();
    assert!(late[0].delayed);
    assert_eq!(late[0].attempts, 1);
    assert_eq!(late[0].next_attempt_at, Some(at(t0 + 60_000)));
}

#[tokio::test]
async fn stale_sending_requeued_after_lease() {
    let f = Fixture::new().await;
    let (id, t0) = f.send(&["a@out.test"], &[], &[]).await;
    f.mail
        .release(id, Some("mx.orbit.test"), at(t0))
        .await
        .unwrap()
        .expect("released");
    // A worker claimed it and stopped before the result.
    f.sql(
        &format!(
            "UPDATE mail_outbound_recipients SET status = 'sending', attempts = 1, \
             attempt_id = '{}', updated_at = {t0} WHERE outbound_id = $1",
            Id::new_v7()
        ),
        &[&id.to_string()],
    )
    .await;

    // Inside the lease: not sent, the next run is when the lease ends.
    let next = deliver_due(&f.state(), id, at(t0 + 1000)).await.unwrap();
    assert_eq!(next, NextRun::At(at(t0 + SENDING_LEASE_MS)));
    assert!(f.fake.calls().is_empty());
    assert_eq!(
        f.mail
            .requeue_stale_sending(SENDING_LEASE_MS, at(t0 + SENDING_LEASE_MS - 1))
            .await
            .unwrap(),
        0
    );

    let t1 = t0 + SENDING_LEASE_MS;
    assert_eq!(
        f.mail
            .requeue_stale_sending(SENDING_LEASE_MS, at(t1))
            .await
            .unwrap(),
        1
    );
    assert_eq!(f.status_of(id, "a@out.test").await, "deferred");
    let next = deliver_due(&f.state(), id, at(t1)).await.unwrap();
    assert_eq!(next, NextRun::Done);
    assert_eq!(f.status_of(id, "a@out.test").await, "delivered");
    assert_eq!(f.recipients(id).await[0].2, 2, "sent again: at least once");
}

#[tokio::test]
async fn local_recipient_goes_to_intake() {
    let f = Fixture::new().await;
    let (second, _) = f.second_workspace("ok").await;
    let (id, t0) = f
        .send(&["carl@two.test", "x@out.test"], &["nobody@two.test"], &[])
        .await;
    f.run(id, t0).await;

    assert_eq!(
        f.fake.recipients(),
        BTreeSet::from(["x@out.test".to_owned()]),
        "only the outside recipient goes by the route"
    );
    assert_eq!(f.status_of(id, "carl@two.test").await, "delivered");
    assert_eq!(f.status_of(id, "x@out.test").await, "delivered");
    let unknown = f
        .recipients(id)
        .await
        .into_iter()
        .find(|row| row.0 == "nobody@two.test")
        .unwrap();
    assert_eq!(unknown.1, "bounced");
    assert!(unknown.4.unwrap().contains("5.1.1"));

    let row = sqlx::query(
        "SELECT id, source, auth, raw_sha256, mail_from FROM mail_receipts WHERE workspace_id = $1",
    )
    .bind(second.to_string())
    .fetch_one(f.database.pool())
    .await
    .unwrap();
    assert_eq!(row.get::<String, _>("source"), "local");
    assert_eq!(row.get::<String, _>("auth"), r#"{"local":true}"#);
    assert_eq!(
        row.get::<Option<String>, _>("mail_from").as_deref(),
        Some("team@one.test")
    );
    let sent_sha: String = sqlx::query_scalar(
        "SELECT r.raw_sha256 FROM mail_outbound o JOIN mail_messages m ON m.id = o.message_id \
         JOIN mail_receipts r ON r.id = m.receipt_id WHERE o.id = $1",
    )
    .bind(id.to_string())
    .fetch_one(f.database.pool())
    .await
    .unwrap();
    assert_eq!(
        row.get::<String, _>("raw_sha256"),
        sent_sha,
        "the same bytes as the sent copy"
    );

    // Ingested, the message shows DKIM as passed.
    let receipt: Id = row.get::<String, _>("id").parse().unwrap();
    let IngestOutcome::Ingested { message_id, .. } = f
        .mail
        .ingest(receipt, TimestampMillis::now())
        .await
        .unwrap()
    else {
        panic!("not ingested");
    };
    let thread: String =
        sqlx::query_scalar("SELECT thread_id FROM mail_thread_messages WHERE message_id = $1")
            .bind(message_id.to_string())
            .fetch_one(f.database.pool())
            .await
            .unwrap();
    let detail = f
        .mail
        .get_thread(second, f.user, thread.parse().unwrap())
        .await
        .unwrap();
    assert_eq!(detail.messages[0].auth.dkim, MailAuthResult::Pass);
}

#[tokio::test]
async fn local_copy_to_own_mailbox_dedupes() {
    let f = Fixture::new().await;
    let (id, t0) = f.send(&["team@one.test"], &[], &[]).await;
    f.run(id, t0).await;
    assert!(f.fake.calls().is_empty());
    assert_eq!(f.status_of(id, "team@one.test").await, "delivered");
    let receipt: String = sqlx::query_scalar("SELECT id FROM mail_receipts WHERE source = 'local'")
        .fetch_one(f.database.pool())
        .await
        .unwrap();
    f.mail
        .ingest(receipt.parse().unwrap(), TimestampMillis::now())
        .await
        .unwrap();
    let copies: i64 = sqlx::query_scalar(
        "SELECT CAST(COUNT(*) AS BIGINT) FROM mail_thread_messages WHERE mailbox_id = $1",
    )
    .bind(f.mailbox.to_string())
    .fetch_one(f.database.pool())
    .await
    .unwrap();
    assert_eq!(copies, 1, "the sent copy only");
}

#[tokio::test]
async fn verified_domain_with_mx_elsewhere_goes_by_route() {
    let f = Fixture::new().await;
    let (second, _) = f.second_workspace("mx.elsewhere.test").await;
    let (id, t0) = f.send(&["carl@two.test"], &[], &[]).await;
    f.run(id, t0).await;
    assert_eq!(
        f.fake.recipients(),
        BTreeSet::from(["carl@two.test".to_owned()])
    );
    assert_eq!(f.status_of(id, "carl@two.test").await, "delivered");
    let receipts: i64 = sqlx::query_scalar(
        "SELECT CAST(COUNT(*) AS BIGINT) FROM mail_receipts WHERE workspace_id = $1",
    )
    .bind(second.to_string())
    .fetch_one(f.database.pool())
    .await
    .unwrap();
    assert_eq!(receipts, 0);
}

#[tokio::test]
async fn suppressed_recipient_skipped() {
    let f = Fixture::new().await;
    f.sql(
        "INSERT INTO mail_suppressions (workspace_id, address, reason, created_at) \
         VALUES ($1, 'gone@out.test', 'hard_bounce', 0)",
        &[&f.workspace.to_string()],
    )
    .await;
    let (id, t0) = f.send(&["Gone@out.test", "a@out.test"], &[], &[]).await;
    f.run(id, t0).await;
    assert_eq!(
        f.fake.recipients(),
        BTreeSet::from(["a@out.test".to_owned()])
    );
    let gone = f
        .recipients(id)
        .await
        .into_iter()
        .find(|row| row.0.eq_ignore_ascii_case("gone@out.test"))
        .unwrap();
    assert_eq!(gone.1, "suppressed");
    assert_eq!(gone.2, 0, "never tried");
    assert_eq!(f.outbound(id).await.0, "done");

    // Once the entry is removed, a new message to the address goes.
    f.sql("DELETE FROM mail_suppressions", &[]).await;
    let (id, t1) = f.send(&["gone@out.test"], &[], &[]).await;
    f.run(id, t1).await;
    assert_eq!(f.status_of(id, "gone@out.test").await, "delivered");
}

#[tokio::test]
async fn limit_hold_then_release() {
    let f = Fixture::new().await;
    f.sql(
        "INSERT INTO mail_send_policies (workspace_id, tier, custom_limits, created_at, updated_at) \
         VALUES ($1, 'custom', $2, 0, 0)",
        &[
            &f.workspace.to_string(),
            &json!({"per_user_hour": 2, "per_user_day": 1000, "per_ws_hour": 1000,
                    "per_ws_day": 1000, "max_recipients": 50})
            .to_string(),
        ],
    )
    .await;
    f.sql(
        "UPDATE mail_member_settings SET undo_send_seconds = 10",
        &[],
    )
    .await;
    // Both pass Send (neither is due yet), and both are due at the same time.
    let now = TimestampMillis::now();
    let (first, due) = f
        .send_at(&["a1@out.test", "a2@out.test"], &[], &[], now)
        .await;
    let (second, due_too) = f.send_at(&["b1@out.test"], &[], &[], now).await;
    assert_eq!(due, due_too);

    // The first finds the second's recipient in the window: over the hour limit, it waits.
    f.run(first, due).await;
    let (status, hold, _) = f.outbound(first).await;
    assert_eq!(
        (status.as_str(), hold.as_deref()),
        ("released", Some("limit"))
    );
    assert!(f.fake.calls().is_empty());
    let room = due + HOUR;
    assert_eq!(f.runs(first).await, vec![room]);
    let released: Vec<Option<i64>> = sqlx::query_scalar(
        "SELECT released_at FROM mail_outbound_recipients WHERE outbound_id = $1",
    )
    .bind(first.to_string())
    .fetch_all(f.database.pool())
    .await
    .unwrap();
    assert_eq!(released, vec![None, None], "a held message does not count");

    // The second goes: the held one does not take its room.
    f.run(second, due).await;
    assert_eq!(f.status_of(second, "b1@out.test").await, "delivered");

    // At its time the first goes, and counts from then.
    f.run(first, room).await;
    assert_eq!(f.status_of(first, "a1@out.test").await, "delivered");
    assert_eq!(f.status_of(first, "a2@out.test").await, "delivered");
    let (status, hold, _) = f.outbound(first).await;
    assert_eq!((status.as_str(), hold), ("done", None));
    let released: Vec<Option<i64>> = sqlx::query_scalar(
        "SELECT released_at FROM mail_outbound_recipients WHERE outbound_id = $1",
    )
    .bind(first.to_string())
    .fetch_all(f.database.pool())
    .await
    .unwrap();
    assert_eq!(released, vec![Some(room), Some(room)]);
}

#[tokio::test]
async fn paused_holds() {
    let f = Fixture::new().await;
    let (id, t0) = f.send(&["a@out.test"], &[], &[]).await;
    f.sql(
        "INSERT INTO mail_send_policies (workspace_id, tier, paused_at, pause_reason, \
         created_at, updated_at) VALUES ($1, 'standard', 1, 'bounces', 0, 0)",
        &[&f.workspace.to_string()],
    )
    .await;
    f.run(id, t0).await;
    let (status, hold, _) = f.outbound(id).await;
    assert_eq!(
        (status.as_str(), hold.as_deref()),
        ("released", Some("paused"))
    );
    assert_eq!(f.status_of(id, "a@out.test").await, "queued");
    assert!(f.fake.calls().is_empty());
    assert!(f.runs(id).await.is_empty(), "no run until the resume");
    // The sweep leaves it to the resume too.
    assert_eq!(f.mail.sweep_deliveries(at(t0 + 1)).await.unwrap(), 0);

    f.sql("UPDATE mail_send_policies SET paused_at = NULL", &[])
        .await;
    f.run(id, t0 + 1000).await;
    assert_eq!(f.status_of(id, "a@out.test").await, "delivered");
    let (status, hold, _) = f.outbound(id).await;
    assert_eq!((status.as_str(), hold), ("done", None));
}

#[tokio::test]
async fn bcc_only_in_envelope() {
    let f = Fixture::new().await;
    let (id, t0) = f.send(&["a@out.test"], &[], &["hidden@out.test"]).await;
    f.run(id, t0).await;
    assert_eq!(
        f.fake.recipients(),
        BTreeSet::from(["a@out.test".to_owned(), "hidden@out.test".to_owned()])
    );
    let key = f.mail.bounce_key();
    for call in f.fake.calls() {
        assert!(!call.raw.contains("hidden@out.test"), "{}", call.raw);
        assert!(!call.raw.to_ascii_lowercase().contains("\nbcc:"));
        // Direct: one envelope for each recipient, with its own bounce address.
        assert_eq!(call.recipients.len(), 1);
        let (local, domain) = call.mail_from.split_once('@').unwrap();
        assert_eq!(domain, "one.test");
        assert_eq!(call.from_domain, "one.test");
        let recipient = orbit_mail::parse_bounce_local_part(local, &key).expect("a bounce token");
        let recipient_id = Id::from_bytes(recipient).unwrap();
        assert_eq!(call.correlation, recipient_id.to_string());
        let address: String =
            sqlx::query_scalar("SELECT address FROM mail_outbound_recipients WHERE id = $1")
                .bind(recipient_id.to_string())
                .fetch_one(f.database.pool())
                .await
                .unwrap();
        assert_eq!(address, call.recipients[0]);
    }
    assert_eq!(f.status_of(id, "hidden@out.test").await, "delivered");
}

#[tokio::test]
async fn envelope_by_route_kind() {
    let f = Fixture::new().await;
    // An instance relay: the context that may reach private addresses, one call for all.
    f.sql(
        "UPDATE mail_send_routes SET kind = 'postmark' WHERE id = $1",
        &[&f.route.to_string()],
    )
    .await;
    let (id, t0) = f.send(&["a@out.test"], &["b@out.test"], &[]).await;
    f.run(id, t0).await;
    assert!(f.fake.calls().is_empty());
    let calls = f.instance_fake.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].mail_from, "team@one.test");
    assert_eq!(calls[0].correlation, id.to_string());
    assert_eq!(calls[0].recipients.len(), 2);
    assert!(
        calls[0].raw.starts_with("X-PM-KeepID: true\r\n"),
        "{}",
        calls[0].raw
    );

    // A workspace's route: public addresses only.
    f.sql(
        "UPDATE mail_send_routes SET workspace_id = $1, kind = 'resend' WHERE id = $2",
        &[&f.workspace.to_string(), &f.route.to_string()],
    )
    .await;
    let (id, t1) = f.send(&["a@out.test"], &["b@out.test"], &[]).await;
    f.run(id, t1).await;
    assert_eq!(f.instance_fake.calls().len(), 1);
    let calls = f.fake.calls();
    // Resend: one for each recipient, keyed by outbound and recipient.
    assert_eq!(calls.len(), 2);
    for call in &calls {
        assert_eq!(call.mail_from, "team@one.test");
        let (outbound, recipient) = call.correlation.split_once(':').unwrap();
        assert_eq!(outbound, id.to_string());
        assert!(call.raw.starts_with(&format!(
            "Resend-Idempotency-Key: {outbound}:{recipient}\r\n"
        )));
    }
    assert_eq!(f.outbound(id).await.0, "done");
}

#[tokio::test]
async fn live_event_on_status_change() {
    let f = Fixture::new().await;
    let (id, t0) = f.send(&["a@out.test"], &[], &[]).await;
    f.mail
        .release(id, Some("mx.orbit.test"), at(t0))
        .await
        .unwrap()
        .expect("released");
    let thread = f.thread_of(id).await;
    let before = f.mail_events().await.len();
    deliver_due(&f.state(), id, at(t0)).await.unwrap();
    let events = f.mail_events().await;
    let new: Vec<_> = events[before..].to_vec();
    assert!(!new.is_empty());
    for (recipients, event) in &new {
        assert_eq!(event["type"], "thread.updated");
        assert_eq!(event["thread_id"], thread);
        assert_eq!(event["mailbox_id"], f.mailbox.to_string());
        assert_eq!(
            recipients,
            &json!({"to": "users", "users": [f.user.to_string()]})
        );
    }
}

#[tokio::test]
async fn sweep_requeues_stranded_delivery() {
    let f = Fixture::new().await;
    let (id, t0) = f.send(&["a@out.test"], &[], &[]).await;
    // Released, then the job was lost before delivery.
    f.mail
        .release(id, Some("mx.orbit.test"), at(t0))
        .await
        .unwrap()
        .expect("released");
    assert_eq!(f.mail.sweep_deliveries(at(t0 + 1)).await.unwrap(), 1);
    assert_eq!(f.runs(id).await, vec![t0 + 1]);
    // A queued run is enough.
    assert_eq!(f.mail.sweep_deliveries(at(t0 + 2)).await.unwrap(), 0);
    // The held-release sweep is unchanged by it.
    assert_eq!(f.mail.sweep_releases(at(t0 + 2)).await.unwrap(), 0);
}
