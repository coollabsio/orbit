use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, Request, StatusCode};
use base64::Engine;
use http_body_util::BodyExt;
use orbit_platform::{Config, EnvironmentMode};
use orbit_server::app::App;
use orbit_server::static_assets::{StaticAssetError, StaticAssets, FRONTEND_REVISION};
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

fn app_config(root: &TempDir) -> Config {
    let mut config = Config {
        environment: EnvironmentMode::Development,
        ..Config::default()
    };
    config.data.database = root.path().join("data/orbit.sqlite");
    config.data.attachments = root.path().join("data/attachments");
    config.data.backups = root.path().join("backups");
    config
}

fn login_request(origin: Option<&str>) -> Request<Body> {
    let mut builder =
        Request::post("/api/v1/auth/login").header(header::CONTENT_TYPE, "application/json");
    if let Some(origin) = origin {
        builder = builder.header(header::ORIGIN, origin);
    }
    builder
        .body(Body::from(
            serde_json::json!({
                "email": "missing@example.com",
                "password": "not a real password"
            })
            .to_string(),
        ))
        .unwrap()
}

#[tokio::test]
async fn development_app_accepts_distinct_http_origins_but_still_requires_one() {
    let root = TempDir::new().unwrap();
    let app = App::build(app_config(&root)).await.unwrap();

    for origin in [
        "http://localhost:18888",
        "https://jean-server.tail661ee3.ts.net:8888",
    ] {
        let response = app
            .router()
            .oneshot(login_request(Some(origin)))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{origin}");
    }

    let missing = app.router().oneshot(login_request(None)).await.unwrap();
    assert_eq!(missing.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn embedded_spa_uses_safe_cache_policies_and_never_masks_api_404s() {
    let assets =
        StaticAssets::verified(orbit_server::openapi::CONTRACT_ID, FRONTEND_REVISION).unwrap();
    assert!(matches!(
        StaticAssets::verified(orbit_server::openapi::CONTRACT_ID, "other-revision"),
        Err(StaticAssetError::RevisionMismatch { .. })
    ));
    let router = assets.router();

    let spa = router
        .clone()
        .oneshot(
            Request::get("/settings/members")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(spa.status(), StatusCode::OK);
    assert_eq!(
        spa.headers()[header::CONTENT_TYPE],
        "text/html; charset=utf-8"
    );
    assert_eq!(spa.headers()[header::CACHE_CONTROL], "no-store");
    assert!(
        String::from_utf8(spa.into_body().collect().await.unwrap().to_bytes().to_vec())
            .unwrap()
            .contains("<title>Orbit</title>")
    );

    let manifest = router
        .clone()
        .oneshot(
            Request::get("/manifest.webmanifest")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(manifest.status(), StatusCode::OK);
    assert_eq!(manifest.headers()[header::CACHE_CONTROL], "no-store");

    let asset_path = assets
        .immutable_asset_path()
        .expect("the production frontend contains a hashed asset");
    let asset = router
        .clone()
        .oneshot(Request::get(&asset_path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(asset.status(), StatusCode::OK);
    assert_eq!(
        asset.headers()[header::CACHE_CONTROL],
        "public, max-age=31536000, immutable"
    );

    let api = router
        .oneshot(Request::get("/api/v1/missing").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(api.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        api.headers()[header::CONTENT_TYPE],
        "application/problem+json"
    );
    let problem: serde_json::Value =
        serde_json::from_slice(&api.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(problem["code"], "route_not_found");

    let api_root = assets
        .router()
        .oneshot(Request::get("/api").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(api_root.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        api_root.headers()[header::CONTENT_TYPE],
        "application/problem+json"
    );
}

#[tokio::test]
async fn production_csp_hashes_the_exact_embedded_bootstrap_text() {
    let root = TempDir::new().unwrap();
    let app = App::build(app_config(&root)).await.unwrap();
    let response = app
        .router()
        .oneshot(Request::get("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let csp = response.headers()[header::CONTENT_SECURITY_POLICY]
        .to_str()
        .unwrap()
        .to_owned();
    let html = String::from_utf8(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    let script_start = html.find("<script>").unwrap() + "<script>".len();
    let script =
        &html[script_start..script_start + html[script_start..].find("</script>").unwrap()];
    let exact_hash = format!(
        "sha256-{}",
        base64::engine::general_purpose::STANDARD.encode(Sha256::digest(script.as_bytes()))
    );

    assert!(csp.contains(&format!("script-src 'self' '{exact_hash}'")));
    assert!(csp.contains("style-src 'self'; style-src-attr 'unsafe-inline'"));
}

#[tokio::test]
async fn app_build_migrates_checks_readiness_and_returns_first_run_setup_url() {
    let root = TempDir::new().unwrap();
    let app = App::build(app_config(&root)).await.unwrap();

    assert!(app.setup_url().is_some_and(|url| {
        url.starts_with("http://127.0.0.1:8080/setup?token=") && !url.ends_with("token=")
    }));
    assert_eq!(
        app.database()
            .scalar::<i64>("SELECT MAX(version) FROM schema_migrations")
            .await
            .unwrap(),
        18
    );

    let readiness = app
        .router()
        .oneshot(Request::get("/health/ready").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(readiness.status(), StatusCode::OK);
    let report: serde_json::Value =
        serde_json::from_slice(&readiness.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(report["ready"], true);

    let missing = app
        .router()
        .oneshot(
            Request::get("/api/v1/not-a-route")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let response_request_id = missing.headers()["x-request-id"]
        .to_str()
        .unwrap()
        .to_owned();
    let problem: serde_json::Value =
        serde_json::from_slice(&missing.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(problem["request_id"], response_request_id);

    let router = app.router();
    let checks = (0..20).map(|_| {
        router
            .clone()
            .oneshot(Request::get("/health/ready").body(Body::empty()).unwrap())
    });
    for response in futures_util::future::join_all(checks).await {
        assert_eq!(response.unwrap().status(), StatusCode::OK);
    }
}

#[tokio::test]
async fn metrics_are_counted_without_exposing_them_on_the_public_listener() {
    let root = TempDir::new().unwrap();
    let app = App::build(app_config(&root)).await.unwrap();

    let public = app
        .router()
        .oneshot(Request::get("/metrics").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(
        public.headers()[header::CONTENT_TYPE],
        "text/html; charset=utf-8"
    );

    let metrics = app
        .metrics_router()
        .oneshot(Request::get("/metrics").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(metrics.status(), StatusCode::OK);
    assert_eq!(
        metrics.headers()[header::CONTENT_TYPE],
        "text/plain; version=0.0.4; charset=utf-8"
    );
    let body = String::from_utf8(
        metrics
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    assert!(body.contains("orbit_http_requests_total 1"));
    assert!(body.contains("orbit_http_requests_in_flight 0"));
}

#[tokio::test]
async fn app_build_fails_closed_before_listening_when_storage_is_not_writable() {
    let root = TempDir::new().unwrap();
    let config = app_config(&root);
    std::fs::create_dir_all(config.data.attachments.parent().unwrap()).unwrap();
    std::fs::write(&config.data.attachments, b"not a directory").unwrap();

    let error = App::build(config).await.unwrap_err();

    assert!(error.to_string().contains("writable storage"));
}

#[tokio::test]
async fn app_build_rejects_backups_inside_live_attachment_storage() {
    let root = TempDir::new().unwrap();
    let mut config = app_config(&root);
    config.data.backups = config.data.attachments.join("backups");

    let error = App::build(config).await.unwrap_err();

    assert!(error.to_string().contains("backup directory"));
}

#[tokio::test]
async fn app_build_rejects_a_database_inside_live_attachment_storage() {
    let root = TempDir::new().unwrap();
    let mut config = app_config(&root);
    config.data.database = config.data.attachments.join("orbit.sqlite");

    let error = App::build(config).await.unwrap_err();

    assert!(error.to_string().contains("database file must be outside"));
}

#[tokio::test]
async fn trusted_https_proxy_produces_a_secure_host_only_session_cookie() {
    let root = TempDir::new().unwrap();
    let mut config = app_config(&root);
    config.environment = EnvironmentMode::Production;
    config.http.public_origin = "https://orbit.example".to_owned();
    config.http.trusted_proxies = vec!["127.0.0.1/32".parse().unwrap()];
    let app = App::build(config).await.unwrap();
    let token = app.setup_url().unwrap().split_once("token=").unwrap().1;
    let request = Request::post("/api/v1/setup/complete")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, "https://orbit.example")
        .header("x-forwarded-proto", "https")
        .extension(ConnectInfo(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            43100,
        )))
        .body(Body::from(
            serde_json::json!({
                "token": token,
                "email": "owner@example.com",
                "display_name": "Owner",
                "password": "a long unique launch password 9347",
                "workspace_name": "Orbit",
                "project_name": "Tasks"
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.router().oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
    let cookie = response.headers()[header::SET_COOKIE].to_str().unwrap();
    assert!(cookie.starts_with("__Host-orbit_session="));
    assert!(cookie.contains("; Secure; HttpOnly; SameSite=Lax"));
    let max_age = cookie
        .split(';')
        .map(str::trim)
        .find_map(|part| part.strip_prefix("Max-Age="))
        .unwrap()
        .parse::<u64>()
        .unwrap();
    let ninety_days = 90 * 24 * 60 * 60;
    assert!((ninety_days - 60..=ninety_days).contains(&max_age));
    assert!(response.headers().contains_key("strict-transport-security"));
}

#[tokio::test]
async fn api_method_mismatches_are_problem_details() {
    let root = TempDir::new().unwrap();
    let app = App::build(app_config(&root)).await.unwrap();

    let response = app
        .router()
        .oneshot(
            Request::put("/api/v1/auth/me")
                .header(header::ORIGIN, "http://127.0.0.1:8080")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "application/problem+json"
    );
    let response_request_id = response.headers()["x-request-id"]
        .to_str()
        .unwrap()
        .to_owned();
    let problem: serde_json::Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(problem["code"], "method_not_allowed");
    assert_eq!(problem["request_id"], response_request_id);
}

#[tokio::test]
async fn installation_admin_can_create_an_online_backup() {
    let root = TempDir::new().unwrap();
    let app = App::build(app_config(&root)).await.unwrap();
    let token = app.setup_url().unwrap().split_once("token=").unwrap().1;
    let setup = app
        .router()
        .oneshot(
            Request::post("/api/v1/setup/complete")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::ORIGIN, "http://127.0.0.1:8080")
                .body(Body::from(
                    serde_json::json!({
                        "token": token,
                        "email": "owner@example.com",
                        "display_name": "Owner",
                        "password": "a long unique launch password 9347",
                        "workspace_name": "Orbit",
                        "project_name": "Tasks"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(setup.status(), StatusCode::CREATED);
    let cookie = setup.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();

    let response = app
        .router()
        .oneshot(
            Request::post("/api/v1/admin/backups")
                .header(header::ORIGIN, "http://127.0.0.1:8080")
                .header(header::COOKIE, cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
    let body: serde_json::Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let id = body["id"].as_str().unwrap();
    let backups = orbit_platform::BackupService::new(
        root.path().join("backups"),
        root.path().join("data/attachments"),
    );
    backups.verify(id).await.unwrap();
}

#[tokio::test]
async fn critical_durable_schedules_exist_before_serve() {
    let root = TempDir::new().unwrap();
    let app = App::build(app_config(&root)).await.unwrap();

    let kinds: Vec<String> = sqlx::query_scalar("SELECT job_kind FROM schedules ORDER BY job_kind")
        .fetch_all(app.database().pool())
        .await
        .unwrap();

    assert_eq!(kinds, ["integrity.weekly", "workspace.retention"]);
    assert!(app.production_services_ready());
}

#[tokio::test]
async fn startup_disables_legacy_automatic_backup_work() {
    let root = TempDir::new().unwrap();
    let config = app_config(&root);
    let first = App::build(config.clone()).await.unwrap();
    sqlx::query(
        "INSERT INTO schedules (id, job_kind, payload_json, schedule, next_run_at, enabled, updated_at) \
         VALUES ('legacy-backup', 'backup.daily', '{}', '@every 86400000ms', 1, 1, 1)",
    )
    .execute(first.database().pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO jobs (id, kind, payload_json, state, priority, max_attempts, available_at, created_at, updated_at) \
         VALUES ('legacy-backup-job', 'backup.daily', '{}', 'queued', 'critical', 8, 1, 1, 1)",
    )
    .execute(first.database().pool())
    .await
    .unwrap();
    drop(first);

    let app = App::build(config).await.unwrap();
    let enabled: i64 =
        sqlx::query_scalar("SELECT enabled FROM schedules WHERE job_kind = 'backup.daily'")
            .fetch_one(app.database().pool())
            .await
            .unwrap();
    let queued: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM jobs WHERE kind = 'backup.daily'")
        .fetch_one(app.database().pool())
        .await
        .unwrap();
    assert_eq!(enabled, 0);
    assert_eq!(queued, 0);
}

#[tokio::test]
async fn malformed_critical_schedule_fails_before_serve() {
    let root = TempDir::new().unwrap();
    let config = app_config(&root);
    let first = App::build(config.clone()).await.unwrap();
    sqlx::query(
        "UPDATE schedules SET schedule = 'not-a-schedule', next_run_at = 4102444800000 \
         WHERE job_kind = 'integrity.weekly'",
    )
    .execute(first.database().pool())
    .await
    .unwrap();
    drop(first);

    let error = App::build(config).await.unwrap_err();
    assert!(error.to_string().contains("invalid stored schedule"));
}

#[tokio::test]
async fn weekly_foreign_key_failure_stops_serving() {
    let root = TempDir::new().unwrap();
    let config = app_config(&root);
    let app = App::build(config.clone()).await.unwrap();
    let mut connection = app.database().pool().acquire().await.unwrap();
    sqlx::query("PRAGMA foreign_keys = OFF")
        .execute(&mut *connection)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO sessions (id, token_hash, user_id, created_at, last_activity_at, \
         idle_expires_at, absolute_expires_at) VALUES (?, zeroblob(32), ?, 1, 1, 2, 2)",
    )
    .bind(orbit_platform::Id::new_v7().to_string())
    .bind(orbit_platform::Id::new_v7().to_string())
    .execute(&mut *connection)
    .await
    .unwrap();
    drop(connection);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        app.serve(listener, CancellationToken::new()),
    )
    .await
    .expect("integrity failure stops the server")
    .unwrap_err();

    assert!(result.to_string().contains("integrity"));
    let restart = App::build(config).await.unwrap_err();
    assert!(
        restart.to_string().contains("integrity"),
        "a durable full-check failure must remain fail-closed across restart"
    );
}

#[tokio::test]
async fn graceful_shutdown_stops_http_then_drains_production_services() {
    let root = TempDir::new().unwrap();
    let app = App::build(app_config(&root)).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let shutdown = CancellationToken::new();
    let trigger = shutdown.clone();
    let server = tokio::spawn(app.serve(listener, shutdown));

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    trigger.cancel();

    tokio::time::timeout(std::time::Duration::from_secs(5), server)
        .await
        .expect("production services drain promptly when idle")
        .unwrap()
        .unwrap();
}
