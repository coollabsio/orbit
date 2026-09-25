//! Notion client tests against a local fake Notion API (axum).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Json;
use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, Query};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use orbit_server::notion::client::{NOTION_VERSION, NotionClient, NotionClientConfig, NotionError};
use orbit_server::notion::model::{SearchKind, SearchResult};
use serde_json::{Value, json};

const PAGE_A: &str = "59833787-2cf9-4fdf-8782-e53db20768a5";
const DB: &str = "b8595b75-abd1-4cad-8dfe-f935a8ef57cb";
const DS: &str = "1a44be12-0953-4631-b498-9e5817518db8";

async fn serve(router: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    format!("http://{address}")
}

fn config(base_url: String) -> NotionClientConfig {
    NotionClientConfig {
        base_url,
        requests_per_second: 1000.0,
        initial_backoff: Duration::from_millis(5),
        max_backoff: Duration::from_millis(40),
        ..NotionClientConfig::default()
    }
}

fn client(base_url: String) -> NotionClient {
    NotionClient::new("ntn_test_token", config(base_url)).unwrap()
}

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(
            json!({"object": "error", "status": status.as_u16(), "code": code, "message": message}),
        ),
    )
        .into_response()
}

fn block(id: &str, kind: &str, has_children: bool) -> Value {
    json!({
        "object": "block", "id": id, "type": kind, "has_children": has_children, "in_trash": false,
        kind: {"rich_text": [{"type": "text", "text": {"content": id}, "plain_text": id}], "title": id}
    })
}

fn uuid(n: u32) -> String {
    format!("c0000000-0000-4000-8000-{n:012}")
}

#[tokio::test]
async fn me_sends_auth_and_version_headers() {
    let seen = Arc::new(Mutex::new(None::<(String, String)>));
    let recorder = seen.clone();
    let base = serve(Router::new().route(
        "/v1/users/me",
        get(move |headers: HeaderMap| {
            let recorder = recorder.clone();
            async move {
                let header = |name: &str| headers.get(name).unwrap().to_str().unwrap().to_owned();
                *recorder.lock().unwrap() = Some((header("authorization"), header("notion-version")));
                Json(json!({
                    "object": "user", "id": "9188c6a5-7381-452f-b3dc-d4865aa89bdf", "name": "Test Connection",
                    "avatar_url": null, "type": "bot",
                    "bot": {"owner": {"type": "workspace", "workspace": true}, "workspace_name": "Ada Lovelace’s Notion",
                            "workspace_id": "w1", "workspace_limits": {"max_file_upload_size_in_bytes": 5}},
                    "future_field": 1
                }))
            }
        }),
    ))
    .await;
    let me = client(base).me().await.unwrap();
    assert_eq!(me.kind.as_deref(), Some("bot"));
    assert_eq!(me.workspace_name(), Some("Ada Lovelace’s Notion"));
    let (auth, version) = seen.lock().unwrap().clone().unwrap();
    assert_eq!(auth, "Bearer ntn_test_token");
    assert_eq!(version, NOTION_VERSION);
    assert_eq!(version, "2026-03-11");
}

#[tokio::test]
async fn invalid_token_is_a_distinct_error() {
    let base = serve(Router::new().route(
        "/v1/users/me",
        get(|| async {
            error(
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "API token is invalid.",
            )
        }),
    ))
    .await;
    let error = client(base).me().await.unwrap_err();
    assert!(
        matches!(&error, NotionError::Unauthorized { message } if message == "API token is invalid.")
    );
    assert!(!error.to_string().contains("ntn_test_token"));
}

#[tokio::test]
async fn restricted_and_missing_objects_map_to_variants() {
    let base = serve(
        Router::new()
            .route(
                "/v1/pages/{id}",
                get(|| async { error(StatusCode::FORBIDDEN, "restricted_resource", "no access") }),
            )
            .route(
                "/v1/databases/{id}",
                get(|| async {
                    error(
                        StatusCode::NOT_FOUND,
                        "object_not_found",
                        "Could not find database",
                    )
                }),
            )
            .route(
                "/v1/data_sources/{id}",
                get(|| async { error(StatusCode::BAD_REQUEST, "validation_error", "bad") }),
            ),
    )
    .await;
    let client = client(base);
    assert!(matches!(
        client.retrieve_page(PAGE_A).await,
        Err(NotionError::RestrictedResource { .. })
    ));
    assert!(matches!(
        client.retrieve_database(DB).await,
        Err(NotionError::ObjectNotFound { .. })
    ));
    assert!(matches!(
        client.retrieve_data_source(DS).await,
        Err(NotionError::Api { status: 400, ref code, .. }) if code == "validation_error"
    ));
    assert!(matches!(
        client.retrieve_page("../users/me").await,
        Err(NotionError::InvalidId)
    ));
}

#[tokio::test]
async fn search_all_follows_cursors_in_the_body() {
    let bodies = Arc::new(Mutex::new(Vec::<Value>::new()));
    let recorder = bodies.clone();
    let base = serve(Router::new().route(
        "/v1/search",
        post(move |Json(body): Json<Value>| {
            let recorder = recorder.clone();
            async move {
                recorder.lock().unwrap().push(body.clone());
                let response = match body.get("start_cursor").and_then(Value::as_str) {
                    None => json!({
                        "object": "list", "type": "page_or_data_source", "page_or_data_source": {},
                        "results": [
                            {"object": "page", "id": PAGE_A, "parent": {"type": "workspace", "workspace": true},
                             "properties": {"title": {"id": "title", "type": "title", "title": []}}},
                            {"object": "page", "id": uuid(1), "parent": {"type": "block_id", "block_id": uuid(9)}}
                        ],
                        "next_cursor": "cursor-2", "has_more": true
                    }),
                    Some("cursor-2") => json!({
                        "object": "list", "results": [
                            {"object": "data_source", "id": DS, "title": [], "properties": {},
                             "parent": {"type": "database_id", "database_id": DB},
                             "database_parent": {"type": "page_id", "page_id": PAGE_A}},
                            {"object": "future_thing", "id": "x"}
                        ],
                        "next_cursor": "cursor-3", "has_more": true
                    }),
                    Some(_) => json!({
                        "object": "list",
                        "results": [{"object": "page", "id": uuid(2), "parent": {"type": "agent_id", "agent_id": "a"}}],
                        "next_cursor": null, "has_more": false,
                        "request_status": {"type": "incomplete", "incomplete_reason": "query_result_limit_reached"}
                    }),
                };
                Json(response)
            }
        }),
    ))
    .await;
    let results = client(base).search_all(SearchKind::All).await.unwrap();
    assert_eq!(results.results.len(), 4, "unknown objects are dropped");
    assert!(results.incomplete);
    let SearchResult::DataSource(data_source) = &results.results[2] else {
        panic!("expected a data source");
    };
    assert_eq!(data_source.database_id(), Some(DB));
    let bodies = bodies.lock().unwrap();
    assert_eq!(bodies.len(), 3);
    assert_eq!(bodies[0]["page_size"], 100);
    assert!(bodies[0].get("filter").is_none());
    assert_eq!(bodies[2]["start_cursor"], "cursor-3");
}

#[tokio::test]
async fn search_filters_by_kind() {
    let filters = Arc::new(Mutex::new(Vec::<Value>::new()));
    let recorder = filters.clone();
    let base = serve(Router::new().route(
        "/v1/search",
        post(move |Json(body): Json<Value>| {
            let recorder = recorder.clone();
            async move {
                recorder.lock().unwrap().push(body["filter"].clone());
                Json(json!({"object": "list", "results": [], "next_cursor": null, "has_more": false}))
            }
        }),
    ))
    .await;
    let client = client(base);
    client.search_all(SearchKind::Pages).await.unwrap();
    client.search_all(SearchKind::DataSources).await.unwrap();
    let filters = filters.lock().unwrap();
    assert_eq!(filters[0], json!({"property": "object", "value": "page"}));
    assert_eq!(
        filters[1],
        json!({"property": "object", "value": "data_source"})
    );
}

#[tokio::test]
async fn block_tree_recurses_but_not_into_child_pages() {
    let requested = Arc::new(Mutex::new(Vec::<String>::new()));
    let recorder = requested.clone();
    let base = serve(Router::new().route(
        "/v1/blocks/{id}/children",
        get(
            move |Path(id): Path<String>,
                  Query(query): Query<std::collections::HashMap<String, String>>| {
                let recorder = recorder.clone();
                async move {
                    recorder.lock().unwrap().push(id.clone());
                    assert_eq!(query.get("page_size").map(String::as_str), Some("100"));
                    let cursor = query.get("start_cursor").cloned();
                    let body = if id == PAGE_A && cursor.is_none() {
                        json!({"object": "list", "results": [
                            block(&uuid(1), "toggle", true),
                            block(&uuid(2), "child_page", true),
                        ], "next_cursor": "p2", "has_more": true})
                    } else if id == PAGE_A {
                        assert_eq!(cursor.as_deref(), Some("p2"));
                        json!({"object": "list", "results": [
                            block(&uuid(3), "synced_block", true),
                            block(&uuid(4), "child_database", true),
                        ], "next_cursor": null, "has_more": false})
                    } else if id == uuid(1) {
                        json!({"object": "list", "results": [block(&uuid(5), "paragraph", true)],
                               "next_cursor": null, "has_more": false})
                    } else if id == uuid(5) {
                        json!({"object": "list", "results": [block(&uuid(6), "paragraph", false)],
                               "next_cursor": null, "has_more": false})
                    } else if id == uuid(3) {
                        return error(
                            StatusCode::NOT_FOUND,
                            "object_not_found",
                            "original not shared",
                        );
                    } else {
                        panic!("unexpected children request for {id}");
                    };
                    Json(body).into_response()
                }
            },
        ),
    ))
    .await;
    let tree = client(base).fetch_block_tree(PAGE_A).await.unwrap();
    assert!(!tree.truncated);
    assert_eq!(tree.block_count, 6);
    let kinds: Vec<&str> = tree
        .blocks
        .iter()
        .map(|node| node.block.kind.as_str())
        .collect();
    assert_eq!(
        kinds,
        ["toggle", "child_page", "synced_block", "child_database"]
    );
    assert_eq!(tree.blocks[0].children[0].children[0].block.id, uuid(6));
    assert!(tree.blocks[1].children.is_empty());
    assert!(
        tree.blocks[2].children.is_empty(),
        "inaccessible children are left empty"
    );
    let requested = requested.lock().unwrap();
    assert!(!requested.contains(&uuid(2)) && !requested.contains(&uuid(4)));
}

#[tokio::test]
async fn block_tree_respects_caps() {
    let base = serve(Router::new().route(
        "/v1/blocks/{id}/children",
        get(|Path(id): Path<String>| async move {
            // Every block has one child: an endless chain.
            let next = format!("{}{}", &id[..24], "999999999999");
            let child = if id.ends_with("999999999999") {
                uuid(7)
            } else {
                next
            };
            Json(
                json!({"object": "list", "results": [block(&child, "toggle", true)],
                        "next_cursor": null, "has_more": false}),
            )
        }),
    ))
    .await;
    let tree = NotionClient::new(
        "t",
        NotionClientConfig {
            max_tree_depth: 3,
            ..config(base)
        },
    )
    .unwrap()
    .fetch_block_tree(PAGE_A)
    .await
    .unwrap();
    assert!(tree.truncated);
    assert_eq!(tree.block_count, 3);
}

#[tokio::test]
async fn rate_limit_waits_for_retry_after_then_retries() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let base = serve(Router::new().route(
        "/v1/pages/{id}",
        get(move || {
            let counter = counter.clone();
            async move {
                match counter.fetch_add(1, Ordering::SeqCst) {
                    0 => (
                        StatusCode::TOO_MANY_REQUESTS,
                        [("retry-after", "0.2")],
                        Json(json!({"object": "error", "code": "rate_limited", "message": "slow down",
                                    "additional_data": {"rate_limit_reason": "public_api_request_rate_limit", "retry_after": "0"}})),
                    )
                        .into_response(),
                    1 => (
                        StatusCode::from_u16(529).unwrap(),
                        Json(json!({"object": "error", "code": "service_overload", "message": "overloaded",
                                    "additional_data": {"retry_after": "0.1"}})),
                    )
                        .into_response(),
                    _ => Json(json!({"object": "page", "id": PAGE_A, "properties": {}})).into_response(),
                }
            }
        }),
    ))
    .await;
    let started = Instant::now();
    let page = client(base).retrieve_page(PAGE_A).await.unwrap();
    assert_eq!(page.id, PAGE_A);
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert!(
        started.elapsed() >= Duration::from_millis(300),
        "{:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn rate_limit_gives_up_after_max_attempts() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let base = serve(Router::new().route(
        "/v1/users/me",
        get(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            async {
                (
                    StatusCode::TOO_MANY_REQUESTS,
                    [("retry-after", "0.01")],
                    "{}",
                )
                    .into_response()
            }
        }),
    ))
    .await;
    let client = NotionClient::new(
        "t",
        NotionClientConfig {
            max_attempts: 3,
            ..config(base)
        },
    )
    .unwrap();
    let error = client.me().await.unwrap_err();
    assert!(
        matches!(
            error,
            NotionError::RateLimited {
                retry_after: Some(_)
            }
        ),
        "{error:?}"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn long_retry_after_and_blocked_connections_fail_fast() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let base = serve(
        Router::new()
            .route(
                "/v1/users/me",
                get(|| async { (StatusCode::TOO_MANY_REQUESTS, [("retry-after", "600")], "").into_response() }),
            )
            .route(
                "/v1/pages/{id}",
                get(move || {
                    counter.fetch_add(1, Ordering::SeqCst);
                    async {
                        (
                            StatusCode::TOO_MANY_REQUESTS,
                            [("retry-after", "1")],
                            Json(json!({"object": "error", "code": "rate_limited", "message": "blocked",
                                        "additional_data": {"rate_limit_reason": "public_api_request_blocked"}})),
                        )
                            .into_response()
                    }
                }),
            ),
    )
    .await;
    let client = client(base);
    let started = Instant::now();
    let error = client.me().await.unwrap_err();
    assert!(
        matches!(error, NotionError::RateLimited { retry_after: Some(wait) } if wait == Duration::from_secs(600))
    );
    let error = client.retrieve_page(PAGE_A).await.unwrap_err();
    assert!(matches!(error, NotionError::Blocked { .. }), "{error:?}");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[tokio::test]
async fn server_errors_retry_with_backoff() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let failing = Arc::new(AtomicUsize::new(0));
    let failing_counter = failing.clone();
    let base = serve(
        Router::new()
            .route(
                "/v1/databases/{id}",
                get(move || {
                    let counter = counter.clone();
                    async move {
                        if counter.fetch_add(1, Ordering::SeqCst) < 2 {
                            error(StatusCode::BAD_GATEWAY, "bad_gateway", "Bad Gateway")
                        } else {
                            Json(json!({"object": "database", "id": DB, "title": [],
                                        "data_sources": [{"id": DS, "name": "Tasks"}]}))
                            .into_response()
                        }
                    }
                }),
            )
            .route(
                "/v1/users/me",
                get(move || {
                    failing_counter.fetch_add(1, Ordering::SeqCst);
                    async {
                        error(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "internal_server_error",
                            "boom",
                        )
                    }
                }),
            ),
    )
    .await;
    let client = client(base);
    let database = client.retrieve_database(DB).await.unwrap();
    assert_eq!(database.data_sources[0].id, DS);
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    let error = client.me().await.unwrap_err();
    assert!(
        matches!(error, NotionError::Api { status: 500, .. }),
        "{error:?}"
    );
    assert_eq!(failing.load(Ordering::SeqCst), 5, "default max_attempts");
}

#[tokio::test]
async fn network_errors_are_retried_then_reported() {
    // Nothing listens on this port.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let error = client(base).me().await.unwrap_err();
    assert!(matches!(error, NotionError::Http(_)), "{error:?}");
}

#[tokio::test]
async fn query_data_source_paginates_and_keeps_pages() {
    let bodies = Arc::new(Mutex::new(Vec::<Value>::new()));
    let recorder = bodies.clone();
    let base = serve(Router::new().route(
        "/v1/data_sources/{id}/query",
        post(move |Path(id): Path<String>, body: Bytes| {
            let recorder = recorder.clone();
            async move {
                assert_eq!(id, DS);
                let body: Value = serde_json::from_slice(&body).unwrap();
                recorder.lock().unwrap().push(body.clone());
                let row = |n: u32| {
                    json!({"object": "page", "id": uuid(n),
                           "parent": {"type": "data_source_id", "data_source_id": DS, "database_id": DB},
                           "properties": {"Name": {"id": "title", "type": "title", "title": []}}})
                };
                Json(if body.get("start_cursor").is_none() {
                    json!({"object": "list", "results": [row(1), row(2)], "next_cursor": "c2", "has_more": true})
                } else {
                    json!({"object": "list", "results": [row(3), {"object": "data_source", "id": uuid(9)}],
                           "next_cursor": null, "has_more": false})
                })
            }
        }),
    ))
    .await;
    let rows = client(base).query_data_source_all(DS).await.unwrap();
    assert_eq!(rows.pages.len(), 3);
    assert!(!rows.incomplete);
    let bodies = bodies.lock().unwrap();
    assert_eq!(bodies[0]["sorts"][0]["timestamp"], "created_time");
    assert_eq!(bodies[1]["start_cursor"], "c2");
}

#[tokio::test]
async fn requests_are_spaced_by_the_rate_limiter() {
    let base = serve(Router::new().route(
        "/v1/users/me",
        get(|| async {
            Json(json!({"object": "user", "id": "u", "type": "person", "person": {}}))
        }),
    ))
    .await;
    let client = NotionClient::new(
        "t",
        NotionClientConfig {
            requests_per_second: 20.0,
            ..config(base)
        },
    )
    .unwrap();
    let shared = client.clone();
    let started = Instant::now();
    let (first, second) = tokio::join!(
        async {
            for _ in 0..3 {
                client.me().await.unwrap();
            }
        },
        async {
            for _ in 0..3 {
                shared.me().await.unwrap();
            }
        }
    );
    let _ = (first, second);
    // Six requests 50 ms apart: the last starts at least 250 ms after the first.
    assert!(
        started.elapsed() >= Duration::from_millis(250),
        "{:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn download_rejects_non_notion_urls() {
    let client = NotionClient::new("t", NotionClientConfig::default()).unwrap();
    for url in [
        "http://prod-files-secure.s3.us-west-2.amazonaws.com/a.png",
        "https://example.com/a.png",
        "https://s3.us-west-2.amazonaws.com/someone-elses-bucket/a.png",
        "ftp://file.notion.so/a",
    ] {
        assert!(
            matches!(
                client.download_file(url, 10).await,
                Err(NotionError::FileNotAllowed)
            ),
            "{url}"
        );
    }
}
