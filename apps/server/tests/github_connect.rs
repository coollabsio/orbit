//! The workspace GitHub App's OAuth client, re-creating apps from before PR review, and connecting a GitHub account
//! for each member (plan 1 of PR review).

mod fake_github;

use axum::http::StatusCode;
use fake_github::{
    KEY, decrypt_for_test, fixture, json_body, location, query_param, GithubFixture,
};
use orbit_platform::{Row, TimestampMillis};
use orbit_server::github::user_tokens::{TokenError, mark_broken, user_token};
use orbit_server::github::{GithubClient, GithubEndpoints, GithubError};
use serde_json::json;
use sha2::{Digest, Sha256};

fn me(fixture: &GithubFixture) -> String {
    format!("/api/v1/workspaces/{}/github/me", fixture.workspace())
}

/// A pending manifest registration whose state is `state`.
async fn register_state(fixture: &GithubFixture, state: &str) {
    let hash: [u8; 32] = Sha256::digest(state.as_bytes()).into();
    sqlx::query("INSERT INTO github_app_registrations (state_hash, workspace_id, expires_at, public_origin) VALUES ($1, $2, $3, 'https://orbit.test')")
        .bind(hash.to_vec())
        .bind(fixture.workspace())
        .bind(TimestampMillis::now().as_millis() + 60_000)
        .execute(fixture.database.pool())
        .await
        .unwrap();
}

fn conversion(id: i64) -> serde_json::Value {
    json!({ "id": id, "slug": format!("orbit-{id}"), "pem": "pem", "webhook_secret": "hook",
            "client_id": "Iv1.abc", "client_secret": "s3cret" })
}

/// Starts a connect flow for `cookie` and returns GitHub's authorize URL.
async fn start(fixture: &GithubFixture, cookie: &str) -> String {
    let response = fixture
        .send("POST", &me(fixture), cookie, Some(json!({ "client": "web" })))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await["authorize_url"]
        .as_str()
        .unwrap()
        .to_owned()
}

async fn callback(fixture: &GithubFixture, code: &str, state: &str) -> String {
    let response = fixture
        .send(
            "GET",
            &format!("/api/v1/integrations/github/user/callback?code={code}&state={state}"),
            "",
            None,
        )
        .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    location(&response)
}

/// Finishes a flow from the page the browser returned to, signed in with `cookie`: (status, body).
async fn finish(fixture: &GithubFixture, back: &str, cookie: &str) -> (StatusCode, serde_json::Value) {
    let token = query_param(back, "token").expect("a finish token");
    let response = fixture
        .send(
            "POST",
            &format!("{}/finish", me(fixture)),
            cookie,
            Some(json!({ "token": token })),
        )
        .await;
    (response.status(), json_body(response).await)
}

#[tokio::test]
async fn manifest_requests_review_permissions() {
    let fixture = fixture().await;
    let response = fixture
        .send(
            "POST",
            &format!("/api/v1/workspaces/{}/github/manifest", fixture.workspace()),
            &fixture.session_cookie,
            Some(json!({})),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let manifest = json_body(response).await["manifest"].clone();
    assert_eq!(manifest["default_permissions"]["pull_requests"], "write");
    assert_eq!(manifest["default_permissions"]["contents"], "write");
    assert_eq!(manifest["default_permissions"]["checks"], "read");
    assert!(
        manifest["default_events"]
            .as_array()
            .unwrap()
            .contains(&json!("pull_request_review"))
    );
    assert_eq!(
        manifest["callback_urls"][0],
        "https://orbit.test/api/v1/integrations/github/user/callback"
    );
}

#[tokio::test]
async fn manifest_callback_stores_client_credentials() {
    let fixture = fixture().await;
    fixture.github.with(|state| state.conversion = conversion(91));
    register_state(&fixture, "reg-state").await;
    let response = fixture
        .send(
            "GET",
            "/api/v1/integrations/github/manifest/callback?code=abc&state=reg-state",
            "",
            None,
        )
        .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let row = sqlx::query("SELECT client_id, client_secret_encrypted, callback_origin FROM github_apps WHERE workspace_id = $1")
        .bind(fixture.workspace())
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(row.get::<String, _>("client_id"), "Iv1.abc");
    assert_eq!(
        decrypt_for_test(&KEY, &row.get::<Vec<u8>, _>("client_secret_encrypted")),
        "s3cret"
    );
    assert_eq!(row.get::<String, _>("callback_origin"), "https://orbit.test");
}

#[tokio::test]
async fn old_app_reports_needs_recreate() {
    let fixture = fixture().await;
    fixture.seed_app(false).await;
    let settings = json_body(
        fixture
            .get(&format!("/api/v1/workspaces/{}/github", fixture.workspace()))
            .await,
    )
    .await;
    assert_eq!(settings["needs_recreate"], true);
    let project = json_body(
        fixture
            .get(&format!(
                "/api/v1/workspaces/{}/projects/{}/github",
                fixture.workspace(),
                fixture.project_id
            ))
            .await,
    )
    .await;
    assert_eq!(project["needs_recreate"], true);
    assert_eq!(json_body(fixture.get(&me(&fixture)).await).await["status"], "needs_recreate");
}

#[tokio::test]
async fn current_app_does_not_need_recreate() {
    let fixture = fixture().await;
    fixture.seed_app(true).await;
    let settings = json_body(
        fixture
            .get(&format!("/api/v1/workspaces/{}/github", fixture.workspace()))
            .await,
    )
    .await;
    assert_eq!(settings["needs_recreate"], false);
}

#[tokio::test]
async fn recreate_replaces_old_app_and_its_links() {
    let fixture = fixture().await;
    fixture.seed_app(false).await;
    fixture
        .connect(fixture.user_id, (1, "octo"), "ghu_old", None, None)
        .await;
    let start = fixture
        .send(
            "POST",
            &format!("/api/v1/workspaces/{}/github/manifest", fixture.workspace()),
            &fixture.session_cookie,
            Some(json!({})),
        )
        .await;
    assert_eq!(start.status(), StatusCode::OK);
    let action = json_body(start).await["action"].as_str().unwrap().to_owned();
    let registration = query_param(&action, "state").unwrap();
    fixture.github.with(|state| state.conversion = conversion(92));
    let response = fixture
        .send(
            "GET",
            &format!("/api/v1/integrations/github/manifest/callback?code=abc&state={registration}"),
            "",
            None,
        )
        .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let pool = fixture.database.pool();
    let count = |sql: &'static str| async move {
        sqlx::query_scalar::<_, i64>(sql)
            .fetch_one(pool)
            .await
            .unwrap()
    };
    assert_eq!(count("SELECT COUNT(*) FROM github_installations").await, 0);
    assert_eq!(count("SELECT COUNT(*) FROM github_project_connections").await, 0);
    assert_eq!(count("SELECT COUNT(*) FROM github_user_connections").await, 0);
    let app_id: i64 = sqlx::query_scalar("SELECT app_id FROM github_apps WHERE workspace_id = $1")
        .bind(fixture.workspace())
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(app_id, 92);
}

#[tokio::test]
async fn manifest_start_refused_when_app_is_current() {
    let fixture = fixture().await;
    fixture.seed_app(true).await;
    let response = fixture
        .send(
            "POST",
            &format!("/api/v1/workspaces/{}/github/manifest", fixture.workspace()),
            &fixture.session_cookie,
            Some(json!({})),
        )
        .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn connect_round_trip_stores_encrypted_token() {
    let fixture = fixture().await;
    fixture.seed_app(true).await;
    assert_eq!(json_body(fixture.get(&me(&fixture)).await).await["status"], "disconnected");
    fixture.github.with(|state| {
        state.codes.insert("code-1".into(), (1, "octo".into()));
    });
    let authorize = start(&fixture, &fixture.session_cookie).await;
    assert!(authorize.contains("/github/web/login/oauth/authorize?"));
    assert_eq!(query_param(&authorize, "client_id").as_deref(), Some("Iv1.abc"));
    assert_eq!(
        query_param(&authorize, "code_challenge_method").as_deref(),
        Some("S256")
    );
    assert_eq!(
        query_param(&authorize, "redirect_uri").as_deref(),
        Some("https://orbit.test/api/v1/integrations/github/user/callback")
    );
    let state = query_param(&authorize, "state").unwrap();
    let back = callback(&fixture, "code-1", &state).await;
    assert!(back.starts_with("https://orbit.test/profile/github?github=finish&token="));
    // the callback alone connects nothing: the signed-in member must finish
    assert_eq!(json_body(fixture.get(&me(&fixture)).await).await["status"], "disconnected");
    let (code, body) = finish(&fixture, &back, &fixture.session_cookie).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(body["status"], "connected");
    let status = json_body(fixture.get(&me(&fixture)).await).await;
    assert_eq!(status["status"], "connected");
    assert_eq!(status["login"], "octo");
    let stored: Vec<u8> = sqlx::query_scalar(
        "SELECT access_token_encrypted FROM github_user_connections WHERE user_id = $1",
    )
    .bind(fixture.user_id.to_string())
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    assert!(!String::from_utf8_lossy(&stored).contains("ghu_1"));
    assert_eq!(decrypt_for_test(&KEY, &stored), "ghu_1");
    let form = fixture.github.with(|state| state.token_forms[0].clone());
    assert!(form.get("code_verifier").is_some_and(|value| value.len() >= 43));
    assert_eq!(form.get("client_secret").map(String::as_str), Some("s3cret"));
}

#[tokio::test]
async fn callback_state_is_single_use() {
    let fixture = fixture().await;
    fixture.seed_app(true).await;
    fixture.github.with(|state| {
        state.codes.insert("code-1".into(), (1, "octo".into()));
    });
    let state = query_param(&start(&fixture, &fixture.session_cookie).await, "state").unwrap();
    let back = callback(&fixture, "code-1", &state).await;
    assert!(back.contains("github=finish"));
    assert!(callback(&fixture, "code-1", &state).await.ends_with("github=failed"));
    assert_eq!(finish(&fixture, &back, &fixture.session_cookie).await.0, StatusCode::OK);
    sqlx::query("DELETE FROM github_user_connections")
        .execute(fixture.database.pool())
        .await
        .unwrap();
    // the finish token is single use too
    assert_eq!(
        finish(&fixture, &back, &fixture.session_cookie).await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM github_user_connections")
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn callback_expired_state_fails() {
    let fixture = fixture().await;
    fixture.seed_app(true).await;
    fixture.github.with(|state| {
        state.codes.insert("code-1".into(), (1, "octo".into()));
    });
    let state = query_param(&start(&fixture, &fixture.session_cookie).await, "state").unwrap();
    sqlx::query("UPDATE github_user_authorizations SET expires_at = 1")
        .execute(fixture.database.pool())
        .await
        .unwrap();
    assert!(callback(&fixture, "code-1", &state).await.ends_with("github=failed"));
}

#[tokio::test]
async fn unknown_state_fails() {
    let fixture = fixture().await;
    fixture.seed_app(true).await;
    assert_eq!(
        callback(&fixture, "code-1", "nope").await,
        "https://orbit.test/profile/github?github=failed"
    );
}

#[tokio::test]
async fn same_github_account_cannot_join_twice() {
    let fixture = fixture().await;
    fixture.seed_app(true).await;
    fixture
        .connect(fixture.user_id, (1, "octo"), "ghu_a", None, None)
        .await;
    let (member, cookie) = fixture.add_member("member@example.com").await;
    fixture.github.with(|state| {
        state.codes.insert("code-b".into(), (1, "octo".into()));
    });
    let state = query_param(&start(&fixture, &cookie).await, "state").unwrap();
    let back = callback(&fixture, "code-b", &state).await;
    let (code, body) = finish(&fixture, &back, &cookie).await;
    assert_eq!(code, StatusCode::CONFLICT);
    assert_eq!(body["code"], "github_account_taken");
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM github_user_connections WHERE user_id = $1")
            .bind(member.to_string())
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn disconnect_removes_the_connection() {
    let fixture = fixture().await;
    fixture.seed_app(true).await;
    fixture
        .connect(fixture.user_id, (1, "octo"), "ghu_a", None, None)
        .await;
    let response = fixture
        .send("DELETE", &me(&fixture), &fixture.session_cookie, None)
        .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(json_body(fixture.get(&me(&fixture)).await).await["status"], "disconnected");
}

#[tokio::test]
async fn valid_token_is_returned_without_refresh() {
    let fixture = fixture().await;
    fixture.seed_app(true).await;
    let later = TimestampMillis::now().as_millis() + 3_600_000;
    fixture
        .connect(fixture.user_id, (1, "octo"), "ghu_a", Some("ghr_a"), Some(later))
        .await;
    let token = user_token(&fixture.state, &fixture.workspace_id, &fixture.user_id)
        .await
        .ok()
        .unwrap();
    assert_eq!(token.token, "ghu_a");
    assert_eq!(fixture.github.with(|state| state.refresh_calls), 0);
}

#[tokio::test]
async fn expired_token_refreshes_once() {
    let fixture = fixture().await;
    fixture.seed_app(true).await;
    let past = TimestampMillis::now().as_millis() - 1;
    fixture
        .connect(fixture.user_id, (1, "octo"), "ghu_a", Some("ghr_a"), Some(past))
        .await;
    let token = user_token(&fixture.state, &fixture.workspace_id, &fixture.user_id)
        .await
        .ok()
        .unwrap();
    assert_eq!(token.token, "ghu_1");
    assert_eq!(token.login, "octo");
    let again = user_token(&fixture.state, &fixture.workspace_id, &fixture.user_id)
        .await
        .ok()
        .unwrap();
    assert_eq!(again.token, "ghu_1");
    assert_eq!(fixture.github.with(|state| state.refresh_calls), 1);
}

#[tokio::test]
async fn refresh_race_uses_winner_token() {
    let fixture = fixture().await;
    fixture.seed_app(true).await;
    let past = TimestampMillis::now().as_millis() - 1;
    fixture
        .connect(fixture.user_id, (1, "octo"), "ghu_a", Some("ghr_a"), Some(past))
        .await;
    fixture.github.with(|state| state.refresh_delay_ms = 150);
    // Two servers: each has its own process-local lock, so both refresh with the same token.
    let other = fixture.second_server();
    let (first, second) = tokio::join!(
        user_token(&fixture.state, &fixture.workspace_id, &fixture.user_id),
        user_token(&other, &fixture.workspace_id, &fixture.user_id),
    );
    let (first, second) = (first.ok().unwrap(), second.ok().unwrap());
    assert_eq!(first.token, second.token);
    assert_eq!(first.token, "ghu_1");
    let broken: i64 = sqlx::query_scalar("SELECT broken FROM github_user_connections")
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(broken, 0);
}

#[tokio::test]
async fn refresh_failure_marks_broken() {
    let fixture = fixture().await;
    fixture.seed_app(true).await;
    let past = TimestampMillis::now().as_millis() - 1;
    fixture
        .connect(fixture.user_id, (1, "octo"), "ghu_a", Some("ghr_a"), Some(past))
        .await;
    fixture.github.with(|state| state.valid_refresh.clear());
    let result = user_token(&fixture.state, &fixture.workspace_id, &fixture.user_id).await;
    assert!(matches!(result, Err(TokenError::Broken)));
    assert_eq!(json_body(fixture.get(&me(&fixture)).await).await["status"], "broken");
}

#[tokio::test]
async fn not_connected_user_has_no_token() {
    let fixture = fixture().await;
    fixture.seed_app(true).await;
    let result = user_token(&fixture.state, &fixture.workspace_id, &fixture.user_id).await;
    assert!(matches!(result, Err(TokenError::NotConnected)));
}

#[tokio::test]
async fn mobile_return_url_must_be_allowed() {
    let fixture = fixture().await;
    fixture.seed_app(true).await;
    let refused = fixture
        .send(
            "POST",
            &me(&fixture),
            &fixture.session_cookie,
            Some(json!({ "client": "mobile", "return_url": "https://evil.example/x" })),
        )
        .await;
    assert_eq!(refused.status(), StatusCode::UNPROCESSABLE_ENTITY);
    fixture.github.with(|state| {
        state.codes.insert("code-1".into(), (1, "octo".into()));
    });
    let allowed = fixture
        .send(
            "POST",
            &me(&fixture),
            &fixture.session_cookie,
            Some(json!({ "client": "mobile", "return_url": "orbit://auth/callback" })),
        )
        .await;
    assert_eq!(allowed.status(), StatusCode::OK);
    let authorize = json_body(allowed).await["authorize_url"]
        .as_str()
        .unwrap()
        .to_owned();
    let state = query_param(&authorize, "state").unwrap();
    let back = callback(&fixture, "code-1", &state).await;
    assert!(back.starts_with("orbit://auth/callback?github=finish&token="));
    assert_eq!(finish(&fixture, &back, &fixture.session_cookie).await.0, StatusCode::OK);
}

#[tokio::test]
async fn non_member_gets_404() {
    let fixture = fixture().await;
    fixture.seed_app(true).await;
    let cookie = fixture.outsider().await;
    let response = fixture.send("GET", &me(&fixture), &cookie, None).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn no_app_reports_unavailable() {
    let fixture = fixture().await;
    assert_eq!(json_body(fixture.get(&me(&fixture)).await).await["status"], "unavailable");
}

#[tokio::test]
async fn a_link_opened_by_someone_else_cannot_connect_their_account() {
    let fixture = fixture().await;
    fixture.seed_app(true).await;
    let (mallory, mallory_cookie) = fixture.add_member("mallory@example.com").await;
    fixture.github.with(|state| {
        state.codes.insert("alice-code".into(), (7, "alice".into()));
    });
    // Mallory starts a flow and sends the GitHub link to the owner, who authorizes it.
    let state = query_param(&start(&fixture, &mallory_cookie).await, "state").unwrap();
    let back = callback(&fixture, "alice-code", &state).await;
    // The owner's browser lands on Orbit, signed in as the owner: the flow is not theirs.
    let (code, body) = finish(&fixture, &back, &fixture.session_cookie).await;
    assert_eq!(code, StatusCode::FORBIDDEN);
    assert_eq!(body["code"], "github_connect_mismatch");
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM github_user_connections")
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(count, 0);
    // GitHub's code was never exchanged, and the token cannot be retried by Mallory.
    assert!(fixture.github.with(|state| state.token_forms.is_empty()));
    assert_eq!(
        finish(&fixture, &back, &mallory_cookie).await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let _ = mallory;
}

#[tokio::test]
async fn expired_finish_token_fails() {
    let fixture = fixture().await;
    fixture.seed_app(true).await;
    fixture.github.with(|state| {
        state.codes.insert("code-1".into(), (1, "octo".into()));
    });
    let state = query_param(&start(&fixture, &fixture.session_cookie).await, "state").unwrap();
    let back = callback(&fixture, "code-1", &state).await;
    sqlx::query("UPDATE github_user_authorizations SET expires_at = 1")
        .execute(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(
        finish(&fixture, &back, &fixture.session_cookie).await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
}

#[tokio::test]
async fn bad_code_fails_at_finish() {
    let fixture = fixture().await;
    fixture.seed_app(true).await;
    let state = query_param(&start(&fixture, &fixture.session_cookie).await, "state").unwrap();
    let back = callback(&fixture, "unknown-code", &state).await;
    let (code, body) = finish(&fixture, &back, &fixture.session_cookie).await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["code"], "github_connect_failed");
}

#[tokio::test]
async fn server_without_app_key_still_reports_no_app() {
    let fixture = fixture().await;
    let app = fixture.router_without_key();
    let response = fixture.send_to(&app, "GET", &me(&fixture), &fixture.session_cookie).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["status"], "unavailable");
}

#[tokio::test]
async fn github_outage_during_refresh_keeps_the_connection() {
    let fixture = fixture().await;
    fixture.seed_app(true).await;
    let past = TimestampMillis::now().as_millis() - 1;
    fixture
        .connect(fixture.user_id, (1, "octo"), "ghu_a", Some("ghr_a"), Some(past))
        .await;
    fixture.github.with(|state| state.token_reply = Some((503, json!({}))));
    let result = user_token(&fixture.state, &fixture.workspace_id, &fixture.user_id).await;
    assert!(matches!(result, Err(TokenError::Github(_))));
    let broken: i64 = sqlx::query_scalar("SELECT broken FROM github_user_connections")
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(broken, 0);
}

#[tokio::test]
async fn rejected_app_credentials_do_not_break_members() {
    let fixture = fixture().await;
    fixture.seed_app(true).await;
    let past = TimestampMillis::now().as_millis() - 1;
    fixture
        .connect(fixture.user_id, (1, "octo"), "ghu_a", Some("ghr_a"), Some(past))
        .await;
    fixture.github.with(|state| {
        state.token_reply = Some((200, json!({ "error": "incorrect_client_credentials" })));
    });
    let result = user_token(&fixture.state, &fixture.workspace_id, &fixture.user_id).await;
    assert!(matches!(result, Err(TokenError::Github(_))));
    let broken: i64 = sqlx::query_scalar("SELECT broken FROM github_user_connections")
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(broken, 0);
}

#[tokio::test]
async fn a_401_for_an_old_token_does_not_break_a_renewed_connection() {
    let fixture = fixture().await;
    fixture.seed_app(true).await;
    fixture
        .connect(fixture.user_id, (1, "octo"), "ghu_new", None, None)
        .await;
    mark_broken(&fixture.state, &fixture.workspace_id, &fixture.user_id, "ghu_old")
        .await
        .ok()
        .unwrap();
    let broken: i64 = sqlx::query_scalar("SELECT broken FROM github_user_connections")
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(broken, 0);
    mark_broken(&fixture.state, &fixture.workspace_id, &fixture.user_id, "ghu_new")
        .await
        .ok()
        .unwrap();
    let broken: i64 = sqlx::query_scalar("SELECT broken FROM github_user_connections")
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(broken, 1);
}

#[tokio::test]
async fn paths_cannot_leave_the_api_root() {
    let fixture = fixture().await;
    let client = GithubClient::new(GithubEndpoints::local(&fixture.github.base));
    let result = client
        .rest::<serde_json::Value>("", reqwest::Method::GET, "repos/a/../../../web/x", None)
        .await;
    assert!(matches!(result, Err(GithubError::NotFound)));
    assert!(fixture.github.with(|state| state.api_requests.is_empty()));
}
