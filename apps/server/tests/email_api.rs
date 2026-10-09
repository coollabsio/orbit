//! Mail settings, open registration, emailed password reset and emailed invitations.

use std::sync::Arc;
use std::time::Duration;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{AuthenticatedUser, Id, PasswordService, TestDatabase, TimestampMillis};
use orbit_server::auth_routes::{AuthState, CookieMode, auth_router};
use orbit_server::mail::{Mailer, Outbox, OutgoingEmail};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::workspace_routes::{WorkspaceState, workspace_router};
use serde_json::{Value, json};
use tower::ServiceExt;

const ORIGIN: &str = "https://orbit.test";

struct Fixture {
    _database: TestDatabase,
    identity: Arc<IdentityRepository>,
    root_cookie: String,
    workspace_id: String,
}

/// A root account with a workspace.
async fn fixture() -> Fixture {
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
                email: "root@example.com".to_owned(),
                display_name: "Root".to_owned(),
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
    Fixture {
        _database: database,
        identity,
        root_cookie: format!("__Host-orbit_session={}", setup.session.token),
        workspace_id: setup.workspace_id.to_string(),
    }
}

/// The auth and workspace routes, sharing `mailer` when given.
fn router(identity: &Arc<IdentityRepository>, mailer: Option<Mailer>) -> axum::Router {
    let mut auth = AuthState::new(Arc::clone(identity), CookieMode::secure());
    let mut workspaces = WorkspaceState::new(
        Arc::clone(identity),
        ORIGIN.to_owned(),
        CookieMode::secure(),
    );
    if let Some(mailer) = mailer {
        auth = auth.with_mailer(mailer.clone());
        workspaces = workspaces.with_mailer(mailer);
    }
    auth_router(auth).merge(workspace_router(workspaces))
}

async fn call(
    app: &axum::Router,
    method: &str,
    uri: &str,
    cookie: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut request = Request::builder().method(method).uri(uri);
    if let Some(cookie) = cookie {
        request = request.header(header::COOKIE, cookie);
    }
    let request = match body {
        Some(body) => request
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string())),
        None => request.body(Body::empty()),
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

fn smtp_body() -> Value {
    json!({
        "host": "smtp.example.com", "port": 587, "security": "starttls", "username": "orbit",
        "password": "s3cret-smtp-password", "from_address": "orbit@example.com", "from_name": "Orbit"
    })
}

/// Waits for background sends.
async fn sent(outbox: &Outbox, count: usize) -> Vec<OutgoingEmail> {
    for _ in 0..100 {
        if outbox.lock().unwrap().len() >= count {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    outbox.lock().unwrap().clone()
}

fn token_in(body: &str, prefix: &str) -> String {
    let start = body.find(prefix).unwrap() + prefix.len();
    body[start..].split_whitespace().next().unwrap().to_owned()
}

#[tokio::test]
async fn only_the_root_user_saves_mail_settings_and_the_password_is_encrypted() {
    let fixture = fixture().await;
    let app = &router(
        &fixture.identity,
        Some(Mailer::new(
            fixture.identity.database().clone(),
            Some([9; 32]),
            ORIGIN,
        )),
    );
    let root = Some(fixture.root_cookie.as_str());

    let member = Id::new_v7();
    sqlx::query("INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at) VALUES (?, 'm@example.com', 'm@example.com', 'M', 'x', 0, 0)")
        .bind(member.to_string())
        .execute(fixture.identity.database().pool())
        .await
        .unwrap();
    let member_session = fixture
        .identity
        .create_session(
            &AuthenticatedUser {
                id: member,
                email: "m@example.com".to_owned(),
                display_name: "M".to_owned(),
            },
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    let member_cookie = format!("__Host-orbit_session={}", member_session.token);
    assert_eq!(
        call(
            app,
            "GET",
            "/api/v1/admin/settings",
            Some(&member_cookie),
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );

    let (status, body) = call(
        app,
        "PUT",
        "/api/v1/admin/settings/registration",
        root,
        Some(json!({"open": true})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "email_not_configured");
    assert_eq!(
        call(app, "GET", "/api/v1/auth/options", None, None).await.1,
        json!({"registration_open": false, "email_enabled": false, "passkeys_enabled": false})
    );

    let (status, body) = call(
        app,
        "PUT",
        "/api/v1/admin/settings/smtp",
        root,
        Some(smtp_body()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["smtp"]["password_set"], true);
    assert!(body["smtp"].get("password").is_none());
    let stored: Vec<u8> = sqlx::query_scalar("SELECT smtp_password FROM instance_settings")
        .fetch_one(fixture.identity.database().pool())
        .await
        .unwrap();
    assert!(!String::from_utf8_lossy(&stored).contains("s3cret-smtp-password"));

    // Saving without a password keeps the stored one.
    let mut without_password = smtp_body();
    without_password.as_object_mut().unwrap().remove("password");
    let (_, body) = call(
        app,
        "PUT",
        "/api/v1/admin/settings/smtp",
        root,
        Some(without_password),
    )
    .await;
    assert_eq!(body["smtp"]["password_set"], true);
    let (status, _) = call(app, "PUT", "/api/v1/admin/settings/smtp", root, Some(json!({
        "host": "smtp.example.com", "port": 587, "security": "starttls", "from_address": "not an address"
    }))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let (status, body) = call(
        app,
        "PUT",
        "/api/v1/admin/settings/registration",
        root,
        Some(json!({"open": true})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["registration_open"], true);
    assert_eq!(
        call(app, "GET", "/api/v1/auth/options", None, None).await.1,
        json!({"registration_open": true, "email_enabled": true, "passkeys_enabled": false})
    );

    // Removing the mail server closes registration.
    let (_, body) = call(app, "DELETE", "/api/v1/admin/settings/smtp", root, None).await;
    assert_eq!(body["registration_open"], false);
    assert!(body["smtp"].is_null());
}

#[tokio::test]
async fn open_registration_emails_a_link_that_creates_a_verified_account_once() {
    let fixture = fixture().await;
    let (mailer, outbox) = Mailer::recording(fixture.identity.database().clone(), ORIGIN);
    let app = router(&fixture.identity, Some(mailer));
    let root = Some(fixture.root_cookie.as_str());

    // Closed by default after setup.
    let (status, body) = call(
        &app,
        "POST",
        "/api/v1/auth/register/request",
        None,
        Some(json!({"email": "new@example.com"})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["code"], "registration_closed");

    call(
        &app,
        "PUT",
        "/api/v1/admin/settings/smtp",
        root,
        Some(smtp_body()),
    )
    .await;
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/settings/registration",
            root,
            Some(json!({"open": true}))
        )
        .await
        .0,
        StatusCode::OK
    );

    // An existing address gets the same answer and no email.
    let existing = call(
        &app,
        "POST",
        "/api/v1/auth/register/request",
        None,
        Some(json!({"email": "root@example.com"})),
    )
    .await;
    let fresh = call(
        &app,
        "POST",
        "/api/v1/auth/register/request",
        None,
        Some(json!({"email": "New@Example.com"})),
    )
    .await;
    assert_eq!(existing, fresh);
    assert_eq!(fresh.0, StatusCode::ACCEPTED);
    let emails = sent(&outbox, 1).await;
    assert_eq!(emails.len(), 1);
    assert_eq!(emails[0].to, "New@Example.com");
    let token = token_in(&emails[0].body, "https://orbit.test/register?token=");

    let complete = json!({"token": token, "display_name": "New Person", "password": "another long password 42"});
    let (status, _) = call(
        &app,
        "POST",
        "/api/v1/auth/register/complete",
        None,
        Some(complete.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let verified: Option<i64> = sqlx::query_scalar(
        "SELECT email_verified_at FROM users WHERE normalized_email = 'new@example.com'",
    )
    .fetch_one(fixture.identity.database().pool())
    .await
    .unwrap();
    assert!(verified.is_some());
    let (status, body) = call(
        &app,
        "POST",
        "/api/v1/auth/register/complete",
        None,
        Some(complete),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "invalid_registration_token");
}

#[tokio::test]
async fn password_reset_is_emailed_only_with_a_mail_server_and_never_reveals_accounts() {
    let fixture = fixture().await;
    let request = json!({"email": "root@example.com"});
    let (status, body) = call(
        &router(&fixture.identity, None),
        "POST",
        "/api/v1/auth/recovery/request",
        None,
        Some(request.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert!(body["detail"].as_str().unwrap().contains("administrator"));

    let (mailer, outbox) = Mailer::recording(fixture.identity.database().clone(), ORIGIN);
    let app = router(&fixture.identity, Some(mailer));
    let unknown = call(
        &app,
        "POST",
        "/api/v1/auth/recovery/request",
        None,
        Some(json!({"email": "nobody@example.com"})),
    )
    .await;
    let known = call(
        &app,
        "POST",
        "/api/v1/auth/recovery/request",
        None,
        Some(request),
    )
    .await;
    assert_eq!(unknown, known);
    let emails = sent(&outbox, 1).await;
    assert_eq!(emails.len(), 1);
    assert_eq!(emails[0].to, "root@example.com");
    let token = token_in(&emails[0].body, "https://orbit.test/recovery?token=");
    assert!(
        fixture
            .identity
            .recovery_token_valid(&token, TimestampMillis::now())
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn emailed_invitations_need_a_mail_server_and_send_the_link_instead_of_returning_it() {
    let fixture = fixture().await;
    let uri = format!("/api/v1/workspaces/{}/invitations", fixture.workspace_id);
    let invite = json!({"email": "guest@example.com", "role": "member", "delivery": "smtp"});
    let (status, body) = call(
        &router(&fixture.identity, None),
        "POST",
        &uri,
        Some(&fixture.root_cookie),
        Some(invite.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "email_not_configured");

    let (mailer, outbox) = Mailer::recording(fixture.identity.database().clone(), ORIGIN);
    let app = router(&fixture.identity, Some(mailer));
    let (status, body) = call(&app, "POST", &uri, Some(&fixture.root_cookie), Some(invite)).await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(body.get("url").is_none());
    let emails = sent(&outbox, 1).await;
    assert_eq!(emails[0].to, "guest@example.com");
    assert_eq!(emails[0].subject, "Root invited you to Orbit on Orbit");
    assert!(
        emails[0]
            .body
            .contains("https://orbit.test/accept-invitation?token=")
    );
}
