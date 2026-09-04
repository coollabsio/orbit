use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::body::{Body, Bytes, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{
    AttachmentMutationCoordinator, HttpLimits, HttpPlatformLayer, LocalBlobStore, OriginPolicy,
    PasswordService, TestDatabase, TimestampMillis, UploadLimits, UploadService,
};
use orbit_server::attachment_routes::{AttachmentState, attachment_router};
use orbit_server::auth_routes::CookieMode;
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::task_routes::{TaskState, task_router};
use serde_json::{Value, json};
use tower::ServiceExt;

const PNG: &[u8] = b"\x89\x50\x4e\x47\x0d\x0a\x1a\x0a\x00\x00\x00\x0d\x49\x48\x44\x52\x00\x00\x00\x01\x00\x00\x00\x01\x08\x04\x00\x00\x00\xb5\x1c\x0c\x02\x00\x00\x00\x0b\x49\x44\x41\x54\x78\xda\x63\x64\xf8\x0f\x00\x01\x05\x01\x01\x27\x18\xe3\x66\x00\x00\x00\x00\x49\x45\x4e\x44\xae\x42\x60\x82";
const SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script></svg>"#;

struct Fixture {
    _root: tempfile::TempDir,
    database: TestDatabase,
    identity: Arc<IdentityRepository>,
    store: LocalBlobStore,
    app: axum::Router,
    workspace_id: String,
    owner_cookie: String,
    owner_id: String,
    task_id: String,
}

impl Fixture {
    async fn new(limits: UploadLimits) -> Self {
        let root = tempfile::tempdir().unwrap();
        let database = TestDatabase::new().await.unwrap();
        let identity = Arc::new(IdentityRepository::new((*database).clone()));
        let now = TimestampMillis::now();
        identity
            .store_setup_token(
                "operator-secret",
                TimestampMillis::from_millis(now.as_millis() + 60_000),
            )
            .await
            .unwrap();
        let setup = identity
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
        let status_id: String = sqlx::query_scalar(
            "SELECT id FROM task_statuses WHERE project_id = ? ORDER BY position LIMIT 1",
        )
        .bind(setup.project_id.to_string())
        .fetch_one(database.pool())
        .await
        .unwrap();
        let owner_cookie = format!("__Host-orbit_session={}", setup.session.token);
        let tasks = task_router(TaskState::new(Arc::clone(&identity), CookieMode::secure()));
        let task_response = tasks
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/api/v1/workspaces/{}/tasks", setup.workspace_id),
                &owner_cookie,
                json!({
                    "project_id": setup.project_id,
                    "status_id": status_id,
                    "title": "Attachment task"
                }),
            ))
            .await
            .unwrap();
        assert_eq!(task_response.status(), StatusCode::CREATED);
        let task_id = response_json(task_response).await["id"]
            .as_str()
            .unwrap()
            .to_owned();

        let store = LocalBlobStore::new(root.path().join("attachments"));
        let uploads = UploadService::new(
            (*database).clone(),
            Arc::new(store.clone()),
            AttachmentMutationCoordinator::default(),
            limits,
        );
        let attachments = attachment_router(AttachmentState::new(
            Arc::clone(&identity),
            uploads,
            CookieMode::secure(),
        ));
        Self {
            _root: root,
            database,
            identity,
            store,
            app: tasks.merge(attachments),
            workspace_id: setup.workspace_id.to_string(),
            owner_cookie,
            owner_id: setup.user_id.to_string(),
            task_id,
        }
    }

    fn task_attachments(&self) -> String {
        format!(
            "/api/v1/workspaces/{}/tasks/{}/attachments",
            self.workspace_id, self.task_id
        )
    }

    async fn upload(&self, name: &str, bytes: &[u8]) -> axum::response::Response {
        self.app
            .clone()
            .oneshot(multipart_request(
                &self.task_attachments(),
                &self.owner_cookie,
                &[(name, bytes)],
            ))
            .await
            .unwrap()
    }
}

#[tokio::test]
async fn duplicate_png_uploads_share_one_workspace_blob_but_keep_metadata() {
    let fixture = Fixture::new(UploadLimits::default()).await;
    let first = fixture.upload("first.png", PNG).await;
    assert_eq!(first.status(), StatusCode::CREATED);
    let first = response_json(first).await;
    let second = fixture.upload("second.png", PNG).await;
    assert_eq!(second.status(), StatusCode::CREATED);
    let second = response_json(second).await;
    assert_ne!(first["id"], second["id"]);
    assert_eq!(first["media_type"], "image/png");
    assert!(first.get("blob_id").is_none());

    let blobs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM attachment_blobs")
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    let references: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM attachment_references")
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!((blobs, references), (1, 2));
    assert_eq!(fixture.store.blobs().await.unwrap().len(), 1);

    let listed = fixture
        .app
        .clone()
        .oneshot(cookie_request(
            "GET",
            &fixture.task_attachments(),
            &fixture.owner_cookie,
        ))
        .await
        .unwrap();
    assert_eq!(listed.status(), StatusCode::OK);
    assert_eq!(
        response_json(listed).await["items"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn authorization_precedes_multipart_consumption_and_cross_workspace_access_is_hidden() {
    let fixture = Fixture::new(UploadLimits::default()).await;
    let unauthorized = fixture
        .app
        .clone()
        .oneshot(multipart_request(
            &fixture.task_attachments(),
            "",
            &[("secret.png", PNG)],
        ))
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
    assert!(fixture.store.temporary_files().await.unwrap().is_empty());

    let wrong_workspace = orbit_platform::Id::new_v7();
    let denied = fixture
        .app
        .clone()
        .oneshot(cookie_request(
            "GET",
            &format!(
                "/api/v1/workspaces/{wrong_workspace}/tasks/{}/attachments",
                fixture.task_id
            ),
            &fixture.owner_cookie,
        ))
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn target_is_revalidated_atomically_after_the_stream_finishes() {
    let fixture = Fixture::new(UploadLimits::default()).await;
    let boundary = "orbit-stream-boundary";
    let (sender, receiver) = tokio::sync::mpsc::channel(2);
    sender
        .send(Ok::<_, std::io::Error>(Bytes::from(format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"late.png\"\r\n\r\n"
        ))))
        .await
        .unwrap();
    let request = Request::builder()
        .method("POST")
        .uri(fixture.task_attachments())
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .header(header::COOKIE, &fixture.owner_cookie)
        .body(Body::from_stream(
            tokio_stream::wrappers::ReceiverStream::new(receiver),
        ))
        .unwrap();
    let app = fixture.app.clone();
    let response = tokio::spawn(async move { app.oneshot(request).await.unwrap() });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    sqlx::query("UPDATE tasks SET deleted_at = ? WHERE id = ?")
        .bind(TimestampMillis::now().as_millis())
        .bind(&fixture.task_id)
        .execute(fixture.database.pool())
        .await
        .unwrap();
    sender.send(Ok(Bytes::from_static(PNG))).await.unwrap();
    sender
        .send(Ok(Bytes::from(format!("\r\n--{boundary}--\r\n"))))
        .await
        .unwrap();
    drop(sender);

    let response = response.await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let references: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM attachment_references")
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(references, 0);
    assert!(fixture.store.temporary_files().await.unwrap().is_empty());
}

#[tokio::test]
async fn oversized_stream_is_rejected_and_partial_temporary_file_is_removed() {
    let fixture = Fixture::new(UploadLimits::new(16, 32).unwrap()).await;
    let response = fixture.upload("too-large.bin", &[7; 17]).await;
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert!(fixture.store.temporary_files().await.unwrap().is_empty());
    let pending: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pending_uploads")
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(pending, 0);
}

#[tokio::test]
async fn streaming_upload_is_not_capped_by_axums_two_mib_default_body_limit() {
    let fixture = Fixture::new(UploadLimits::new(3 * 1024 * 1024, 3 * 1024 * 1024).unwrap()).await;
    let bytes = vec![5; 2 * 1024 * 1024 + 1];
    let response = fixture.upload("large.bin", &bytes).await;
    assert_eq!(response.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn cumulative_request_limit_rejects_before_any_attachment_metadata_is_finalized() {
    let fixture = Fixture::new(UploadLimits::new(16, 16).unwrap()).await;
    let response = fixture
        .app
        .clone()
        .oneshot(multipart_request(
            &fixture.task_attachments(),
            &fixture.owner_cookie,
            &[("one.bin", &[1; 9]), ("two.bin", &[2; 9])],
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let references: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM attachment_references")
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(references, 0);
    assert!(fixture.store.temporary_files().await.unwrap().is_empty());
    let staged: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pending_uploads WHERE state IN ('receiving', 'staged')",
    )
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(staged, 0);
}

#[tokio::test]
async fn request_limit_counts_multipart_overhead_and_non_file_fields() {
    let fixture = Fixture::new(UploadLimits::new(16, 16).unwrap()).await;
    let boundary = "request-budget";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"ignored\"\r\n\r\n{}\r\n--{boundary}--\r\n",
        "x".repeat(17)
    );
    let response = fixture
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(fixture.task_attachments())
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .header(header::COOKIE, &fixture.owner_cookie)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn chunked_request_crossing_the_wire_limit_mid_file_returns_413_and_cleans_up() {
    let fixture = Fixture::new(UploadLimits::new(256, 256).unwrap()).await;
    let boundary = "chunked-budget";
    let frames = vec![
        Ok::<_, std::io::Error>(Bytes::from(format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"chunk.bin\"\r\n\r\n{}",
            "a".repeat(100)
        ))),
        Ok(Bytes::from("b".repeat(100))),
        Ok(Bytes::from(format!("\r\n--{boundary}--\r\n"))),
    ];
    let response = fixture
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(fixture.task_attachments())
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .header(header::COOKIE, &fixture.owner_cookie)
                .body(Body::from_stream(tokio_stream::iter(frames)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert!(fixture.store.temporary_files().await.unwrap().is_empty());
}

#[tokio::test]
async fn upload_endpoints_reject_multiple_files_without_creating_hidden_attachments() {
    let fixture = Fixture::new(UploadLimits::default()).await;
    let response = fixture
        .app
        .clone()
        .oneshot(multipart_request(
            &fixture.task_attachments(),
            &fixture.owner_cookie,
            &[("one.png", PNG), ("two.png", PNG)],
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let references: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM attachment_references")
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(references, 0);
}

#[tokio::test]
async fn upload_rejects_a_file_bearing_part_under_another_field_name() {
    let fixture = Fixture::new(UploadLimits::default()).await;
    let boundary = "alternate-file-field";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"one.png\"\r\n\r\n{}\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"other\"; filename=\"two.png\"\r\n\r\n{}\r\n--{boundary}--\r\n",
        String::from_utf8_lossy(PNG),
        String::from_utf8_lossy(PNG)
    );
    let response = fixture
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(fixture.task_attachments())
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .header(header::COOKIE, &fixture.owner_cookie)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let references: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM attachment_references")
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(references, 0);
}

#[tokio::test]
async fn revoked_actor_cannot_finalize_after_streaming_bytes() {
    let fixture = Fixture::new(UploadLimits::default()).await;
    let boundary = "revocation-boundary";
    let (sender, receiver) = tokio::sync::mpsc::channel(2);
    sender
        .send(Ok::<_, std::io::Error>(Bytes::from(format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"late.png\"\r\n\r\n"
        ))))
        .await
        .unwrap();
    let request = Request::builder()
        .method("POST")
        .uri(fixture.task_attachments())
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .header(header::COOKIE, &fixture.owner_cookie)
        .body(Body::from_stream(
            tokio_stream::wrappers::ReceiverStream::new(receiver),
        ))
        .unwrap();
    let app = fixture.app.clone();
    let response = tokio::spawn(async move { app.oneshot(request).await.unwrap() });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    sqlx::query("UPDATE users SET suspended_at = ? WHERE id = ?")
        .bind(TimestampMillis::now().as_millis())
        .bind(&fixture.owner_id)
        .execute(fixture.database.pool())
        .await
        .unwrap();
    sender.send(Ok(Bytes::from_static(PNG))).await.unwrap();
    sender
        .send(Ok(Bytes::from(format!("\r\n--{boundary}--\r\n"))))
        .await
        .unwrap();
    drop(sender);
    assert_eq!(response.await.unwrap().status(), StatusCode::NOT_FOUND);
    let references: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM attachment_references")
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(references, 0);
}

#[tokio::test]
async fn malformed_multipart_after_a_file_discards_every_staged_temporary() {
    let fixture = Fixture::new(UploadLimits::default()).await;
    let boundary = "broken-boundary";
    let mut body = Vec::new();
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"one.png\"\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(PNG);
    body.extend_from_slice(format!("\r\n--{boundary}\r\nnot-a-header\r\n").as_bytes());
    let response = fixture
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(fixture.task_attachments())
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .header(header::COOKIE, &fixture.owner_cookie)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(fixture.store.temporary_files().await.unwrap().is_empty());
    let staged: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pending_uploads WHERE state IN ('receiving', 'staged')",
    )
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(staged, 0);
}

#[tokio::test]
async fn truncated_multipart_inside_file_is_bad_request_and_cleans_up() {
    let fixture = Fixture::new(UploadLimits::default()).await;
    let boundary = "truncated-file";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"one.png\"\r\n\r\n{}",
        String::from_utf8_lossy(PNG)
    );
    let response = fixture
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(fixture.task_attachments())
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .header(header::COOKIE, &fixture.owner_cookie)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(fixture.store.temporary_files().await.unwrap().is_empty());
    let pending: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pending_uploads")
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(pending, 0);
}

#[tokio::test]
async fn revoked_session_cannot_finalize_after_streaming_bytes() {
    let fixture = Fixture::new(UploadLimits::default()).await;
    let boundary = "session-revocation-boundary";
    let (sender, receiver) = tokio::sync::mpsc::channel(2);
    sender.send(Ok::<_, std::io::Error>(Bytes::from(format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"late.png\"\r\n\r\n"
    )))).await.unwrap();
    let request = Request::builder()
        .method("POST")
        .uri(fixture.task_attachments())
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .header(header::COOKIE, &fixture.owner_cookie)
        .body(Body::from_stream(
            tokio_stream::wrappers::ReceiverStream::new(receiver),
        ))
        .unwrap();
    let app = fixture.app.clone();
    let response = tokio::spawn(async move { app.oneshot(request).await.unwrap() });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    sqlx::query("UPDATE sessions SET revoked_at = ? WHERE user_id = ?")
        .bind(TimestampMillis::now().as_millis())
        .bind(&fixture.owner_id)
        .execute(fixture.database.pool())
        .await
        .unwrap();
    sender.send(Ok(Bytes::from_static(PNG))).await.unwrap();
    sender
        .send(Ok(Bytes::from(format!("\r\n--{boundary}--\r\n"))))
        .await
        .unwrap();
    drop(sender);
    assert_eq!(response.await.unwrap().status(), StatusCode::NOT_FOUND);
    let references: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM attachment_references")
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(references, 0);
}

#[tokio::test]
async fn attachment_lists_use_keyset_cursors() {
    let fixture = Fixture::new(UploadLimits::default()).await;
    assert_eq!(
        fixture.upload("one.png", PNG).await.status(),
        StatusCode::CREATED
    );
    assert_eq!(
        fixture.upload("two.png", PNG).await.status(),
        StatusCode::CREATED
    );
    let first = fixture
        .app
        .clone()
        .oneshot(cookie_request(
            "GET",
            &format!("{}?limit=1", fixture.task_attachments()),
            &fixture.owner_cookie,
        ))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    let first = response_json(first).await;
    assert_eq!(first["items"].as_array().unwrap().len(), 1);
    let cursor = first["next_cursor"].as_str().unwrap();
    let second = fixture
        .app
        .clone()
        .oneshot(cookie_request(
            "GET",
            &format!("{}?limit=1&cursor={cursor}", fixture.task_attachments()),
            &fixture.owner_cookie,
        ))
        .await
        .unwrap();
    let second = response_json(second).await;
    assert_eq!(second["items"].as_array().unwrap().len(), 1);
    assert_ne!(first["items"][0]["id"], second["items"][0]["id"]);
}

#[tokio::test]
async fn configured_production_layer_allows_streaming_uploads_above_one_mibibyte() {
    let limits = UploadLimits::new(3 * 1024 * 1024, 3 * 1024 * 1024).unwrap();
    let fixture = Fixture::new(limits).await;
    let app = fixture.app.clone().layer(
        HttpPlatformLayer::new(OriginPolicy::new("https://orbit.test")).with_limits(HttpLimits {
            max_body_bytes: limits.max_request_bytes() as usize,
            ..HttpLimits::default()
        }),
    );
    let bytes = vec![5; 1024 * 1024 + 1];
    let mut request = multipart_request(
        &fixture.task_attachments(),
        &fixture.owner_cookie,
        &[("large.bin", &bytes)],
    );
    request
        .headers_mut()
        .insert("origin", "https://orbit.test".parse().unwrap());
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn composed_production_router_authenticates_before_polling_upload_body() {
    let limits = UploadLimits::default();
    let fixture = Fixture::new(limits).await;
    let app = fixture.app.clone().layer(
        HttpPlatformLayer::new(OriginPolicy::new("https://orbit.test")).with_limits(HttpLimits {
            max_body_bytes: limits.max_request_bytes() as usize,
            ..HttpLimits::default()
        }),
    );
    let polls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&polls);
    let body = futures_util::stream::poll_fn(move |_| {
        observed.fetch_add(1, Ordering::SeqCst);
        std::task::Poll::<Option<Result<Bytes, std::io::Error>>>::Ready(None)
    });
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(fixture.task_attachments())
                .header("origin", "https://orbit.test")
                .header(header::CONTENT_TYPE, "multipart/form-data; boundary=unused")
                .body(Body::from_stream(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(polls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn composed_production_router_enforces_streaming_request_cap() {
    let limits = UploadLimits::new(512, 512).unwrap();
    let fixture = Fixture::new(limits).await;
    let app = fixture.app.clone().layer(
        HttpPlatformLayer::new(OriginPolicy::new("https://orbit.test")).with_limits(HttpLimits {
            max_body_bytes: limits.max_request_bytes() as usize,
            ..HttpLimits::default()
        }),
    );
    let boundary = "production-cap";
    let frames = vec![Ok::<_, std::io::Error>(Bytes::from(format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"large.bin\"\r\n\r\n{}\r\n--{boundary}--\r\n",
        "x".repeat(500)
    )))];
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(fixture.task_attachments())
                .header("origin", "https://orbit.test")
                .header(header::COOKIE, &fixture.owner_cookie)
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from_stream(tokio_stream::iter(frames)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert!(fixture.store.temporary_files().await.unwrap().is_empty());
}

#[tokio::test]
async fn cancelling_http_after_a_complete_field_discards_staged_bytes() {
    let fixture = Fixture::new(UploadLimits::default()).await;
    let boundary = "cancel-boundary";
    let (sender, receiver) = tokio::sync::mpsc::channel(2);
    sender
        .send(Ok::<_, std::io::Error>(Bytes::from(format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"cancel.png\"\r\n\r\n{}\r\n--{boundary}\r\n",
            String::from_utf8_lossy(PNG)
        ))))
        .await
        .unwrap();
    let request = Request::builder()
        .method("POST")
        .uri(fixture.task_attachments())
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .header(header::COOKIE, &fixture.owner_cookie)
        .body(Body::from_stream(
            tokio_stream::wrappers::ReceiverStream::new(receiver),
        ))
        .unwrap();
    let app = fixture.app.clone();
    let task = tokio::spawn(async move { app.oneshot(request).await });
    for _ in 0..100 {
        let staged: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM pending_uploads WHERE state = 'staged'")
                .fetch_one(fixture.database.pool())
                .await
                .unwrap();
        if staged == 1 {
            break;
        }
        tokio::task::yield_now().await;
    }
    task.abort();
    drop(sender);
    let _ = task.await;
    for _ in 0..100 {
        if fixture.store.temporary_files().await.unwrap().is_empty() {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("cancelled HTTP upload left staged temporary bytes");
}

#[tokio::test]
async fn database_finalization_failure_leaves_new_bytes_quarantined_for_reconciliation() {
    let fixture = Fixture::new(UploadLimits::default()).await;
    sqlx::query(
        "CREATE TRIGGER fail_attachment_finalize BEFORE INSERT ON attachment_references \
         BEGIN SELECT RAISE(ABORT, 'injected metadata failure'); END",
    )
    .execute(fixture.database.pool())
    .await
    .unwrap();
    let response = fixture.upload("orphan.png", PNG).await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(fixture.store.blobs().await.unwrap().len(), 1);
    let tracked: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM attachment_blobs")
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(tracked, 0, "the failed database transaction is rolled back");

    let state = AttachmentState::new(
        Arc::clone(&fixture.identity),
        UploadService::new(
            (*fixture.database).clone(),
            Arc::new(fixture.store.clone()),
            AttachmentMutationCoordinator::default(),
            UploadLimits::default(),
        ),
        CookieMode::secure(),
    );
    state
        .reconcile_at(TimestampMillis::now().as_millis())
        .await
        .unwrap();
    assert_eq!(fixture.store.blobs().await.unwrap().len(), 1);
}

#[tokio::test]
async fn attachment_only_comment_and_reference_finalize_in_one_database_transaction() {
    let fixture = Fixture::new(UploadLimits::default()).await;
    sqlx::query(
        "CREATE TRIGGER fail_comment_attachment BEFORE INSERT ON attachment_references \
         BEGIN SELECT RAISE(ABORT, 'injected comment attachment failure'); END",
    )
    .execute(fixture.database.pool())
    .await
    .unwrap();
    let response = fixture
        .app
        .clone()
        .oneshot(multipart_request(
            &format!(
                "/api/v1/workspaces/{}/tasks/{}/comments/attachments",
                fixture.workspace_id, fixture.task_id
            ),
            &fixture.owner_cookie,
            &[("comment.png", PNG)],
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let comments: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM task_comments")
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    let comment_audits: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM audit_events WHERE action LIKE 'comment.%'")
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    assert_eq!((comments, comment_audits), (0, 0));
}

#[tokio::test]
async fn svg_download_is_authenticated_nosniff_and_forced_to_attachment() {
    let fixture = Fixture::new(UploadLimits::default()).await;
    let uploaded = response_json(fixture.upload("danger.svg", SVG).await).await;
    let id = uploaded["id"].as_str().unwrap();
    let uri = format!("{}/{id}/download", fixture.task_attachments());

    let anonymous = fixture
        .app
        .clone()
        .oneshot(cookie_request("GET", &uri, ""))
        .await
        .unwrap();
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);

    let download = fixture
        .app
        .clone()
        .oneshot(cookie_request("GET", &uri, &fixture.owner_cookie))
        .await
        .unwrap();
    assert_eq!(download.status(), StatusCode::OK);
    assert_eq!(download.headers()[header::CONTENT_TYPE], "image/svg+xml");
    assert_eq!(download.headers()["x-content-type-options"], "nosniff");
    assert_eq!(
        download.headers()[header::CONTENT_DISPOSITION],
        "attachment; filename=\"danger.svg\""
    );
    assert_eq!(to_bytes(download.into_body(), 1024).await.unwrap(), SVG);
}

#[tokio::test]
async fn deleting_last_reference_quarantines_blob_and_attachment_only_comment_is_supported() {
    let fixture = Fixture::new(UploadLimits::default()).await;
    let uploaded = response_json(fixture.upload("kept.png", PNG).await).await;
    let id = uploaded["id"].as_str().unwrap();
    let deleted = fixture
        .app
        .clone()
        .oneshot(cookie_request(
            "DELETE",
            &format!("{}/{id}", fixture.task_attachments()),
            &fixture.owner_cookie,
        ))
        .await
        .unwrap();
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);
    let refs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM attachment_references")
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    let quarantine: i64 = sqlx::query_scalar("SELECT quarantine_until FROM attachment_blobs")
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(refs, 0);
    assert!(quarantine > TimestampMillis::now().as_millis());
    assert_eq!(fixture.store.blobs().await.unwrap().len(), 1);

    let comment = fixture
        .app
        .clone()
        .oneshot(multipart_request(
            &format!(
                "/api/v1/workspaces/{}/tasks/{}/comments/attachments",
                fixture.workspace_id, fixture.task_id
            ),
            &fixture.owner_cookie,
            &[("comment.png", PNG)],
        ))
        .await
        .unwrap();
    assert_eq!(comment.status(), StatusCode::CREATED);
    let comment = response_json(comment).await;
    assert_eq!(comment["comment"]["body"], "");
    assert_eq!(comment["attachments"].as_array().unwrap().len(), 1);
}

fn multipart_request(uri: &str, cookie: &str, files: &[(&str, &[u8])]) -> Request<Body> {
    let boundary = "orbit-test-boundary";
    let mut body = Vec::new();
    for (name, bytes) in files {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\n\r\n")
                .as_bytes(),
        );
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    let mut builder = Request::builder().method("POST").uri(uri).header(
        header::CONTENT_TYPE,
        format!("multipart/form-data; boundary={boundary}"),
    );
    if !cookie.is_empty() {
        builder = builder.header(header::COOKIE, cookie);
    }
    builder.body(Body::from(body)).unwrap()
}

fn json_request(method: &str, uri: &str, cookie: &str, value: Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::COOKIE, cookie)
        .body(Body::from(value.to_string()))
        .unwrap()
}

fn cookie_request(method: &str, uri: &str, cookie: &str) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    if !cookie.is_empty() {
        builder = builder.header(header::COOKIE, cookie);
    }
    builder.body(Body::empty()).unwrap()
}

async fn response_json(response: axum::response::Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap()
}
