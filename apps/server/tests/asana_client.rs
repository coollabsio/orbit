//! The Asana client against a local fake API: pagination, rate limits, errors and downloads.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use axum::Json;
use axum::Router;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use orbit_server::asana::client::{AsanaClient, AsanaClientConfig, AsanaError};
use serde_json::{Value, json};

#[derive(Default)]
struct Fake {
    rate_limited: AtomicUsize,
    project_requests: AtomicUsize,
}

async fn serve() -> (u16, Arc<Fake>) {
    let fake = Arc::new(Fake::default());
    let router = Router::new()
        .route("/api/1.0/users/me", get(me))
        .route("/api/1.0/projects", get(projects))
        .route("/api/1.0/projects/{gid}/sections", get(sections))
        .route("/api/1.0/projects/{gid}/tasks", get(forbidden))
        .route("/files/a.png", get(|| async { vec![7u8; 10] }))
        .route("/files/big.bin", get(|| async { vec![0u8; 4096] }))
        .route(
            "/files/out",
            get(|| async {
                Redirect::temporary("https://asana-user-private-us-east-1.s3.amazonaws.com/x")
            }),
        )
        .with_state(Arc::clone(&fake));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (port, fake)
}

fn client(port: u16, token: &str) -> AsanaClient {
    AsanaClient::new(
        token,
        AsanaClientConfig {
            base_url: format!("http://127.0.0.1:{port}/api/1.0"),
            requests_per_second: 1000.0,
            max_attempts: 3,
            initial_backoff: Duration::from_millis(5),
            max_retry_after: Duration::from_secs(2),
            loopback_file_port: Some(port),
            ..AsanaClientConfig::default()
        },
    )
    .unwrap()
}

async fn me(headers: HeaderMap) -> Response {
    if headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        != Some("Bearer good")
    {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"errors": [{"message": "Not Authorized"}]})),
        )
            .into_response();
    }
    Json(json!({"data": {"gid": "1", "name": "Ada", "email": "ada@example.com"}})).into_response()
}

/// Three pages of two projects; the second page is rate limited once with `Retry-After: 1`.
async fn projects(
    State(fake): State<Arc<Fake>>,
    Query(query): Query<Vec<(String, String)>>,
) -> Response {
    fake.project_requests.fetch_add(1, Ordering::SeqCst);
    let get = |key: &str| {
        query
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.clone())
    };
    assert_eq!(get("workspace").as_deref(), Some("100"));
    assert_eq!(get("limit").as_deref(), Some("100"));
    let offset = get("offset");
    if offset.as_deref() == Some("page2") && fake.rate_limited.fetch_add(1, Ordering::SeqCst) == 0 {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [(header::RETRY_AFTER, "1")],
            Json(json!({"errors": [{"message": "You have made too many requests recently."}]})),
        )
            .into_response();
    }
    let (gids, next): (&[&str], Value) = match offset.as_deref() {
        None => (
            &["1", "2"],
            json!({"offset": "page2", "path": "/projects?offset=page2", "uri": "x"}),
        ),
        Some("page2") => (
            &["3", "4"],
            json!({"offset": "page3", "path": "/projects?offset=page3", "uri": "x"}),
        ),
        _ => (&["5"], Value::Null),
    };
    let data: Vec<Value> = gids
        .iter()
        .map(|gid| json!({"gid": gid, "name": format!("P{gid}"), "archived": *gid == "5", "team": {"gid": "9", "name": "Eng"}}))
        .collect();
    Json(json!({"data": data, "next_page": next})).into_response()
}

/// Always rate limited for longer than the client waits.
async fn sections() -> Response {
    (
        StatusCode::TOO_MANY_REQUESTS,
        [(header::RETRY_AFTER, "30")],
        Json(json!({"errors": [{"message": "slow down"}]})),
    )
        .into_response()
}

async fn forbidden() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(json!({"errors": [{"message": "You do not have access"}]})),
    )
        .into_response()
}

#[tokio::test]
async fn follows_next_page_and_waits_for_retry_after() {
    let (port, fake) = serve().await;
    let started = Instant::now();
    let projects = client(port, "good")
        .projects("100", usize::MAX)
        .await
        .unwrap();
    let names: Vec<_> = projects.iter().map(|p| p.name.clone().unwrap()).collect();
    assert_eq!(names, ["P1", "P2", "P3", "P4", "P5"]);
    assert!(projects[4].archived);
    assert_eq!(
        projects[0].team.as_ref().unwrap().name.as_deref(),
        Some("Eng")
    );
    assert!(
        started.elapsed() >= Duration::from_secs(1),
        "waited for Retry-After"
    );
    assert_eq!(fake.project_requests.load(Ordering::SeqCst), 4);
}

#[tokio::test]
async fn stops_at_the_requested_maximum() {
    let (port, fake) = serve().await;
    let projects = client(port, "good").projects("100", 2).await.unwrap();
    assert_eq!(projects.len(), 2);
    assert_eq!(fake.project_requests.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn maps_errors() {
    let (port, _) = serve().await;
    let me = client(port, "good").me().await.unwrap();
    assert_eq!(
        (me.name.as_deref(), me.email.as_deref()),
        (Some("Ada"), Some("ada@example.com"))
    );
    let error = client(port, "bad").me().await.unwrap_err();
    assert!(
        matches!(&error, AsanaError::Unauthorized { message } if message == "Not Authorized"),
        "{error:?}"
    );
    let error = client(port, "good").sections("7").await.unwrap_err();
    assert!(
        matches!(error, AsanaError::RateLimited { retry_after: Some(wait) } if wait == Duration::from_secs(30)),
    );
    let error = client(port, "good")
        .project_tasks("7", 10)
        .await
        .unwrap_err();
    assert!(matches!(error, AsanaError::Forbidden { .. }), "{error:?}");
    let error = client(port, "good")
        .sections("7/../../users")
        .await
        .unwrap_err();
    assert!(matches!(error, AsanaError::InvalidGid));
}

#[tokio::test]
async fn downloads_only_allowlisted_files_within_the_size_cap() {
    let (port, _) = serve().await;
    let client = client(port, "good");
    let file = client
        .download_file(
            &format!("http://127.0.0.1:{port}/files/a.png?X-Amz-Expires=120"),
            100,
        )
        .await
        .unwrap();
    assert_eq!(
        (file.bytes, file.file_name.as_str()),
        (vec![7u8; 10], "a.png")
    );
    let error = client
        .download_file(&format!("http://127.0.0.1:{port}/files/big.bin"), 1000)
        .await
        .unwrap_err();
    assert!(matches!(error, AsanaError::FileTooLarge { limit: 1000 }));
    let error = client
        .download_file(&format!("http://127.0.0.1:{port}/files/out"), 100)
        .await
        .unwrap_err();
    assert!(
        matches!(error, AsanaError::FileNotAllowed),
        "redirect off the loopback port"
    );
    for url in [
        "https://drive.google.com/file/d/1".to_owned(),
        format!("http://localhost:{port}/files/a.png"),
    ] {
        let error = client.download_file(&url, 100).await.unwrap_err();
        assert!(matches!(error, AsanaError::FileNotAllowed), "{url}");
    }
    let production = AsanaClient::new("good", AsanaClientConfig::default()).unwrap();
    let error = production
        .download_file(&format!("http://127.0.0.1:{port}/files/a.png"), 100)
        .await
        .unwrap_err();
    assert!(
        matches!(error, AsanaError::FileNotAllowed),
        "production refuses loopback"
    );
}
