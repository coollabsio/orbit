//! The admin mail page (spec §7, §8.1): the instance switch, the health card (listener, PTR),
//! the test message, and the failed receipts of a workspace with **Retry**.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::Arc;
use std::time::Duration;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use lettre::address::Envelope;
use lettre::transport::smtp::client::Tls;
use lettre::transport::smtp::extension::ClientId;
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};
use orbit_mail::{AuthOutcome, DmarcVerdict, TlsInfo, Verdict};
use orbit_platform::{
    AttachmentMutationCoordinator, AuthenticatedUser, Config, EnvironmentMode, Id, LocalBlobStore,
    MailMode, PasswordService, TestDatabase, TimestampMillis, UploadLimits, UploadService,
};
use orbit_server::app::App;
use orbit_server::auth_routes::CookieMode;
use orbit_server::mail_dns::{DnsError, FakeDns, PtrState, check_ptr};
use orbit_server::mail_intake::{InboundMessage, MailIntake, Recipient, Source};
use orbit_server::mail_listener::MailListenerStatus;
use orbit_server::mail_routes::{MailSettings, MailState, mail_router};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::instance_settings::InstanceSettingsRepository;
use orbit_server::repositories::mail::{MailRepository, RecipientLookup, lookup_recipient};
use serde_json::{Value, json};
use tower::ServiceExt;

const APP_KEY: [u8; 32] = [5; 32];
const HOSTNAME: &str = "mx.orbit.test";
const HOUR: i64 = 60 * 60 * 1000;

struct Member {
    cookie: String,
}

struct Fixture {
    _root: tempfile::TempDir,
    database: TestDatabase,
    identity: Arc<IdentityRepository>,
    intake: MailIntake,
    dns: FakeDns,
    app: axum::Router,
    workspace: Id,
    root: Member,
}

impl Fixture {
    async fn new() -> Self {
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
        let root = tempfile::tempdir().unwrap();
        let uploads = UploadService::new(
            (*database).clone(),
            Arc::new(LocalBlobStore::new(root.path().join("attachments"))),
            AttachmentMutationCoordinator::default(),
            UploadLimits::default(),
        );
        let mail = Arc::new(MailRepository::new((*database).clone(), uploads, APP_KEY));
        let intake = MailIntake::new(Arc::clone(&mail), 52_428_800);
        let dns = FakeDns::new();
        let app = mail_router(
            MailState::new(
                Arc::clone(&identity),
                Arc::clone(&mail),
                CookieMode::secure(),
                MailSettings {
                    enabled: true,
                    hostname: Some(HOSTNAME.to_owned()),
                    mode: MailMode::Direct,
                },
            )
            .with_health(
                Arc::new(MailListenerStatus::default()),
                Arc::new(dns.clone()),
            ),
        );
        Self {
            _root: root,
            database,
            identity,
            intake,
            dns,
            app,
            workspace: setup.workspace_id,
            root: Member {
                cookie: format!("__Host-orbit_session={}", setup.session.token),
            },
        }
    }

    /// A user with a session, a member of the workspace with `role` when given.
    async fn user(&self, name: &str, role: Option<&str>) -> Member {
        let id = Id::new_v7();
        let now = TimestampMillis::now();
        let email = format!("{name}@example.com");
        sqlx::query(
            "INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, 'unused', $5, $6)",
        )
        .bind(id.to_string())
        .bind(&email)
        .bind(&email)
        .bind(name)
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(self.database.pool())
        .await
        .unwrap();
        if let Some(role) = role {
            sqlx::query(
                "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at) \
                 VALUES ($1, $2, $3, $4, 0, $5, $6)",
            )
            .bind(Id::new_v7().to_string())
            .bind(self.workspace.to_string())
            .bind(id.to_string())
            .bind(role)
            .bind(now.as_millis())
            .bind(now.as_millis())
            .execute(self.database.pool())
            .await
            .unwrap();
        }
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
        Member {
            cookie: format!("__Host-orbit_session={}", session.token),
        }
    }

    async fn call(
        &self,
        member: &Member,
        method: &str,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        request(&self.app, &member.cookie, None, method, uri, body).await
    }

    async fn execute(&self, sql: &str, binds: &[&str]) {
        let mut query = sqlx::query(sqlx::AssertSqlSafe(sql.to_owned()));
        for bind in binds {
            query = query.bind(*bind);
        }
        query.execute(self.database.pool()).await.unwrap();
    }

    async fn count(&self, sql: &str, bind: &str) -> i64 {
        sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
            .bind(bind)
            .fetch_one(self.database.pool())
            .await
            .unwrap()
    }

    /// A domain with `status`, and a shared mailbox with the primary address `box@`; returns
    /// (domain id, address id).
    async fn domain(&self, name: &str, status: &str) -> (Id, Id) {
        insert_domain(&self.database, self.workspace, name, status).await
    }

    fn test_path(&self) -> String {
        "/api/v1/admin/mail/test-address".to_owned()
    }

    fn receipts_path(&self) -> String {
        format!("/api/v1/workspaces/{}/mail/receipts", self.workspace)
    }
}

async fn request(
    app: &axum::Router,
    cookie: &str,
    origin: Option<&str>,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::COOKIE, cookie);
    if let Some(origin) = origin {
        builder = builder.header(header::ORIGIN, origin);
    }
    let request = match body {
        Some(value) => builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(value.to_string())),
        None => builder.body(Body::empty()),
    }
    .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn insert_domain(
    database: &orbit_platform::Database,
    workspace: Id,
    name: &str,
    status: &str,
) -> (Id, Id) {
    let now = TimestampMillis::now().as_millis().to_string();
    let domain_id = Id::new_v7();
    let mailbox_id = Id::new_v7();
    let address_id = Id::new_v7();
    let statements: [(String, Vec<String>); 3] = [
        (
            format!(
                "INSERT INTO mail_domains (id, workspace_id, name, status, verify_token, created_at, updated_at) \
                 VALUES ($1, $2, $3, $4, 'token', {now}, {now})"
            ),
            vec![
                domain_id.to_string(),
                workspace.to_string(),
                name.to_owned(),
                status.to_owned(),
            ],
        ),
        (
            format!(
                "INSERT INTO mailboxes (id, workspace_id, kind, name, owner_membership_id, status, created_at, updated_at) \
                 VALUES ($1, $2, 'shared', 'Box', NULL, 'active', {now}, {now})"
            ),
            vec![mailbox_id.to_string(), workspace.to_string()],
        ),
        (
            format!(
                "INSERT INTO mail_addresses (id, domain_id, mailbox_id, local_part, kind, created_at) \
                 VALUES ($1, $2, $3, 'box', 'primary', {now})"
            ),
            vec![
                address_id.to_string(),
                domain_id.to_string(),
                mailbox_id.to_string(),
            ],
        ),
    ];
    for (sql, binds) in statements {
        let mut query = sqlx::query(sqlx::AssertSqlSafe(sql));
        for bind in binds {
            query = query.bind(bind);
        }
        query.execute(database.pool()).await.unwrap();
    }
    (domain_id, address_id)
}

fn auth() -> AuthOutcome {
    AuthOutcome {
        spf: Verdict::Pass,
        dkim: Verdict::Pass,
        dmarc: DmarcVerdict::Pass,
        aligned_domain: Some("sender.test".to_owned()),
        temp_error: false,
        spf_domain: Some("sender.test".to_owned()),
        dkim_domain: Some("sender.test".to_owned()),
        from_domain: Some("sender.test".to_owned()),
    }
}

fn message(raw: &str, recipients: Vec<Recipient>) -> InboundMessage {
    InboundMessage {
        source: Source::Smtp,
        trace_len: 0,
        raw: raw.as_bytes().to_vec(),
        mail_from: Some("ann@sender.test".to_owned()),
        recipients,
        peer_ip: IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)),
        helo: "mx.sender.test".to_owned(),
        tls: Some(TlsInfo {
            version: "TLSv1.3".to_owned(),
            cipher: "TLS13_AES_128_GCM_SHA256".to_owned(),
        }),
        auth: auth(),
    }
}

#[tokio::test]
async fn admin_routes_need_the_root_user() {
    let fixture = Fixture::new().await;
    let (domain, _) = fixture.domain("one.test", "verified").await;
    let admin = fixture.user("ann", Some("admin")).await;
    let test = format!("{}/{}", fixture.test_path(), Id::new_v7());
    for (method, path, body) in [
        ("GET", "/api/v1/admin/mail".to_owned(), None),
        (
            "PUT",
            "/api/v1/admin/mail".to_owned(),
            Some(json!({ "domains_enabled": true })),
        ),
        (
            "POST",
            fixture.test_path(),
            Some(json!({ "domain_id": domain.to_string() })),
        ),
        ("GET", test, None),
    ] {
        let (status, body) = fixture.call(&admin, method, &path, body).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path}");
        assert_eq!(body["code"], "installation_admin_required");
    }
    let nobody = Member {
        cookie: "__Host-orbit_session=nope".to_owned(),
    };
    let (status, _) = fixture
        .call(&nobody, "GET", "/api/v1/admin/mail", None)
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        fixture
            .count(
                "SELECT COUNT(*) FROM mail_health_tests WHERE domain_id = $1",
                &domain.to_string()
            )
            .await,
        0
    );
}

#[tokio::test]
async fn the_switch_persists_and_is_audited() {
    let fixture = Fixture::new().await;
    let settings = InstanceSettingsRepository::new((*fixture.database).clone());
    assert!(!settings.mail_domains_enabled().await.unwrap());

    let (status, body) = fixture
        .call(
            &fixture.root,
            "PUT",
            "/api/v1/admin/mail",
            Some(json!({ "domains_enabled": true })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["domains_enabled"], true);
    assert!(settings.mail_domains_enabled().await.unwrap());
    let (_, body) = fixture
        .call(&fixture.root, "GET", "/api/v1/admin/mail", None)
        .await;
    assert_eq!(body["domains_enabled"], true);

    let (status, body) = fixture
        .call(
            &fixture.root,
            "PUT",
            "/api/v1/admin/mail",
            Some(json!({ "domains_enabled": false })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["domains_enabled"], false);
    assert!(!settings.mail_domains_enabled().await.unwrap());
    // both switches together: one change, one audit event
    let (status, body) = fixture
        .call(
            &fixture.root,
            "PUT",
            "/api/v1/admin/mail",
            Some(json!({ "domains_enabled": true, "workspace_providers_enabled": false })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(settings.mail_domains_enabled().await.unwrap());
    assert!(!settings.mail_workspace_providers_enabled().await.unwrap());
    assert_eq!(
        fixture
            .count(
                "SELECT COUNT(*) FROM audit_events WHERE action = $1",
                "instance.mail_updated"
            )
            .await,
        3
    );
}

#[tokio::test]
async fn the_health_card_shows_the_listener_ptr_and_test_domains() {
    let fixture = Fixture::new().await;
    let (verified, _) = fixture.domain("one.test", "verified").await;
    fixture.domain("two.test", "pending").await;
    let address = Ipv4Addr::new(192, 0, 2, 10);
    fixture.dns.set_a(HOSTNAME, &[address]);
    fixture.dns.set_ptr(IpAddr::V4(address), &[HOSTNAME]);

    let (status, body) = fixture
        .call(&fixture.root, "GET", "/api/v1/admin/mail", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["enabled"], true);
    assert_eq!(body["hostname"], HOSTNAME);
    assert_eq!(body["mode"], "direct");
    assert_eq!(body["listener"]["running"], false);
    assert_eq!(body["listener"]["cert"], Value::Null);
    assert_eq!(body["listener"]["bridge_peers"], 0);
    assert_eq!(body["ptr"]["status"], "ok", "{body}");
    assert_eq!(body["ptr"]["addresses"][0]["ip"], "192.0.2.10");
    assert_eq!(body["ptr"]["addresses"][0]["names"][0], HOSTNAME);
    let domains = body["test_domains"].as_array().unwrap();
    assert_eq!(domains.len(), 1, "{body}");
    assert_eq!(domains[0]["id"], verified.to_string());
    assert_eq!(domains[0]["name"], "one.test");
}

#[tokio::test]
async fn ptr_check_needs_every_address_to_point_back() {
    let dns = FakeDns::new();
    assert_eq!(check_ptr(&dns, None).await.status, PtrState::NoHostname);
    assert_eq!(
        check_ptr(&dns, Some(HOSTNAME)).await.status,
        PtrState::NoAddress
    );

    let v4 = Ipv4Addr::new(192, 0, 2, 10);
    let v6: Ipv6Addr = "2001:db8::10".parse().unwrap();
    dns.set_a(HOSTNAME, &[v4]);
    dns.set_aaaa(HOSTNAME, &[v6]);
    dns.set_ptr(IpAddr::V4(v4), &["MX.Orbit.Test."]);
    let report = check_ptr(&dns, Some(HOSTNAME)).await;
    assert_eq!(report.status, PtrState::Mismatch);
    assert_eq!(report.addresses.len(), 2);
    assert!(report.addresses[0].matches);
    assert!(!report.addresses[1].matches);
    assert!(report.addresses[1].names.is_empty());

    dns.set_ptr(IpAddr::V6(v6), &["other.test"]);
    let report = check_ptr(&dns, Some(HOSTNAME)).await;
    assert_eq!(report.status, PtrState::Mismatch);
    assert_eq!(report.addresses[1].names, vec!["other.test".to_owned()]);

    dns.set_ptr(IpAddr::V6(v6), &[HOSTNAME]);
    assert_eq!(check_ptr(&dns, Some(HOSTNAME)).await.status, PtrState::Ok);

    dns.fail(HOSTNAME, DnsError::Timeout);
    assert_eq!(
        check_ptr(&dns, Some(HOSTNAME)).await.status,
        PtrState::Unknown
    );
}

#[tokio::test]
async fn a_test_address_records_what_arrived() {
    let fixture = Fixture::new().await;
    let (domain, _) = fixture.domain("one.test", "verified").await;
    let before = TimestampMillis::now().as_millis();

    let (status, body) = fixture
        .call(
            &fixture.root,
            "POST",
            &fixture.test_path(),
            Some(json!({ "domain_id": domain.to_string() })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let id: Id = body["id"].as_str().unwrap().parse().unwrap();
    let address = body["address"].as_str().unwrap().to_owned();
    let (local, host) = address.split_once('@').unwrap();
    assert_eq!(host, "one.test");
    let random = local.strip_prefix("check-").unwrap();
    assert_eq!(random.len(), 8, "{address}");
    assert!(
        random
            .chars()
            .all(|c| c.is_ascii_lowercase() || ('2'..='7').contains(&c)),
        "{address}"
    );
    let expires_at = chrono_ms(&body["expires_at"]);
    assert!(expires_at >= before + HOUR - 60_000 && expires_at <= before + HOUR + 60_000);

    assert_eq!(
        lookup_recipient(fixture.database.pool(), &address, TimestampMillis::now())
            .await
            .unwrap(),
        RecipientLookup::HealthTest { test_id: id }
    );
    let path = format!("{}/{id}", fixture.test_path());
    let (status, body) = fixture.call(&fixture.root, "GET", &path, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "waiting");
    assert_eq!(body["address"], address);
    assert_eq!(body["peer_ip"], Value::Null);

    fixture
        .intake
        .accept(message(
            "From: ann@sender.test\r\nSubject: Check\r\n\r\nHi\r\n",
            vec![Recipient::HealthTest {
                test_id: id,
                rcpt: address.clone(),
            }],
        ))
        .await
        .unwrap();
    let (_, body) = fixture.call(&fixture.root, "GET", &path, None).await;
    assert_eq!(body["status"], "received", "{body}");
    assert_eq!(body["peer_ip"], "192.0.2.1");
    assert_eq!(body["helo"], "mx.sender.test");
    assert_eq!(body["mail_from"], "ann@sender.test");
    assert_eq!(body["tls"]["version"], "TLSv1.3");
    assert_eq!(body["auth"]["spf"], "pass");
    assert_eq!(body["auth"]["dkim"], "pass");
    assert_eq!(body["auth"]["dmarc"], "pass");
    assert!(body["received_at"].is_string());
    assert_eq!(
        fixture
            .count(
                "SELECT COUNT(*) FROM mail_receipts WHERE workspace_id = $1",
                &fixture.workspace.to_string()
            )
            .await,
        0
    );
}

/// RFC 3339 → Unix milliseconds.
fn chrono_ms(value: &Value) -> i64 {
    chrono::DateTime::parse_from_rfc3339(value.as_str().unwrap())
        .unwrap()
        .timestamp_millis()
}

#[tokio::test]
async fn an_expired_test_address_is_unknown() {
    let fixture = Fixture::new().await;
    let (domain, _) = fixture.domain("one.test", "verified").await;
    let (_, body) = fixture
        .call(
            &fixture.root,
            "POST",
            &fixture.test_path(),
            Some(json!({ "domain_id": domain.to_string() })),
        )
        .await;
    let id = body["id"].as_str().unwrap().to_owned();
    let address = body["address"].as_str().unwrap().to_owned();
    fixture
        .execute(
            &format!(
                "UPDATE mail_health_tests SET expires_at = {} WHERE id = $1",
                TimestampMillis::now().as_millis() - 1
            ),
            &[&id],
        )
        .await;

    assert_eq!(
        lookup_recipient(fixture.database.pool(), &address, TimestampMillis::now())
            .await
            .unwrap(),
        RecipientLookup::UnknownUser
    );
    let (status, body) = fixture
        .call(
            &fixture.root,
            "GET",
            &format!("{}/{id}", fixture.test_path()),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "expired");
}

#[tokio::test]
async fn a_test_address_needs_a_verified_domain() {
    let fixture = Fixture::new().await;
    let (pending, _) = fixture.domain("two.test", "pending").await;
    let (status, body) = fixture
        .call(
            &fixture.root,
            "POST",
            &fixture.test_path(),
            Some(json!({ "domain_id": pending.to_string() })),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "domain_not_verified");

    let (status, _) = fixture
        .call(
            &fixture.root,
            "POST",
            &fixture.test_path(),
            Some(json!({ "domain_id": Id::new_v7().to_string() })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = fixture
        .call(
            &fixture.root,
            "GET",
            &format!("{}/{}", fixture.test_path(), Id::new_v7()),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn failed_and_stuck_receipts_are_listed_and_retried() {
    let fixture = Fixture::new().await;
    let (_, address) = fixture.domain("one.test", "verified").await;
    let mut ids = Vec::new();
    for subject in ["One", "Two", "Three", "Four"] {
        let receipt = fixture
            .intake
            .accept(message(
                &format!("From: ann@sender.test\r\nSubject: {subject}\r\n\r\nHi\r\n"),
                vec![Recipient::Mailbox {
                    workspace_id: fixture.workspace,
                    address_id: address,
                    rcpt: "box@one.test".to_owned(),
                }],
            ))
            .await
            .unwrap();
        ids.push(receipt[0].to_string());
    }
    let (failed, stuck, waiting, fresh) = (&ids[0], &ids[1], &ids[2], &ids[3]);
    let old = TimestampMillis::now().as_millis() - 2 * HOUR;
    fixture
        .execute(
            "UPDATE mail_receipts SET status = 'failed', attempts = 8, error = 'parse exploded' \
             WHERE id = $1",
            &[failed],
        )
        .await;
    fixture
        .execute(
            "UPDATE jobs SET state = 'dead' WHERE payload_json LIKE '%' || $1 || '%'",
            &[failed],
        )
        .await;
    // Stuck: pending for two hours and no ingest job left (the server died on the last attempt).
    fixture
        .execute(
            &format!("UPDATE mail_receipts SET updated_at = {old} WHERE id = $1 OR id = $2"),
            &[stuck, waiting],
        )
        .await;
    fixture
        .execute(
            "UPDATE jobs SET state = 'dead' WHERE payload_json LIKE '%' || $1 || '%'",
            &[stuck],
        )
        .await;

    let member = fixture.user("bob", Some("member")).await;
    let outsider = fixture.user("eve", None).await;
    let (status, body) = fixture
        .call(&member, "GET", &fixture.receipts_path(), None)
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["code"], "mail_forbidden");
    let (status, _) = fixture
        .call(&outsider, "GET", &fixture.receipts_path(), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, body) = fixture
        .call(&fixture.root, "GET", &fixture.receipts_path(), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["count"], 2, "{body}");
    let listed = body["receipts"].as_array().unwrap();
    let find = |id: &str| listed.iter().find(|r| r["id"] == id).cloned();
    let one = find(failed).unwrap();
    assert_eq!(one["status"], "failed");
    assert_eq!(one["error"], "parse exploded");
    assert_eq!(one["mail_from"], "ann@sender.test");
    assert_eq!(one["attempts"], 8);
    assert!(one["size"].as_i64().unwrap() > 0);
    assert_eq!(find(stuck).unwrap()["status"], "stuck");
    assert!(find(waiting).is_none());
    assert!(find(fresh).is_none());

    let retry = |id: &str| format!("{}/{id}/retry", fixture.receipts_path());
    let (status, _) = fixture.call(&member, "POST", &retry(failed), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    for id in [failed, stuck] {
        let (status, body) = fixture.call(&fixture.root, "POST", &retry(id), None).await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
        let (state, attempts, error): (String, i64, Option<String>) =
            sqlx::query_as("SELECT status, attempts, error FROM mail_receipts WHERE id = $1")
                .bind(id)
                .fetch_one(fixture.database.pool())
                .await
                .unwrap();
        assert_eq!((state.as_str(), attempts, error), ("pending", 0, None));
        assert_eq!(
            fixture
                .count(
                    "SELECT COUNT(*) FROM jobs WHERE kind = 'mail.ingest' AND state = 'queued' \
                     AND payload_json LIKE '%' || $1 || '%'",
                    id
                )
                .await,
            1
        );
    }
    for id in [failed, waiting, fresh] {
        let (status, body) = fixture.call(&fixture.root, "POST", &retry(id), None).await;
        assert_eq!(status, StatusCode::CONFLICT, "{id}");
        assert_eq!(body["code"], "receipt_not_failed");
    }
    let (status, _) = fixture
        .call(
            &fixture.root,
            "POST",
            &retry(&Id::new_v7().to_string()),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, body) = fixture
        .call(&fixture.root, "GET", &fixture.receipts_path(), None)
        .await;
    assert_eq!(body["count"], 0, "{body}");
}

/// The real server: the switch in the instance settings, and a test message sent with `lettre`
/// to the listener lands on the health card.
#[tokio::test]
async fn a_test_message_over_smtp_reaches_the_health_card() {
    let root = tempfile::tempdir().unwrap();
    let mut config = Config {
        environment: EnvironmentMode::Development,
        app_key: Some(orbit_platform::AppKey::new(APP_KEY)),
        ..Config::default()
    };
    config.data.database = root.path().join("data/orbit.sqlite");
    config.data.attachments = root.path().join("data/attachments");
    config.data.backups = root.path().join("backups");
    config.mail.enabled = true;
    config.mail.hostname = Some(HOSTNAME.to_owned());
    config.mail.smtp_bind = IpAddr::V4(Ipv4Addr::LOCALHOST);
    config.mail.smtp_port = 0;
    config.mail.mode = MailMode::Relay;
    config.mail.relay_cidrs = vec!["127.0.0.1/32".parse().unwrap()];
    config.mail.relay_authserv_id = Some("relay.test".to_owned());
    // Nothing listens there: the PTR lookups fail at once.
    config.mail.dns_resolver = Some("127.0.0.1:9".parse().unwrap());
    let app = App::build(config).await.unwrap();
    let database = app.database().clone();
    let status = app.mail_listener_status();
    let router = app.router();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let shutdown = tokio_util::sync::CancellationToken::new();
    let token = shutdown.clone();
    let task = tokio::spawn(async move {
        app.serve(listener, token).await.unwrap();
    });
    let mut port = None;
    for _ in 0..500 {
        if let Some(address) = status.local_addr() {
            port = Some(address.port());
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let port = port.expect("the mail listener did not start");

    let identity = IdentityRepository::new(database.clone());
    let now = TimestampMillis::now();
    identity
        .store_setup_token(
            "mail-setup",
            TimestampMillis::from_millis(now.as_millis() + 60_000),
        )
        .await
        .unwrap();
    let setup = identity
        .complete_setup(
            SetupRequest {
                token: "mail-setup".to_owned(),
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
    let cookie = format!("orbit_session_dev={}", setup.session.token);
    let origin = Some("http://127.0.0.1:8080");
    let (domain, _) = insert_domain(&database, setup.workspace_id, "one.test", "verified").await;

    let (code, body) = request(
        &router,
        &cookie,
        origin,
        "PUT",
        "/api/v1/admin/mail",
        Some(json!({ "domains_enabled": true })),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let (_, body) = request(
        &router,
        &cookie,
        origin,
        "GET",
        "/api/v1/admin/settings",
        None,
    )
    .await;
    assert_eq!(body["mail"]["domains_enabled"], true, "{body}");

    let (code, body) = request(
        &router,
        &cookie,
        origin,
        "POST",
        "/api/v1/admin/mail/test-address",
        Some(json!({ "domain_id": domain.to_string() })),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{body}");
    let id = body["id"].as_str().unwrap().to_owned();
    let address = body["address"].as_str().unwrap().to_owned();

    let transport = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous("127.0.0.1")
        .port(port)
        .hello_name(ClientId::Domain("client.sender.test".to_owned()))
        .timeout(Some(Duration::from_secs(30)))
        .tls(Tls::None)
        .build();
    let envelope = Envelope::new(
        Some("ann@sender.test".parse().unwrap()),
        vec![address.parse().unwrap()],
    )
    .unwrap();
    let response = transport
        .send_raw(
            &envelope,
            format!("From: ann@sender.test\r\nTo: {address}\r\nSubject: Check\r\n\r\nHi\r\n")
                .as_bytes(),
        )
        .await
        .unwrap();
    assert_eq!(response.code().to_string(), "250");

    let (_, body) = request(
        &router,
        &cookie,
        origin,
        "GET",
        &format!("/api/v1/admin/mail/test-address/{id}"),
        None,
    )
    .await;
    assert_eq!(body["status"], "received", "{body}");
    assert_eq!(body["peer_ip"], "127.0.0.1");
    assert_eq!(body["helo"], "client.sender.test");
    assert_eq!(body["tls"], Value::Null);
    assert_eq!(body["auth"]["spf"], "none");

    let (code, body) = request(&router, &cookie, origin, "GET", "/api/v1/admin/mail", None).await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(body["mode"], "relay");
    assert_eq!(body["listener"]["running"], true, "{body}");
    assert_eq!(body["listener"]["last_peer_ip"], "127.0.0.1");
    assert_eq!(body["listener"]["address"], format!("127.0.0.1:{port}"));
    assert_eq!(body["ptr"]["status"], "unknown", "{body}");

    let receipts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM mail_receipts")
        .fetch_one(database.pool())
        .await
        .unwrap();
    assert_eq!(receipts, 0);
    shutdown.cancel();
    task.await.unwrap();
}
