//! Mail domains: claims, DNS verification with a fake resolver, the state machine, the schedule,
//! permissions and the instance switch (spec §6.1, §7).

use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{
    AttachmentMutationCoordinator, AuthenticatedUser, Id, LocalBlobStore, MailMode,
    PasswordService, TestDatabase, TimestampMillis, UploadLimits, UploadService,
};
use orbit_server::auth_routes::CookieMode;
use orbit_server::mail_dns::{DnsChecker, DnsError, FakeDns};
use orbit_server::mail_routes::{MailSettings, MailState, mail_router};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::instance_settings::InstanceSettingsRepository;
use orbit_server::repositories::mail::{
    DomainStatus, MailRepository, PENDING_CHECK_MS, PENDING_WINDOW_MS, SUSPENDED_GRACE_MS,
};
use orbit_server::repositories::workspaces::WorkspaceRepository;
use serde_json::{Value, json};
use tower::ServiceExt;

const APP_KEY: [u8; 32] = [9; 32];
const HOSTNAME: &str = "mx.orbit.test";
const MINUTE: i64 = 60 * 1000;
const HOUR: i64 = 60 * MINUTE;

#[derive(Clone)]
struct Member {
    id: Id,
    cookie: String,
}

struct Fixture {
    _root: tempfile::TempDir,
    database: TestDatabase,
    identity: Arc<IdentityRepository>,
    mail: Arc<MailRepository>,
    dns: FakeDns,
    checker: DnsChecker,
    app: axum::Router,
    /// The first workspace (A) and a second one (B), both owned by `owner`.
    first: Id,
    second: Id,
    owner: Member,
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
        let second = WorkspaceRepository::new((*database).clone())
            .create(setup.user_id, "Second".to_owned(), "request", now)
            .await
            .unwrap();
        InstanceSettingsRepository::new((*database).clone())
            .set_mail_domains_enabled(setup.user_id, true, "request", now)
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
            .with_origins("https://orbit.test", None),
        );
        Self {
            _root: root,
            database,
            identity,
            mail,
            dns,
            checker,
            app,
            first: setup.workspace_id,
            second: second.id,
            owner: Member {
                id: setup.user_id,
                cookie: format!("__Host-orbit_session={}", setup.session.token),
            },
        }
    }

    /// A user with a session, a member of `workspace` with `role` when given.
    async fn user(&self, name: &str, membership: Option<(Id, &str)>) -> Member {
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
        if let Some((workspace_id, role)) = membership {
            sqlx::query(
                "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at) \
                 VALUES ($1, $2, $3, $4, 0, $5, $6)",
            )
            .bind(Id::new_v7().to_string())
            .bind(workspace_id.to_string())
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
            id,
            cookie: format!("__Host-orbit_session={}", session.token),
        }
    }

    async fn raw(
        &self,
        member: &Member,
        method: &str,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::COOKIE, &member.cookie);
        let request = match body {
            Some(value) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(value.to_string())),
            None => builder.body(Body::empty()),
        }
        .unwrap();
        let response = self.app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn call(
        &self,
        member: &Member,
        workspace: Id,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let uri = format!("/api/v1/workspaces/{workspace}/mail{path}");
        self.raw(member, method, &uri, body).await
    }

    /// Adds `name` to `workspace` as the owner; the call must succeed.
    async fn add(&self, workspace: Id, name: &str) -> Value {
        let (status, value) = self
            .call(
                &self.owner,
                workspace,
                "POST",
                "/domains",
                Some(json!({ "name": name })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "add {name}: {value}");
        value
    }

    async fn refused(&self, workspace: Id, name: &str) -> (StatusCode, String) {
        let (status, value) = self
            .call(
                &self.owner,
                workspace,
                "POST",
                "/domains",
                Some(json!({ "name": name })),
            )
            .await;
        (
            status,
            value["code"].as_str().unwrap_or_default().to_owned(),
        )
    }

    async fn get(&self, workspace: Id, domain: &Value) -> Value {
        let (status, value) = self
            .call(
                &self.owner,
                workspace,
                "GET",
                &format!("/domains/{}", domain["id"].as_str().unwrap()),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{value}");
        value
    }

    async fn check(&self, domain: &Value, now: TimestampMillis) -> DomainStatus {
        self.checker
            .check_domain(id(domain), now)
            .await
            .unwrap()
            .unwrap()
    }

    /// A shared mailbox in `workspace`, with `local@<domain>` as its primary address when given.
    async fn mailbox(&self, workspace: Id, address: Option<(&Value, &str)>) -> (Id, Option<Id>) {
        let mailbox_id = Id::new_v7();
        let now = TimestampMillis::now().as_millis();
        sqlx::query(
            "INSERT INTO mailboxes (id, workspace_id, kind, name, status, created_at, updated_at) \
             VALUES ($1, $2, 'shared', 'Support', 'active', $3, $3)",
        )
        .bind(mailbox_id.to_string())
        .bind(workspace.to_string())
        .bind(now)
        .execute(self.database.pool())
        .await
        .unwrap();
        let address_id = match address {
            Some((domain, local)) => {
                let address_id = Id::new_v7();
                sqlx::query(
                    "INSERT INTO mail_addresses (id, domain_id, mailbox_id, local_part, kind, created_at) \
                     VALUES ($1, $2, $3, $4, 'primary', $5)",
                )
                .bind(address_id.to_string())
                .bind(domain["id"].as_str().unwrap())
                .bind(mailbox_id.to_string())
                .bind(local)
                .bind(now)
                .execute(self.database.pool())
                .await
                .unwrap();
                Some(address_id)
            }
            None => None,
        };
        (mailbox_id, address_id)
    }

    async fn count(&self, sql: &str, value: &str) -> i64 {
        sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
            .bind(value.to_owned())
            .fetch_one(self.database.pool())
            .await
            .unwrap()
    }
}

fn id(domain: &Value) -> Id {
    domain["id"].as_str().unwrap().parse().unwrap()
}

fn record<'a>(domain: &'a Value, kind: &str) -> &'a Value {
    domain["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["kind"] == kind)
        .unwrap_or_else(|| panic!("no {kind} record in {domain}"))
}

fn status(domain: &Value, kind: &str) -> String {
    record(domain, kind)["status"]["status"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// Publishes every record the domain asks for.
fn publish_all(dns: &FakeDns, domain: &Value) {
    for kind in ["verify", "dkim", "dmarc"] {
        let record = record(domain, kind);
        dns.set_txt(
            record["name"].as_str().unwrap(),
            &[record["value"].as_str().unwrap()],
        );
    }
    let name = domain["name"].as_str().unwrap();
    dns.set_txt(name, &["google-site-verification=x", "v=spf1 mx -all"]);
    dns.set_txt(&format!("_dmarc.{name}"), &["v=DMARC1; p=reject"]);
    dns.set_mx(name, &[(10, HOSTNAME)]);
}

fn at(millis: i64) -> TimestampMillis {
    TimestampMillis::from_millis(millis)
}

#[tokio::test]
async fn create_normalizes_names_and_makes_token_and_dkim_key() {
    let fixture = Fixture::new().await;
    let domain = fixture.add(fixture.first, "  Example.COM. ").await;
    assert_eq!(domain["name"], "example.com");
    assert_eq!(domain["status"], "pending");
    assert_eq!(domain["checked_at"], Value::Null);

    let verify = record(&domain, "verify");
    assert_eq!(verify["type"], "TXT");
    assert_eq!(verify["name"], "_orbit-verify.example.com");
    let value = verify["value"].as_str().unwrap();
    let token = value.strip_prefix("orbit-verify=").unwrap();
    assert_eq!(token.len(), 52);
    assert_eq!(status(&domain, "verify"), "unknown");

    // The DKIM record is longer than one TXT string; the API also gives it split.
    let dkim = record(&domain, "dkim");
    // The first selector of the year: `o2026a`.
    let dkim_name = dkim["name"].as_str().unwrap();
    let selector = dkim_name.strip_suffix("._domainkey.example.com").unwrap();
    assert_eq!(selector.len(), 6, "{selector}");
    assert!(
        selector.starts_with("o20") && selector.ends_with('a'),
        "{selector}"
    );
    let text = dkim["value"].as_str().unwrap();
    assert!(text.starts_with("v=DKIM1; k=rsa; p=MII"), "{text}");
    assert!(text.len() > 255);
    let strings: Vec<&str> = dkim["txt_strings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|part| part.as_str().unwrap())
        .collect();
    assert!(strings.len() >= 2);
    assert!(strings.iter().all(|part| part.len() <= 255));
    assert_eq!(strings.concat(), text);

    let mx = record(&domain, "mx");
    assert_eq!(mx["type"], "MX");
    assert_eq!(mx["value"], HOSTNAME);
    assert_eq!(mx["priority"], 10);

    // One pending key with a sealed private key; the creation is audited.
    let domain_id = domain["id"].as_str().unwrap();
    assert_eq!(
        fixture
            .count(
                "SELECT COUNT(*) FROM mail_dkim_keys WHERE domain_id = $1 AND status = 'pending' \
                 AND length(private_sealed) > 1000",
                domain_id,
            )
            .await,
        1
    );
    assert_eq!(
        fixture
            .count(
                "SELECT COUNT(*) FROM audit_events WHERE resource_id = $1 \
                 AND action = 'mail.domain_created'",
                domain_id,
            )
            .await,
        1
    );

    let idna = fixture.add(fixture.first, "Bücher.Example").await;
    assert_eq!(idna["name"], "xn--bcher-kva.example");
    for invalid in ["localhost", "not a domain", "", "-bad.example", "a..b"] {
        let (status, code) = fixture.refused(fixture.first, invalid).await;
        assert_eq!(
            (status, code.as_str()),
            (StatusCode::UNPROCESSABLE_ENTITY, "validation_failed"),
            "{invalid}"
        );
    }
}

#[tokio::test]
async fn claims_conflict_with_parents_and_subdomains_of_other_workspaces() {
    let fixture = Fixture::new().await;
    fixture.add(fixture.first, "example.com").await;
    for name in [
        "example.com",
        "EXAMPLE.com",
        "mail.example.com",
        "a.b.example.com",
    ] {
        assert_eq!(
            fixture.refused(fixture.second, name).await,
            (StatusCode::CONFLICT, "domain_claimed".to_owned()),
            "{name}"
        );
    }
    assert_eq!(
        fixture.refused(fixture.first, "example.com").await,
        (StatusCode::CONFLICT, "domain_exists".to_owned())
    );
    // The same workspace adds subdomains; another one adds unrelated names.
    fixture.add(fixture.first, "mail.example.com").await;
    fixture.add(fixture.second, "notexample.com").await;
    fixture.add(fixture.second, "deep.other.test").await;
    assert_eq!(
        fixture.refused(fixture.first, "other.test").await,
        (StatusCode::CONFLICT, "domain_claimed".to_owned())
    );
    let (status, listed) = fixture
        .call(&fixture.owner, fixture.first, "GET", "/domains", None)
        .await;
    assert_eq!(status, StatusCode::OK);
    let names: Vec<&str> = listed
        .as_array()
        .unwrap()
        .iter()
        .map(|domain| domain["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["example.com", "mail.example.com"]);
}

#[tokio::test]
async fn verification_suspension_and_timeouts_follow_the_state_machine() {
    let fixture = Fixture::new().await;
    let domain = fixture.add(fixture.first, "example.com").await;
    let start = TimestampMillis::now().as_millis();

    // Nothing published: still pending, every record missing.
    assert_eq!(
        fixture.check(&domain, at(start)).await,
        DomainStatus::Pending
    );
    let read = fixture.get(fixture.first, &domain).await;
    assert_eq!(read["status"], "pending");
    for kind in ["verify", "mx", "dkim", "spf", "dmarc"] {
        assert_eq!(status(&read, kind), "missing", "{kind}");
    }
    assert!(read["checked_at"].is_string());

    // A wrong token and a wrong MX are reported as such.
    fixture
        .dns
        .set_txt("_orbit-verify.example.com", &["orbit-verify=someone-else"]);
    fixture
        .dns
        .set_mx("example.com", &[(5, "aspmx.l.google.com.")]);
    assert_eq!(
        fixture.check(&domain, at(start + MINUTE)).await,
        DomainStatus::Pending
    );
    let read = fixture.get(fixture.first, &domain).await;
    assert_eq!(
        record(&read, "verify")["status"],
        json!({ "status": "wrong", "found": "orbit-verify=someone-else" })
    );
    assert_eq!(
        record(&read, "mx")["status"],
        json!({ "status": "wrong", "found": "5 aspmx.l.google.com" })
    );

    // Every record published: verified.
    publish_all(&fixture.dns, &domain);
    assert_eq!(
        fixture.check(&domain, at(start + 2 * MINUTE)).await,
        DomainStatus::Verified
    );
    let read = fixture.get(fixture.first, &domain).await;
    assert_eq!(read["status"], "verified");
    for kind in ["verify", "mx", "dkim", "spf", "dmarc"] {
        assert_eq!(status(&read, kind), "ok", "{kind}");
    }
    assert_eq!(read["dmarc_policy"], "reject");
    let verified_at = read["verified_at"].clone();
    assert!(verified_at.is_string());

    // A DNS timeout changes nothing (not even `checked_at`).
    let checked_at = read["checked_at"].clone();
    fixture
        .dns
        .fail("_orbit-verify.example.com", DnsError::Timeout);
    assert_eq!(
        fixture.check(&domain, at(start + 3 * MINUTE)).await,
        DomainStatus::Verified
    );
    let read = fixture.get(fixture.first, &domain).await;
    assert_eq!(read["status"], "verified");
    assert_eq!(status(&read, "verify"), "unknown");
    assert_eq!(read["checked_at"], checked_at);

    // The TXT record is gone: suspended, with a grace end 7 days later.
    fixture.dns.remove("_orbit-verify.example.com");
    let suspended = start + 4 * MINUTE;
    assert_eq!(
        fixture.check(&domain, at(suspended)).await,
        DomainStatus::Suspended
    );
    let read = fixture.get(fixture.first, &domain).await;
    assert_eq!(read["status"], "suspended");
    let suspended_at = read["suspended_at"].clone();
    assert_eq!(suspended_at, serde_json::to_value(at(suspended)).unwrap());
    assert_eq!(
        read["grace_ends_at"],
        serde_json::to_value(at(suspended + SUSPENDED_GRACE_MS)).unwrap()
    );

    // Still missing, or a timeout: suspended since the first time.
    assert_eq!(
        fixture.check(&domain, at(suspended + HOUR)).await,
        DomainStatus::Suspended
    );
    fixture.dns.fail(
        "_orbit-verify.example.com",
        DnsError::Other("SERVFAIL".to_owned()),
    );
    assert_eq!(
        fixture.check(&domain, at(suspended + 2 * HOUR)).await,
        DomainStatus::Suspended
    );
    assert_eq!(
        fixture.get(fixture.first, &domain).await["suspended_at"],
        suspended_at
    );

    // The record is back: verified again, the first verification date kept.
    publish_all(&fixture.dns, &domain);
    assert_eq!(
        fixture.check(&domain, at(suspended + 3 * HOUR)).await,
        DomainStatus::Verified
    );
    let read = fixture.get(fixture.first, &domain).await;
    assert_eq!(read["status"], "verified");
    assert_eq!(read["suspended_at"], Value::Null);
    assert_eq!(read["grace_ends_at"], Value::Null);
    assert_eq!(read["verified_at"], verified_at);

    // A DKIM record with another key is wrong; the status does not depend on it.
    let dkim = record(&domain, "dkim");
    fixture.dns.set_txt(
        dkim["name"].as_str().unwrap(),
        &["v=DKIM1; k=rsa; p=MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAother"],
    );
    assert_eq!(
        fixture.check(&domain, at(suspended + 4 * HOUR)).await,
        DomainStatus::Verified
    );
    assert_eq!(
        status(&fixture.get(fixture.first, &domain).await, "dkim"),
        "wrong"
    );
    // Status changes by the check are audited without an actor.
    assert_eq!(
        fixture
            .count(
                "SELECT COUNT(*) FROM audit_events WHERE resource_id = $1 \
                 AND action = 'mail.domain_updated' AND actor_id IS NULL",
                domain["id"].as_str().unwrap(),
            )
            .await,
        3
    );
}

#[tokio::test]
async fn the_schedule_checks_pending_domains_often_and_others_daily() {
    let fixture = Fixture::new().await;
    let pending = fixture.add(fixture.first, "pending.test").await;
    let old = fixture.add(fixture.first, "old.test").await;
    let verified = fixture.add(fixture.first, "verified.test").await;
    let disabled = fixture.add(fixture.first, "disabled.test").await;
    let now = TimestampMillis::now().as_millis();
    sqlx::query("UPDATE mail_domains SET created_at = $1 WHERE id = $2")
        .bind(now - PENDING_WINDOW_MS - HOUR)
        .bind(old["id"].as_str().unwrap())
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let (status, _) = fixture
        .call(
            &fixture.owner,
            fixture.first,
            "PUT",
            &format!("/domains/{}/enabled", disabled["id"].as_str().unwrap()),
            Some(json!({ "enabled": false })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    publish_all(&fixture.dns, &verified);

    // Never checked: all enabled domains are due. The sweep checks them.
    let mut due = fixture.mail.domains_due(at(now)).await.unwrap();
    due.sort_unstable();
    let mut expected = vec![id(&pending), id(&old), id(&verified)];
    expected.sort_unstable();
    assert_eq!(due, expected);
    assert_eq!(fixture.checker.check_due(at(now)).await.unwrap(), 3);
    assert_eq!(
        fixture.get(fixture.first, &verified).await["status"],
        "verified"
    );
    assert_eq!(
        fixture.get(fixture.first, &disabled).await["checked_at"],
        Value::Null
    );

    // A young pending domain again after 10 minutes, the others after a day.
    assert!(
        fixture
            .mail
            .domains_due(at(now + 5 * MINUTE))
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        fixture
            .mail
            .domains_due(at(now + PENDING_CHECK_MS))
            .await
            .unwrap(),
        [id(&pending)]
    );
    assert_eq!(
        fixture.mail.domains_due(at(now + 23 * HOUR)).await.unwrap(),
        [id(&pending)]
    );
    let mut daily = fixture.mail.domains_due(at(now + 24 * HOUR)).await.unwrap();
    daily.sort_unstable();
    assert_eq!(daily, expected);

    // The job: `{}` is the sweep, `{"domain_id"}` one domain.
    fixture.dns.set_txt(
        record(&pending, "verify")["name"].as_str().unwrap(),
        &[record(&pending, "verify")["value"].as_str().unwrap()],
    );
    fixture
        .checker
        .run(&json!({ "domain_id": pending["id"] }))
        .await
        .unwrap();
    assert_eq!(
        fixture.get(fixture.first, &pending).await["status"],
        "verified"
    );
    fixture.checker.run(&json!({})).await.unwrap();
    assert!(
        fixture
            .checker
            .run(&json!({ "domain_id": 7 }))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn only_owners_and_admins_manage_domains() {
    let fixture = Fixture::new().await;
    let domain = fixture.add(fixture.first, "example.com").await;
    let domain_path = format!("/domains/{}", domain["id"].as_str().unwrap());
    let admin = fixture.user("admin", Some((fixture.first, "admin"))).await;
    let member = fixture
        .user("member", Some((fixture.first, "member")))
        .await;
    let outsider = fixture.user("outsider", None).await;

    let (status, listed) = fixture
        .call(&admin, fixture.first, "GET", "/domains", None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed.as_array().unwrap().len(), 1);

    let requests = [
        ("GET", "/domains".to_owned(), None),
        (
            "POST",
            "/domains".to_owned(),
            Some(json!({ "name": "member.test" })),
        ),
        ("GET", domain_path.clone(), None),
        ("DELETE", domain_path.clone(), None),
        ("POST", format!("{domain_path}/check"), None),
        (
            "PUT",
            format!("{domain_path}/enabled"),
            Some(json!({ "enabled": false })),
        ),
        (
            "PUT",
            format!("{domain_path}/catch-all"),
            Some(json!({ "mailbox_id": null })),
        ),
    ];
    for (method, path, body) in &requests {
        let (status, value) = fixture
            .call(&member, fixture.first, method, path, body.clone())
            .await;
        assert_eq!(
            (status, value["code"].as_str()),
            (StatusCode::FORBIDDEN, Some("mail_forbidden")),
            "{method} {path}"
        );
        let (status, value) = fixture
            .call(&outsider, fixture.first, method, path, body.clone())
            .await;
        assert_eq!(
            (status, value["code"].as_str()),
            (StatusCode::NOT_FOUND, Some("mail_not_found")),
            "{method} {path}"
        );
    }
    // The domain of another workspace is not found from this one.
    let (status, _) = fixture
        .call(&fixture.owner, fixture.second, "GET", &domain_path, None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        fixture.get(fixture.first, &domain).await["status"],
        "pending"
    );
}

#[tokio::test]
async fn the_instance_switch_off_refuses_new_domains() {
    let fixture = Fixture::new().await;
    fixture.add(fixture.first, "example.com").await;
    InstanceSettingsRepository::new((*fixture.database).clone())
        .set_mail_domains_enabled(fixture.owner.id, false, "request", TimestampMillis::now())
        .await
        .unwrap();
    assert_eq!(
        fixture.refused(fixture.first, "other.test").await,
        (StatusCode::FORBIDDEN, "mail_domains_disabled".to_owned())
    );
    let (status, listed) = fixture
        .call(&fixture.owner, fixture.first, "GET", "/domains", None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn a_domain_with_addresses_is_not_deleted() {
    let fixture = Fixture::new().await;
    let domain = fixture.add(fixture.first, "example.com").await;
    let path = format!("/domains/{}", domain["id"].as_str().unwrap());
    let (_, address) = fixture
        .mailbox(fixture.first, Some((&domain, "support")))
        .await;
    assert_eq!(
        fixture.get(fixture.first, &domain).await["address_count"],
        1
    );
    let (status, value) = fixture
        .call(&fixture.owner, fixture.first, "DELETE", &path, None)
        .await;
    assert_eq!(
        (status, value["code"].as_str()),
        (StatusCode::CONFLICT, Some("domain_has_mailboxes"))
    );

    sqlx::query("DELETE FROM mail_addresses WHERE id = $1")
        .bind(address.unwrap().to_string())
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let (status, _) = fixture
        .call(&fixture.owner, fixture.first, "DELETE", &path, None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = fixture
        .call(&fixture.owner, fixture.first, "GET", &path, None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let domain_id = domain["id"].as_str().unwrap();
    assert_eq!(
        fixture
            .count(
                "SELECT COUNT(*) FROM mail_dkim_keys WHERE domain_id = $1",
                domain_id
            )
            .await,
        0
    );
    assert_eq!(
        fixture
            .count(
                "SELECT COUNT(*) FROM audit_events WHERE resource_id = $1 \
                 AND action = 'mail.domain_deleted'",
                domain_id,
            )
            .await,
        1
    );
}

/// Review Focus 5: a domain removed by one workspace and added by another is a new domain.
#[tokio::test]
async fn a_domain_re_added_by_another_workspace_starts_over() {
    let fixture = Fixture::new().await;
    let old = fixture.add(fixture.first, "example.com").await;
    publish_all(&fixture.dns, &old);
    let now = TimestampMillis::now().as_millis();
    assert_eq!(fixture.check(&old, at(now)).await, DomainStatus::Verified);
    let (mailbox, address) = fixture
        .mailbox(fixture.first, Some((&old, "support")))
        .await;
    let (status, _) = fixture
        .call(
            &fixture.owner,
            fixture.first,
            "PUT",
            &format!("/domains/{}/catch-all", old["id"].as_str().unwrap()),
            Some(json!({ "mailbox_id": mailbox.to_string() })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    // A thread that arrived through the old domain.
    sqlx::query(
        "INSERT INTO mail_threads (id, workspace_id, mailbox_id, subject, folder, last_message_at, \
         message_count, created_at, updated_at) VALUES ($1, $2, $3, 'Hello', 'inbox', $4, 1, $4, $4)",
    )
    .bind(Id::new_v7().to_string())
    .bind(fixture.first.to_string())
    .bind(mailbox.to_string())
    .bind(now)
    .execute(fixture.database.pool())
    .await
    .unwrap();

    // The old workspace removes its address and the domain.
    sqlx::query("DELETE FROM mail_addresses WHERE id = $1")
        .bind(address.unwrap().to_string())
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let (status, _) = fixture
        .call(
            &fixture.owner,
            fixture.first,
            "DELETE",
            &format!("/domains/{}", old["id"].as_str().unwrap()),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Another workspace claims it: a new id, a new token, a new DKIM key, nothing verified.
    let new = fixture.add(fixture.second, "example.com").await;
    assert_ne!(new["id"], old["id"]);
    assert_ne!(
        record(&new, "verify")["value"],
        record(&old, "verify")["value"]
    );
    assert_ne!(record(&new, "dkim")["value"], record(&old, "dkim")["value"]);
    assert_eq!(new["status"], "pending");
    assert_eq!(new["verified_at"], Value::Null);
    assert_eq!(new["catch_all_mailbox_id"], Value::Null);
    assert_eq!(new["address_count"], 0);
    assert_eq!(status_of_all(&new), ["unknown"; 5]);
    // The old records do not verify the new claim.
    assert_eq!(
        fixture.check(&new, at(now + MINUTE)).await,
        DomainStatus::Pending
    );
    assert_eq!(
        fixture
            .count(
                "SELECT COUNT(*) FROM mail_dkim_keys WHERE domain_id <> $1",
                new["id"].as_str().unwrap(),
            )
            .await,
        0
    );

    // The old domain is gone for both workspaces; the old mail stays in the old workspace.
    for workspace in [fixture.first, fixture.second] {
        let (status, _) = fixture
            .call(
                &fixture.owner,
                workspace,
                "GET",
                &format!("/domains/{}", old["id"].as_str().unwrap()),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
    let (_, listed) = fixture
        .call(&fixture.owner, fixture.first, "GET", "/domains", None)
        .await;
    assert_eq!(listed, json!([]));
    assert_eq!(
        fixture
            .count(
                "SELECT COUNT(*) FROM mail_threads WHERE workspace_id = $1",
                &fixture.first.to_string(),
            )
            .await,
        1
    );
    assert_eq!(
        fixture
            .count(
                "SELECT COUNT(*) FROM mail_threads WHERE workspace_id = $1",
                &fixture.second.to_string(),
            )
            .await,
        0
    );
    assert_eq!(
        fixture
            .count(
                "SELECT COUNT(*) FROM mailboxes WHERE workspace_id = $1",
                &fixture.second.to_string(),
            )
            .await,
        0
    );
}

fn status_of_all(domain: &Value) -> Vec<String> {
    ["verify", "mx", "dkim", "spf", "dmarc"]
        .iter()
        .map(|kind| status(domain, kind))
        .collect()
}

#[tokio::test]
async fn check_now_queues_one_job_at_a_time() {
    let fixture = Fixture::new().await;
    let domain = fixture.add(fixture.first, "example.com").await;
    let path = format!("/domains/{}/check", domain["id"].as_str().unwrap());
    for _ in 0..2 {
        let (status, _) = fixture
            .call(&fixture.owner, fixture.first, "POST", &path, None)
            .await;
        assert_eq!(status, StatusCode::ACCEPTED);
    }
    assert_eq!(
        fixture
            .count(
                "SELECT COUNT(*) FROM jobs WHERE kind = $1 AND state = 'queued'",
                "mail.dns_check",
            )
            .await,
        1
    );
    let payload: String = sqlx::query_scalar("SELECT payload_json FROM jobs WHERE kind = $1")
        .bind("mail.dns_check")
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    let payload: Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(payload["domain_id"], domain["id"]);
    fixture.checker.run(&payload).await.unwrap();
    assert!(fixture.get(fixture.first, &domain).await["checked_at"].is_string());
}

#[tokio::test]
async fn catch_all_and_disable() {
    let fixture = Fixture::new().await;
    let domain = fixture.add(fixture.first, "example.com").await;
    let path = format!("/domains/{}", domain["id"].as_str().unwrap());
    let (mailbox, _) = fixture.mailbox(fixture.first, None).await;
    let (other, _) = fixture.mailbox(fixture.second, None).await;

    let (status, value) = fixture
        .call(
            &fixture.owner,
            fixture.first,
            "PUT",
            &format!("{path}/catch-all"),
            Some(json!({ "mailbox_id": mailbox.to_string() })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["catch_all_mailbox_id"], mailbox.to_string());
    for invalid in [other.to_string(), "nope".to_owned()] {
        let (status, value) = fixture
            .call(
                &fixture.owner,
                fixture.first,
                "PUT",
                &format!("{path}/catch-all"),
                Some(json!({ "mailbox_id": invalid })),
            )
            .await;
        assert_eq!(
            (status, value["detail"].as_str()),
            (StatusCode::UNPROCESSABLE_ENTITY, Some("mailbox_id"))
        );
    }
    let (_, value) = fixture
        .call(
            &fixture.owner,
            fixture.first,
            "PUT",
            &format!("{path}/catch-all"),
            Some(json!({ "mailbox_id": null })),
        )
        .await;
    assert_eq!(value["catch_all_mailbox_id"], Value::Null);

    // Disabled: a passing check does not enable it; enabling restores the checked status.
    let (_, value) = fixture
        .call(
            &fixture.owner,
            fixture.first,
            "PUT",
            &format!("{path}/enabled"),
            Some(json!({ "enabled": false })),
        )
        .await;
    assert_eq!(value["status"], "disabled");
    publish_all(&fixture.dns, &domain);
    let now = TimestampMillis::now().as_millis();
    assert_eq!(
        fixture.check(&domain, at(now)).await,
        DomainStatus::Disabled
    );
    let (_, value) = fixture
        .call(
            &fixture.owner,
            fixture.first,
            "PUT",
            &format!("{path}/enabled"),
            Some(json!({ "enabled": true })),
        )
        .await;
    assert_eq!(value["status"], "pending");
    assert_eq!(
        fixture.check(&domain, at(now + MINUTE)).await,
        DomainStatus::Verified
    );
    fixture
        .call(
            &fixture.owner,
            fixture.first,
            "PUT",
            &format!("{path}/enabled"),
            Some(json!({ "enabled": false })),
        )
        .await;
    let (_, value) = fixture
        .call(
            &fixture.owner,
            fixture.first,
            "PUT",
            &format!("{path}/enabled"),
            Some(json!({ "enabled": true })),
        )
        .await;
    assert_eq!(value["status"], "verified");
}

#[tokio::test]
async fn an_instance_admin_moves_a_domain_without_addresses() {
    let fixture = Fixture::new().await;
    let domain = fixture.add(fixture.first, "example.com").await;
    let uri = "/api/v1/admin/mail/domains/Example.com/workspace";
    let body = json!({ "workspace_id": fixture.second.to_string() });

    let admin = fixture.user("admin", Some((fixture.first, "admin"))).await;
    let (status, value) = fixture.raw(&admin, "PUT", uri, Some(body.clone())).await;
    assert_eq!(
        (status, value["code"].as_str()),
        (StatusCode::FORBIDDEN, Some("installation_admin_required"))
    );

    let (_, address) = fixture
        .mailbox(fixture.first, Some((&domain, "support")))
        .await;
    let (status, value) = fixture
        .raw(&fixture.owner, "PUT", uri, Some(body.clone()))
        .await;
    assert_eq!(
        (status, value["code"].as_str()),
        (StatusCode::CONFLICT, Some("domain_has_mailboxes"))
    );
    sqlx::query("DELETE FROM mail_addresses WHERE id = $1")
        .bind(address.unwrap().to_string())
        .execute(fixture.database.pool())
        .await
        .unwrap();

    let (status, _) = fixture
        .raw(&fixture.owner, "PUT", uri, Some(body.clone()))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, first) = fixture
        .call(&fixture.owner, fixture.first, "GET", "/domains", None)
        .await;
    assert_eq!(first, json!([]));
    let moved = fixture.get(fixture.second, &domain).await;
    assert_eq!(moved["name"], "example.com");
    assert_eq!(
        record(&moved, "verify")["value"],
        record(&domain, "verify")["value"]
    );

    let (status, _) = fixture
        .raw(
            &fixture.owner,
            "PUT",
            "/api/v1/admin/mail/domains/unknown.test/workspace",
            Some(body),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn record_dns_report_ignores_a_removed_domain() {
    let fixture = Fixture::new().await;
    assert_eq!(
        fixture
            .checker
            .check_domain(Id::new_v7(), TimestampMillis::now())
            .await
            .unwrap(),
        None
    );
}

// ---------------------------------------------------------------- providers and routes

impl Fixture {
    async fn admin_call(
        &self,
        member: &Member,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        self.raw(member, method, &format!("/api/v1/admin/mail{path}"), body)
            .await
    }
}

fn mx_values(domain: &Value) -> Vec<String> {
    domain["records"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|record| record["kind"] == "mx")
        .map(|record| record["value"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn create_returns_setup_once() {
    let fixture = Fixture::new().await;
    let (status, created) = fixture
        .admin_call(
            &fixture.owner,
            "POST",
            "/providers",
            Some(json!({ "kind": "postmark", "name": "Postmark" })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let password = created["generated_secrets"]["basic_password"]
        .as_str()
        .unwrap()
        .to_owned();
    let url = created["setup"]["webhook_url"].as_str().unwrap();
    assert!(url.contains(&password), "{url}");
    assert!(url.ends_with(&format!(
        "/api/v1/mail/inbound/postmark/{}",
        created["id"].as_str().unwrap()
    )));
    assert!(!created["setup"]["steps"].as_array().unwrap().is_empty());
    assert_eq!(
        created["secrets_set"],
        json!(["basic_user", "basic_password"])
    );

    let (status, listed) = fixture
        .admin_call(&fixture.owner, "GET", "/providers", None)
        .await;
    assert_eq!(status, StatusCode::OK);
    let listed = &listed["providers"][0];
    assert!(listed["generated_secrets"].is_null());
    assert!(!listed.to_string().contains(&password), "{listed}");

    let (status, cloudflare) = fixture
        .admin_call(
            &fixture.owner,
            "POST",
            "/providers",
            Some(json!({ "kind": "cloudflare", "name": "Cloudflare" })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(
        cloudflare["setup"]["worker_script"]
            .as_str()
            .unwrap()
            .contains("setReject")
    );
    // a provider whose secrets come from the provider needs them
    let (status, _) = fixture
        .admin_call(
            &fixture.owner,
            "POST",
            "/providers",
            Some(json!({ "kind": "mailgun", "name": "Mailgun" })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn workspace_lists_instance_providers_read_only() {
    let fixture = Fixture::new().await;
    let (_, instance) = fixture
        .admin_call(
            &fixture.owner,
            "POST",
            "/providers",
            Some(json!({ "kind": "cloudflare", "name": "Shared" })),
        )
        .await;
    let (status, own) = fixture
        .call(
            &fixture.owner,
            fixture.first,
            "POST",
            "/providers",
            Some(json!({ "kind": "mailgun", "name": "Ours", "secrets": { "signing_key": "k" } })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{own}");
    let (status, listed) = fixture
        .call(&fixture.owner, fixture.first, "GET", "/providers", None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed["workspace_providers_enabled"], true);
    let providers = listed["providers"].as_array().unwrap();
    assert_eq!(providers.len(), 2);
    let shared = providers
        .iter()
        .find(|p| p["id"] == instance["id"])
        .unwrap();
    assert_eq!(shared["scope"], "instance");
    // no webhook URL or steps of the instance's account for a workspace
    assert!(shared["setup"].is_null(), "{shared}");
    let path = format!("/providers/{}", instance["id"].as_str().unwrap());
    let (status, _) = fixture
        .call(&fixture.owner, fixture.first, "DELETE", &path, None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // not this workspace's provider: 404 before the body is read for the provider's kind
    let (status, _) = fixture
        .call(
            &fixture.owner,
            fixture.first,
            "PATCH",
            &path,
            Some(json!({ "secrets": { "signing_key": "k" } })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // the other workspace sees the instance provider only
    let (_, other) = fixture
        .call(&fixture.owner, fixture.second, "GET", "/providers", None)
        .await;
    assert_eq!(other["providers"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn route_change_updates_the_mx_expectation() {
    let fixture = Fixture::new().await;
    let domain = fixture.add(fixture.first, "example.com").await;
    let (_, provider) = fixture
        .admin_call(
            &fixture.owner,
            "POST",
            "/providers",
            Some(json!({ "kind": "cloudflare", "name": "Cloudflare" })),
        )
        .await;
    let path = format!("/domains/{}/route", domain["id"].as_str().unwrap());
    let (code, routed) = fixture
        .call(
            &fixture.owner,
            fixture.first,
            "PUT",
            &path,
            Some(json!({ "provider_id": provider["id"] })),
        )
        .await;
    assert_eq!(code, StatusCode::OK, "{routed}");
    assert_eq!(routed["provider"]["kind"], "cloudflare");
    assert_eq!(
        mx_values(&routed),
        vec![
            "route1.mx.cloudflare.net",
            "route2.mx.cloudflare.net",
            "route3.mx.cloudflare.net"
        ]
    );
    fixture
        .dns
        .set_mx("example.com", &[(86, "route2.mx.cloudflare.net.")]);
    fixture.check(&domain, TimestampMillis::now()).await;
    let read = fixture.get(fixture.first, &domain).await;
    assert_eq!(status(&read, "mx"), "ok");

    // back to Orbit's listener: the MX must name this server again
    let (code, smtp) = fixture
        .call(
            &fixture.owner,
            fixture.first,
            "PUT",
            &path,
            Some(json!({ "provider_id": null })),
        )
        .await;
    assert_eq!(code, StatusCode::OK);
    assert!(smtp["provider"].is_null());
    assert_eq!(mx_values(&smtp), vec![HOSTNAME.to_owned()]);
    fixture.check(&domain, TimestampMillis::now()).await;
    let read = fixture.get(fixture.first, &domain).await;
    assert_eq!(
        record(&read, "mx")["status"],
        json!({ "status": "wrong", "found": "86 route2.mx.cloudflare.net" })
    );
}

#[tokio::test]
async fn root_only_instance_routes() {
    let fixture = Fixture::new().await;
    let admin = fixture.user("admin", Some((fixture.first, "admin"))).await;
    for (method, body) in [
        ("GET", None),
        ("POST", Some(json!({ "kind": "cloudflare", "name": "x" }))),
    ] {
        let (status, _) = fixture.admin_call(&admin, method, "/providers", body).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method}");
    }
    let (status, _) = fixture
        .admin_call(
            &admin,
            "PUT",
            "",
            Some(json!({ "workspace_providers_enabled": false })),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // the root user turns workspace providers off: workspaces cannot add one
    let (status, settings) = fixture
        .admin_call(
            &fixture.owner,
            "PUT",
            "",
            Some(json!({ "domains_enabled": true, "workspace_providers_enabled": false })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{settings}");
    assert_eq!(settings["workspace_providers_enabled"], false);
    let (status, refused) = fixture
        .call(
            &fixture.owner,
            fixture.first,
            "POST",
            "/providers",
            Some(json!({ "kind": "cloudflare", "name": "Ours" })),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(refused["code"], "workspace_providers_disabled");
}

#[tokio::test]
async fn a_domain_with_a_foreign_provider_is_not_created() {
    let fixture = Fixture::new().await;
    let (_, foreign) = fixture
        .call(
            &fixture.owner,
            fixture.second,
            "POST",
            "/providers",
            Some(json!({ "kind": "cloudflare", "name": "Second's" })),
        )
        .await;
    let (status, _) = fixture
        .call(
            &fixture.owner,
            fixture.first,
            "POST",
            "/domains",
            Some(json!({ "name": "example.com", "provider_id": foreign["id"] })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (_, domains) = fixture
        .call(&fixture.owner, fixture.first, "GET", "/domains", None)
        .await;
    assert_eq!(domains, json!([]));
}

#[tokio::test]
async fn a_workspace_sees_no_details_of_instance_providers() {
    let fixture = Fixture::new().await;
    let (status, instance) = fixture
        .admin_call(
            &fixture.owner,
            "POST",
            "/providers",
            Some(json!({
                "kind": "ses",
                "name": "Shared SES",
                "secrets": { "access_key_id": "AKIA", "secret_access_key": "s" },
                "settings": {
                    "region": "eu-west-1",
                    "bucket": "instance-bucket",
                    "prefix": "inbound/",
                    "topic_arn": "arn:aws:sns:eu-west-1:123456789012:instance-topic"
                }
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{instance}");
    // the second workspace routes a domain to it
    let domain = fixture.add(fixture.second, "second.example").await;
    let route = format!("/domains/{}/route", domain["id"].as_str().unwrap());
    let (status, routed) = fixture
        .call(
            &fixture.owner,
            fixture.second,
            "PUT",
            &route,
            Some(json!({ "provider_id": instance["id"] })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{routed}");
    let (_, listed) = fixture
        .call(&fixture.owner, fixture.first, "GET", "/providers", None)
        .await;
    let shared = &listed["providers"][0];
    assert_eq!(shared["id"], instance["id"]);
    let text = shared.to_string();
    // the MX hosts stay: the domain's MX must point to them
    for hidden in ["instance-bucket", "instance-topic", "inbound/"] {
        assert!(!text.contains(hidden), "{hidden}: {text}");
    }
    assert!(shared["last_error"].is_null());
    // only the domains of this workspace count
    assert_eq!(shared["domain_count"], 0);
    let (_, second) = fixture
        .call(&fixture.owner, fixture.second, "GET", "/providers", None)
        .await;
    assert_eq!(second["providers"][0]["domain_count"], 1);
    // the root user sees everything
    let (_, admin) = fixture
        .admin_call(&fixture.owner, "GET", "/providers", None)
        .await;
    assert_eq!(
        admin["providers"][0]["settings"]["bucket"],
        "instance-bucket"
    );
    assert_eq!(admin["providers"][0]["domain_count"], 1);
}

#[tokio::test]
async fn new_secrets_replace_the_ones_orbit_made() {
    let fixture = Fixture::new().await;
    let (_, created) = fixture
        .admin_call(
            &fixture.owner,
            "POST",
            "/providers",
            Some(json!({ "kind": "postmark", "name": "Postmark" })),
        )
        .await;
    let old = created["generated_secrets"]["basic_password"]
        .as_str()
        .unwrap()
        .to_owned();
    let path = format!("/providers/{}", created["id"].as_str().unwrap());
    let (status, changed) = fixture
        .admin_call(
            &fixture.owner,
            "PATCH",
            &path,
            Some(json!({ "regenerate_secrets": true })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{changed}");
    let new = changed["generated_secrets"]["basic_password"]
        .as_str()
        .unwrap();
    assert_ne!(new, old);
    // shown once, in the webhook URL too
    assert!(
        changed["setup"]["webhook_url"]
            .as_str()
            .unwrap()
            .contains(new)
    );
    let (_, listed) = fixture
        .admin_call(&fixture.owner, "GET", "/providers", None)
        .await;
    assert!(!listed.to_string().contains(new));
    // a provider whose secrets come from the provider cannot regenerate them
    let (_, mailgun) = fixture
        .admin_call(
            &fixture.owner,
            "POST",
            "/providers",
            Some(
                json!({ "kind": "mailgun", "name": "Mailgun", "secrets": { "signing_key": "k" } }),
            ),
        )
        .await;
    let path = format!("/providers/{}", mailgun["id"].as_str().unwrap());
    let (status, _) = fixture
        .admin_call(
            &fixture.owner,
            "PATCH",
            &path,
            Some(json!({ "regenerate_secrets": true })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}
