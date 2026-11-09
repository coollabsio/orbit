//! The Trello client against a local fake REST API: the OAuth header, error mapping, retries,
//! comment paging and authenticated downloads.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Json;
use axum::Router;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use orbit_server::trello::client::{TrelloClient, TrelloClientConfig, TrelloError};
use serde_json::{Value, json};

const KEY: &str = "trellokey1";
const TOKEN: &str = "trellotoken1";
const AUTHORIZATION: &str = "OAuth oauth_consumer_key=\"trellokey1\", oauth_token=\"trellotoken1\"";
const BOARD: &str = "b1b1b1b1b1b1b1b1b1b1b1b1";

#[derive(Default)]
struct Fake {
    /// Path and query of every API request.
    uris: Mutex<Vec<String>>,
    /// `Authorization` headers of the API requests.
    api_auth: Mutex<Vec<Option<String>>>,
    hits: Mutex<HashMap<String, usize>>,
    /// The `before` parameter of each comment page request.
    comment_pages: Mutex<Vec<Option<String>>>,
    /// `Authorization` headers the file routes saw, by route.
    file_auth: Mutex<Vec<(String, Option<String>)>>,
}

impl Fake {
    fn hit(&self, name: &str) -> usize {
        let mut hits = self.hits.lock().unwrap();
        let count = hits.entry(name.to_owned()).or_default();
        *count += 1;
        *count
    }
}

fn authorization(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

fn comment(id: String) -> Value {
    json!({"id": id, "date": "2024-05-01T09:30:00.000Z",
           "data": {"text": "hi", "card": {"id": "c1"}},
           "memberCreator": {"id": "m1", "fullName": "Ada Lovelace", "username": "ada"}})
}

async fn api(
    State(fake): State<Arc<Fake>>,
    uri: Uri,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    fake.uris.lock().unwrap().push(uri.to_string());
    let auth = authorization(&headers);
    fake.api_auth.lock().unwrap().push(auth.clone());
    if auth.as_deref() != Some(AUTHORIZATION) {
        return (StatusCode::UNAUTHORIZED, "invalid key").into_response();
    }
    let boards = format!("/1/boards/{BOARD}");
    match uri.path() {
        "/1/members/me" => {
            assert_eq!(
                query.get("fields").map(String::as_str),
                Some("fullName,username")
            );
            Json(json!({"id": "m1", "fullName": "Ada Lovelace", "username": "ada"})).into_response()
        }
        "/1/members/me/organizations" => {
            if fake.hit("organizations") == 1 {
                return (
                    StatusCode::TOO_MANY_REQUESTS,
                    [(header::RETRY_AFTER, "1")],
                    "API_TOKEN_LIMIT_EXCEEDED",
                )
                    .into_response();
            }
            Json(json!([{"id": "o1", "displayName": "Acme", "name": "acme"}])).into_response()
        }
        "/1/members/me/boards" => {
            if fake.hit("boards") <= 2 {
                return (StatusCode::BAD_GATEWAY, "upstream").into_response();
            }
            assert_eq!(query.get("filter").map(String::as_str), Some("all"));
            Json(json!([{"id": BOARD, "name": "Roadmap", "closed": false, "idOrganization": "o1"}]))
                .into_response()
        }
        path if path == format!("{boards}/labels") => {
            fake.hit("labels");
            (StatusCode::SERVICE_UNAVAILABLE, "down").into_response()
        }
        path if path == format!("{boards}/members") => (
            StatusCode::TOO_MANY_REQUESTS,
            [(header::RETRY_AFTER, "30")],
            "slow down",
        )
            .into_response(),
        path if path == format!("{boards}/actions") => {
            assert_eq!(
                (
                    query.get("filter").map(String::as_str),
                    query.get("limit").map(String::as_str),
                    query.get("memberCreator").map(String::as_str),
                ),
                (Some("commentCard"), Some("1000"), Some("true"))
            );
            let before = query.get("before").cloned();
            fake.comment_pages.lock().unwrap().push(before.clone());
            let page: Vec<Value> = match before.as_deref() {
                None => (0..1000).map(|n| comment(format!("p1-{n:04}"))).collect(),
                Some("p1-0999") => (0..3).map(|n| comment(format!("p2-{n:04}"))).collect(),
                Some(other) => panic!("unexpected before={other}"),
            };
            Json(Value::Array(page)).into_response()
        }
        _ => (
            StatusCode::NOT_FOUND,
            "The requested resource was not found.",
        )
            .into_response(),
    }
}

/// The fake API and its file routes on one port, and a second "file store" port.
async fn serve() -> (u16, u16, Arc<Fake>) {
    let fake = Arc::new(Fake::default());
    let elsewhere = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let elsewhere_port = elsewhere.local_addr().unwrap().port();
    let store = Router::new()
        .route(
            "/signed/a.png",
            get(
                |State(fake): State<Arc<Fake>>, headers: HeaderMap| async move {
                    record(&fake, "store", &headers);
                    vec![9u8; 12]
                },
            ),
        )
        .with_state(Arc::clone(&fake));
    tokio::spawn(async move { axum::serve(elsewhere, store).await.unwrap() });

    let files = "/1/cards/c1/attachments";
    let router = Router::new()
        .route(
            &format!("{files}/a1/download/shot.png"),
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
            &format!("{files}/a2/download/moved.png"),
            get(
                move |State(fake): State<Arc<Fake>>, headers: HeaderMap| async move {
                    record(&fake, "moved", &headers);
                    Redirect::temporary(&format!("http://127.0.0.1:{elsewhere_port}/signed/a.png"))
                },
            ),
        )
        .route(
            &format!("{files}/a3/download/big.bin"),
            get(|| async { vec![0u8; 4096] }),
        )
        .route(
            &format!("{files}/a4/download/locked.png"),
            get(|| async { StatusCode::UNAUTHORIZED }),
        )
        .fallback(api)
        .with_state(Arc::clone(&fake));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (port, elsewhere_port, fake)
}

fn record(fake: &Fake, route: &str, headers: &HeaderMap) {
    fake.file_auth
        .lock()
        .unwrap()
        .push((route.to_owned(), authorization(headers)));
}

fn client(port: u16, key: &str) -> TrelloClient {
    TrelloClient::new(
        key,
        TOKEN,
        TrelloClientConfig {
            base_url: format!("http://127.0.0.1:{port}/1"),
            requests_per_second: 1000.0,
            max_attempts: 3,
            initial_backoff: Duration::from_millis(5),
            max_backoff: Duration::from_millis(20),
            max_retry_after: Duration::from_secs(2),
            loopback_file_port: Some(port),
            ..TrelloClientConfig::default()
        },
    )
    .unwrap()
}

#[tokio::test]
async fn sends_the_credentials_only_in_the_oauth_header() {
    let (port, _, fake) = serve().await;
    let member = client(port, KEY).me().await.unwrap();
    assert_eq!(
        (member.id.as_str(), member.full_name.as_deref()),
        ("m1", Some("Ada Lovelace"))
    );
    assert_eq!(
        *fake.api_auth.lock().unwrap(),
        [Some(AUTHORIZATION.to_owned())]
    );
    let uris = fake.uris.lock().unwrap().clone();
    assert_eq!(uris, ["/1/members/me?fields=fullName%2Cusername"]);
    assert!(
        uris.iter()
            .all(|uri| !uri.contains(KEY) && !uri.contains(TOKEN)),
        "{uris:?}"
    );
}

#[tokio::test]
async fn maps_a_rejected_key_to_unauthorized_with_trellos_text() {
    let (port, _, fake) = serve().await;
    match client(port, "wrongkey").me().await {
        Err(TrelloError::Unauthorized { message }) => assert_eq!(message, "invalid key"),
        other => panic!("expected Unauthorized, got {other:?}"),
    }
    assert_eq!(fake.uris.lock().unwrap().len(), 1, "a 401 is not retried");
}

#[tokio::test]
async fn waits_out_a_rate_limit_and_retries() {
    let (port, _, fake) = serve().await;
    let started = Instant::now();
    let organizations = client(port, KEY).organizations().await.unwrap();
    assert_eq!(
        (
            organizations.len(),
            organizations[0].display_name.as_deref()
        ),
        (1, Some("Acme"))
    );
    assert!(
        started.elapsed() >= Duration::from_secs(1),
        "waited for Retry-After"
    );
    assert_eq!(fake.hits.lock().unwrap()["organizations"], 2);

    let started = Instant::now();
    match client(port, KEY).members(BOARD).await {
        Err(TrelloError::RateLimited { retry_after }) => {
            assert_eq!(retry_after, Some(Duration::from_secs(30)));
        }
        other => panic!("expected RateLimited, got {other:?}"),
    }
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "a wait over max_retry_after fails at once"
    );
}

#[tokio::test]
async fn retries_server_errors_with_backoff() {
    let (port, _, fake) = serve().await;
    let boards = client(port, KEY).boards().await.unwrap();
    assert_eq!(
        (boards.len(), boards[0].name.as_deref()),
        (1, Some("Roadmap"))
    );
    assert_eq!(
        fake.hits.lock().unwrap()["boards"],
        3,
        "two 502s, then success"
    );
    match client(port, KEY).labels(BOARD).await {
        Err(TrelloError::Api { status, message }) => {
            assert_eq!((status, message.as_str()), (503, "down"));
        }
        other => panic!("expected Api, got {other:?}"),
    }
    assert_eq!(fake.hits.lock().unwrap()["labels"], 3, "max_attempts");
}

#[tokio::test]
async fn pages_comments_before_the_last_id_until_a_short_page() {
    let (port, _, fake) = serve().await;
    let comments = client(port, KEY).comments(BOARD).await.unwrap();
    assert_eq!(comments.len(), 1003);
    assert_eq!(
        (comments[0].id.as_str(), comments[1002].id.as_str()),
        ("p1-0000", "p2-0002")
    );
    assert_eq!(comments[0].data.text.as_deref(), Some("hi"));
    assert_eq!(
        comments[0]
            .member_creator
            .as_ref()
            .and_then(|member| member.display())
            .as_deref(),
        Some("Ada Lovelace")
    );
    assert_eq!(
        *fake.comment_pages.lock().unwrap(),
        [None, Some("p1-0999".to_owned())]
    );
}

#[tokio::test]
async fn refuses_ids_that_are_not_trello_ids() {
    let (port, _, fake) = serve().await;
    match client(port, KEY).lists("../members/me").await {
        Err(TrelloError::NotFound { message }) => assert_eq!(message, "invalid Trello id"),
        other => panic!("expected NotFound, got {other:?}"),
    }
    assert!(
        fake.uris.lock().unwrap().is_empty(),
        "refused before any request"
    );
}

#[tokio::test]
async fn downloads_with_the_credentials_and_follows_redirects_without_them() {
    let (port, elsewhere, fake) = serve().await;
    let trello = client(port, KEY);
    let files = format!("http://127.0.0.1:{port}/1/cards/c1/attachments");
    let file = trello
        .download_file(&format!("{files}/a1/download/shot.png"), 1024)
        .await
        .unwrap();
    assert_eq!(
        (file.bytes, file.file_name.as_str()),
        (vec![7u8; 10], "diagram.png")
    );
    let moved = trello
        .download_file(&format!("{files}/a2/download/moved.png"), 1024)
        .await
        .unwrap();
    assert_eq!(
        (moved.bytes, moved.file_name.as_str()),
        (vec![9u8; 12], "moved.png")
    );
    assert_eq!(
        *fake.file_auth.lock().unwrap(),
        [
            ("upload".to_owned(), Some(AUTHORIZATION.to_owned())),
            ("moved".to_owned(), Some(AUTHORIZATION.to_owned())),
            ("store".to_owned(), None),
        ],
        "the credentials go only to the Trello host"
    );

    assert!(matches!(
        trello
            .download_file(&format!("{files}/a3/download/big.bin"), 100)
            .await,
        Err(TrelloError::FileTooLarge { limit: 100 })
    ));
    assert!(matches!(
        trello
            .download_file(&format!("{files}/a4/download/locked.png"), 1024)
            .await,
        Err(TrelloError::FileRefused { status: 401 })
    ));

    let before = fake.file_auth.lock().unwrap().len();
    for url in [
        format!("{files}/a1/preview/shot.png"),
        format!("http://127.0.0.1:{elsewhere}/signed/download/a.png"),
        "https://trello-attachments.s3.amazonaws.com/a/download/b.png".to_owned(),
        "not a url".to_owned(),
    ] {
        assert!(
            matches!(
                trello.download_file(&url, 1024).await,
                Err(TrelloError::FileNotAllowed)
            ),
            "{url}"
        );
        assert!(!trello.is_download_url(&url), "{url}");
    }
    assert_eq!(
        fake.file_auth.lock().unwrap().len(),
        before,
        "refused before any request"
    );
    assert!(trello.is_download_url(&format!("{files}/a1/download/shot.png")));
}
