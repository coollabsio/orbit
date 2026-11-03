//! Deleting your own account and changing its email, over the auth, workspace and task routers.

use std::sync::Arc;
use std::time::Duration;

use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode, header};
use orbit_platform::{AuthenticatedUser, Id, PasswordService, TestDatabase, TimestampMillis};
use orbit_server::auth_routes::{AuthState, CookieMode, auth_router};
use orbit_server::mail::{Mailer, Outbox, OutgoingEmail};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::task_routes::{TaskState, task_router};
use orbit_server::workspace_routes::{WorkspaceState, workspace_router};
use serde_json::{Value, json};
use tower::ServiceExt;

const ORIGIN: &str = "https://orbit.test";
const PASSWORD: &str = "correct horse battery";
const COOKIE: &str = "__Host-orbit_session";

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Value,
}

impl Reply {
    fn code(&self) -> &str {
        self.body["code"].as_str().unwrap_or_default()
    }
}

struct Fixture {
    database: TestDatabase,
    identity: Arc<IdentityRepository>,
    app: axum::Router,
    outbox: Outbox,
    root_cookie: String,
    workspace_id: String,
    project_id: String,
    status_id: String,
}

impl Fixture {
    /// A root account with a workspace; a mail server unless `mail` is false.
    async fn new(mail: bool) -> Self {
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
                    password_hash: PasswordService::default().hash(PASSWORD).unwrap(),
                    workspace_name: "Orbit".to_owned(),
                    project_name: "General".to_owned(),
                },
                now,
            )
            .await
            .unwrap();
        let status_id: String = sqlx::query_scalar(
            "SELECT id FROM task_statuses WHERE project_id = $1 ORDER BY position LIMIT 1",
        )
        .bind(setup.project_id.to_string())
        .fetch_one(database.pool())
        .await
        .unwrap();
        let (mailer, outbox) = Mailer::recording((*database).clone(), ORIGIN);
        let mut auth = AuthState::new(Arc::clone(&identity), CookieMode::secure());
        let mut workspaces = WorkspaceState::new(
            Arc::clone(&identity),
            ORIGIN.to_owned(),
            CookieMode::secure(),
        );
        if mail {
            auth = auth.with_mailer(mailer.clone());
            workspaces = workspaces.with_mailer(mailer);
        }
        let app = auth_router(auth)
            .merge(workspace_router(workspaces))
            .merge(task_router(TaskState::new(
                Arc::clone(&identity),
                CookieMode::secure(),
            )));
        Self {
            database,
            identity,
            app,
            outbox,
            root_cookie: format!("{COOKIE}={}", setup.session.token),
            workspace_id: setup.workspace_id.to_string(),
            project_id: setup.project_id.to_string(),
            status_id,
        }
    }

    async fn send(
        &self,
        method: &str,
        uri: &str,
        cookie: Option<&str>,
        body: Option<Value>,
    ) -> Reply {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::ORIGIN, ORIGIN);
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
        let response = self.app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        Reply {
            status,
            headers,
            body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        }
    }

    async fn bearer(&self, method: &str, uri: &str, token: &str) -> StatusCode {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap();
        self.app.clone().oneshot(request).await.unwrap().status()
    }

    /// An account with `password` (none: a provider-only account), a member of the root workspace when `member`.
    /// Returns its id and a fresh session cookie.
    async fn add_user(&self, email: &str, password: Option<&str>, member: bool) -> (Id, String) {
        let id = Id::new_v7();
        let now = TimestampMillis::now();
        let hash = password.map_or_else(
            || "!".to_owned(),
            |password| PasswordService::default().hash(password).unwrap(),
        );
        sqlx::query(
            "INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at) \
             VALUES ($1, $2, $3, 'Ada', $4, $5, $5)",
        )
        .bind(id.to_string())
        .bind(email)
        .bind(email.to_lowercase())
        .bind(hash)
        .bind(now.as_millis())
        .execute(self.database.pool())
        .await
        .unwrap();
        if member {
            sqlx::query(
                "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at) \
                 VALUES ($1, $2, $3, 'member', 0, $4, $4)",
            )
            .bind(Id::new_v7().to_string())
            .bind(&self.workspace_id)
            .bind(id.to_string())
            .bind(now.as_millis())
            .execute(self.database.pool())
            .await
            .unwrap();
        }
        (
            id,
            format!("{COOKIE}={}", self.session_token(id, email).await),
        )
    }

    async fn session_token(&self, id: Id, email: &str) -> String {
        self.identity
            .create_session(
                &AuthenticatedUser {
                    id,
                    email: email.to_owned(),
                    display_name: "Ada".to_owned(),
                },
                TimestampMillis::now(),
            )
            .await
            .unwrap()
            .token
    }

    async fn scalar(&self, sql: &'static str, bind: &str) -> i64 {
        sqlx::query_scalar(sql)
            .bind(bind)
            .fetch_one(self.database.pool())
            .await
            .unwrap()
    }

    async fn login(&self, email: &str, password: &str) -> StatusCode {
        self.send(
            "POST",
            "/api/v1/auth/login",
            None,
            Some(json!({"email": email, "password": password})),
        )
        .await
        .status
    }

    /// Waits for `count` background emails.
    async fn sent(&self, count: usize) -> Vec<OutgoingEmail> {
        for _ in 0..100 {
            if self.outbox.lock().unwrap().len() >= count {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
        self.outbox.lock().unwrap().clone()
    }
}

fn token_in(body: &str, prefix: &str) -> String {
    let start = body.find(prefix).unwrap() + prefix.len();
    body[start..].split_whitespace().next().unwrap().to_owned()
}

#[tokio::test]
async fn deleting_an_account_is_refused_for_root_while_it_owns_workspaces_and_without_proof() {
    let fixture = Fixture::new(true).await;
    let root = fixture
        .send(
            "DELETE",
            "/api/v1/auth/me",
            Some(&fixture.root_cookie),
            Some(json!({"email": "root@example.com", "password": PASSWORD})),
        )
        .await;
    assert_eq!(
        (root.status, root.code()),
        (StatusCode::CONFLICT, "root_account")
    );

    let (_, cookie) = fixture
        .add_user("ada@example.com", Some(PASSWORD), true)
        .await;
    let created = fixture
        .send(
            "POST",
            "/api/v1/workspaces",
            Some(&cookie),
            Some(json!({"name": "Side project"})),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let side_id = created.body["id"].as_str().unwrap().to_owned();
    let delete = |body: Value| fixture.send("DELETE", "/api/v1/auth/me", Some(&cookie), Some(body));

    let mismatch = delete(json!({"email": "someone@example.com", "password": PASSWORD})).await;
    assert_eq!(
        (mismatch.status, mismatch.code()),
        (StatusCode::UNPROCESSABLE_ENTITY, "confirmation_mismatch")
    );
    let missing = delete(json!({"email": "ada@example.com"})).await;
    assert_eq!(
        (missing.status, missing.code()),
        (StatusCode::UNAUTHORIZED, "password_required")
    );
    let wrong = delete(json!({"email": "ada@example.com", "password": "not the password"})).await;
    assert_eq!(
        (wrong.status, wrong.code()),
        (StatusCode::UNAUTHORIZED, "invalid_credentials")
    );
    let owns = delete(json!({"email": " Ada@Example.com ", "password": PASSWORD})).await;
    assert_eq!(
        (owns.status, owns.code()),
        (StatusCode::CONFLICT, "owns_workspaces")
    );
    assert_eq!(
        owns.body["workspaces"],
        json!([{"id": side_id, "name": "Side project"}])
    );

    // A workspace in the trash does not block: it is purged later.
    let version = fixture
        .scalar("SELECT version FROM workspaces WHERE id = $1", &side_id)
        .await;
    let trashed = fixture
        .send(
            "DELETE",
            &format!("/api/v1/workspaces/{side_id}?expected_version={version}"),
            Some(&cookie),
            None,
        )
        .await;
    assert_eq!(trashed.status, StatusCode::NO_CONTENT, "{}", trashed.body);
    let deleted = delete(json!({"email": "ada@example.com", "password": PASSWORD})).await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);
    // The trashed workspace keeps its owner membership until the purge removes both.
    assert_eq!(
        fixture
            .scalar(
                "SELECT COUNT(*) FROM memberships WHERE workspace_id = $1",
                &side_id
            )
            .await,
        1
    );
}

#[tokio::test]
async fn a_deleted_account_is_anonymized_signed_out_everywhere_and_frees_its_email() {
    let fixture = Fixture::new(true).await;
    let (ada, cookie) = fixture
        .add_user("ada@example.com", Some(PASSWORD), true)
        .await;
    let bearer = fixture.session_token(ada, "ada@example.com").await;
    let task = fixture
        .send(
            "POST",
            &format!("/api/v1/workspaces/{}/tasks", fixture.workspace_id),
            Some(&cookie),
            Some(json!({"project_id": fixture.project_id, "status_id": fixture.status_id, "title": "Ada's task"})),
        )
        .await;
    assert_eq!(task.status, StatusCode::CREATED, "{}", task.body);
    let task_id = task.body["id"].as_str().unwrap().to_owned();
    let comments = format!(
        "/api/v1/workspaces/{}/tasks/{task_id}/comments",
        fixture.workspace_id
    );
    let comment = fixture
        .send(
            "POST",
            &comments,
            Some(&cookie),
            Some(json!({"body": "Ada was here"})),
        )
        .await;
    assert_eq!(comment.status, StatusCode::CREATED, "{}", comment.body);

    let deleted = fixture
        .send(
            "DELETE",
            "/api/v1/auth/me",
            Some(&cookie),
            Some(json!({"email": "ada@example.com", "password": PASSWORD})),
        )
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);
    assert_eq!(
        deleted.headers[header::SET_COOKIE].to_str().unwrap(),
        format!("{COOKIE}=; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age=0")
    );

    assert_eq!(
        fixture
            .send("GET", "/api/v1/auth/me", Some(&cookie), None)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        fixture.bearer("GET", "/api/v1/auth/me", &bearer).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        fixture.login("ada@example.com", PASSWORD).await,
        StatusCode::UNAUTHORIZED
    );
    let ada_id = ada.to_string();
    for (sql, expected) in [
        ("SELECT COUNT(*) FROM memberships WHERE user_id = $1", 0),
        ("SELECT COUNT(*) FROM sessions WHERE user_id = $1", 0),
        (
            "SELECT COUNT(*) FROM users WHERE id = $1 AND deleted_at IS NOT NULL \
             AND suspended_at IS NOT NULL AND password_hash = '!'",
            1,
        ),
        (
            "SELECT COUNT(*) FROM audit_events WHERE actor_id = $1 AND action = 'account.deleted'",
            1,
        ),
    ] {
        assert_eq!(fixture.scalar(sql, &ada_id).await, expected, "{sql}");
    }
    let (name, email): (String, String) =
        sqlx::query_as("SELECT display_name, email FROM users WHERE id = $1")
            .bind(&ada_id)
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    assert_eq!(
        (name.as_str(), email),
        ("Deleted user", format!("deleted-{ada_id}@deleted.invalid"))
    );

    // What Ada wrote keeps its author.
    let listed = fixture
        .send("GET", &comments, Some(&fixture.root_cookie), None)
        .await;
    assert_eq!(listed.body["items"][0]["body"], "Ada was here");
    assert_eq!(listed.body["items"][0]["author_id"], ada_id.as_str());

    // Admin lists the account as deleted and cannot reinstate it.
    let users = fixture
        .send(
            "GET",
            "/api/v1/admin/users?q=deleted",
            Some(&fixture.root_cookie),
            None,
        )
        .await;
    let row = &users.body["items"][0];
    assert_eq!(
        (
            row["id"].as_str(),
            row["display_name"].as_str(),
            row["deleted"].as_bool(),
            row["suspended"].as_bool()
        ),
        (
            Some(ada_id.as_str()),
            Some("Deleted user"),
            Some(true),
            Some(true)
        )
    );
    let reinstate = fixture
        .send(
            "POST",
            &format!("/api/v1/admin/users/{ada_id}/suspension"),
            Some(&fixture.root_cookie),
            Some(json!({"suspended": false})),
        )
        .await;
    assert_eq!(
        (reinstate.status, reinstate.code()),
        (StatusCode::CONFLICT, "account_deleted")
    );

    // The address can register again.
    fixture
        .database
        .execute("UPDATE instance_settings SET registration_open = 1 WHERE id = 1")
        .await
        .unwrap();
    let request = fixture
        .send(
            "POST",
            "/api/v1/auth/register/request",
            None,
            Some(json!({"email": "ada@example.com"})),
        )
        .await;
    assert_eq!(request.status, StatusCode::ACCEPTED);
    let emails = fixture.sent(1).await;
    assert_eq!(emails.len(), 1);
    let token = token_in(&emails[0].body, "https://orbit.test/register?token=");
    let registered = fixture
        .send(
            "POST",
            "/api/v1/auth/register/complete",
            None,
            Some(json!({"token": token, "display_name": "Ada Again", "password": "another long password 42"})),
        )
        .await;
    assert_eq!(
        registered.status,
        StatusCode::CREATED,
        "{}",
        registered.body
    );
    assert_ne!(registered.body["user_id"], ada_id.as_str());
}

#[tokio::test]
async fn an_account_without_a_password_deletes_itself_after_a_fresh_sign_in() {
    let fixture = Fixture::new(true).await;
    let (ada, cookie) = fixture.add_user("ada@example.com", None, true).await;
    sqlx::query("UPDATE sessions SET created_at = created_at - 3600000 WHERE user_id = $1")
        .bind(ada.to_string())
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let body = json!({"email": "ada@example.com"});
    let stale = fixture
        .send(
            "DELETE",
            "/api/v1/auth/me",
            Some(&cookie),
            Some(body.clone()),
        )
        .await;
    assert_eq!(
        (stale.status, stale.code()),
        (StatusCode::UNAUTHORIZED, "reauthentication_required")
    );
    let fresh = format!(
        "{COOKIE}={}",
        fixture.session_token(ada, "ada@example.com").await
    );
    let deleted = fixture
        .send("DELETE", "/api/v1/auth/me", Some(&fresh), Some(body))
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);
}

#[tokio::test]
async fn an_email_change_is_confirmed_from_the_new_address_and_signs_out_other_sessions() {
    let fixture = Fixture::new(true).await;
    let (ada, cookie) = fixture
        .add_user("ada@example.com", Some(PASSWORD), true)
        .await;
    let other = format!(
        "{COOKIE}={}",
        fixture.session_token(ada, "ada@example.com").await
    );
    let requested = fixture
        .send(
            "POST",
            "/api/v1/auth/me/email",
            Some(&cookie),
            Some(json!({"email": " Ada@New.example ", "password": PASSWORD})),
        )
        .await;
    assert_eq!(requested.status, StatusCode::ACCEPTED, "{}", requested.body);
    let emails = fixture.sent(1).await;
    assert_eq!(emails.len(), 1);
    assert_eq!(
        (emails[0].to.as_str(), emails[0].subject.as_str()),
        ("Ada@New.example", "Confirm your new Orbit email")
    );
    let token = token_in(&emails[0].body, "https://orbit.test/confirm-email?token=");
    // Nothing changes before the link is opened.
    assert_eq!(
        fixture.login("ada@example.com", PASSWORD).await,
        StatusCode::OK
    );

    let confirmed = fixture
        .send(
            "POST",
            "/api/v1/auth/email-change/confirm",
            Some(&cookie),
            Some(json!({"token": token})),
        )
        .await;
    assert_eq!(confirmed.status, StatusCode::OK, "{}", confirmed.body);
    assert_eq!(confirmed.body, json!({"email": "Ada@New.example"}));
    let me = fixture
        .send("GET", "/api/v1/auth/me", Some(&cookie), None)
        .await;
    assert_eq!(me.body["email"], "Ada@New.example");
    assert_eq!(
        fixture
            .send("GET", "/api/v1/auth/me", Some(&other), None)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        fixture
            .scalar(
                "SELECT COUNT(*) FROM users WHERE id = $1 AND normalized_email = 'ada@new.example' \
                 AND email_verified_at IS NOT NULL",
                &ada.to_string()
            )
            .await,
        1
    );
    assert_eq!(
        fixture.login("ada@new.example", PASSWORD).await,
        StatusCode::OK
    );
    assert_eq!(
        fixture.login("ada@example.com", PASSWORD).await,
        StatusCode::UNAUTHORIZED
    );
    let emails = fixture.sent(2).await;
    assert_eq!(emails.len(), 2);
    assert_eq!(
        (emails[1].to.as_str(), emails[1].subject.as_str()),
        ("ada@example.com", "Your Orbit email was changed")
    );
    assert!(
        emails[1].body.contains("changed to Ada@New.example"),
        "{}",
        emails[1].body
    );
    assert_eq!(
        fixture
            .scalar(
                "SELECT COUNT(*) FROM audit_events WHERE actor_id = $1 AND action = 'account.email_changed'",
                &ada.to_string()
            )
            .await,
        1
    );
    // Self-hosted: no Stripe job.
    assert_eq!(
        fixture
            .scalar(
                "SELECT COUNT(*) FROM jobs WHERE kind = $1",
                "billing.stripe_customer_email"
            )
            .await,
        0
    );

    let reused = fixture
        .send(
            "POST",
            "/api/v1/auth/email-change/confirm",
            None,
            Some(json!({"token": token})),
        )
        .await;
    assert_eq!(
        (reused.status, reused.code()),
        (StatusCode::BAD_REQUEST, "invalid_email_change_token")
    );
}

#[tokio::test]
async fn an_email_change_does_not_reveal_taken_addresses_and_links_expire() {
    let fixture = Fixture::new(true).await;
    let (ada, cookie) = fixture
        .add_user("ada@example.com", Some(PASSWORD), true)
        .await;
    let request = |email: &str| {
        fixture.send(
            "POST",
            "/api/v1/auth/me/email",
            Some(&cookie),
            Some(json!({"email": email, "password": PASSWORD})),
        )
    };
    let invalid = request("not an address").await;
    assert_eq!(
        (invalid.status, invalid.code()),
        (StatusCode::UNPROCESSABLE_ENTITY, "invalid_email")
    );
    let same = request("ADA@example.com").await;
    assert_eq!(
        (same.status, same.code()),
        (StatusCode::UNPROCESSABLE_ENTITY, "same_email")
    );
    let wrong = fixture
        .send(
            "POST",
            "/api/v1/auth/me/email",
            Some(&cookie),
            Some(json!({"email": "free@example.com", "password": "nope nope nope"})),
        )
        .await;
    assert_eq!(
        (wrong.status, wrong.code()),
        (StatusCode::UNAUTHORIZED, "invalid_credentials")
    );

    // A taken address gets the same answer and no email.
    let taken = request("root@example.com").await;
    let free = request("free@example.com").await;
    assert_eq!((taken.status, &taken.body), (free.status, &free.body));
    assert_eq!(free.status, StatusCode::ACCEPTED);
    let emails = fixture.sent(1).await;
    assert_eq!(emails.len(), 1);
    assert_eq!(emails[0].to, "free@example.com");
    let token = token_in(&emails[0].body, "https://orbit.test/confirm-email?token=");

    // Someone else took the address before the link was opened.
    fixture
        .add_user("free@example.com", Some(PASSWORD), false)
        .await;
    let confirm = |token: String| {
        fixture.send(
            "POST",
            "/api/v1/auth/email-change/confirm",
            None,
            Some(json!({"token": token})),
        )
    };
    let raced = confirm(token.clone()).await;
    assert_eq!(
        (raced.status, raced.code()),
        (StatusCode::CONFLICT, "email_taken")
    );
    assert_eq!(
        fixture
            .scalar(
                "SELECT COUNT(*) FROM users WHERE id = $1 AND email = 'ada@example.com'",
                &ada.to_string()
            )
            .await,
        1
    );
    assert_eq!(confirm(token).await.code(), "invalid_email_change_token");

    // An expired link does nothing.
    let other = request("later@example.com").await;
    assert_eq!(other.status, StatusCode::ACCEPTED);
    let emails = fixture.sent(2).await;
    let token = token_in(&emails[1].body, "https://orbit.test/confirm-email?token=");
    fixture
        .database
        .execute("UPDATE email_change_tokens SET expires_at = 1")
        .await
        .unwrap();
    let expired = confirm(token).await;
    assert_eq!(
        (expired.status, expired.code()),
        (StatusCode::BAD_REQUEST, "invalid_email_change_token")
    );
    assert_eq!(
        fixture.login("ada@example.com", PASSWORD).await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn an_email_change_needs_a_mail_server() {
    let fixture = Fixture::new(false).await;
    let (_, cookie) = fixture
        .add_user("ada@example.com", Some(PASSWORD), true)
        .await;
    let reply = fixture
        .send(
            "POST",
            "/api/v1/auth/me/email",
            Some(&cookie),
            Some(json!({"email": "new@example.com", "password": PASSWORD})),
        )
        .await;
    assert_eq!(
        (reply.status, reply.code()),
        (StatusCode::CONFLICT, "mail_unavailable")
    );
}

impl Fixture {
    async fn admin_change_email(&self, cookie: &str, user_id: &str, email: &str) -> Reply {
        self.send(
            "PUT",
            &format!("/api/v1/admin/users/{user_id}/email"),
            Some(cookie),
            Some(json!({"email": email})),
        )
        .await
    }
}

#[tokio::test]
async fn an_admin_changes_an_email_without_a_mail_server_and_signs_the_account_out() {
    let fixture = Fixture::new(false).await;
    let (ada, cookie) = fixture
        .add_user("ada@example.com", Some(PASSWORD), true)
        .await;
    let other = format!(
        "{COOKIE}={}",
        fixture.session_token(ada, "ada@example.com").await
    );
    let link = fixture
        .send(
            "POST",
            &format!("/api/v1/admin/users/{ada}/recovery-link"),
            Some(&fixture.root_cookie),
            None,
        )
        .await;
    assert_eq!(link.status, StatusCode::CREATED, "{}", link.body);

    let changed = fixture
        .admin_change_email(
            &fixture.root_cookie,
            &ada.to_string(),
            " Ada@Admin.example ",
        )
        .await;
    assert_eq!(changed.status, StatusCode::NO_CONTENT, "{}", changed.body);
    for session in [&cookie, &other] {
        assert_eq!(
            fixture
                .send("GET", "/api/v1/auth/me", Some(session), None)
                .await
                .status,
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        fixture.login("ada@admin.example", PASSWORD).await,
        StatusCode::OK
    );
    assert_eq!(
        fixture.login("ada@example.com", PASSWORD).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        fixture
            .scalar(
                "SELECT COUNT(*) FROM users WHERE id = $1 AND email = 'Ada@Admin.example' \
                 AND normalized_email = 'ada@admin.example' AND email_verified_at IS NOT NULL",
                &ada.to_string()
            )
            .await,
        1
    );
    assert_eq!(
        fixture
            .scalar(
                "SELECT COUNT(*) FROM recovery_tokens WHERE user_id = $1",
                &ada.to_string()
            )
            .await,
        0
    );
    let audit: (String, String) = sqlx::query_as(
        "SELECT actor_id, metadata_json FROM audit_events \
         WHERE resource_id = $1 AND action = 'account.email_changed'",
    )
    .bind(ada.to_string())
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    let root_id: String =
        sqlx::query_scalar("SELECT id FROM users WHERE normalized_email = 'root@example.com'")
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    assert_eq!(
        (audit.0, serde_json::from_str::<Value>(&audit.1).unwrap()),
        (root_id, json!({"changed_by": "admin"}))
    );
    assert_eq!(fixture.sent(0).await.len(), 0);
}

#[tokio::test]
async fn an_admin_email_change_is_refused_for_bad_addresses_other_people_and_deleted_accounts() {
    let fixture = Fixture::new(false).await;
    let (ada, ada_cookie) = fixture
        .add_user("ada@example.com", Some(PASSWORD), true)
        .await;
    let ada = ada.to_string();
    let root = &fixture.root_cookie;
    let answer = |reply: Reply| (reply.status, reply.code().to_owned());

    assert_eq!(
        answer(
            fixture
                .admin_change_email(&ada_cookie, &ada, "new@example.com")
                .await
        ),
        (StatusCode::FORBIDDEN, "installation_admin_required".into())
    );
    assert_eq!(
        answer(
            fixture
                .admin_change_email(root, &ada, "not an address")
                .await
        ),
        (StatusCode::UNPROCESSABLE_ENTITY, "invalid_email".into())
    );
    assert_eq!(
        answer(
            fixture
                .admin_change_email(root, &ada, "ADA@example.com")
                .await
        ),
        (StatusCode::UNPROCESSABLE_ENTITY, "same_email".into())
    );
    assert_eq!(
        answer(
            fixture
                .admin_change_email(root, &ada, "Root@Example.com")
                .await
        ),
        (StatusCode::CONFLICT, "email_taken".into())
    );
    assert_eq!(
        answer(
            fixture
                .admin_change_email(root, &Id::new_v7().to_string(), "new@example.com")
                .await
        ),
        (StatusCode::NOT_FOUND, "user_not_found".into())
    );
    assert_eq!(
        fixture.login("ada@example.com", PASSWORD).await,
        StatusCode::OK
    );

    let deleted = fixture
        .send(
            "DELETE",
            "/api/v1/auth/me",
            Some(&ada_cookie),
            Some(json!({"email": "ada@example.com", "password": PASSWORD})),
        )
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);
    assert_eq!(
        answer(
            fixture
                .admin_change_email(root, &ada, "new@example.com")
                .await
        ),
        (StatusCode::CONFLICT, "account_deleted".into())
    );
}

#[tokio::test]
async fn an_admin_email_change_notifies_the_old_address_and_keeps_an_admins_own_session() {
    let fixture = Fixture::new(true).await;
    let (ada, _) = fixture
        .add_user("ada@example.com", Some(PASSWORD), true)
        .await;
    let changed = fixture
        .admin_change_email(&fixture.root_cookie, &ada.to_string(), "ada@new.example")
        .await;
    assert_eq!(changed.status, StatusCode::NO_CONTENT, "{}", changed.body);
    let emails = fixture.sent(1).await;
    assert_eq!(
        emails
            .iter()
            .map(|email| (email.to.as_str(), email.subject.as_str()))
            .collect::<Vec<_>>(),
        [("ada@example.com", "Your Orbit email was changed")]
    );
    assert!(
        emails[0].body.contains("changed to ada@new.example"),
        "{}",
        emails[0].body
    );

    let root_id: String =
        sqlx::query_scalar("SELECT id FROM users WHERE normalized_email = 'root@example.com'")
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    let other = format!(
        "{COOKIE}={}",
        fixture
            .session_token(root_id.parse().unwrap(), "root@example.com")
            .await
    );
    let own = fixture
        .admin_change_email(&fixture.root_cookie, &root_id, "root@new.example")
        .await;
    assert_eq!(own.status, StatusCode::NO_CONTENT, "{}", own.body);
    let me = fixture
        .send("GET", "/api/v1/auth/me", Some(&fixture.root_cookie), None)
        .await;
    assert_eq!(
        (me.status, me.body["email"].as_str()),
        (StatusCode::OK, Some("root@new.example"))
    );
    assert_eq!(
        fixture
            .send("GET", "/api/v1/auth/me", Some(&other), None)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
}

impl Fixture {
    /// Turns Orbit Cloud billing on for this database.
    fn enable_cloud(&self) -> orbit_server::billing::Billing {
        orbit_server::billing::Billing::new(
            orbit_platform::BillingConfig::new(
                orbit_platform::Secret::new("sk_test_unused"),
                orbit_platform::Secret::new("whsec_unused"),
                vec![],
            ),
            (*self.database).clone(),
            ORIGIN,
        )
    }

    /// Ada, a member with an authenticator app turned on.
    async fn add_user_with_two_factor(&self) -> Id {
        let (ada, _) = self.add_user("ada@example.com", Some(PASSWORD), true).await;
        let now = TimestampMillis::now();
        self.identity
            .begin_totp_setup(ada, b"encrypted", now)
            .await
            .unwrap();
        self.identity
            .enable_totp(ada, 1, "test", now)
            .await
            .unwrap()
            .unwrap();
        ada
    }

    async fn totp_enabled(&self, id: Id) -> bool {
        self.identity
            .two_factor_status(id)
            .await
            .unwrap()
            .totp_enabled
    }
}

#[tokio::test]
async fn orbit_cloud_refuses_admin_account_actions_without_a_mail_server() {
    let fixture = Fixture::new(false).await;
    let _billing = fixture.enable_cloud();
    let ada = fixture.add_user_with_two_factor().await;
    let (_, member) = fixture
        .add_user("bob@example.com", Some(PASSWORD), true)
        .await;
    let detail = "Orbit Cloud emails the person about this change. Save a mail server in Admin → Settings first.";

    let changed = fixture
        .admin_change_email(&fixture.root_cookie, &ada.to_string(), "new@example.com")
        .await;
    let recovery = fixture
        .send(
            "POST",
            &format!("/api/v1/admin/users/{ada}/recovery-link"),
            Some(&fixture.root_cookie),
            None,
        )
        .await;
    let reset = fixture
        .send(
            "POST",
            &format!("/api/v1/admin/users/{ada}/two-factor/reset"),
            Some(&fixture.root_cookie),
            None,
        )
        .await;
    for reply in [&changed, &recovery, &reset] {
        assert_eq!(
            (reply.status, reply.code(), reply.body["detail"].as_str()),
            (StatusCode::CONFLICT, "smtp_required", Some(detail))
        );
    }
    let not_admin = fixture
        .admin_change_email(&member, &ada.to_string(), "new@example.com")
        .await;
    assert_eq!(
        (not_admin.status, not_admin.code()),
        (StatusCode::FORBIDDEN, "installation_admin_required")
    );

    assert_eq!(
        fixture
            .scalar(
                "SELECT COUNT(*) FROM users WHERE id = $1 AND email = 'ada@example.com'",
                &ada.to_string()
            )
            .await,
        1
    );
    assert_eq!(
        fixture
            .scalar(
                "SELECT COUNT(*) FROM recovery_tokens WHERE user_id = $1",
                &ada.to_string()
            )
            .await,
        0
    );
    assert!(fixture.totp_enabled(ada).await);
}

#[tokio::test]
async fn orbit_cloud_tells_the_old_and_the_new_address_about_an_admin_email_change() {
    let fixture = Fixture::new(true).await;
    let _billing = fixture.enable_cloud();
    let (ada, _) = fixture
        .add_user("ada@example.com", Some(PASSWORD), true)
        .await;

    let changed = fixture
        .admin_change_email(&fixture.root_cookie, &ada.to_string(), "ada@new.example")
        .await;
    assert_eq!(changed.status, StatusCode::NO_CONTENT, "{}", changed.body);
    let mut emails: Vec<_> = fixture
        .sent(2)
        .await
        .into_iter()
        .map(|email| (email.to, email.subject, email.body))
        .collect();
    emails.sort();
    assert_eq!(
        emails,
        [
            (
                "ada@example.com".to_owned(),
                "Your Orbit email was changed".to_owned(),
                "The email of your Orbit account was changed to ada@new.example.\n\nIf this was not you, contact support right away.\n".to_owned(),
            ),
            (
                "ada@new.example".to_owned(),
                "Your Orbit email was changed".to_owned(),
                "An Orbit administrator set this address as the email of your account.\n\nIf you did not ask for this, contact support.\n".to_owned(),
            ),
        ]
    );
}

#[tokio::test]
async fn orbit_cloud_emails_an_admin_recovery_link_instead_of_returning_it() {
    let fixture = Fixture::new(true).await;
    let _billing = fixture.enable_cloud();
    let (ada, _) = fixture
        .add_user("ada@example.com", Some(PASSWORD), true)
        .await;
    let before = TimestampMillis::now().as_millis();

    let reply = fixture
        .send(
            "POST",
            &format!("/api/v1/admin/users/{ada}/recovery-link"),
            Some(&fixture.root_cookie),
            None,
        )
        .await;
    assert_eq!(reply.status, StatusCode::ACCEPTED, "{}", reply.body);
    let mut keys: Vec<_> = reply.body.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(keys, ["expires_at", "sent_to"]);
    assert_eq!(reply.body["sent_to"], "ada@example.com");

    let emails = fixture.sent(1).await;
    assert_eq!(emails.len(), 1);
    assert_eq!(
        (emails[0].to.as_str(), emails[0].subject.as_str()),
        ("ada@example.com", "Reset your Orbit password")
    );
    assert!(
        emails[0]
            .body
            .starts_with("An Orbit administrator started a password reset for your account.")
    );
    let token = token_in(&emails[0].body, "https://orbit.test/recovery?token=");
    let expires_at: i64 =
        sqlx::query_scalar("SELECT expires_at FROM recovery_tokens WHERE user_id = $1")
            .bind(ada.to_string())
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    assert!((before + 30 * 60_000..=before + 30 * 60_000 + 10_000).contains(&expires_at));

    let completed = fixture
        .send(
            "POST",
            "/api/v1/auth/recovery/complete",
            None,
            Some(json!({"token": token, "password": "a brand new passphrase"})),
        )
        .await;
    assert_eq!(
        completed.status,
        StatusCode::NO_CONTENT,
        "{}",
        completed.body
    );
    assert_eq!(
        fixture
            .login("ada@example.com", "a brand new passphrase")
            .await,
        StatusCode::OK
    );
    assert_eq!(
        fixture.login("ada@example.com", PASSWORD).await,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn self_hosted_returns_the_admin_recovery_link_even_with_a_mail_server() {
    let fixture = Fixture::new(true).await;
    let (ada, _) = fixture
        .add_user("ada@example.com", Some(PASSWORD), true)
        .await;

    let reply = fixture
        .send(
            "POST",
            &format!("/api/v1/admin/users/{ada}/recovery-link"),
            Some(&fixture.root_cookie),
            None,
        )
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    let mut keys: Vec<_> = reply.body.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(keys, ["expires_at", "url"]);
    let token = token_in(
        reply.body["url"].as_str().unwrap(),
        "https://orbit.test/recovery?token=",
    );
    assert!(
        fixture
            .identity
            .recovery_token_valid(&token, TimestampMillis::now())
            .await
            .unwrap()
    );
    assert!(fixture.sent(1).await.is_empty());
}

#[tokio::test]
async fn orbit_cloud_emails_a_notice_when_an_admin_turns_off_two_step_sign_in() {
    let fixture = Fixture::new(true).await;
    let _billing = fixture.enable_cloud();
    let ada = fixture.add_user_with_two_factor().await;

    let reply = fixture
        .send(
            "POST",
            &format!("/api/v1/admin/users/{ada}/two-factor/reset"),
            Some(&fixture.root_cookie),
            None,
        )
        .await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT, "{}", reply.body);
    assert!(!fixture.totp_enabled(ada).await);
    let emails: Vec<_> = fixture
        .sent(1)
        .await
        .into_iter()
        .map(|email| (email.to, email.subject, email.body))
        .collect();
    assert_eq!(
        emails,
        [(
            "ada@example.com".to_owned(),
            "Two-step sign-in was turned off".to_owned(),
            "An Orbit administrator turned off two-step sign-in for your account. If you did not ask for this, contact support.\n".to_owned(),
        )]
    );
}
