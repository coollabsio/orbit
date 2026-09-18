use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{
    HttpPlatformLayer, OriginPolicy, PasswordService, TestDatabase, TimestampMillis,
};
use orbit_server::integration_routes::{IntegrationState, integration_router};
use orbit_server::repositories::api_tokens::ApiTokenRepository;
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::tasks::TaskRepository;
use serde_json::{Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn discord_events_create_one_labeled_task_and_retries_are_idempotent() {
    let fixture = fixture().await;
    let issued = fixture
        .tokens
        .create(
            fixture.workspace_id,
            fixture.user_id,
            "Discord".to_owned(),
            vec![fixture.project_id],
            true,
            true,
            true,
            None,
            "create-token",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    let rotated = fixture
        .tokens
        .create(
            fixture.workspace_id,
            fixture.user_id,
            "discord".to_owned(),
            vec![fixture.project_id],
            true,
            true,
            true,
            None,
            "rotate-token",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    assert_eq!(
        issued.api_token.service_account_id,
        rotated.api_token.service_account_id
    );
    sqlx::query("UPDATE users SET suspended_at = ? WHERE id = ?")
        .bind(TimestampMillis::now().as_millis())
        .bind(fixture.user_id.to_string())
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let body = json!({
        "event_id": "discord-message-123",
        "message": "A very long Discord message title that needs truncation\nMore details from Discord.",
        "message_url": "https://discord.com/channels/1/2/3"
    });

    let created = fixture
        .app
        .clone()
        .oneshot(request(Some(&issued.token), body.clone()))
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::CREATED);
    let created = response_json(created).await;
    assert_eq!(created["duplicate"], false);
    assert_eq!(
        created["task"]["creator_service_account_id"],
        issued.api_token.service_account_id.unwrap().to_string()
    );
    assert_eq!(created["task"]["creator_service_account_name"], "Discord");
    assert_eq!(created["task"]["creator_id"], fixture.user_id.to_string());
    assert_eq!(
        created["task"]["title"],
        "A very long Discord message title tha..."
    );
    assert_eq!(
        created["task"]["project_id"],
        fixture.project_id.to_string()
    );
    assert_eq!(
        created["task"]["description"],
        "A very long Discord message title that needs truncation\nMore details from Discord.\n\nSource: https://discord.com/channels/1/2/3"
    );
    let task_id = created["task"]["id"].as_str().unwrap().to_owned();

    let repeated = fixture
        .app
        .clone()
        .oneshot(request(Some(&issued.token), body.clone()))
        .await
        .unwrap();
    assert_eq!(repeated.status(), StatusCode::OK);
    let repeated = response_json(repeated).await;
    assert_eq!(repeated["duplicate"], true);
    assert_eq!(repeated["task"]["id"], task_id);
    let counts: (i64, i64, i64) = sqlx::query_as("SELECT (SELECT COUNT(*) FROM tasks WHERE id = ?), (SELECT COUNT(*) FROM integration_events WHERE external_event_id = 'discord-message-123'), (SELECT COUNT(*) FROM labels WHERE workspace_id = ? AND lower(name) = 'discord')")
        .bind(&task_id).bind(fixture.workspace_id.to_string()).fetch_one(fixture.database.pool()).await.unwrap();
    assert_eq!(counts, (1, 1, 1));

    let conflict = fixture.app.oneshot(request(Some(&issued.token), json!({ "event_id": "discord-message-123", "message": "Changed", "message_url": "https://discord.com/channels/1/2/3" }))).await.unwrap();
    assert_eq!(conflict.status(), StatusCode::CONFLICT);
    assert_eq!(
        response_json(conflict).await["code"],
        "integration_event_conflict"
    );
}

#[tokio::test]
async fn discord_events_require_an_active_write_token_and_valid_url() {
    let fixture = fixture().await;
    let missing = fixture
        .app
        .clone()
        .oneshot(request(None, valid_body("missing")))
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(response_json(missing).await["code"], "api_token_required");

    let now = TimestampMillis::now();
    let expired = fixture
        .tokens
        .create(
            fixture.workspace_id,
            fixture.user_id,
            "Expired".to_owned(),
            vec![fixture.project_id],
            false,
            true,
            false,
            Some(now),
            "expired-token",
            now,
        )
        .await
        .unwrap();
    let expired_response = fixture
        .app
        .clone()
        .oneshot(request(Some(&expired.token), valid_body("expired")))
        .await
        .unwrap();
    assert_eq!(expired_response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        response_json(expired_response).await["code"],
        "invalid_api_token"
    );

    let read_only = fixture
        .tokens
        .create(
            fixture.workspace_id,
            fixture.user_id,
            "Read".to_owned(),
            vec![fixture.project_id],
            true,
            false,
            false,
            None,
            "read-token",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    let forbidden = fixture
        .app
        .clone()
        .oneshot(request(Some(&read_only.token), valid_body("read")))
        .await
        .unwrap();
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        response_json(forbidden).await["code"],
        "api_token_scope_forbidden"
    );

    let revoked = fixture
        .tokens
        .create(
            fixture.workspace_id,
            fixture.user_id,
            "Revoked".to_owned(),
            vec![fixture.project_id],
            false,
            true,
            false,
            None,
            "revoked-token",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    fixture
        .tokens
        .revoke(
            fixture.workspace_id,
            revoked.api_token.id,
            fixture.user_id,
            "revoke-token",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    let unauthorized = fixture
        .app
        .clone()
        .oneshot(request(Some(&revoked.token), valid_body("revoked")))
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        response_json(unauthorized).await["code"],
        "invalid_api_token"
    );

    let write = fixture
        .tokens
        .create(
            fixture.workspace_id,
            fixture.user_id,
            "Write".to_owned(),
            vec![fixture.project_id],
            false,
            true,
            false,
            None,
            "write-token",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    let invalid = fixture.app.oneshot(request(Some(&write.token), json!({ "event_id": "bad-url", "message": "Message", "message_url": "https://example.com/message" }))).await.unwrap();
    assert_eq!(invalid.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(response_json(invalid).await["code"], "validation_failed");
}

#[tokio::test]
async fn discord_events_select_one_project_allowed_by_a_multi_project_token() {
    let fixture = fixture().await;
    let second_project = TaskRepository::new((*fixture.database).clone())
        .create_project(
            fixture.workspace_id,
            fixture.user_id,
            "Platform".to_owned(),
            "PLAT".to_owned(),
            "#7c3aed".to_owned(),
            "create-project",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    let issued = fixture
        .tokens
        .create(
            fixture.workspace_id,
            fixture.user_id,
            "Multi-project".to_owned(),
            vec![fixture.project_id, second_project.id],
            false,
            true,
            false,
            None,
            "create-token",
            TimestampMillis::now(),
        )
        .await
        .unwrap();

    let selected = fixture
        .app
        .clone()
        .oneshot(request(
            Some(&issued.token),
            json!({
                "event_id": "selected-project",
                "message": "Discord message",
                "message_url": "https://discord.com/channels/1/2/3",
                "project_id": second_project.id,
            }),
        ))
        .await
        .unwrap();
    assert_eq!(selected.status(), StatusCode::CREATED);
    assert_eq!(
        response_json(selected).await["task"]["project_id"],
        second_project.id.to_string()
    );

    let missing = fixture
        .app
        .oneshot(request(Some(&issued.token), valid_body("missing-project")))
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

struct Fixture {
    app: axum::Router,
    database: TestDatabase,
    tokens: ApiTokenRepository,
    user_id: orbit_platform::Id,
    workspace_id: orbit_platform::Id,
    project_id: orbit_platform::Id,
}

async fn fixture() -> Fixture {
    let database = TestDatabase::new().await.unwrap();
    let identity = IdentityRepository::new((*database).clone());
    let now = TimestampMillis::now();
    identity
        .store_setup_token(
            "setup",
            TimestampMillis::from_millis(now.as_millis() + 60_000),
        )
        .await
        .unwrap();
    let setup = identity
        .complete_setup(
            SetupRequest {
                token: "setup".to_owned(),
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
    let tokens = ApiTokenRepository::new((*database).clone());
    let tasks = Arc::new(TaskRepository::new((*database).clone()));
    let app = integration_router(IntegrationState::new(Arc::new(tokens.clone()), tasks)).layer(
        HttpPlatformLayer::new(OriginPolicy::new("https://orbit.test")),
    );
    Fixture {
        app,
        database,
        tokens,
        user_id: setup.user_id,
        workspace_id: setup.workspace_id,
        project_id: setup.project_id,
    }
}

fn valid_body(event_id: &str) -> Value {
    json!({ "event_id": event_id, "message": "Discord message", "message_url": "https://discord.com/channels/1/2/3" })
}

fn request(token: Option<&str>, body: Value) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/api/v1/integrations/discord/events")
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    builder.body(Body::from(body.to_string())).unwrap()
}

async fn response_json(response: axum::response::Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap()
}
