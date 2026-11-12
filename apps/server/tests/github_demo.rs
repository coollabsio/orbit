//! The development demo GitHub: seeded rows plus a local fake, so the PR review screens show sample data with no real
//! GitHub App. Every request goes through the real routes, as in the app.

mod fake_github;

use std::sync::Arc;

use axum::http::StatusCode;
use fake_github::{GithubFixture, KEY, ORIGIN, fixture, json_body};
use orbit_platform::{HttpPlatformLayer, OriginPolicy};
use orbit_server::auth_routes::CookieMode;
use orbit_server::github::GithubEndpoints;
use orbit_server::github::demo::{DEMO_REPOSITORY, seed_demo, start_demo_github};
use orbit_server::integration_routes::{IntegrationState, integration_router};
use orbit_server::repositories::api_tokens::ApiTokenRepository;
use orbit_server::repositories::tasks::TaskRepository;
use serde_json::json;

/// The fixture's workspace seeded with the demo, and a router whose GitHub is the demo server.
async fn demo() -> (GithubFixture, axum::Router, orbit_platform::Id) {
    let fixture = fixture().await;
    let (member, _) = fixture.add_member("mia@example.com").await;
    seed_demo(
        &fixture.database,
        &KEY,
        fixture.workspace_id,
        fixture.project_id,
        fixture.user_id,
        member,
    )
    .await
    .unwrap();
    let base = start_demo_github().await;
    let state = IntegrationState::new(
        Arc::new(ApiTokenRepository::new((*fixture.database).clone())),
        Arc::new(TaskRepository::new((*fixture.database).clone())),
    )
    .with_github_settings(
        Arc::new(fixture.identity.clone()),
        CookieMode::secure(),
        ORIGIN.to_owned(),
        false,
        Some(KEY),
    )
    .with_github_endpoints(GithubEndpoints::local(&base));
    let app = integration_router(state).layer(HttpPlatformLayer::new(OriginPolicy::new(ORIGIN)));
    (fixture, app, member)
}

async fn call(
    fixture: &GithubFixture,
    app: &axum::Router,
    method: &str,
    path: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
    use tower::ServiceExt;
    let mut request = axum::http::Request::builder()
        .method(method)
        .uri(path)
        .header(axum::http::header::COOKIE, &fixture.session_cookie)
        .header(axum::http::header::ORIGIN, ORIGIN);
    let body = match body {
        Some(body) => {
            request = request.header(axum::http::header::CONTENT_TYPE, "application/json");
            axum::body::Body::from(body.to_string())
        }
        None => axum::body::Body::empty(),
    };
    let response = app
        .clone()
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    (status, json_body(response).await)
}

#[tokio::test]
async fn the_demo_is_connected_and_lists_pull_requests() {
    let (fixture, app, _) = demo().await;
    let (status, me) = call(
        &fixture,
        &app,
        "GET",
        &format!("/api/v1/workspaces/{}/github/me", fixture.workspace()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["status"], "connected");
    let (status, list) = call(
        &fixture,
        &app,
        "GET",
        &format!(
            "/api/v1/workspaces/{}/projects/{}/github/pulls",
            fixture.workspace(),
            fixture.project_id
        ),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(list["items"].as_array().unwrap().len() >= 3, "{list}");
    let (_, review) = call(
        &fixture,
        &app,
        "GET",
        &format!(
            "/api/v1/workspaces/{}/projects/{}/github/pulls?filter=merged",
            fixture.workspace(),
            fixture.project_id
        ),
        None,
    )
    .await;
    assert!(
        review["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["state"] == "merged")
    );
}

#[tokio::test]
async fn a_demo_pull_request_reads_and_takes_a_review() {
    let (fixture, app, _) = demo().await;
    let pull = format!(
        "/api/v1/workspaces/{}/github/pulls/{DEMO_REPOSITORY}/482",
        fixture.workspace()
    );
    let (status, detail) = call(&fixture, &app, "GET", &pull, None).await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert!(detail["title"].as_str().unwrap().len() > 5);
    assert_eq!(detail["viewer"]["can_review"], true);
    let (status, files) = call(&fixture, &app, "GET", &format!("{pull}/files"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        files
            .as_array()
            .unwrap()
            .iter()
            .any(|file| file["patch"].is_string())
    );
    let (_, threads) = call(&fixture, &app, "GET", &format!("{pull}/threads"), None).await;
    let before = threads["threads"].as_array().unwrap().len();
    let comment = json!({ "path": files[0]["path"], "side": "RIGHT", "line": 3, "body": "Demo comment", "mode": "pending", "head_sha": detail["head_sha"] });
    let (status, _) = call(
        &fixture,
        &app,
        "POST",
        &format!("{pull}/comments"),
        Some(comment),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, threads) = call(&fixture, &app, "GET", &format!("{pull}/threads"), None).await;
    assert_eq!(threads["threads"].as_array().unwrap().len(), before + 1);
    assert_eq!(threads["pending_review"]["comment_count"], 1);
    let (status, _) = call(
        &fixture,
        &app,
        "POST",
        &format!("{pull}/review"),
        Some(json!({ "event": "APPROVE", "body": "Looks good" })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, threads) = call(&fixture, &app, "GET", &format!("{pull}/threads"), None).await;
    assert_eq!(threads["pending_review"], serde_json::Value::Null);
    let (status, merged) = call(
        &fixture,
        &app,
        "POST",
        &format!("{pull}/merge"),
        Some(json!({ "method": "squash", "expected_head_sha": detail["head_sha"] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{merged}");
    let (_, detail) = call(&fixture, &app, "GET", &pull, None).await;
    assert_eq!(detail["state"], "merged");
}

#[tokio::test]
async fn the_demo_seed_runs_twice_without_errors_and_adds_notices() {
    let (fixture, _, member) = demo().await;
    seed_demo(
        &fixture.database,
        &KEY,
        fixture.workspace_id,
        fixture.project_id,
        fixture.user_id,
        member,
    )
    .await
    .unwrap();
    let notices: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notifications WHERE kind LIKE 'github_%' AND recipient_user_id = $1",
    )
    .bind(fixture.user_id.to_string())
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    assert!(notices >= 2, "{notices}");
}
