use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{
    AuthenticatedUser, HttpPlatformLayer, Id, OriginPolicy, PasswordService, TestDatabase,
    TimestampMillis,
};
use orbit_server::auth_routes::CookieMode;
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::workspace_routes::{WorkspaceState, workspace_router};
use serde_json::{Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn workspace_memberships_invitations_and_audit_are_path_scoped() {
    let database = TestDatabase::new().await.unwrap();
    let identity = Arc::new(IdentityRepository::new((*database).clone()));
    let setup = setup_owner(&identity).await;
    let owner_cookie = format!("__Host-orbit_session={}", setup.1);
    let app = workspace_router(WorkspaceState::new(
        Arc::clone(&identity),
        "https://orbit.test".to_owned(),
        CookieMode::secure(),
    ))
    .layer(HttpPlatformLayer::new(OriginPolicy::new(
        "https://orbit.test",
    )));

    let listed = app
        .clone()
        .oneshot(cookie_request("GET", "/api/v1/workspaces", &owner_cookie))
        .await
        .unwrap();
    assert_eq!(listed.status(), StatusCode::OK);
    let listed = response_json(listed).await;
    assert_eq!(listed[0]["id"], setup.0);
    assert_eq!(listed[0]["role"], "owner");

    let created = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/v1/workspaces",
            &owner_cookie,
            json!({"name": "Second"}),
        ))
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::CREATED);
    let second = response_json(created).await["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let invite = app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/api/v1/workspaces/{second}/invitations"),
            &owner_cookie,
            json!({"email": "Member@Example.com", "role": "member", "delivery": "manual"}),
        ))
        .await
        .unwrap();
    assert_eq!(invite.status(), StatusCode::CREATED);
    let invite = response_json(invite).await;
    assert_eq!(invite["invitation"]["delivery"], "manual");
    assert!(invite["url"].as_str().unwrap().contains("token="));

    let invitations = app
        .clone()
        .oneshot(cookie_request(
            "GET",
            &format!("/api/v1/workspaces/{second}/invitations"),
            &owner_cookie,
        ))
        .await
        .unwrap();
    let invitations = response_json(invitations).await;
    assert_eq!(invitations["items"][0]["email"], "Member@Example.com");
    assert!(invitations["items"][0].get("token").is_none());
    assert!(invitations["items"][0].get("url").is_none());

    let wrong_scope = app
        .clone()
        .oneshot(cookie_request(
            "GET",
            &format!("/api/v1/workspaces/{}/invitations", setup.0),
            &owner_cookie,
        ))
        .await
        .unwrap();
    assert_eq!(wrong_scope.status(), StatusCode::OK);
    assert!(
        response_json(wrong_scope).await["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let audit = app
        .oneshot(cookie_request(
            "GET",
            &format!("/api/v1/workspaces/{second}/audit"),
            &owner_cookie,
        ))
        .await
        .unwrap();
    assert_eq!(audit.status(), StatusCode::OK);
    let audit = response_json(audit).await;
    assert!(
        audit["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|event| event["workspace_id"] == second)
    );
}

#[tokio::test]
async fn invitation_acceptance_enforces_identity_provenance_and_single_use() {
    let database = TestDatabase::new().await.unwrap();
    let identity = Arc::new(IdentityRepository::new((*database).clone()));
    let setup = setup_owner(&identity).await;
    let owner_cookie = format!("__Host-orbit_session={}", setup.1);
    let member = create_user(&database, &identity, "member@example.com", "Member").await;
    let wrong = create_user(&database, &identity, "wrong@example.com", "Wrong").await;
    let app = workspace_router(WorkspaceState::new(
        Arc::clone(&identity),
        "https://orbit.test".to_owned(),
        CookieMode::secure(),
    ))
    .layer(HttpPlatformLayer::new(OriginPolicy::new(
        "https://orbit.test",
    )));

    let issued = app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/api/v1/workspaces/{}/invitations", setup.0),
            &owner_cookie,
            json!({"email": "MEMBER@example.com", "role": "member", "delivery": "manual"}),
        ))
        .await
        .unwrap();
    assert_eq!(issued.status(), StatusCode::CREATED);
    let issued = response_json(issued).await;
    let token = issued["url"]
        .as_str()
        .unwrap()
        .split("token=")
        .nth(1)
        .unwrap();

    let mismatch = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/v1/workspaces/invitations/accept",
            &wrong.1,
            json!({"token": token}),
        ))
        .await
        .unwrap();
    assert_eq!(mismatch.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        response_json(mismatch).await["code"],
        "invitation_email_mismatch"
    );

    let accepted = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/v1/workspaces/invitations/accept",
            &member.1,
            json!({"token": token}),
        ))
        .await
        .unwrap();
    assert_eq!(accepted.status(), StatusCode::OK);
    let accepted = response_json(accepted).await;
    assert_eq!(accepted["created"], true);
    assert_eq!(accepted["email_verified"], false);
    let verified: Option<i64> =
        sqlx::query_scalar("SELECT email_verified_at FROM users WHERE id = ?")
            .bind(member.0.to_string())
            .fetch_one(database.pool())
            .await
            .unwrap();
    assert_eq!(verified, None, "manual links do not verify email ownership");

    let reused = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/v1/workspaces/invitations/accept",
            &member.1,
            json!({"token": token}),
        ))
        .await
        .unwrap();
    assert_eq!(reused.status(), StatusCode::NOT_FOUND);

    let forbidden_real = app
        .clone()
        .oneshot(cookie_request(
            "GET",
            &format!("/api/v1/workspaces/{}/members", setup.0),
            &wrong.1,
        ))
        .await
        .unwrap();
    let forbidden_unknown = app
        .oneshot(cookie_request(
            "GET",
            &format!("/api/v1/workspaces/{}/members", Id::new_v7()),
            &wrong.1,
        ))
        .await
        .unwrap();
    assert_eq!(forbidden_real.status(), StatusCode::NOT_FOUND);
    assert_eq!(forbidden_unknown.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        response_json(forbidden_real).await["code"],
        response_json(forbidden_unknown).await["code"]
    );
}

#[tokio::test]
async fn owners_and_admins_manage_members_without_crossing_global_account_scope() {
    let database = TestDatabase::new().await.unwrap();
    let identity = Arc::new(IdentityRepository::new((*database).clone()));
    let setup = setup_owner(&identity).await;
    let owner_cookie = format!("__Host-orbit_session={}", setup.1);
    let admin = create_user(&database, &identity, "admin@example.com", "Admin").await;
    let member = create_user(&database, &identity, "member2@example.com", "Member").await;
    let existing = create_user(&database, &identity, "existing@example.com", "Existing").await;
    let app = workspace_router(WorkspaceState::new(
        Arc::clone(&identity),
        "https://orbit.test".to_owned(),
        CookieMode::secure(),
    ))
    .layer(HttpPlatformLayer::new(OriginPolicy::new(
        "https://orbit.test",
    )));

    invite_and_accept(
        &app,
        &setup.0,
        &owner_cookie,
        "admin@example.com",
        "admin",
        &admin.1,
    )
    .await;

    let revoked =
        create_invitation(&app, &setup.0, &admin.1, "member2@example.com", "member").await;
    let revoke = app
        .clone()
        .oneshot(cookie_request(
            "DELETE",
            &format!(
                "/api/v1/workspaces/{}/invitations/{}",
                setup.0,
                revoked["invitation"]["id"].as_str().unwrap()
            ),
            &admin.1,
        ))
        .await
        .unwrap();
    assert_eq!(revoke.status(), StatusCode::NO_CONTENT);
    let revoked_accept = accept_url(&app, &member.1, revoked["url"].as_str().unwrap()).await;
    assert_eq!(revoked_accept.status(), StatusCode::NOT_FOUND);

    let old = create_invitation(&app, &setup.0, &admin.1, "member2@example.com", "member").await;
    let replacement =
        create_invitation(&app, &setup.0, &admin.1, "MEMBER2@example.com", "member").await;
    assert_eq!(
        accept_url(&app, &member.1, old["url"].as_str().unwrap())
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        accept_url(&app, &member.1, replacement["url"].as_str().unwrap())
            .await
            .status(),
        StatusCode::OK
    );

    let idempotent =
        create_invitation(&app, &setup.0, &admin.1, "existing@example.com", "member").await;
    let existing_membership = Id::new_v7();
    let now = TimestampMillis::now();
    sqlx::query(
        "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at) \
         VALUES (?, ?, ?, 'member', 0, ?, ?)",
    )
    .bind(existing_membership.to_string())
    .bind(&setup.0)
    .bind(existing.0.to_string())
    .bind(now.as_millis())
    .bind(now.as_millis())
    .execute(database.pool())
    .await
    .unwrap();
    let accepted = accept_url(&app, &existing.1, idempotent["url"].as_str().unwrap()).await;
    assert_eq!(accepted.status(), StatusCode::OK);
    let accepted = response_json(accepted).await;
    assert_eq!(accepted["created"], false);
    assert_eq!(accepted["membership_id"], existing_membership.to_string());

    let members = app
        .clone()
        .oneshot(cookie_request(
            "GET",
            &format!("/api/v1/workspaces/{}/members?limit=100", setup.0),
            &owner_cookie,
        ))
        .await
        .unwrap();
    let members = response_json(members).await;
    let owner_membership = members["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["role"] == "owner")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let admin_membership = members["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["user_id"] == admin.0.to_string())
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let protected = app
        .clone()
        .oneshot(cookie_request(
            "DELETE",
            &format!("/api/v1/workspaces/{}/members/{owner_membership}", setup.0),
            &owner_cookie,
        ))
        .await
        .unwrap();
    assert_eq!(protected.status(), StatusCode::CONFLICT);
    assert_eq!(
        response_json(protected).await["code"],
        "ownership_transfer_required"
    );

    let admin_cannot_alter_owner = app
        .clone()
        .oneshot(cookie_request(
            "DELETE",
            &format!("/api/v1/workspaces/{}/members/{owner_membership}", setup.0),
            &admin.1,
        ))
        .await
        .unwrap();
    assert_eq!(admin_cannot_alter_owner.status(), StatusCode::FORBIDDEN);

    let transfer = app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/api/v1/workspaces/{}/transfer-ownership", setup.0),
            &owner_cookie,
            json!({"membership_id": admin_membership}),
        ))
        .await
        .unwrap();
    assert_eq!(transfer.status(), StatusCode::NO_CONTENT);
    let remove_old_owner = app
        .clone()
        .oneshot(cookie_request(
            "DELETE",
            &format!("/api/v1/workspaces/{}/members/{owner_membership}", setup.0),
            &admin.1,
        ))
        .await
        .unwrap();
    assert_eq!(remove_old_owner.status(), StatusCode::NO_CONTENT);

    identity
        .create_session(
            &AuthenticatedUser {
                id: member.0,
                email: "member2@example.com".to_owned(),
                display_name: "Member".to_owned(),
            },
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    let workspace_admin_cannot_suspend = app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/api/v1/admin/users/{}/suspension", member.0),
            &admin.1,
            json!({"suspended": true}),
        ))
        .await
        .unwrap();
    assert_eq!(
        workspace_admin_cannot_suspend.status(),
        StatusCode::FORBIDDEN
    );
    let suspend = app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/api/v1/admin/users/{}/suspension", member.0),
            &owner_cookie,
            json!({"suspended": true}),
        ))
        .await
        .unwrap();
    assert_eq!(suspend.status(), StatusCode::NO_CONTENT);
    let active_sessions: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sessions WHERE user_id = ? AND revoked_at IS NULL",
    )
    .bind(member.0.to_string())
    .fetch_one(database.pool())
    .await
    .unwrap();
    assert_eq!(active_sessions, 0);
    let suspended = app
        .clone()
        .oneshot(cookie_request(
            "GET",
            &format!("/api/v1/workspaces/{}", setup.0),
            &member.1,
        ))
        .await
        .unwrap();
    assert_eq!(suspended.status(), StatusCode::UNAUTHORIZED);

    let deleted = app
        .clone()
        .oneshot(cookie_request(
            "DELETE",
            &format!("/api/v1/workspaces/{}", setup.0),
            &admin.1,
        ))
        .await
        .unwrap();
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);
    let trash = app
        .clone()
        .oneshot(cookie_request("GET", "/api/v1/workspaces/trash", &admin.1))
        .await
        .unwrap();
    assert_eq!(response_json(trash).await[0]["id"], setup.0);
    let restored = app
        .oneshot(cookie_request(
            "POST",
            &format!("/api/v1/workspaces/{}/restore", setup.0),
            &admin.1,
        ))
        .await
        .unwrap();
    assert_eq!(restored.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn malformed_workspace_dtos_use_problem_details() {
    let database = TestDatabase::new().await.unwrap();
    let identity = Arc::new(IdentityRepository::new((*database).clone()));
    let setup = setup_owner(&identity).await;
    let owner_cookie = format!("__Host-orbit_session={}", setup.1);
    let app = workspace_router(WorkspaceState::new(
        identity,
        "https://orbit.test".to_owned(),
        CookieMode::secure(),
    ));

    let malformed_query = app
        .clone()
        .oneshot(cookie_request(
            "GET",
            &format!("/api/v1/workspaces/{}/members?limit=not-a-number", setup.0),
            &owner_cookie,
        ))
        .await
        .unwrap();
    assert_eq!(malformed_query.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        malformed_query.headers()[header::CONTENT_TYPE],
        "application/problem+json"
    );
    assert_eq!(
        response_json(malformed_query).await["code"],
        "invalid_request"
    );

    let unknown_field = app
        .oneshot(json_request(
            "POST",
            "/api/v1/workspaces",
            &owner_cookie,
            json!({"name": "Nope", "unexpected": true}),
        ))
        .await
        .unwrap();
    assert_eq!(unknown_field.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        response_json(unknown_field).await["code"],
        "invalid_request"
    );
}

async fn setup_owner(repository: &IdentityRepository) -> (String, String) {
    let now = TimestampMillis::now();
    repository
        .store_setup_token(
            "operator-secret",
            TimestampMillis::from_millis(now.as_millis() + 60_000),
        )
        .await
        .unwrap();
    let result = repository
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
    (result.workspace_id.to_string(), result.session.token)
}

async fn create_user(
    database: &TestDatabase,
    repository: &IdentityRepository,
    email: &str,
    display_name: &str,
) -> (Id, String) {
    let id = Id::new_v7();
    let now = TimestampMillis::now();
    sqlx::query(
        "INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at) \
         VALUES (?, ?, ?, ?, 'unused', ?, ?)",
    )
    .bind(id.to_string())
    .bind(email)
    .bind(email.to_ascii_lowercase())
    .bind(display_name)
    .bind(now.as_millis())
    .bind(now.as_millis())
    .execute(database.pool())
    .await
    .unwrap();
    let session = repository
        .create_session(
            &AuthenticatedUser {
                id,
                email: email.to_owned(),
                display_name: display_name.to_owned(),
            },
            now,
        )
        .await
        .unwrap();
    (id, format!("__Host-orbit_session={}", session.token))
}

async fn create_invitation(
    app: &axum::Router,
    workspace_id: &str,
    cookie: &str,
    email: &str,
    role: &str,
) -> Value {
    let response = app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/api/v1/workspaces/{workspace_id}/invitations"),
            cookie,
            json!({"email": email, "role": role, "delivery": "manual"}),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    response_json(response).await
}

async fn invite_and_accept(
    app: &axum::Router,
    workspace_id: &str,
    inviter_cookie: &str,
    email: &str,
    role: &str,
    recipient_cookie: &str,
) {
    let invitation = create_invitation(app, workspace_id, inviter_cookie, email, role).await;
    let accepted = accept_url(app, recipient_cookie, invitation["url"].as_str().unwrap()).await;
    assert_eq!(accepted.status(), StatusCode::OK);
}

async fn accept_url(app: &axum::Router, cookie: &str, url: &str) -> axum::response::Response {
    let token = url.split("token=").nth(1).unwrap();
    app.clone()
        .oneshot(json_request(
            "POST",
            "/api/v1/workspaces/invitations/accept",
            cookie,
            json!({"token": token}),
        ))
        .await
        .unwrap()
}

fn json_request(method: &str, uri: &str, cookie: &str, value: Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::COOKIE, cookie)
        .header(header::ORIGIN, "https://orbit.test")
        .body(Body::from(value.to_string()))
        .unwrap()
}

fn cookie_request(method: &str, uri: &str, cookie: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::COOKIE, cookie)
        .header(header::ORIGIN, "https://orbit.test")
        .body(Body::empty())
        .unwrap()
}

async fn response_json(response: axum::response::Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap()
}
