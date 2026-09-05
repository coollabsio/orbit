use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use orbit_platform::Config;
use orbit_server::app::App;
use orbit_server::static_assets::{FRONTEND_REVISION, StaticAssetError, StaticAssets};
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

fn app_config(root: &TempDir) -> Config {
    let mut config = Config::default();
    config.data.database = root.path().join("data/orbit.sqlite");
    config.data.attachments = root.path().join("data/attachments");
    config.data.backups = root.path().join("backups");
    config
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
    assert_eq!(spa.headers()[header::CACHE_CONTROL], "no-cache");
    assert!(
        String::from_utf8(spa.into_body().collect().await.unwrap().to_bytes().to_vec())
            .unwrap()
            .contains("<title>Orbit</title>")
    );

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
        9
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
    assert!(response.headers().contains_key("strict-transport-security"));
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
