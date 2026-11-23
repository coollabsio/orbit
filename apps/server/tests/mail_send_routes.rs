//! Send routes (spec §5.5, §7): sealed secrets, the workspace switch, the SSRF checks, the default
//! route and "Also send through this provider".

use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{
    AttachmentMutationCoordinator, AuthenticatedUser, Id, LocalBlobStore, MailMode,
    PasswordService, TestDatabase, TimestampMillis, UploadLimits, UploadService,
};
use orbit_server::auth_routes::CookieMode;
use orbit_server::mail_routes::{MailSettings, MailState, mail_router};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::instance_settings::InstanceSettingsRepository;
use orbit_server::repositories::mail::{
    HostClass, MailRepository, RouteSecrets, SendRouteKind, classify_ip,
};
use orbit_server::repositories::workspaces::WorkspaceRepository;
use serde_json::{Value, json};
use tower::ServiceExt;

const APP_KEY: [u8; 32] = [9; 32];
const HOSTNAME: &str = "mx.orbit.test";

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
    app: axum::Router,
    first: Id,
    owner: Member,
}

impl Fixture {
    async fn new() -> Self {
        Self::with_hostname(Some(HOSTNAME)).await
    }

    async fn with_hostname(hostname: Option<&str>) -> Self {
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
        let app = mail_router(
            MailState::new(
                Arc::clone(&identity),
                Arc::clone(&mail),
                CookieMode::secure(),
                MailSettings {
                    enabled: true,
                    hostname: hostname.map(str::to_owned),
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
            app,
            first: setup.workspace_id,
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

    async fn admin(&self, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        self.raw(
            &self.owner,
            method,
            &format!("/api/v1/admin/mail{path}"),
            body,
        )
        .await
    }

    async fn ws(&self, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        self.raw(
            &self.owner,
            method,
            &format!("/api/v1/workspaces/{}/mail{path}", self.first),
            body,
        )
        .await
    }

    async fn smtp(&self, instance: bool, host: &str, security: &str) -> (StatusCode, Value) {
        let body = Some(json!({
            "kind": "smtp",
            "name": "Relay",
            "secrets": { "username": "user", "password": "hunter2-secret" },
            "settings": { "host": host, "security": security },
        }));
        if instance {
            self.admin("POST", "/send-routes", body).await
        } else {
            self.ws("POST", "/send-routes", body).await
        }
    }

    /// A second workspace, owned by the owner.
    async fn second_workspace(&self) -> Id {
        WorkspaceRepository::new((*self.database).clone())
            .create(
                self.owner.id,
                "Second".to_owned(),
                "request",
                TimestampMillis::now(),
            )
            .await
            .unwrap()
            .id
    }

    async fn switch(&self, enabled: bool) {
        InstanceSettingsRepository::new((*self.database).clone())
            .set_mail_workspace_providers_enabled(
                self.owner.id,
                enabled,
                "request",
                TimestampMillis::now(),
            )
            .await
            .unwrap();
    }
}

fn code(value: &Value) -> &str {
    value["code"].as_str().unwrap_or_default()
}

#[tokio::test]
async fn create_route_seals_secrets() {
    let fixture = Fixture::new().await;
    let (status, created) = fixture.smtp(true, "smtp.relay.test", "tls").await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["settings"]["port"], 465);
    assert_eq!(created["settings"]["max_message_bytes"], 25_000_000);
    assert_eq!(created["secrets_set"], json!(["username", "password"]));
    let id = created["id"].as_str().unwrap();
    let raw: Vec<u8> = sqlx::query_scalar("SELECT secrets FROM mail_send_routes WHERE id = $1")
        .bind(id)
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert!(
        !String::from_utf8_lossy(&raw).contains("hunter2-secret"),
        "the column holds the password in the clear"
    );
    let (route, secrets) = fixture
        .mail
        .load_route(id.parse().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(route.kind, SendRouteKind::Smtp);
    assert_eq!(
        secrets,
        RouteSecrets::Smtp {
            username: "user".to_owned(),
            password: "hunter2-secret".to_owned()
        }
    );
    // sealed for this route only: another route's bytes do not open
    let (_, second) = fixture.smtp(true, "smtp2.relay.test", "tls").await;
    let second_id = second["id"].as_str().unwrap();
    sqlx::query("UPDATE mail_send_routes SET secrets = $1 WHERE id = $2")
        .bind(raw)
        .bind(second_id)
        .execute(fixture.database.pool())
        .await
        .unwrap();
    assert!(
        fixture
            .mail
            .load_route(second_id.parse().unwrap())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn workspace_route_refused_when_switch_off() {
    let fixture = Fixture::new().await;
    fixture.switch(false).await;
    let (status, value) = fixture.smtp(false, "203.0.113.9", "starttls").await;
    assert_eq!(
        (status, code(&value)),
        (StatusCode::FORBIDDEN, "workspace_providers_disabled")
    );
    // the root user is not bound by the switch
    let (status, _) = fixture.smtp(true, "203.0.113.9", "starttls").await;
    assert_eq!(status, StatusCode::CREATED);
    fixture.switch(true).await;
    let (status, value) = fixture.smtp(false, "203.0.113.9", "starttls").await;
    assert_eq!(status, StatusCode::CREATED, "{value}");
    assert_eq!(value["scope"], "workspace");
}

#[tokio::test]
async fn direct_needs_hostname() {
    let without = Fixture::with_hostname(None).await;
    let body = Some(json!({ "kind": "direct", "name": "Direct" }));
    let (status, value) = without.admin("POST", "/send-routes", body.clone()).await;
    assert_eq!(
        (status, code(&value)),
        (StatusCode::UNPROCESSABLE_ENTITY, "hostname_required")
    );
    let (_, list) = without.admin("GET", "/send-routes", None).await;
    assert_eq!(list["direct_available"], false);
    let with = Fixture::new().await;
    let (status, value) = with.admin("POST", "/send-routes", body).await;
    assert_eq!(status, StatusCode::CREATED, "{value}");
    assert_eq!(value["kind"], "direct");
    assert_eq!(value["secrets_set"], json!([]));
}

#[tokio::test]
async fn plain_smtp_only_for_root_private_host() {
    let fixture = Fixture::new().await;
    let (status, value) = fixture.smtp(true, "10.0.0.5", "none").await;
    assert_eq!(status, StatusCode::CREATED, "{value}");
    assert_eq!(value["settings"]["port"], 25);
    for host in ["203.0.113.9", "169.254.169.254"] {
        let (status, value) = fixture.smtp(true, host, "none").await;
        assert_eq!(
            (status, code(&value)),
            (StatusCode::UNPROCESSABLE_ENTITY, "plain_smtp_refused"),
            "{host}"
        );
    }
    // a workspace never, not even to a private host
    let (status, value) = fixture.smtp(false, "10.0.0.5", "none").await;
    assert_eq!(
        (status, code(&value)),
        (StatusCode::UNPROCESSABLE_ENTITY, "plain_smtp_refused")
    );
    // and a PostShiba route is never plain
    let (status, value) = fixture
        .admin(
            "POST",
            "/send-routes",
            Some(json!({
                "kind": "postshiba", "name": "PS",
                "secrets": { "smtp_username": "u", "smtp_password": "p", "webhook_secret": "w" },
                "settings": { "host": "10.0.0.5", "security": "none" },
            })),
        )
        .await;
    assert_eq!(
        (status, code(&value)),
        (StatusCode::UNPROCESSABLE_ENTITY, "plain_smtp_refused")
    );
}

#[tokio::test]
async fn workspace_smtp_route_refuses_private_host() {
    let fixture = Fixture::new().await;
    for host in [
        "10.0.0.5",
        "127.0.0.1",
        "169.254.169.254",
        "192.168.1.1",
        "172.16.0.9",
        "0.0.0.0",
        "224.0.0.1",
        "::1",
        "localhost",
        "fd00::1",
        "fe80::1",
        "::ffff:10.0.0.5",
    ] {
        let (status, value) = fixture.smtp(false, host, "starttls").await;
        assert_eq!(
            (status, code(&value)),
            (StatusCode::UNPROCESSABLE_ENTITY, "private_host_refused"),
            "{host}"
        );
    }
    // a workspace PostShiba route is checked as well
    let (status, value) = fixture
        .ws(
            "POST",
            "/send-routes",
            Some(json!({
                "kind": "postshiba", "name": "PS",
                "secrets": { "smtp_username": "u", "smtp_password": "p", "webhook_secret": "w" },
                "settings": { "host": "127.0.0.1" },
            })),
        )
        .await;
    assert_eq!(
        (status, code(&value)),
        (StatusCode::UNPROCESSABLE_ENTITY, "private_host_refused")
    );
    // an update cannot move a route to a private host either
    let (_, created) = fixture.smtp(false, "203.0.113.9", "starttls").await;
    let (status, value) = fixture
        .ws(
            "PATCH",
            &format!("/send-routes/{}", created["id"].as_str().unwrap()),
            Some(json!({ "settings": { "host": "10.0.0.5" } })),
        )
        .await;
    assert_eq!(
        (status, code(&value)),
        (StatusCode::UNPROCESSABLE_ENTITY, "private_host_refused")
    );
    // a base URL is for instance routes
    let (status, _) = fixture
        .ws(
            "POST",
            "/send-routes",
            Some(json!({
                "kind": "mailgun", "name": "MG",
                "secrets": { "api_key": "k", "webhook_signing_key": "s" },
                "settings": { "base_url": "https://169.254.169.254" },
            })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[test]
fn ip_classes() {
    let class = |ip: &str| classify_ip(ip.parse().unwrap());
    assert_eq!(class("203.0.113.9"), HostClass::Public);
    assert_eq!(class("8.8.8.8"), HostClass::Public);
    assert_eq!(class("2606:4700::1111"), HostClass::Public);
    assert_eq!(class("10.1.2.3"), HostClass::Private);
    assert_eq!(class("172.31.255.255"), HostClass::Private);
    assert_eq!(class("172.32.0.1"), HostClass::Public);
    assert_eq!(class("100.64.0.1"), HostClass::Private);
    assert_eq!(class("fc00::1"), HostClass::Private);
    assert_eq!(class("127.0.0.1"), HostClass::Loopback);
    assert_eq!(class("169.254.169.254"), HostClass::LinkLocal);
    assert_eq!(class("fe80::1"), HostClass::LinkLocal);
    assert_eq!(class("0.0.0.0"), HostClass::Unusable);
    assert_eq!(class("255.255.255.255"), HostClass::Unusable);
    assert_eq!(class("224.0.0.1"), HostClass::Unusable);
    assert_eq!(class("ff02::1"), HostClass::Unusable);
    assert_eq!(class("::ffff:127.0.0.1"), HostClass::Loopback);
    assert_eq!(class("::10.0.0.5"), HostClass::Unusable);
    assert_eq!(class("64:ff9b::a00:5"), HostClass::Unusable);
    assert_eq!(class("2002:a00:5::1"), HostClass::Unusable);
    assert_eq!(class("fec0::1"), HostClass::Private);
    assert_eq!(class("192.0.0.8"), HostClass::Unusable);
    assert_eq!(class("198.18.0.1"), HostClass::Unusable);
    assert_eq!(class("198.19.255.1"), HostClass::Unusable);
    assert_eq!(class("198.20.0.1"), HostClass::Public);
}

#[tokio::test]
async fn domain_falls_back_to_instance_default() {
    let fixture = Fixture::new().await;
    let domain = fixture
        .mail
        .create_domain(fixture.first, fixture.owner.id, "example.org", "request")
        .await
        .unwrap();
    assert!(
        fixture
            .mail
            .route_for_domain(domain.id)
            .await
            .unwrap()
            .is_none()
    );
    let (_, shared) = fixture.smtp(true, "smtp.relay.test", "starttls").await;
    let shared_id = shared["id"].as_str().unwrap().to_owned();
    let (status, list) = fixture
        .admin(
            "PUT",
            "/send-default",
            Some(json!({ "route_id": shared_id })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(list["default_route_id"], shared_id.as_str());
    assert_eq!(list["routes"][0]["is_default"], true);
    let (route, _) = fixture
        .mail
        .route_for_domain(domain.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(route.id.to_string(), shared_id);
    let domain_path = format!("/domains/{}/send-route", domain.id);
    let (_, own) = fixture.smtp(false, "203.0.113.9", "starttls").await;
    let own_id = own["id"].as_str().unwrap().to_owned();
    let (status, view) = fixture
        .ws("PUT", &domain_path, Some(json!({ "route_id": own_id })))
        .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["send_route"]["id"], own_id.as_str());
    let (route, _) = fixture
        .mail
        .route_for_domain(domain.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(route.id.to_string(), own_id);
    // a route in use stays
    let (status, value) = fixture
        .ws("DELETE", &format!("/send-routes/{own_id}"), None)
        .await;
    assert_eq!(
        (status, code(&value)),
        (StatusCode::CONFLICT, "route_in_use")
    );
    let (status, view) = fixture
        .ws("PUT", &domain_path, Some(json!({ "route_id": null })))
        .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["send_route"]["id"], shared_id.as_str());
    let (status, _) = fixture
        .ws("DELETE", &format!("/send-routes/{own_id}"), None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    // only an instance route is a default
    let (_, other) = fixture.smtp(false, "203.0.113.9", "starttls").await;
    let (status, _) = fixture
        .admin(
            "PUT",
            "/send-default",
            Some(json!({ "route_id": other["id"] })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    // another workspace's route is not one a domain may use
    let (status, _) = fixture
        .ws(
            "PUT",
            &domain_path,
            Some(json!({ "route_id": Id::new_v7().to_string() })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    // a route a domain picked itself stays
    fixture
        .ws("PUT", &domain_path, Some(json!({ "route_id": shared_id })))
        .await;
    let (status, value) = fixture
        .admin("DELETE", &format!("/send-routes/{shared_id}"), None)
        .await;
    assert_eq!(
        (status, code(&value)),
        (StatusCode::CONFLICT, "route_in_use")
    );
    fixture
        .ws("PUT", &domain_path, Some(json!({ "route_id": null })))
        .await;
    // the default stays until the root user clears it
    let (status, value) = fixture
        .admin("DELETE", &format!("/send-routes/{shared_id}"), None)
        .await;
    assert_eq!(
        (status, code(&value)),
        (StatusCode::CONFLICT, "route_is_default")
    );
    let (status, _) = fixture
        .admin("PUT", "/send-default", Some(json!({ "route_id": null })))
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = fixture
        .admin("DELETE", &format!("/send-routes/{shared_id}"), None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(fixture.mail.default_send_route().await.unwrap().is_none());
}

#[tokio::test]
async fn secrets_never_in_responses() {
    let fixture = Fixture::new().await;
    let (_, created) = fixture.smtp(true, "smtp.relay.test", "tls").await;
    let id = created["id"].as_str().unwrap().to_owned();
    fixture.smtp(false, "203.0.113.9", "starttls").await;
    let (_, patched) = fixture
        .admin(
            "PATCH",
            &format!("/send-routes/{id}"),
            Some(json!({ "secrets": { "username": "u2", "password": "hunter2-secret" } })),
        )
        .await;
    let (_, admin_list) = fixture.admin("GET", "/send-routes", None).await;
    let (_, ws_list) = fixture.ws("GET", "/send-routes", None).await;
    let (_, tested) = fixture
        .admin(
            "POST",
            &format!("/send-routes/{id}/test"),
            Some(json!({ "to": "me@example.net" })),
        )
        .await;
    for value in [&created, &patched, &admin_list, &ws_list, &tested] {
        let text = value.to_string();
        assert!(!text.contains("hunter2-secret"), "{text}");
        assert!(!text.contains("\"u2\""), "{text}");
    }
    assert_eq!(patched["secrets_set"], json!(["username", "password"]));
    // the workspace sees an instance route's name and kind only
    let shared = ws_list["routes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|route| route["scope"] == "instance")
        .unwrap();
    assert!(shared["settings"].is_null());
    assert_eq!(shared["secrets_set"], json!([]));
    // a wrong secret set is refused
    let (status, _) = fixture
        .admin(
            "PATCH",
            &format!("/send-routes/{id}"),
            Some(json!({ "secrets": { "api_key": "u2" } })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = fixture
        .admin(
            "POST",
            "/send-routes",
            Some(json!({ "kind": "resend", "name": "R", "secrets": { "api_key": "k" } })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn send_route_from_provider() {
    let fixture = Fixture::new().await;
    let (status, provider) = fixture
        .admin(
            "POST",
            "/providers",
            Some(json!({
                "kind": "resend", "name": "Resend",
                "secrets": { "api_key": "re_key", "webhook_secret": "whsec_x" },
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{provider}");
    let provider_id = provider["id"].as_str().unwrap().to_owned();
    // the inbound webhook's secret is not the events webhook's: the admin enters it
    let (status, _) = fixture
        .admin(
            "POST",
            &format!("/providers/{provider_id}/send-route"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, route) = fixture
        .admin(
            "POST",
            &format!("/providers/{provider_id}/send-route"),
            Some(json!({ "secrets": { "webhook_secret": "whsec_events" } })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{route}");
    assert_eq!(route["kind"], "resend");
    assert_eq!(route["provider_id"], provider_id.as_str());
    assert_eq!(route["name"], "Resend");
    assert_eq!(route["settings"]["max_message_bytes"], 25_000_000);
    let (loaded, secrets) = fixture
        .mail
        .load_route(route["id"].as_str().unwrap().parse().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        loaded.provider_id.map(|id| id.to_string()),
        Some(provider_id)
    );
    assert_eq!(
        secrets,
        RouteSecrets::Resend {
            api_key: "re_key".to_owned(),
            webhook_secret: Some("whsec_events".to_owned())
        }
    );
    // Postmark: the webhook login is Orbit's, the server token is missing
    let (_, postmark) = fixture
        .admin(
            "POST",
            "/providers",
            Some(json!({ "kind": "postmark", "name": "Postmark" })),
        )
        .await;
    let path = format!("/providers/{}/send-route", postmark["id"].as_str().unwrap());
    let (status, _) = fixture.admin("POST", &path, Some(json!({}))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, route) = fixture
        .admin(
            "POST",
            &path,
            Some(json!({ "secrets": { "server_token": "pm-token" } })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{route}");
    assert_eq!(route["settings"]["stream"], "outbound");
    assert_eq!(route["settings"]["max_message_bytes"], 10_000_000);
    assert_eq!(
        route["secrets_set"],
        json!(["server_token", "webhook_user", "webhook_password"])
    );
    // a workspace cannot reach an instance provider through its own endpoint
    let (status, _) = fixture.ws("POST", &path, Some(json!({}))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn member_cannot_manage_routes() {
    let fixture = Fixture::new().await;
    let admin = fixture.user("admin", Some((fixture.first, "admin"))).await;
    let member = fixture
        .user("member", Some((fixture.first, "member")))
        .await;
    let outsider = fixture.user("outsider", None).await;
    let path = format!("/api/v1/workspaces/{}/mail/send-routes", fixture.first);
    let body = json!({
        "kind": "smtp", "name": "R", "settings": { "host": "203.0.113.9" },
    });
    let (status, created) = fixture.raw(&admin, "POST", &path, Some(body.clone())).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let one = format!("{path}/{}", created["id"].as_str().unwrap());
    for (who, expected, expected_code) in [
        (&member, StatusCode::FORBIDDEN, "mail_forbidden"),
        (&outsider, StatusCode::NOT_FOUND, "mail_not_found"),
    ] {
        for (method, uri, payload) in [
            ("GET", path.clone(), None),
            ("POST", path.clone(), Some(body.clone())),
            ("PATCH", one.clone(), Some(json!({ "name": "X" }))),
            ("DELETE", one.clone(), None),
            (
                "POST",
                format!("{one}/test"),
                Some(json!({ "to": "a@b.test" })),
            ),
        ] {
            let (status, value) = fixture.raw(who, method, &uri, payload).await;
            assert_eq!(
                (status, code(&value)),
                (expected, expected_code),
                "{method} {uri}"
            );
        }
    }
    // the admin routes are the root user's
    for who in [&admin, &member, &outsider] {
        for (method, uri) in [
            ("GET", "/api/v1/admin/mail/send-routes"),
            ("PUT", "/api/v1/admin/mail/send-default"),
        ] {
            let payload = (method == "PUT").then(|| json!({ "route_id": null }));
            let (status, value) = fixture.raw(who, method, uri, payload).await;
            assert_eq!(
                (status, code(&value)),
                (StatusCode::FORBIDDEN, "installation_admin_required"),
                "{method} {uri}"
            );
        }
    }
    // the admin of this workspace deletes the route it made
    let (status, _) = fixture.raw(&admin, "DELETE", &one, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn send_test_reports_each_step() {
    let fixture = Fixture::new().await;
    let (_, route) = fixture.smtp(true, "smtp.relay.test", "tls").await;
    let id = route["id"].as_str().unwrap();
    let (status, result) = fixture
        .admin(
            "POST",
            &format!("/send-routes/{id}/test"),
            Some(json!({ "to": "me@example.net" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    let steps: Vec<(&str, &str)> = result["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|step| {
            (
                step["name"].as_str().unwrap(),
                step["status"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(steps[0], ("secrets", "ok"));
    assert_eq!(steps.last().unwrap().0, "delivery");
    let (status, _) = fixture
        .admin(
            "POST",
            &format!("/send-routes/{id}/test"),
            Some(json!({ "to": "not an address" })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn region_and_account_id_are_checked() {
    let fixture = Fixture::new().await;
    let ses = |region: &str| {
        json!({
            "kind": "ses", "name": "SES",
            "secrets": { "access_key_id": "a", "secret_access_key": "b" },
            "settings": { "region": region },
        })
    };
    for region in [
        "x@169.254.169.254/?",
        "us-east-1.evil.test",
        "us_east_1",
        "",
        "east-1",
        "us-east-",
        "US-EAST-1/x",
    ] {
        let (status, _) = fixture
            .admin("POST", "/send-routes", Some(ses(region)))
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{region}");
    }
    let (status, route) = fixture
        .admin("POST", "/send-routes", Some(ses("eu-west-1")))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{route}");
    let cloudflare = |account: &str| {
        json!({
            "kind": "cloudflare", "name": "CF",
            "secrets": { "api_token": "t", "event_secret": "e" },
            "settings": { "account_id": account },
        })
    };
    for account in ["../../x", "abc", "zz".repeat(16).as_str(), "a@b/?"] {
        let (status, _) = fixture
            .admin("POST", "/send-routes", Some(cloudflare(account)))
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{account}");
    }
    let (status, _) = fixture
        .admin("POST", "/send-routes", Some(cloudflare(&"0a".repeat(16))))
        .await;
    assert_eq!(status, StatusCode::CREATED);
    // a region copied from a provider goes through the same check
    let (_, provider) = fixture
        .admin(
            "POST",
            "/providers",
            Some(json!({
                "kind": "ses", "name": "SES in",
                "secrets": { "access_key_id": "a", "secret_access_key": "b" },
                "settings": { "region": "us-east-1", "bucket": "b", "topic_arn": "arn:aws:sns:us-east-1:1:t" },
            })),
        )
        .await;
    let id = provider["id"].as_str().unwrap();
    sqlx::query("UPDATE mail_providers SET settings = $1 WHERE id = $2")
        .bind(r#"{"region":"x@169.254.169.254/?","mx_hosts":[]}"#)
        .bind(id)
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let (status, _) = fixture
        .admin(
            "POST",
            &format!("/providers/{id}/send-route"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn moving_a_domain_clears_its_workspace_route() {
    let fixture = Fixture::new().await;
    let second = fixture.second_workspace().await;
    let domain = fixture
        .mail
        .create_domain(fixture.first, fixture.owner.id, "moving.example", "request")
        .await
        .unwrap();
    let (_, own) = fixture.smtp(false, "203.0.113.9", "starttls").await;
    let (_, shared) = fixture.smtp(true, "smtp.relay.test", "tls").await;
    let (status, _) = fixture
        .ws(
            "PUT",
            &format!("/domains/{}/send-route", domain.id),
            Some(json!({ "route_id": own["id"] })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let move_to = |workspace: Id| Some(json!({ "workspace_id": workspace.to_string() }));
    let (status, value) = fixture
        .admin("PUT", "/domains/moving.example/workspace", move_to(second))
        .await;
    assert!(status.is_success(), "{status} {value}");
    let stored: Option<String> =
        sqlx::query_scalar("SELECT send_route_id FROM mail_domains WHERE id = $1")
            .bind(domain.id.to_string())
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    assert!(stored.is_none());
    // an instance route stays through a move
    sqlx::query("UPDATE mail_domains SET send_route_id = $1 WHERE id = $2")
        .bind(shared["id"].as_str().unwrap())
        .bind(domain.id.to_string())
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let (status, value) = fixture
        .admin(
            "PUT",
            "/domains/moving.example/workspace",
            move_to(fixture.first),
        )
        .await;
    assert!(status.is_success(), "{status} {value}");
    let stored: Option<String> =
        sqlx::query_scalar("SELECT send_route_id FROM mail_domains WHERE id = $1")
            .bind(domain.id.to_string())
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    assert_eq!(stored.as_deref(), shared["id"].as_str());
}

#[tokio::test]
async fn one_secret_can_change_alone() {
    let fixture = Fixture::new().await;
    let (_, provider) = fixture
        .admin(
            "POST",
            "/providers",
            Some(json!({ "kind": "postmark", "name": "Postmark" })),
        )
        .await;
    let (_, route) = fixture
        .admin(
            "POST",
            &format!("/providers/{}/send-route", provider["id"].as_str().unwrap()),
            Some(json!({ "secrets": { "server_token": "old-token" } })),
        )
        .await;
    let id: Id = route["id"].as_str().unwrap().parse().unwrap();
    let (_, before) = fixture.mail.load_route(id).await.unwrap().unwrap();
    let (status, value) = fixture
        .admin(
            "PATCH",
            &format!("/send-routes/{id}"),
            Some(json!({ "secrets": { "server_token": "new-token" } })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{value}");
    let (_, after) = fixture.mail.load_route(id).await.unwrap().unwrap();
    let (
        RouteSecrets::Postmark {
            webhook_user: u1,
            webhook_password: p1,
            ..
        },
        RouteSecrets::Postmark {
            server_token,
            webhook_user: u2,
            webhook_password: p2,
        },
    ) = (before, after)
    else {
        panic!("not Postmark secrets");
    };
    assert_eq!(server_token, "new-token");
    assert_eq!((u1, p1), (u2, p2));
    // an unknown name is refused
    let (status, _) = fixture
        .admin(
            "PATCH",
            &format!("/send-routes/{id}"),
            Some(json!({ "secrets": { "nope": "x" } })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn manager_of_another_workspace_gets_nothing() {
    let fixture = Fixture::new().await;
    let second = fixture.second_workspace().await;
    let b_manager = fixture.user("bmanager", Some((second, "admin"))).await;
    let a_path = format!("/api/v1/workspaces/{}/mail/send-routes", fixture.first);
    let b_path = format!("/api/v1/workspaces/{second}/mail/send-routes");
    let (_, a_route) = fixture.smtp(false, "203.0.113.9", "starttls").await;
    let (_, shared) = fixture.smtp(true, "smtp.relay.test", "tls").await;
    let a_one = format!("{a_path}/{}", a_route["id"].as_str().unwrap());
    // B's manager reaches A's route through B's own path: not found
    let b_via_b = format!("{b_path}/{}", a_route["id"].as_str().unwrap());
    let shared_via_b = format!("{b_path}/{}", shared["id"].as_str().unwrap());
    for uri in [&b_via_b, &shared_via_b] {
        for (method, payload) in [
            ("PATCH", Some(json!({ "name": "X" }))),
            ("DELETE", None),
            ("POST", Some(json!({ "to": "a@b.test" }))),
        ] {
            let uri = if method == "POST" {
                format!("{uri}/test")
            } else {
                uri.clone()
            };
            let (status, _) = fixture.raw(&b_manager, method, &uri, payload).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{method} {uri}");
        }
    }
    // and through A's path: not a member of A
    let (status, value) = fixture.raw(&b_manager, "GET", &a_path, None).await;
    assert_eq!(
        (status, code(&value)),
        (StatusCode::NOT_FOUND, "mail_not_found")
    );
    for (method, payload) in [("PATCH", Some(json!({ "name": "X" }))), ("DELETE", None)] {
        let (status, value) = fixture.raw(&b_manager, method, &a_one, payload).await;
        assert_eq!(
            (status, code(&value)),
            (StatusCode::NOT_FOUND, "mail_not_found"),
            "{method}"
        );
    }
    // B's list does not show A's route
    let (_, listed) = fixture.raw(&b_manager, "GET", &b_path, None).await;
    let ids: Vec<&str> = listed["routes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect();
    assert!(!ids.contains(&a_route["id"].as_str().unwrap()));
    // a domain of B cannot use A's route; A's domain cannot use B's route
    let (_, b_route) = fixture
        .raw(
            &fixture.owner,
            "POST",
            &b_path,
            Some(json!({ "kind": "smtp", "name": "B", "settings": { "host": "203.0.113.10" } })),
        )
        .await;
    let a_domain = fixture
        .mail
        .create_domain(fixture.first, fixture.owner.id, "a.example", "request")
        .await
        .unwrap();
    let (status, _) = fixture
        .ws(
            "PUT",
            &format!("/domains/{}/send-route", a_domain.id),
            Some(json!({ "route_id": b_route["id"] })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let b_domain = fixture
        .mail
        .create_domain(second, fixture.owner.id, "b.example", "request")
        .await
        .unwrap();
    let (status, _) = fixture
        .raw(
            &b_manager,
            "PUT",
            &format!(
                "/api/v1/workspaces/{second}/mail/domains/{}/send-route",
                b_domain.id
            ),
            Some(json!({ "route_id": a_route["id"] })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    // B's manager cannot change A's domain either
    let (status, _) = fixture
        .raw(
            &b_manager,
            "PUT",
            &format!(
                "/api/v1/workspaces/{second}/mail/domains/{}/send-route",
                a_domain.id
            ),
            Some(json!({ "route_id": null })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---- Settings fields (Task G3) ----

#[tokio::test]
async fn domain_view_has_own_route_and_clears_it() {
    let fixture = Fixture::new().await;
    let domain = fixture
        .mail
        .create_domain(fixture.first, fixture.owner.id, "example.org", "request")
        .await
        .unwrap();
    let (_, shared) = fixture.smtp(true, "smtp.relay.test", "starttls").await;
    let shared_id = shared["id"].as_str().unwrap().to_owned();
    fixture
        .admin(
            "PUT",
            "/send-default",
            Some(json!({ "route_id": shared_id })),
        )
        .await;
    let path = format!("/domains/{}/send-route", domain.id);
    let (status, view) = fixture
        .ws("GET", &format!("/domains/{}", domain.id), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["send_route"]["id"], shared_id.as_str());
    assert!(view["own_send_route_id"].is_null(), "{view}");
    assert_eq!(view["dkim_rotating"], false);
    let (_, own) = fixture.smtp(false, "203.0.113.9", "starttls").await;
    let own_id = own["id"].as_str().unwrap().to_owned();
    let (_, view) = fixture
        .ws("PUT", &path, Some(json!({ "route_id": own_id })))
        .await;
    assert_eq!(view["own_send_route_id"], own_id.as_str(), "{view}");
    assert_eq!(view["send_route"]["id"], own_id.as_str());
    let (status, view) = fixture
        .ws("PUT", &path, Some(json!({ "route_id": null })))
        .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert!(view["own_send_route_id"].is_null(), "{view}");
    assert_eq!(view["send_route"]["id"], shared_id.as_str());
}

#[tokio::test]
async fn event_credentials_are_generated_once() {
    let fixture = Fixture::new().await;
    let (status, route) = fixture
        .admin(
            "POST",
            "/send-routes",
            Some(json!({ "kind": "postmark", "name": "PM",
                         "secrets": { "server_token": "tok" } })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{route}");
    let user = route["generated"]["webhook_user"].as_str().unwrap();
    let password = route["generated"]["webhook_password"].as_str().unwrap();
    assert!(user.starts_with("orbit-") && user.len() == 18, "{user}");
    assert_eq!(password.len(), 64);
    assert!(route["generated"].get("server_token").is_none());
    let id: Id = route["id"].as_str().unwrap().parse().unwrap();
    let (_, secrets) = fixture.mail.load_route(id).await.unwrap().unwrap();
    assert_eq!(
        secrets,
        RouteSecrets::Postmark {
            server_token: "tok".to_owned(),
            webhook_user: user.to_owned(),
            webhook_password: password.to_owned(),
        }
    );
    // Shown once: a list does not carry them.
    let (_, list) = fixture.admin("GET", "/send-routes", None).await;
    assert!(list["routes"][0]["generated"].is_null(), "{list}");
    // Given values are kept and not echoed.
    let (_, given) = fixture
        .admin(
            "POST",
            "/send-routes",
            Some(json!({ "kind": "postmark", "name": "PM2",
                         "secrets": { "server_token": "t", "webhook_user": "me",
                                      "webhook_password": "pw" } })),
        )
        .await;
    assert!(given["generated"].is_null(), "{given}");
    // Cloudflare: the event secret.
    let (status, cloudflare) = fixture
        .admin(
            "POST",
            "/send-routes",
            Some(json!({ "kind": "cloudflare", "name": "CF",
                         "secrets": { "api_token": "t" },
                         "settings": { "account_id": "0123456789abcdef0123456789abcdef" } })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{cloudflare}");
    assert_eq!(
        cloudflare["generated"]["event_secret"]
            .as_str()
            .unwrap()
            .len(),
        64
    );
    assert_eq!(cloudflare["event_secret_from_provider"], false);
}

#[tokio::test]
async fn cloudflare_route_from_provider_keeps_the_signing_secret() {
    let fixture = Fixture::new().await;
    let (status, provider) = fixture
        .admin(
            "POST",
            "/providers",
            Some(json!({ "kind": "cloudflare", "name": "CF" })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{provider}");
    let (status, route) = fixture
        .admin(
            "POST",
            &format!("/providers/{}/send-route", provider["id"].as_str().unwrap()),
            Some(json!({ "secrets": { "api_token": "t" },
                         "settings": { "account_id": "0123456789abcdef0123456789abcdef" } })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{route}");
    assert_eq!(route["event_secret_from_provider"], true);
    assert!(route["generated"].is_null(), "{route}");
    // The flag is a property of the route: the list and a change answer carry it too.
    let id = route["id"].as_str().unwrap();
    let (_, list) = fixture.admin("GET", "/send-routes", None).await;
    assert_eq!(
        list["routes"][0]["event_secret_from_provider"], true,
        "{list}"
    );
    let (status, patched) = fixture
        .admin(
            "PATCH",
            &format!("/send-routes/{id}"),
            Some(json!({ "name": "CF 2" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{patched}");
    assert_eq!(patched["event_secret_from_provider"], true);
    assert!(patched["generated"].is_null(), "{patched}");
    // It takes no event secret of its own, at creation or later.
    let (status, _) = fixture
        .admin(
            "PATCH",
            &format!("/send-routes/{id}"),
            Some(json!({ "secrets": { "event_secret": "mine" } })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = fixture
        .admin(
            "POST",
            &format!("/providers/{}/send-route", provider["id"].as_str().unwrap()),
            Some(
                json!({ "secrets": { "api_token": "t", "event_secret": "mine" },
                         "settings": { "account_id": "0123456789abcdef0123456789abcdef" } }),
            ),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn route_view_has_events_url() {
    let fixture = Fixture::new().await;
    let (_, mailgun) = fixture
        .admin(
            "POST",
            "/send-routes",
            Some(json!({ "kind": "mailgun", "name": "MG",
                         "secrets": { "api_key": "k", "webhook_signing_key": "s" },
                         "settings": {} })),
        )
        .await;
    let id = mailgun["id"].as_str().unwrap();
    assert_eq!(
        mailgun["events_url"],
        format!("https://orbit.test/api/v1/mail/events/mailgun/{id}")
    );
    let (_, smtp) = fixture.smtp(true, "smtp.relay.test", "tls").await;
    assert!(smtp["events_url"].is_null(), "{smtp}");
}

#[tokio::test]
async fn smtp_login_can_be_cleared() {
    let fixture = Fixture::new().await;
    let (_, route) = fixture.smtp(true, "smtp.relay.test", "tls").await;
    let id: Id = route["id"].as_str().unwrap().parse().unwrap();
    assert_eq!(route["secrets_set"], json!(["username", "password"]));
    let (status, cleared) = fixture
        .admin(
            "PATCH",
            &format!("/send-routes/{id}"),
            Some(json!({ "clear_login": true })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{cleared}");
    assert_eq!(cleared["secrets_set"], json!([]));
    assert!(cleared["generated"].is_null(), "{cleared}");
    let audited: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events WHERE action = 'mail.send_route_updated' \
         AND CAST(metadata_json AS TEXT) LIKE '%login_cleared%true%'",
    )
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(audited, 1);
    let (_, secrets) = fixture.mail.load_route(id).await.unwrap().unwrap();
    assert_eq!(secrets, RouteSecrets::None);
    // Not together with new secrets, and not for other kinds.
    let (status, _) = fixture
        .admin(
            "PATCH",
            &format!("/send-routes/{id}"),
            Some(json!({ "clear_login": true, "secrets": { "username": "u", "password": "p" } })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (_, postmark) = fixture
        .admin(
            "POST",
            "/send-routes",
            Some(json!({ "kind": "postmark", "name": "PM", "secrets": { "server_token": "t" } })),
        )
        .await;
    let (status, _) = fixture
        .admin(
            "PATCH",
            &format!("/send-routes/{}", postmark["id"].as_str().unwrap()),
            Some(json!({ "clear_login": true })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn admin_tier_defaults() {
    let fixture = Fixture::new().await;
    let (status, tiers) = fixture.admin("GET", "/send-tiers", None).await;
    assert_eq!(status, StatusCode::OK, "{tiers}");
    assert_eq!(tiers["new"]["per_user_hour"], 30);
    assert_eq!(tiers["standard"]["max_recipients"], 100);
    let mut changed = tiers.clone();
    changed["new"]["per_user_hour"] = json!(40);
    changed["standard"]["per_ws_day"] = json!(20_000);
    let (status, saved) = fixture
        .admin("PUT", "/send-tiers", Some(changed.clone()))
        .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    let (_, read) = fixture.admin("GET", "/send-tiers", None).await;
    assert_eq!(read, changed);
    for bad in [0, 10_000_001] {
        let mut invalid = changed.clone();
        invalid["new"]["max_recipients"] = json!(bad);
        let (status, _) = fixture.admin("PUT", "/send-tiers", Some(invalid)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{bad}");
    }
    let (status, _) = fixture
        .admin("PUT", "/send-tiers", Some(json!({ "new": changed["new"] })))
        .await;
    assert!(status.is_client_error());
    let audited: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events WHERE action = 'instance.mail_send_tiers_updated'",
    )
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(audited, 1);
    // A saved number reaches the limits a workspace has (its tier is standard here).
    let (_, policy) = fixture
        .admin("GET", &format!("/send-policies/{}", fixture.first), None)
        .await;
    assert_eq!(policy["limits"]["per_ws_day"], 20_000, "{policy}");
    // Root only: an authenticated workspace admin gets nothing, for GET and PUT.
    let admin = fixture.user("admin", Some((fixture.first, "admin"))).await;
    for (method, body) in [("GET", None), ("PUT", Some(changed.clone()))] {
        let (status, value) = fixture
            .raw(&admin, method, "/api/v1/admin/mail/send-tiers", body)
            .await;
        assert!(
            status == StatusCode::FORBIDDEN || status == StatusCode::NOT_FOUND,
            "{method} {status} {value}"
        );
    }
    let (_, read) = fixture.admin("GET", "/send-tiers", None).await;
    assert_eq!(read, changed);
}
