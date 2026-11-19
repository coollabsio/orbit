//! DKIM keys: activation when the record is published, rotation, retiring after 7 days, the SPF
//! record by send route and the readiness to send (spec §6.1, §6.3).

use std::sync::Arc;

use orbit_platform::{
    AttachmentMutationCoordinator, Id, LocalBlobStore, MailMode, PasswordService, TestDatabase,
    TimestampMillis, UploadLimits, UploadService,
};
use orbit_server::mail_dns::{
    DnsChecker, DnsError, FakeDns, RecordKind, expected_records, expected_spf,
};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::instance_settings::InstanceSettingsRepository;
use orbit_server::repositories::mail::{
    Domain, MailError, MailRepository, NotReadyReason, RETIRING_KEY_MS, SendReadiness,
    SendRouteKind,
};

const APP_KEY: [u8; 32] = [9; 32];
const HOSTNAME: &str = "mx.orbit.test";
const DAY: i64 = 24 * 60 * 60 * 1000;
const MINUTE: i64 = 60 * 1000;

struct Fixture {
    _root: tempfile::TempDir,
    database: TestDatabase,
    mail: Arc<MailRepository>,
    dns: FakeDns,
    checker: DnsChecker,
    workspace: Id,
    owner: Id,
}

impl Fixture {
    async fn new() -> Self {
        let database = TestDatabase::new().await.unwrap();
        let identity = IdentityRepository::new((*database).clone());
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
        let dns = FakeDns::new();
        let checker = DnsChecker::new(
            Arc::clone(&mail),
            Arc::new(dns.clone()),
            Some(HOSTNAME.to_owned()),
            MailMode::Direct,
        );
        Self {
            _root: root,
            database,
            mail,
            dns,
            checker,
            workspace: setup.workspace_id,
            owner: setup.user_id,
        }
    }

    async fn add(&self, name: &str) -> Domain {
        self.mail
            .create_domain(self.workspace, self.owner, name, "request")
            .await
            .unwrap()
    }

    async fn get(&self, domain: &Domain) -> Domain {
        self.mail
            .get_domain(self.workspace, self.owner, domain.id)
            .await
            .unwrap()
    }

    async fn check(&self, domain: &Domain, now: i64) {
        self.checker
            .check_domain(domain.id, at(now))
            .await
            .unwrap()
            .unwrap();
    }

    /// Publishes the verify token, the MX and the DMARC record.
    fn publish_base(&self, domain: &Domain) {
        self.dns.set_txt(
            &format!("_orbit-verify.{}", domain.name),
            &[&format!("orbit-verify={}", domain.verify_token)],
        );
        self.dns.set_mx(&domain.name, &[(10, HOSTNAME)]);
        self.dns.set_txt(&domain.name, &["v=spf1 mx ~all"]);
        self.dns
            .set_txt(&format!("_dmarc.{}", domain.name), &["v=DMARC1; p=none"]);
    }

    /// Publishes the DKIM record of the key the domain shows now.
    fn publish_dkim(&self, domain: &Domain) {
        let key = domain.dkim.as_ref().unwrap();
        self.dns.set_txt(
            &format!("{}._domainkey.{}", key.selector, domain.name),
            &[&format!("v=DKIM1; k=rsa; p={}", key.public_key)],
        );
    }

    async fn key_status(&self, domain: &Domain, selector: &str) -> String {
        sqlx::query_scalar(
            "SELECT status FROM mail_dkim_keys WHERE domain_id = $1 AND selector = $2",
        )
        .bind(domain.id.to_string())
        .bind(selector)
        .fetch_one(self.database.pool())
        .await
        .unwrap()
    }

    async fn route(&self, kind: &str, settings: &str, workspace: Option<Id>) -> Id {
        let id = Id::new_v7();
        let now = TimestampMillis::now().as_millis();
        sqlx::query(
            "INSERT INTO mail_send_routes (id, workspace_id, kind, name, secrets, settings, \
             created_at, updated_at) VALUES ($1, $2, $3, 'Route', $4, $5, $6, $6)",
        )
        .bind(id.to_string())
        .bind(workspace.map(|id| id.to_string()))
        .bind(kind)
        .bind(b"sealed".to_vec())
        .bind(settings)
        .bind(now)
        .execute(self.database.pool())
        .await
        .unwrap();
        id
    }

    async fn use_route(&self, domain: &Domain, route: Option<Id>) {
        sqlx::query("UPDATE mail_domains SET send_route_id = $1 WHERE id = $2")
            .bind(route.map(|id| id.to_string()))
            .bind(domain.id.to_string())
            .execute(self.database.pool())
            .await
            .unwrap();
    }

    /// A verified domain whose first key is active.
    async fn active(&self, name: &str, now: i64) -> Domain {
        let domain = self.add(name).await;
        self.publish_base(&domain);
        self.publish_dkim(&domain);
        self.check(&domain, now).await;
        self.get(&domain).await
    }
}

fn at(millis: i64) -> TimestampMillis {
    TimestampMillis::from_millis(millis)
}

fn spf_of(domain: &Domain) -> String {
    expected_records(domain, Some(HOSTNAME))
        .into_iter()
        .find(|record| record.kind == RecordKind::Spf)
        .unwrap()
        .value
}

#[tokio::test]
async fn pending_key_activates_when_record_matches() {
    let fixture = Fixture::new().await;
    let now = TimestampMillis::now().as_millis();
    let domain = fixture.add("example.com").await;
    assert_eq!(domain.dkim.as_ref().unwrap().status, "pending");
    assert!(
        fixture
            .mail
            .active_signing_key(domain.id)
            .await
            .unwrap()
            .is_none()
    );

    fixture.publish_base(&domain);
    fixture.publish_dkim(&domain);
    fixture.check(&domain, now).await;

    let domain = fixture.get(&domain).await;
    let key = domain.dkim.as_ref().unwrap();
    assert_eq!(key.status, "active");
    let signing = fixture
        .mail
        .active_signing_key(domain.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(signing.domain, "example.com");
    assert_eq!(signing.selector, key.selector);
    assert!(!signing.pkcs8_der.is_empty());
}

#[tokio::test]
async fn stays_pending_on_mismatch() {
    let fixture = Fixture::new().await;
    let now = TimestampMillis::now().as_millis();
    let domain = fixture.add("example.com").await;
    fixture.publish_base(&domain);
    let selector = domain.dkim.as_ref().unwrap().selector.clone();
    fixture.dns.set_txt(
        &format!("{selector}._domainkey.example.com"),
        &["v=DKIM1; k=rsa; p=QUJD"],
    );
    fixture.check(&domain, now).await;
    assert_eq!(fixture.key_status(&domain, &selector).await, "pending");
    assert!(
        fixture
            .mail
            .active_signing_key(domain.id)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn rotation_switches_after_new_record() {
    let fixture = Fixture::new().await;
    let start = TimestampMillis::now().as_millis() + MINUTE;
    let domain = fixture.active("example.com", start).await;
    let old = domain.dkim.as_ref().unwrap().selector.clone();
    assert!(!domain.dkim_rotating);

    let rotated = fixture
        .mail
        .rotate_dkim(
            fixture.workspace,
            fixture.owner,
            domain.id,
            "request",
            at(start + 1),
        )
        .await
        .unwrap();
    assert_eq!(rotated.status, "pending");
    assert_ne!(rotated.selector, old);

    // The old key keeps signing while the new record is not published.
    let domain = fixture.get(&domain).await;
    assert!(domain.dkim_rotating, "a pending key beside an active one");
    assert_eq!(domain.dkim.as_ref().unwrap().selector, rotated.selector);
    fixture.check(&domain, start + 2).await;
    let signing = fixture
        .mail
        .active_signing_key(domain.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(signing.selector, old);
    assert_eq!(
        fixture.key_status(&domain, &rotated.selector).await,
        "pending"
    );

    fixture.publish_dkim(&domain);
    fixture.check(&domain, start + 3).await;
    let signing = fixture
        .mail
        .active_signing_key(domain.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(signing.selector, rotated.selector);
    assert_eq!(fixture.key_status(&domain, &old).await, "retiring");
    assert_eq!(
        fixture.key_status(&domain, &rotated.selector).await,
        "active"
    );
    assert!(!fixture.get(&domain).await.dkim_rotating);
}

#[tokio::test]
async fn retiring_retires_after_7_days_and_drops_private_key() {
    let fixture = Fixture::new().await;
    let start = TimestampMillis::now().as_millis() + MINUTE;
    let domain = fixture.active("example.com", start).await;
    let old = domain.dkim.as_ref().unwrap().selector.clone();
    let rotated = fixture
        .mail
        .rotate_dkim(
            fixture.workspace,
            fixture.owner,
            domain.id,
            "request",
            at(start + 1),
        )
        .await
        .unwrap();
    let domain = fixture.get(&domain).await;
    fixture.publish_dkim(&domain);
    let switched = start + 2;
    fixture.check(&domain, switched).await;
    assert_eq!(fixture.key_status(&domain, &old).await, "retiring");

    fixture.check(&domain, switched + RETIRING_KEY_MS - 1).await;
    assert_eq!(fixture.key_status(&domain, &old).await, "retiring");

    fixture.check(&domain, switched + RETIRING_KEY_MS).await;
    assert_eq!(fixture.key_status(&domain, &old).await, "retired");
    let sealed: Vec<u8> = sqlx::query_scalar(
        "SELECT private_sealed FROM mail_dkim_keys WHERE domain_id = $1 AND selector = $2",
    )
    .bind(domain.id.to_string())
    .bind(&old)
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    assert!(sealed.is_empty());
    // The new key is untouched and still signs.
    assert_eq!(
        fixture.key_status(&domain, &rotated.selector).await,
        "active"
    );
    assert!(
        fixture
            .mail
            .active_signing_key(domain.id)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(RETIRING_KEY_MS, 7 * DAY);
}

#[tokio::test]
async fn second_rotation_refused_while_pending() {
    let fixture = Fixture::new().await;
    let start = TimestampMillis::now().as_millis() + MINUTE;
    let domain = fixture.active("example.com", start).await;
    fixture
        .mail
        .rotate_dkim(
            fixture.workspace,
            fixture.owner,
            domain.id,
            "request",
            at(start + 1),
        )
        .await
        .unwrap();
    let error = fixture
        .mail
        .rotate_dkim(
            fixture.workspace,
            fixture.owner,
            domain.id,
            "request",
            at(start + 2),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(error, MailError::Conflict("rotation_pending")),
        "{error:?}"
    );

    // Once the new key is active, the next rotation gets a new selector.
    let domain = fixture.get(&domain).await;
    fixture.publish_dkim(&domain);
    fixture.check(&domain, start + 3).await;
    let third = fixture
        .mail
        .rotate_dkim(
            fixture.workspace,
            fixture.owner,
            domain.id,
            "request",
            at(start + 4),
        )
        .await
        .unwrap();
    assert_ne!(third.selector, domain.dkim.as_ref().unwrap().selector);
}

#[tokio::test]
async fn readiness_reasons() {
    let fixture = Fixture::new().await;
    let start = TimestampMillis::now().as_millis() + MINUTE;
    let domain = fixture.add("example.com").await;
    let ready = |reason| SendReadiness::NotReady(reason);
    let state = |mailbox| fixture.mail.send_readiness(domain.id, mailbox);

    assert_eq!(
        state(None).await.unwrap(),
        ready(NotReadyReason::DomainUnverified)
    );

    // Verified, but the MX is not published: replies could not arrive.
    fixture.dns.set_txt(
        "_orbit-verify.example.com",
        &[&format!("orbit-verify={}", domain.verify_token)],
    );
    fixture.check(&domain, start).await;
    assert_eq!(
        state(None).await.unwrap(),
        ready(NotReadyReason::InboundNotLive)
    );

    fixture.dns.set_mx("example.com", &[(10, HOSTNAME)]);
    fixture.check(&domain, start + 1).await;
    assert_eq!(
        state(None).await.unwrap(),
        ready(NotReadyReason::DkimInactive)
    );

    let domain = fixture.get(&domain).await;
    fixture.publish_dkim(&domain);
    fixture.check(&domain, start + 2).await;
    assert_eq!(state(None).await.unwrap(), ready(NotReadyReason::NoRoute));

    let route = fixture.route("smtp", "{}", None).await;
    fixture.use_route(&domain, Some(route)).await;
    assert_eq!(state(None).await.unwrap(), SendReadiness::Ready);

    // The instance default route serves a domain without a route of its own.
    fixture.use_route(&domain, None).await;
    assert_eq!(state(None).await.unwrap(), ready(NotReadyReason::NoRoute));
    sqlx::query("UPDATE instance_settings SET mail_send_route_id = $1 WHERE id = 1")
        .bind(route.to_string())
        .execute(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(state(None).await.unwrap(), SendReadiness::Ready);

    // A suspended mailbox cannot send.
    let mailbox = Id::new_v7();
    for (id, status) in [(mailbox, "suspended")] {
        sqlx::query(
            "INSERT INTO mailboxes (id, workspace_id, kind, name, status, created_at, updated_at) \
             VALUES ($1, $2, 'shared', 'Support', $3, $4, $4)",
        )
        .bind(id.to_string())
        .bind(fixture.workspace.to_string())
        .bind(status)
        .bind(start)
        .execute(fixture.database.pool())
        .await
        .unwrap();
    }
    assert_eq!(
        state(Some(mailbox)).await.unwrap(),
        ready(NotReadyReason::MailboxSuspended)
    );
    sqlx::query("UPDATE mailboxes SET status = 'active' WHERE id = $1")
        .bind(mailbox.to_string())
        .execute(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(state(Some(mailbox)).await.unwrap(), SendReadiness::Ready);

    // The MX goes wrong: inbound is not live any more.
    fixture.dns.set_mx("example.com", &[(10, "other.example")]);
    fixture.check(&domain, start + 3).await;
    assert_eq!(
        state(None).await.unwrap(),
        ready(NotReadyReason::InboundNotLive)
    );
}

#[tokio::test]
async fn spf_row_by_route() {
    assert_eq!(
        expected_spf(Some(&SendRouteKind::Direct), Some("mx.example.com"), None),
        "v=spf1 a:mx.example.com mx ~all"
    );
    assert!(
        expected_spf(Some(&SendRouteKind::Postmark), Some("mx.example.com"), None)
            .contains("include:spf.mtasv.net")
    );
    assert_eq!(
        expected_spf(None, Some("mx.example.com"), None),
        "v=spf1 mx ~all"
    );
    assert_eq!(
        expected_spf(
            Some(&SendRouteKind::Smtp),
            None,
            Some("include:relay.example")
        ),
        "v=spf1 mx include:relay.example ~all"
    );
    assert_eq!(
        expected_spf(Some(&SendRouteKind::Smtp), None, None),
        "v=spf1 mx ~all"
    );

    // The setup page shows the record of the domain's route.
    let fixture = Fixture::new().await;
    let domain = fixture.add("example.com").await;
    assert_eq!(spf_of(&fixture.get(&domain).await), "v=spf1 mx ~all");
    let route = fixture.route("direct", "{}", Some(fixture.workspace)).await;
    fixture.use_route(&domain, Some(route)).await;
    assert_eq!(
        spf_of(&fixture.get(&domain).await),
        "v=spf1 a:mx.orbit.test mx ~all"
    );
    let route = fixture
        .route("smtp", r#"{"spf":"include:relay.example"}"#, None)
        .await;
    fixture.use_route(&domain, Some(route)).await;
    assert_eq!(
        spf_of(&fixture.get(&domain).await),
        "v=spf1 mx include:relay.example ~all"
    );
    let route = fixture.route("postmark", "{}", None).await;
    fixture.use_route(&domain, Some(route)).await;
    assert!(spf_of(&fixture.get(&domain).await).contains("include:spf.mtasv.net"));
}

#[tokio::test]
async fn mx_timeout_keeps_previous_status() {
    let fixture = Fixture::new().await;
    let start = TimestampMillis::now().as_millis() + MINUTE;
    let domain = fixture.active("example.com", start).await;
    let route = fixture.route("smtp", "{}", None).await;
    fixture.use_route(&domain, Some(route)).await;
    assert_eq!(
        fixture.mail.send_readiness(domain.id, None).await.unwrap(),
        SendReadiness::Ready
    );

    fixture.dns.fail("example.com", DnsError::Timeout);
    fixture.check(&domain, start + 1).await;
    assert_eq!(
        fixture.mail.send_readiness(domain.id, None).await.unwrap(),
        SendReadiness::Ready
    );

    // A real answer still changes it.
    fixture.dns.remove("example.com");
    fixture.check(&domain, start + 2).await;
    assert_eq!(
        fixture.mail.send_readiness(domain.id, None).await.unwrap(),
        SendReadiness::NotReady(NotReadyReason::InboundNotLive)
    );
}

#[tokio::test]
async fn dns_timeout_does_not_change_keys() {
    let fixture = Fixture::new().await;
    let start = TimestampMillis::now().as_millis() + MINUTE;

    // A pending first key stays pending.
    let first = fixture.add("pending.example").await;
    fixture.publish_base(&first);
    let selector = first.dkim.as_ref().unwrap().selector.clone();
    fixture.dns.fail(
        &format!("{selector}._domainkey.pending.example"),
        DnsError::Timeout,
    );
    fixture.check(&first, start).await;
    assert_eq!(fixture.key_status(&first, &selector).await, "pending");

    // With a rotation under way, the active key stays active.
    let domain = fixture.active("example.com", start).await;
    let old = domain.dkim.as_ref().unwrap().selector.clone();
    let rotated = fixture
        .mail
        .rotate_dkim(
            fixture.workspace,
            fixture.owner,
            domain.id,
            "request",
            at(start + 1),
        )
        .await
        .unwrap();
    fixture.dns.fail(
        &format!("{}._domainkey.example.com", rotated.selector),
        DnsError::Timeout,
    );
    fixture.check(&domain, start + 2).await;
    assert_eq!(fixture.key_status(&domain, &old).await, "active");
    assert_eq!(
        fixture.key_status(&domain, &rotated.selector).await,
        "pending"
    );
}
