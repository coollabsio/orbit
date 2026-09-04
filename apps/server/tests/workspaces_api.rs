use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{
    AuthenticatedUser, HttpPlatformLayer, Id, LocalBlobStore, OriginPolicy, PasswordService,
    TestDatabase, TimestampMillis,
};
use orbit_server::auth_routes::CookieMode;
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::workspaces::WorkspaceRepository;
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

    let foreign_invitation = app
        .clone()
        .oneshot(cookie_request(
            "DELETE",
            &format!(
                "/api/v1/workspaces/{}/invitations/{}",
                setup.0,
                invite["invitation"]["id"].as_str().unwrap()
            ),
            &owner_cookie,
        ))
        .await
        .unwrap();
    assert_eq!(foreign_invitation.status(), StatusCode::NOT_FOUND);

    for email in ["page-a@example.com", "page-b@example.com"] {
        let created = app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/api/v1/workspaces/{second}/invitations"),
                &owner_cookie,
                json!({"email": email, "role": "member", "delivery": "manual"}),
            ))
            .await
            .unwrap();
        assert_eq!(created.status(), StatusCode::CREATED);
    }

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

    let first_page = response_json(
        app.clone()
            .oneshot(cookie_request(
                "GET",
                &format!("/api/v1/workspaces/{second}/invitations?limit=2"),
                &owner_cookie,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(first_page["items"].as_array().unwrap().len(), 2);
    let cursor = first_page["next_cursor"].as_str().unwrap();
    let second_page = response_json(
        app.clone()
            .oneshot(cookie_request(
                "GET",
                &format!("/api/v1/workspaces/{second}/invitations?limit=2&cursor={cursor}"),
                &owner_cookie,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(second_page["items"].as_array().unwrap().len(), 1);
    assert_ne!(first_page["items"][0]["id"], second_page["items"][0]["id"]);

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
            &format!(
                "/api/v1/workspaces/{}/members/{owner_membership}?expected_version=0",
                setup.0
            ),
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
            &format!(
                "/api/v1/workspaces/{}/members/{owner_membership}?expected_version=0",
                setup.0
            ),
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
            json!({"membership_id": admin_membership, "expected_version": 0, "membership_version": 0}),
        ))
        .await
        .unwrap();
    assert_eq!(transfer.status(), StatusCode::NO_CONTENT);
    let remove_old_owner = app
        .clone()
        .oneshot(cookie_request(
            "DELETE",
            &format!(
                "/api/v1/workspaces/{}/members/{owner_membership}?expected_version=1",
                setup.0
            ),
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
            &format!("/api/v1/workspaces/{}?expected_version=1", setup.0),
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
        .oneshot(json_request(
            "POST",
            &format!("/api/v1/workspaces/{}/restore", setup.0),
            &admin.1,
            json!({"expected_version": 2}),
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

#[tokio::test]
async fn invited_new_account_registration_is_atomic_and_creates_a_session() {
    let database = TestDatabase::new().await.unwrap();
    let identity = Arc::new(IdentityRepository::new((*database).clone()));
    let setup = setup_owner(&identity).await;
    let owner_cookie = format!("__Host-orbit_session={}", setup.1);
    let app = workspace_router(WorkspaceState::new(
        identity,
        "https://orbit.test".to_owned(),
        CookieMode::secure(),
    ));
    let invitation =
        create_invitation(&app, &setup.0, &owner_cookie, "new@example.com", "member").await;
    let token = invitation["url"]
        .as_str()
        .unwrap()
        .split("token=")
        .nth(1)
        .unwrap();

    let accepted = app
        .clone()
        .oneshot(json_request_without_cookie(
            "POST",
            "/api/v1/workspaces/invitations/accept",
            json!({
                "token": token,
                "email": "NEW@example.com",
                "display_name": "New User",
                "password": "long enough passphrase"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(accepted.status(), StatusCode::CREATED);
    let cookie = accepted.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .to_owned();
    assert!(cookie.starts_with("__Host-orbit_session="));
    let accepted = response_json(accepted).await;
    assert_eq!(accepted["created"], true);
    assert_eq!(accepted["email_verified"], false);
    let stored: (String, Option<i64>) =
        sqlx::query_as("SELECT password_hash, email_verified_at FROM users WHERE normalized_email = 'new@example.com'")
            .fetch_one(database.pool())
            .await
            .unwrap();
    assert_ne!(stored.0, "long enough passphrase");
    assert_eq!(stored.1, None);
    let me = app
        .oneshot(cookie_request("GET", "/api/v1/workspaces", &cookie))
        .await
        .unwrap();
    assert_eq!(me.status(), StatusCode::OK);
}

#[tokio::test]
async fn invited_registration_rolls_back_account_membership_and_token_when_session_fails() {
    let database = TestDatabase::new().await.unwrap();
    let identity = Arc::new(IdentityRepository::new((*database).clone()));
    let setup = setup_owner(&identity).await;
    let owner_cookie = format!("__Host-orbit_session={}", setup.1);
    let app = workspace_router(WorkspaceState::new(
        identity,
        "https://orbit.test".to_owned(),
        CookieMode::secure(),
    ));
    let invitation = create_invitation(
        &app,
        &setup.0,
        &owner_cookie,
        "rollback@example.com",
        "member",
    )
    .await;
    let token = invitation["url"]
        .as_str()
        .unwrap()
        .split("token=")
        .nth(1)
        .unwrap();
    sqlx::query("CREATE TRIGGER fail_invited_session BEFORE INSERT ON sessions BEGIN SELECT RAISE(ABORT, 'fail'); END")
        .execute(database.pool())
        .await
        .unwrap();
    let failed = app
        .oneshot(json_request_without_cookie(
            "POST",
            "/api/v1/workspaces/invitations/accept",
            json!({
                "token": token,
                "email": "rollback@example.com",
                "display_name": "Rollback",
                "password": "long enough passphrase"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(failed.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let users: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM users WHERE normalized_email = 'rollback@example.com'",
    )
    .fetch_one(database.pool())
    .await
    .unwrap();
    let accepted: Option<i64> = sqlx::query_scalar(
        "SELECT accepted_at FROM workspace_invitations WHERE normalized_email = 'rollback@example.com'",
    )
    .fetch_one(database.pool())
    .await
    .unwrap();
    assert_eq!(users, 0);
    assert_eq!(accepted, None);
}

#[tokio::test]
async fn ordinary_workspace_creation_installs_the_default_project_and_workflow() {
    let database = TestDatabase::new().await.unwrap();
    let identity = Arc::new(IdentityRepository::new((*database).clone()));
    let setup = setup_owner(&identity).await;
    let owner_cookie = format!("__Host-orbit_session={}", setup.1);
    let app = workspace_router(WorkspaceState::new(
        identity,
        "https://orbit.test".to_owned(),
        CookieMode::secure(),
    ));
    let created = app
        .oneshot(json_request(
            "POST",
            "/api/v1/workspaces",
            &owner_cookie,
            json!({"name": "Production"}),
        ))
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::CREATED);
    let workspace_id = response_json(created).await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let projects: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM projects WHERE workspace_id = ?")
        .bind(&workspace_id)
        .fetch_one(database.pool())
        .await
        .unwrap();
    let statuses: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM task_statuses WHERE workspace_id = ? ORDER BY position",
    )
    .bind(&workspace_id)
    .fetch_all(database.pool())
    .await
    .unwrap();
    assert_eq!(projects, 1);
    assert_eq!(
        statuses,
        ["Backlog", "Todo", "In Progress", "Done", "Cancelled"]
    );
}

#[tokio::test]
async fn stale_workspace_versions_return_refresh_metadata_without_mutation() {
    let database = TestDatabase::new().await.unwrap();
    let identity = Arc::new(IdentityRepository::new((*database).clone()));
    let setup = setup_owner(&identity).await;
    let owner_cookie = format!("__Host-orbit_session={}", setup.1);
    let app = workspace_router(WorkspaceState::new(
        identity,
        "https://orbit.test".to_owned(),
        CookieMode::secure(),
    ));

    let renamed = app
        .clone()
        .oneshot(json_request(
            "PATCH",
            &format!("/api/v1/workspaces/{}", setup.0),
            &owner_cookie,
            json!({"name": "Fresh", "expected_version": 0}),
        ))
        .await
        .unwrap();
    assert_eq!(renamed.status(), StatusCode::OK);
    assert_eq!(response_json(renamed).await["version"], 1);

    let stale = app
        .clone()
        .oneshot(json_request(
            "PATCH",
            &format!("/api/v1/workspaces/{}", setup.0),
            &owner_cookie,
            json!({"name": "Lost update", "expected_version": 0}),
        ))
        .await
        .unwrap();
    assert_eq!(stale.status(), StatusCode::CONFLICT);
    let stale = response_json(stale).await;
    assert_eq!(stale["code"], "conflict");
    assert_eq!(stale["conflict"]["current_version"], 1);
    assert_eq!(
        stale["conflict"]["refresh"],
        format!("/api/v1/workspaces/{}", setup.0)
    );
    let name: String = sqlx::query_scalar("SELECT name FROM workspaces WHERE id = ?")
        .bind(&setup.0)
        .fetch_one(database.pool())
        .await
        .unwrap();
    assert_eq!(name, "Fresh");

    let member = create_user(
        &database,
        &Arc::new(IdentityRepository::new((*database).clone())),
        "stale-member@example.com",
        "Stale Member",
    )
    .await;
    invite_and_accept(
        &app,
        &setup.0,
        &owner_cookie,
        "stale-member@example.com",
        "member",
        &member.1,
    )
    .await;
    let members = response_json(
        app.clone()
            .oneshot(cookie_request(
                "GET",
                &format!("/api/v1/workspaces/{}/members", setup.0),
                &owner_cookie,
            ))
            .await
            .unwrap(),
    )
    .await;
    let membership_id = members["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["user_id"] == member.0.to_string())
        .unwrap()["id"]
        .as_str()
        .unwrap();
    assert_eq!(
        app.clone()
            .oneshot(json_request(
                "PATCH",
                &format!("/api/v1/workspaces/{}/members/{membership_id}", setup.0),
                &owner_cookie,
                json!({"role": "admin", "expected_version": 0}),
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    let stale_role = app
        .clone()
        .oneshot(json_request(
            "PATCH",
            &format!("/api/v1/workspaces/{}/members/{membership_id}", setup.0),
            &owner_cookie,
            json!({"role": "member", "expected_version": 0}),
        ))
        .await
        .unwrap();
    assert_eq!(stale_role.status(), StatusCode::CONFLICT);
    assert_eq!(
        response_json(stale_role).await["conflict"]["current_version"],
        1
    );

    let stale_delete = app
        .oneshot(cookie_request(
            "DELETE",
            &format!("/api/v1/workspaces/{}?expected_version=0", setup.0),
            &owner_cookie,
        ))
        .await
        .unwrap();
    assert_eq!(stale_delete.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn owner_constraint_and_durable_retention_bound_workspace_trash() {
    let database = TestDatabase::new().await.unwrap();
    let identity = Arc::new(IdentityRepository::new((*database).clone()));
    let setup = setup_owner(&identity).await;
    let owner_cookie = format!("__Host-orbit_session={}", setup.1);
    let app = workspace_router(WorkspaceState::new(
        identity,
        "https://orbit.test".to_owned(),
        CookieMode::secure(),
    ));

    let owner_membership: String =
        sqlx::query_scalar("SELECT owner_membership_id FROM workspaces WHERE id = ?")
            .bind(&setup.0)
            .fetch_one(database.pool())
            .await
            .unwrap();
    assert!(
        sqlx::query("DELETE FROM memberships WHERE id = ?")
            .bind(&owner_membership)
            .execute(database.pool())
            .await
            .is_err()
    );
    assert!(
        sqlx::query("UPDATE memberships SET role = 'admin' WHERE id = ?")
            .bind(&owner_membership)
            .execute(database.pool())
            .await
            .is_err()
    );
    let owner_user: String = sqlx::query_scalar("SELECT user_id FROM memberships WHERE id = ?")
        .bind(&owner_membership)
        .fetch_one(database.pool())
        .await
        .unwrap();
    assert!(
        sqlx::query("DELETE FROM users WHERE id = ?")
            .bind(&owner_user)
            .execute(database.pool())
            .await
            .is_err()
    );

    let deleted = app
        .clone()
        .oneshot(cookie_request(
            "DELETE",
            &format!("/api/v1/workspaces/{}?expected_version=0", setup.0),
            &owner_cookie,
        ))
        .await
        .unwrap();
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);
    let old = TimestampMillis::now().as_millis() - 31 * 24 * 60 * 60 * 1_000;
    sqlx::query("UPDATE workspaces SET deleted_at = ? WHERE id = ?")
        .bind(old)
        .bind(&setup.0)
        .execute(database.pool())
        .await
        .unwrap();
    let blob_id = Id::new_v7();
    let attachment_root = std::env::temp_dir().join(format!("orbit-retention-{}", Id::new_v7()));
    let blob_path = attachment_root.join(format!("blobs/{}/purge", setup.0));
    let temporary_path = attachment_root.join("temporary/upload-purge");
    std::fs::create_dir_all(blob_path.parent().unwrap()).unwrap();
    std::fs::create_dir_all(temporary_path.parent().unwrap()).unwrap();
    std::fs::write(&blob_path, b"x").unwrap();
    std::fs::write(&temporary_path, b"x").unwrap();
    sqlx::query(
        "INSERT INTO attachment_blobs (id, workspace_id, sha256, byte_size, storage_key, created_at, quarantine_until) \
         VALUES (?, ?, 'purge-sha', 1, ?, ?, ?)",
    )
    .bind(blob_id.to_string())
    .bind(&setup.0)
    .bind(format!("blobs/{}/purge", setup.0))
    .bind(old)
    .bind(old)
    .execute(database.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO attachment_references (id, workspace_id, task_id, owner_id, blob_id, display_name, media_type, byte_size, created_at) \
         VALUES (?, ?, ?, ?, ?, 'purge.txt', 'text/plain', 1, ?)",
    )
    .bind(Id::new_v7().to_string())
    .bind(&setup.0)
    .bind(Id::new_v7().to_string())
    .bind(owner_membership.clone())
    .bind(blob_id.to_string())
    .bind(old)
    .execute(database.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO pending_uploads (id, workspace_id, user_id, temporary_path, original_name, media_type, byte_size, state, created_at, expires_at) \
         VALUES (?, ?, ?, ?, 'pending.txt', 'text/plain', 1, 'staged', ?, ?)",
    )
    .bind(Id::new_v7().to_string())
    .bind(&setup.0)
    .bind(owner_user.clone())
    .bind(temporary_path.to_string_lossy().as_ref())
    .bind(old)
    .bind(old)
    .execute(database.pool())
    .await
    .unwrap();
    let trash = app
        .clone()
        .oneshot(cookie_request(
            "GET",
            "/api/v1/workspaces/trash",
            &owner_cookie,
        ))
        .await
        .unwrap();
    assert!(response_json(trash).await.as_array().unwrap().is_empty());
    let restore = app
        .oneshot(json_request(
            "POST",
            &format!("/api/v1/workspaces/{}/restore", setup.0),
            &owner_cookie,
            json!({"expected_version": 1}),
        ))
        .await
        .unwrap();
    assert_eq!(restore.status(), StatusCode::NOT_FOUND);

    let repository = WorkspaceRepository::with_blob_store(
        (*database).clone(),
        Arc::new(LocalBlobStore::new(&attachment_root)),
    );
    sqlx::query(
        "INSERT INTO audit_events (id, workspace_id, actor_id, action, outcome, resource_type, \
         request_id, metadata_json, occurred_at) VALUES (?, NULL, NULL, 'old.event', 'success', \
         'installation', 'old', '{}', ?)",
    )
    .bind(Id::new_v7().to_string())
    .bind(TimestampMillis::now().as_millis() - 366 * 24 * 60 * 60 * 1_000)
    .execute(database.pool())
    .await
    .unwrap();
    let job_id = repository
        .enqueue_retention(TimestampMillis::now())
        .await
        .unwrap();
    let shutdown = tokio_util::sync::CancellationToken::new();
    let worker = repository
        .retention_worker(
            orbit_platform::WorkerConfig::new(1)
                .unwrap()
                .with_poll_interval(std::time::Duration::from_millis(1)),
        )
        .unwrap();
    let worker_shutdown = shutdown.clone();
    let worker_task = tokio::spawn(async move { worker.run(worker_shutdown).await });
    let job = orbit_platform::JobStore::new((*database).clone())
        .get(job_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(job.kind.name(), "workspace.retention");
    let durable_summary = loop {
        if let Some(summary) = sqlx::query_as::<_, (i64, i64)>(
            "SELECT workspaces_purged, audit_events_purged FROM maintenance_summaries \
             ORDER BY occurred_at DESC LIMIT 1",
        )
        .fetch_optional(database.pool())
        .await
        .unwrap()
        {
            break summary;
        }
        tokio::task::yield_now().await;
    };
    shutdown.cancel();
    worker_task.await.unwrap().unwrap();
    assert_eq!(durable_summary, (1, 1));
    for table in [
        "attachment_references",
        "attachment_blobs",
        "pending_uploads",
    ] {
        let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(database.pool())
            .await
            .unwrap();
        assert_eq!(count, 0, "{table} was not purged");
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM attachment_file_deletions")
            .fetch_one(database.pool())
            .await
            .unwrap(),
        0
    );
    assert!(!blob_path.exists());
    assert!(!temporary_path.exists());
    let _ = std::fs::remove_dir_all(attachment_root);
}

#[tokio::test]
async fn concurrent_ownership_transfers_leave_one_database_backed_owner() {
    let database = TestDatabase::new().await.unwrap();
    let identity = Arc::new(IdentityRepository::new((*database).clone()));
    let setup = setup_owner(&identity).await;
    let first = create_user(&database, &identity, "owner-a@example.com", "Owner A").await;
    let second = create_user(&database, &identity, "owner-b@example.com", "Owner B").await;
    let workspace_id: Id = setup.0.parse().unwrap();
    let actor_id: Id = sqlx::query_scalar::<_, String>(
        "SELECT user_id FROM memberships WHERE workspace_id = ? AND role = 'owner'",
    )
    .bind(&setup.0)
    .fetch_one(database.pool())
    .await
    .unwrap()
    .parse()
    .unwrap();
    let now = TimestampMillis::now();
    let first_membership = Id::new_v7();
    let second_membership = Id::new_v7();
    for (membership_id, user_id) in [(first_membership, first.0), (second_membership, second.0)] {
        sqlx::query(
            "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at) \
             VALUES (?, ?, ?, 'admin', 0, ?, ?)",
        )
        .bind(membership_id.to_string())
        .bind(&setup.0)
        .bind(user_id.to_string())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(database.pool())
        .await
        .unwrap();
    }
    let repository = WorkspaceRepository::new((*database).clone());
    let left = repository.clone();
    let right = repository.clone();
    let (left, right) = tokio::join!(
        left.transfer_ownership(workspace_id, first_membership, actor_id, 0, 0, "left", now,),
        right.transfer_ownership(
            workspace_id,
            second_membership,
            actor_id,
            0,
            0,
            "right",
            now,
        )
    );
    assert_eq!(usize::from(left.is_ok()) + usize::from(right.is_ok()), 1);
    let owners: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM memberships WHERE workspace_id = ? AND role = 'owner'",
    )
    .bind(&setup.0)
    .fetch_one(database.pool())
    .await
    .unwrap();
    let linked_owner_role: String = sqlx::query_scalar(
        "SELECT memberships.role FROM workspaces JOIN memberships \
         ON memberships.id = workspaces.owner_membership_id WHERE workspaces.id = ?",
    )
    .bind(&setup.0)
    .fetch_one(database.pool())
    .await
    .unwrap();
    assert_eq!(owners, 1);
    assert_eq!(linked_owner_role, "owner");
}

#[tokio::test]
async fn invitation_expiry_smtp_provenance_and_concurrent_resend_are_enforced() {
    let database = TestDatabase::new().await.unwrap();
    let identity = Arc::new(IdentityRepository::new((*database).clone()));
    let setup = setup_owner(&identity).await;
    let owner_id: Id = sqlx::query_scalar::<_, String>(
        "SELECT user_id FROM memberships WHERE workspace_id = ? AND role = 'owner'",
    )
    .bind(&setup.0)
    .fetch_one(database.pool())
    .await
    .unwrap()
    .parse()
    .unwrap();
    let smtp_user = create_user(&database, &identity, "smtp@example.com", "SMTP").await;
    let expired_user = create_user(&database, &identity, "expired@example.com", "Expired").await;
    let resend_user = create_user(&database, &identity, "resend@example.com", "Resend").await;
    let repository = WorkspaceRepository::new((*database).clone());
    let workspace_id = setup.0.parse().unwrap();
    let now = TimestampMillis::now();
    let smtp = repository
        .invite(
            workspace_id,
            owner_id,
            "smtp@example.com".to_owned(),
            orbit_domain::WorkspaceRole::Member,
            orbit_server::repositories::workspaces::InvitationDelivery::Smtp,
            "smtp",
            now,
        )
        .await
        .unwrap();
    assert_eq!(
        smtp.invitation.expires_at.as_millis() - now.as_millis(),
        7 * 24 * 60 * 60 * 1_000
    );
    assert!(!format!("{smtp:?}").contains(&smtp.token));
    let accepted = repository
        .accept_invitation(&smtp.token, smtp_user.0, "SMTP@example.com", "accept", now)
        .await
        .unwrap();
    assert!(accepted.email_verified);

    let expired = repository
        .invite(
            workspace_id,
            owner_id,
            "expired@example.com".to_owned(),
            orbit_domain::WorkspaceRole::Member,
            orbit_server::repositories::workspaces::InvitationDelivery::Manual,
            "expired",
            now,
        )
        .await
        .unwrap();
    assert!(
        repository
            .accept_invitation(
                &expired.token,
                expired_user.0,
                "expired@example.com",
                "expired",
                expired.invitation.expires_at,
            )
            .await
            .is_err()
    );

    let first_repository = repository.clone();
    let second_repository = repository.clone();
    let first = tokio::spawn(async move {
        first_repository
            .invite(
                workspace_id,
                owner_id,
                "resend@example.com".to_owned(),
                orbit_domain::WorkspaceRole::Member,
                orbit_server::repositories::workspaces::InvitationDelivery::Manual,
                "first",
                now,
            )
            .await
            .unwrap()
    });
    let second = tokio::spawn(async move {
        second_repository
            .invite(
                workspace_id,
                owner_id,
                "RESEND@example.com".to_owned(),
                orbit_domain::WorkspaceRole::Member,
                orbit_server::repositories::workspaces::InvitationDelivery::Manual,
                "second",
                now,
            )
            .await
            .unwrap()
    });
    let first = first.await.unwrap();
    let second = second.await.unwrap();
    let pending: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM workspace_invitations WHERE workspace_id = ? AND normalized_email = \
         'resend@example.com' AND accepted_at IS NULL AND revoked_at IS NULL AND replaced_at IS NULL",
    )
    .bind(&setup.0)
    .fetch_one(database.pool())
    .await
    .unwrap();
    assert_eq!(pending, 1);
    let results = [first.token, second.token];
    let mut successes = 0;
    for token in results {
        if repository
            .accept_invitation(&token, resend_user.0, "resend@example.com", "resend", now)
            .await
            .is_ok()
        {
            successes += 1;
        }
    }
    assert_eq!(successes, 1);
}

#[tokio::test]
async fn failed_security_actions_are_audited_and_global_audit_is_admin_only() {
    let database = TestDatabase::new().await.unwrap();
    let identity = Arc::new(IdentityRepository::new((*database).clone()));
    let setup = setup_owner(&identity).await;
    let owner_cookie = format!("__Host-orbit_session={}", setup.1);
    let outsider = create_user(
        &database,
        &identity,
        "audit-outsider@example.com",
        "Outsider",
    )
    .await;
    let app = workspace_router(WorkspaceState::new(
        identity,
        "https://orbit.test".to_owned(),
        CookieMode::secure(),
    ));
    let invitation = create_invitation(
        &app,
        &setup.0,
        &owner_cookie,
        "audit-recipient@example.com",
        "member",
    )
    .await;
    let mismatch = accept_url(&app, &outsider.1, invitation["url"].as_str().unwrap()).await;
    assert_eq!(mismatch.status(), StatusCode::FORBIDDEN);
    let outsider_invitation = create_invitation(
        &app,
        &setup.0,
        &owner_cookie,
        "audit-outsider@example.com",
        "member",
    )
    .await;
    assert_eq!(
        accept_url(
            &app,
            &outsider.1,
            outsider_invitation["url"].as_str().unwrap()
        )
        .await
        .status(),
        StatusCode::OK
    );
    let delete_denied = app
        .clone()
        .oneshot(cookie_request(
            "DELETE",
            &format!("/api/v1/workspaces/{}?expected_version=0", setup.0),
            &outsider.1,
        ))
        .await
        .unwrap();
    assert_eq!(delete_denied.status(), StatusCode::FORBIDDEN);
    let owner_invite_denied = app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/api/v1/workspaces/{}/invitations", setup.0),
            &owner_cookie,
            json!({"email":"owner-role@example.com","role":"owner","delivery":"manual"}),
        ))
        .await
        .unwrap();
    assert_eq!(owner_invite_denied.status(), StatusCode::FORBIDDEN);

    let denied = app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/api/v1/admin/users/{}/suspension", setup.0),
            &outsider.1,
            json!({"suspended": true}),
        ))
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    let global_forbidden = app
        .clone()
        .oneshot(cookie_request("GET", "/api/v1/admin/audit", &outsider.1))
        .await
        .unwrap();
    assert_eq!(global_forbidden.status(), StatusCode::FORBIDDEN);
    let global = app
        .clone()
        .oneshot(cookie_request("GET", "/api/v1/admin/audit", &owner_cookie))
        .await
        .unwrap();
    assert_eq!(global.status(), StatusCode::OK);
    let global = response_json(global).await;
    assert!(global["items"].as_array().unwrap().iter().any(|event| {
        event["action"] == "account.suspension_denied" && event["outcome"] == "failure"
    }));
    assert!(global["items"].as_array().unwrap().iter().any(|event| {
        event["action"] == "invitation.accept_failed" && event["outcome"] == "failure"
    }));
    assert!(global["items"].as_array().unwrap().iter().any(|event| {
        event["action"] == "workspace.delete_denied" && event["outcome"] == "failure"
    }));
    assert!(global["items"].as_array().unwrap().iter().any(|event| {
        event["action"] == "invitation.create_denied" && event["outcome"] == "failure"
    }));
    let export = app
        .oneshot(cookie_request(
            "GET",
            "/api/v1/admin/audit/export",
            &owner_cookie,
        ))
        .await
        .unwrap();
    assert_eq!(export.status(), StatusCode::OK);
    assert_eq!(
        export.headers()[header::CONTENT_TYPE],
        "text/csv; charset=utf-8"
    );
}

#[tokio::test]
async fn owner_pointer_cannot_reference_another_workspaces_membership_on_insert() {
    let database = TestDatabase::new().await.unwrap();
    let identity = Arc::new(IdentityRepository::new((*database).clone()));
    let setup = setup_owner(&identity).await;
    let member = create_user(&database, &identity, "pointer-member@example.com", "Member").await;
    let membership_id = Id::new_v7();
    let now = TimestampMillis::now().as_millis();
    sqlx::query(
        "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at) \
         VALUES (?, ?, ?, 'admin', 0, ?, ?)",
    )
    .bind(membership_id.to_string())
    .bind(&setup.0)
    .bind(member.0.to_string())
    .bind(now)
    .bind(now)
    .execute(database.pool())
    .await
    .unwrap();

    let result = sqlx::query(
        "INSERT INTO workspaces (id, name, version, owner_membership_id, created_at, updated_at) \
         VALUES (?, 'Bypass', 0, ?, ?, ?)",
    )
    .bind(Id::new_v7().to_string())
    .bind(membership_id.to_string())
    .bind(now)
    .bind(now)
    .execute(database.pool())
    .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn invalid_invitation_is_rejected_before_password_validation_and_is_summarized() {
    let database = TestDatabase::new().await.unwrap();
    let identity = Arc::new(IdentityRepository::new((*database).clone()));
    setup_owner(&identity).await;
    let app = workspace_router(WorkspaceState::new(
        identity,
        "https://orbit.test".to_owned(),
        CookieMode::secure(),
    ));

    let response = app
        .clone()
        .oneshot(json_request_without_cookie(
            "POST",
            "/api/v1/workspaces/invitations/accept",
            json!({
                "token": "unknown-token",
                "email": "probe@example.com",
                "display_name": "Probe",
                "password": "short"
            }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let mut throttled = None;
    for _ in 0..60 {
        let response = app
            .clone()
            .oneshot(json_request_without_cookie(
                "POST",
                "/api/v1/workspaces/invitations/accept",
                json!({
                    "token": "another-unknown-token",
                    "email": "probe@example.com",
                    "display_name": "Probe",
                    "password": "correct horse battery"
                }),
            ))
            .await
            .unwrap();
        if response.status() == StatusCode::TOO_MANY_REQUESTS {
            throttled = Some(response);
            break;
        }
    }
    assert_eq!(
        response_json(throttled.expect("registration attempts are throttled")).await["code"],
        "invitation_registration_throttled"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM audit_events WHERE action = 'invitation.registration_failed'"
        )
        .fetch_one(database.pool())
        .await
        .unwrap(),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT SUM(attempt_count) FROM security_probe_summaries WHERE kind = 'invitation.registration_invalid'"
        )
        .fetch_one(database.pool())
        .await
        .unwrap(),
        5
    );
}

#[tokio::test]
async fn global_audit_export_follows_every_cursor_page() {
    let database = TestDatabase::new().await.unwrap();
    let identity = Arc::new(IdentityRepository::new((*database).clone()));
    let setup = setup_owner(&identity).await;
    let owner_cookie = format!("__Host-orbit_session={}", setup.1);
    let now = TimestampMillis::now().as_millis();
    for index in 0..125 {
        sqlx::query(
            "INSERT INTO audit_events (id, workspace_id, actor_id, action, outcome, resource_type, \
             request_id, metadata_json, occurred_at) VALUES (?, NULL, NULL, 'export.test', 'success', \
             'installation', ?, '{}', ?)",
        )
        .bind(Id::new_v7().to_string())
        .bind(format!("export-{index}"))
        .bind(now + index)
        .execute(database.pool())
        .await
        .unwrap();
    }
    let app = workspace_router(WorkspaceState::new(
        identity,
        "https://orbit.test".to_owned(),
        CookieMode::secure(),
    ));

    let response = app
        .oneshot(cookie_request(
            "GET",
            "/api/v1/admin/audit/export?action=export.test&limit=17",
            &owner_cookie,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let csv = String::from_utf8(
        to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert_eq!(csv.lines().skip(1).count(), 125);
}

#[tokio::test]
async fn global_audit_export_returns_before_reading_later_pages() {
    let database = TestDatabase::new().await.unwrap();
    let identity = Arc::new(IdentityRepository::new((*database).clone()));
    let setup = setup_owner(&identity).await;
    let owner_cookie = format!("__Host-orbit_session={}", setup.1);
    let now = TimestampMillis::now().as_millis();
    for index in 0..100 {
        sqlx::query("INSERT INTO audit_events (id, action, outcome, resource_type, request_id, metadata_json, occurred_at) VALUES (?, 'stream.test', 'success', 'installation', ?, '{}', ?)")
            .bind(Id::new_v7().to_string()).bind(format!("stream-{index}")).bind(now + index).execute(database.pool()).await.unwrap();
    }
    sqlx::query("INSERT INTO audit_events (id, action, outcome, resource_type, request_id, metadata_json, occurred_at) VALUES ('00000000-0000-0000-0000-000000000000', 'stream.test', 'success', 'installation', 'corrupt-tail', '{}', ?)")
        .bind(now - 1).execute(database.pool()).await.unwrap();
    let app = workspace_router(WorkspaceState::new(
        identity,
        "https://orbit.test".to_owned(),
        CookieMode::secure(),
    ));

    let response = app
        .oneshot(cookie_request(
            "GET",
            "/api/v1/admin/audit/export?action=stream.test&limit=100",
            &owner_cookie,
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "text/csv; charset=utf-8"
    );
}

#[tokio::test]
async fn retention_service_installs_a_recurring_schedule_and_runs_it() {
    let database = TestDatabase::new().await.unwrap();
    let repository = WorkspaceRepository::new((*database).clone());
    let shutdown = tokio_util::sync::CancellationToken::new();
    let service_shutdown = shutdown.clone();
    let task = tokio::spawn(async move {
        repository
            .run_retention_service(
                service_shutdown,
                std::time::Duration::from_millis(5),
                orbit_platform::WorkerConfig::new(1)
                    .unwrap()
                    .with_poll_interval(std::time::Duration::from_millis(1)),
            )
            .await
    });
    loop {
        let summaries: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM maintenance_summaries")
            .fetch_one(database.pool())
            .await
            .unwrap();
        if summaries != 0 {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM schedules WHERE job_kind = 'workspace.retention' AND enabled = 1"
        )
        .fetch_one(database.pool())
        .await
        .unwrap(),
        1
    );
    shutdown.cancel();
    task.await.unwrap().unwrap();
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

fn json_request_without_cookie(method: &str, uri: &str, value: Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
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
