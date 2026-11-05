//! The Linear client against a local fake GraphQL API: paging, nested follow-ups, rate limits,
//! errors and authenticated downloads.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use orbit_server::linear::client::{LinearClient, LinearClientConfig, LinearError};
use serde_json::{Value, json};

const KEY: &str = "lin_api_good";

#[derive(Default)]
struct Fake {
    rate_limited: AtomicUsize,
    queries: Mutex<Vec<String>>,
    /// `Authorization` headers the file routes saw, by route.
    file_auth: Mutex<Vec<(String, Option<String>)>>,
}

fn page(nodes: Vec<Value>, next: Option<&str>) -> Value {
    json!({"nodes": nodes, "pageInfo": {"hasNextPage": next.is_some(), "endCursor": next}})
}

fn graphql_error(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(json!({"errors": [{"message": message, "extensions": {"code": code}}]})),
    )
        .into_response()
}

async fn graphql(
    State(fake): State<Arc<Fake>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let query = body["query"].as_str().unwrap_or_default().to_owned();
    let variables = &body["variables"];
    fake.queries.lock().unwrap().push(query.clone());
    if headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        != Some(KEY)
    {
        return graphql_error(
            StatusCode::BAD_REQUEST,
            "AUTHENTICATION_ERROR",
            "Authentication required, not authenticated",
        );
    }
    let after = variables["after"].as_str();
    if query.contains("viewer") {
        return Json(
            json!({"data": {"viewer": {"id": "u1", "name": "Ada Lovelace",
            "displayName": "ada", "email": "ada@example.com"}}}),
        )
        .into_response();
    }
    if query.contains("teams(") {
        assert!(query.contains("first: 100"), "{query}");
        if after == Some("t1") && fake.rate_limited.fetch_add(1, Ordering::SeqCst) == 0 {
            // Linear may answer a rate limit with 400 and a RATELIMITED error.
            let mut response = graphql_error(
                StatusCode::BAD_REQUEST,
                "RATELIMITED",
                "Rate limit exceeded",
            );
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, "1".parse().unwrap());
            return response;
        }
        let (nodes, next) = match after {
            None => (
                vec![json!({"id": "a", "key": "ENG", "name": "Engineering", "issueCount": 3})],
                Some("t1"),
            ),
            _ => (
                vec![
                    json!({"id": "b", "key": "OPS", "name": "Ops", "issueCount": 0, "archivedAt": "2024-01-01T00:00:00.000Z"}),
                ],
                None,
            ),
        };
        return Json(json!({"data": {"teams": page(nodes, next)}})).into_response();
    }
    if query.contains("states(") {
        return graphql_error(StatusCode::OK, "RATELIMITED", "Rate limit exceeded").into_response();
    }
    if query.contains("issues(") {
        assert!(query.contains("first: 25"), "{query}");
        assert!(query.contains("comments(first: 10)"), "{query}");
        let issue = json!({
            "id": "i1", "identifier": "ENG-1", "title": "First", "priority": 2,
            "labels": page(vec![json!({"id": "l1", "name": "Bug", "parent": null})], None),
            "relations": page(Vec::new(), None),
            "attachments": page(Vec::new(), None),
            "comments": page(vec![json!({"id": "c1", "body": "one"})], Some("c1")),
        });
        let (nodes, next) = match after {
            None => (vec![issue], Some("p2")),
            _ => (
                vec![json!({"id": "i2", "identifier": "ENG-2", "title": "Second"})],
                None,
            ),
        };
        return Json(json!({"data": {"team": {"issues": page(nodes, next)}}})).into_response();
    }
    if query.contains("issue(id: $id) { comments(first: 50") {
        assert_eq!(variables["id"], "i1");
        let (nodes, next) = match after {
            Some("c1") => (vec![json!({"id": "c2", "body": "two"})], Some("c2")),
            _ => (vec![json!({"id": "c3", "body": "three"})], None),
        };
        return Json(json!({"data": {"issue": {"comments": page(nodes, next)}}})).into_response();
    }
    if query.contains("organization") {
        return graphql_error(StatusCode::OK, "FORBIDDEN", "Forbidden").into_response();
    }
    graphql_error(StatusCode::OK, "INVALID_INPUT", "Entity not found: Issue")
}

async fn serve() -> (u16, u16, Arc<Fake>) {
    let fake = Arc::new(Fake::default());
    let elsewhere = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let elsewhere_port = elsewhere.local_addr().unwrap().port();
    let foreign = Router::new()
        .route(
            "/signed/a.png",
            get(
                |State(fake): State<Arc<Fake>>, headers: HeaderMap| async move {
                    record(&fake, "foreign", &headers);
                    vec![9u8; 12]
                },
            ),
        )
        .with_state(Arc::clone(&fake));
    tokio::spawn(async move { axum::serve(elsewhere, foreign).await.unwrap() });

    let router = Router::new()
        .route("/graphql", post(graphql))
        .route(
            "/uploads/org/file",
            get(
                |State(fake): State<Arc<Fake>>, headers: HeaderMap| async move {
                    record(&fake, "upload", &headers);
                    (
                        [(
                            header::CONTENT_DISPOSITION,
                            "attachment; filename=\"diagram.png\"",
                        )],
                        vec![7u8; 10],
                    )
                },
            ),
        )
        .route(
            "/uploads/org/moved",
            get(
                move |State(fake): State<Arc<Fake>>, headers: HeaderMap| async move {
                    record(&fake, "moved", &headers);
                    Redirect::temporary(&format!("http://127.0.0.1:{elsewhere_port}/signed/a.png"))
                },
            ),
        )
        .route("/uploads/org/big", get(|| async { vec![0u8; 4096] }))
        .with_state(Arc::clone(&fake));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (port, elsewhere_port, fake)
}

fn record(fake: &Fake, route: &str, headers: &HeaderMap) {
    fake.file_auth.lock().unwrap().push((
        route.to_owned(),
        headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned),
    ));
}

fn client(port: u16, key: &str) -> LinearClient {
    LinearClient::new(
        key,
        LinearClientConfig {
            endpoint: format!("http://127.0.0.1:{port}/graphql"),
            requests_per_second: 1000.0,
            max_attempts: 3,
            initial_backoff: Duration::from_millis(5),
            max_retry_after: Duration::from_secs(2),
            loopback_file_port: Some(port),
            ..LinearClientConfig::default()
        },
    )
    .unwrap()
}

#[tokio::test]
async fn pages_teams_and_waits_out_a_rate_limit() {
    let (port, _, fake) = serve().await;
    let started = Instant::now();
    let teams = client(port, KEY).teams(usize::MAX).await.unwrap();
    let keys: Vec<_> = teams.iter().map(|team| team.key.clone().unwrap()).collect();
    assert_eq!(keys, ["ENG", "OPS"]);
    assert_eq!(teams[0].issue_count, 3);
    assert!(teams[1].archived_at.is_some());
    assert!(
        started.elapsed() >= Duration::from_secs(1),
        "waited for Retry-After"
    );
    assert_eq!(
        fake.queries.lock().unwrap().len(),
        3,
        "two pages and one retry"
    );
}

#[tokio::test]
async fn completes_nested_connections_of_each_issue() {
    let (port, _, _) = serve().await;
    let issues = client(port, KEY)
        .team_issues("a", usize::MAX)
        .await
        .unwrap();
    assert_eq!(issues.len(), 2);
    let comments: Vec<_> = issues[0]
        .comments
        .nodes
        .iter()
        .map(|comment| comment.body.clone().unwrap())
        .collect();
    assert_eq!(comments, ["one", "two", "three"]);
    assert!(!issues[0].comments.page_info.has_next_page);
    assert_eq!(issues[0].labels.nodes[0].name.as_deref(), Some("Bug"));
    assert_eq!(issues[0].priority, 2.0);
    let limited = client(port, KEY).team_issues("a", 1).await.unwrap();
    assert_eq!(limited.len(), 1, "stops at max");
}

#[tokio::test]
async fn maps_graphql_errors() {
    let (port, _, _) = serve().await;
    assert!(matches!(
        client(port, "lin_api_wrong").viewer().await,
        Err(LinearError::Unauthorized { .. })
    ));
    let viewer = client(port, KEY).viewer().await.unwrap();
    assert_eq!(viewer.email.as_deref(), Some("ada@example.com"));
    let started = Instant::now();
    assert!(matches!(
        client(port, KEY).states("a").await,
        Err(LinearError::RateLimited { .. })
    ));
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "bounded retries"
    );
    assert!(matches!(
        client(port, KEY).organization().await,
        Err(LinearError::Forbidden { .. })
    ));
}

#[tokio::test]
async fn downloads_uploads_with_the_key_and_redirects_without_it() {
    let (port, elsewhere, fake) = serve().await;
    let linear = client(port, KEY);
    let file = linear
        .download_file(&format!("http://127.0.0.1:{port}/uploads/org/file"), 1024)
        .await
        .unwrap();
    assert_eq!(
        (file.bytes.len(), file.file_name.as_str()),
        (10, "diagram.png")
    );
    let moved = linear
        .download_file(&format!("http://127.0.0.1:{port}/uploads/org/moved"), 1024)
        .await
        .unwrap();
    assert_eq!(moved.bytes, vec![9u8; 12]);
    assert_eq!(
        *fake.file_auth.lock().unwrap(),
        [
            ("upload".to_owned(), Some(KEY.to_owned())),
            ("moved".to_owned(), Some(KEY.to_owned())),
            ("foreign".to_owned(), None),
        ],
        "the key goes only to the Linear host"
    );
    assert!(matches!(
        linear
            .download_file(&format!("http://127.0.0.1:{elsewhere}/signed/a.png"), 1024)
            .await,
        Err(LinearError::FileNotAllowed)
    ));
    assert_eq!(
        fake.file_auth.lock().unwrap().len(),
        3,
        "refused before any request"
    );
    assert!(matches!(
        linear
            .download_file(&format!("http://127.0.0.1:{port}/uploads/org/big"), 100)
            .await,
        Err(LinearError::FileTooLarge { limit: 100 })
    ));
    assert!(linear.is_upload_url(&format!("http://127.0.0.1:{port}/uploads/org/file")));
    assert!(!linear.is_upload_url("https://uploads.linear.app/org/file"));
}
