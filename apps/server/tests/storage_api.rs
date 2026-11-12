//! Object storage (S3) settings in Admin → Storage, against the in-memory S3 server.

use std::sync::Arc;
use std::time::Duration;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{
    AttachmentMutationCoordinator, LocalBlobStore, ObjectStorage, ObjectStorageState,
    PasswordService, S3Bucket, S3Config, TestDatabase, TieredBlobStore, TimestampMillis,
    UploadLimits, UploadService, start_fake_s3,
};
use orbit_server::attachment_routes::AttachmentState;
use orbit_server::auth_routes::CookieMode;
use orbit_server::mail::Mailer;
use orbit_server::object_storage::saved_storage;
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::workspace_routes::{WorkspaceState, workspace_router};
use serde_json::{Value, json};
use tower::ServiceExt;

struct Fixture {
    database: TestDatabase,
    _attachments: tempfile::TempDir,
    storage: ObjectStorage,
    app: axum::Router,
    cookie: String,
}

async fn fixture() -> Fixture {
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
                email: "root@example.com".to_owned(),
                display_name: "Root".to_owned(),
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
    let attachments = tempfile::tempdir().unwrap();
    let storage = ObjectStorage::default();
    let store = TieredBlobStore::new(
        LocalBlobStore::new(attachments.path()),
        storage.clone(),
        AttachmentMutationCoordinator::default(),
    );
    let (mailer, _) = Mailer::recording((*database).clone(), "https://orbit.test");
    let app = workspace_router(
        WorkspaceState::new(
            Arc::clone(&identity),
            "https://orbit.test".to_owned(),
            orbit_server::auth_routes::CookieMode::secure(),
        )
        .with_mailer(mailer)
        .with_object_storage(storage.clone(), store),
    );
    Fixture {
        database,
        _attachments: attachments,
        storage,
        app,
        cookie: format!("__Host-orbit_session={}", setup.session.token),
    }
}

impl Fixture {
    async fn call(&self, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::COOKIE, &self.cookie);
        let request = match body {
            Some(body) => request
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string())),
            None => request.body(Body::empty()),
        }
        .unwrap();
        let response = self.app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }
}

async fn s3_body() -> Value {
    let config = start_fake_s3().await;
    json!({
        "endpoint": config.endpoint, "region": "", "bucket": config.bucket, "prefix": "/orbit/",
        "access_key_id": "key", "secret_access_key": "plain-secret-value", "path_style": true
    })
}

#[tokio::test]
async fn the_root_user_saves_a_checked_bucket_with_an_encrypted_secret() {
    let fixture = fixture().await;
    let mut body = s3_body().await;

    body["access_key_id"] = json!("wrong");
    let (status, problem) = fixture
        .call("PUT", "/api/v1/admin/settings/s3", Some(body.clone()))
        .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(problem["code"], "storage_unreachable");

    body["access_key_id"] = json!("key");
    let (status, view) = fixture
        .call("PUT", "/api/v1/admin/settings/s3", Some(body.clone()))
        .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    let storage = &view["storage"];
    assert_eq!(storage["s3"]["prefix"], "orbit");
    assert_eq!(storage["s3"]["region"], "us-east-1");
    assert_eq!(storage["attachments_in_s3"], false);
    assert!(storage["s3"].get("secret_access_key").is_none());
    let sealed: Vec<u8> = fixture
        .database
        .scalar("SELECT s3_secret_access_key FROM instance_settings")
        .await
        .unwrap();
    assert!(
        !sealed
            .windows(b"plain-secret-value".len())
            .any(|window| window == b"plain-secret-value")
    );
    assert!(fixture.storage.bucket().is_some());

    // An absent secret keeps the saved one, which still works.
    body.as_object_mut().unwrap().remove("secret_access_key");
    body["region"] = json!("eu-central-1");
    let (status, view) = fixture
        .call("PUT", "/api/v1/admin/settings/s3", Some(body))
        .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["storage"]["s3"]["region"], "eu-central-1");
}

#[tokio::test]
async fn storage_options_need_a_bucket_and_the_bucket_stays_while_attachments_use_it() {
    let fixture = fixture().await;
    let options =
        json!({"attachments_in_s3": true, "backups_in_s3": true, "backup_schedule": "daily"});
    let (status, problem) = fixture
        .call(
            "PUT",
            "/api/v1/admin/settings/storage",
            Some(options.clone()),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(problem["code"], "storage_not_configured");

    let (status, _) = fixture
        .call("PUT", "/api/v1/admin/settings/s3", Some(s3_body().await))
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, view) = fixture
        .call("PUT", "/api/v1/admin/settings/storage", Some(options))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["storage"]["attachments_in_s3"], true);
    assert_eq!(view["storage"]["backup_schedule"], "daily");
    let running = fixture.storage.current();
    assert!(running.attachments && running.backups);

    let (status, problem) = fixture
        .call("DELETE", "/api/v1/admin/settings/s3", None)
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(problem["code"], "storage_in_use");

    let (status, _) = fixture
        .call(
            "PUT",
            "/api/v1/admin/settings/storage",
            Some(json!({"attachments_in_s3": false, "backups_in_s3": false, "backup_schedule": "off"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, view) = fixture
        .call("DELETE", "/api/v1/admin/settings/s3", None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(view["storage"]["s3"].is_null());
    assert!(fixture.storage.bucket().is_none());
}

#[tokio::test]
async fn settings_another_server_saved_apply_on_reload() {
    let fixture = fixture().await;
    let (status, _) = fixture
        .call("PUT", "/api/v1/admin/settings/s3", Some(s3_body().await))
        .await;
    assert_eq!(status, StatusCode::OK);
    let options =
        json!({"attachments_in_s3": true, "backups_in_s3": false, "backup_schedule": "off"});
    let (status, _) = fixture
        .call("PUT", "/api/v1/admin/settings/storage", Some(options))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(fixture.storage.current().attachments);

    // Another server of the database turns on backups in S3.
    let root: String = fixture
        .database
        .scalar("SELECT id FROM users")
        .await
        .unwrap();
    orbit_server::repositories::instance_settings::InstanceSettingsRepository::new(
        (*fixture.database).clone(),
    )
    .save_storage_options(
        root.parse().unwrap(),
        true,
        true,
        orbit_server::repositories::instance_settings::BackupSchedule::Off,
        None,
        "other-server",
        TimestampMillis::now(),
    )
    .await
    .unwrap();
    assert!(
        !fixture.storage.current().backups,
        "not applied before a reload"
    );

    orbit_server::object_storage::reload_saved(&fixture.database)
        .await
        .unwrap();
    assert!(fixture.storage.current().backups);
}

#[tokio::test]
async fn a_command_without_the_server_finds_the_backups_in_the_saved_bucket() {
    let fixture = fixture().await;
    let (status, _) = fixture
        .call("PUT", "/api/v1/admin/settings/s3", Some(s3_body().await))
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = fixture
        .call(
            "PUT",
            "/api/v1/admin/settings/storage",
            Some(json!({"attachments_in_s3": false, "backups_in_s3": true, "backup_schedule": "off"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (mailer, _) = Mailer::recording((*fixture.database).clone(), "https://orbit.test");
    let app_key = *mailer.app_key().unwrap();
    let backups = tempfile::tempdir().unwrap();
    let attachments = tempfile::tempdir().unwrap();
    let service = |storage| {
        orbit_platform::BackupService::new(backups.path(), attachments.path())
            .with_object_storage(storage)
    };

    let storage = saved_storage(&fixture.database, Some(&app_key))
        .await
        .unwrap();
    let created = service(storage).create(&fixture.database).await.unwrap();
    assert!(created.in_object_storage);

    let listed = service(
        saved_storage(&fixture.database, Some(&app_key))
            .await
            .unwrap(),
    )
    .list()
    .await
    .unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, created.id);
    assert!(listed[0].in_object_storage);

    let error = saved_storage(&fixture.database, Some(&[9; 32]))
        .await
        .unwrap_err();
    assert!(error.contains("cannot be decrypted"), "{error}");
}

#[tokio::test]
async fn rejected_s3_credentials_do_not_stop_attachment_reconciliation() {
    let fixture = fixture().await;
    let root = tempfile::tempdir().unwrap();
    // The fake bucket answers 403 to this key, as a bucket does after the secret was rotated.
    let bucket = S3Bucket::new(S3Config {
        access_key_id: "rotated".to_owned(),
        ..start_fake_s3().await
    })
    .unwrap();
    let storage = ObjectStorage::default();
    storage.configure(ObjectStorageState {
        bucket: Some(bucket),
        backups: true,
        ..ObjectStorageState::default()
    });
    let mutations = AttachmentMutationCoordinator::default();
    let store = TieredBlobStore::new(LocalBlobStore::new(root.path()), storage, mutations.clone());
    let uploads = UploadService::new(
        (*fixture.database).clone(),
        Arc::new(store),
        mutations,
        UploadLimits::new(1024, 1024).unwrap(),
    );
    let state = AttachmentState::new(
        Arc::new(IdentityRepository::new((*fixture.database).clone())),
        uploads,
        CookieMode::secure(),
    );
    assert!(matches!(
        state.reconcile_at(TimestampMillis::now().as_millis()).await,
        Err(orbit_platform::UploadError::BlobStore(
            orbit_platform::BlobStoreError::ObjectStorage(_)
        ))
    ));

    // The first round runs at once and fails on S3; the service must keep running.
    let shutdown = tokio_util::sync::CancellationToken::new();
    let service = tokio::spawn({
        let shutdown = shutdown.clone();
        async move { state.run_reconciliation_service(shutdown).await }
    });
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(!service.is_finished(), "{:?}", service.await);
    shutdown.cancel();
    service.await.unwrap().unwrap();
}

fn environment_with(
    s3: S3Config,
) -> orbit_server::repositories::environment_settings::EnvironmentSettings {
    orbit_server::repositories::environment_settings::EnvironmentSettings::from_config(
        &orbit_platform::InstanceConfig {
            s3: Some(s3),
            ..orbit_platform::InstanceConfig::default()
        },
        &[7; 32],
    )
    .unwrap()
}

#[tokio::test]
async fn another_bucket_waits_until_no_attachment_file_is_left_in_the_recorded_one() {
    use orbit_server::repositories::instance_settings::InstanceSettingsRepository;

    let fixture = fixture().await;
    let database = (*fixture.database).clone();
    let repository = InstanceSettingsRepository::new(database.clone());
    let saved = s3_body().await;
    assert_eq!(
        fixture
            .call("PUT", "/api/v1/admin/settings/s3", Some(saved.clone()))
            .await
            .0,
        StatusCode::OK
    );
    let on = json!({"attachments_in_s3": true, "backups_in_s3": false, "backup_schedule": "off"});
    assert_eq!(
        fixture
            .call("PUT", "/api/v1/admin/settings/storage", Some(on))
            .await
            .0,
        StatusCode::OK
    );
    let recorded = repository.files_bucket().await.unwrap().unwrap();
    assert_eq!(
        (recorded.endpoint.as_str(), recorded.prefix.as_str()),
        (saved["endpoint"].as_str().unwrap(), "orbit")
    );
    let old = S3Bucket::new(S3Config {
        endpoint: recorded.endpoint.clone(),
        region: "us-east-1".to_owned(),
        bucket: recorded.bucket.clone(),
        prefix: "orbit".to_owned(),
        access_key_id: "key".to_owned(),
        secret_access_key: "plain-secret-value".to_owned(),
        path_style: true,
    })
    .unwrap();
    old.put_bytes("blobs/ab/file", b"attachment".to_vec())
        .await
        .unwrap();

    let other = environment_with(start_fake_s3().await);
    let error = other.check_bucket(&database, &[7; 32]).await.unwrap_err();
    assert!(error.contains("files are still in it"), "{error}");

    let off = json!({"attachments_in_s3": false, "backups_in_s3": false, "backup_schedule": "off"});
    assert_eq!(
        fixture
            .call("PUT", "/api/v1/admin/settings/storage", Some(off))
            .await
            .0,
        StatusCode::OK
    );
    old.delete("blobs/ab/file").await.unwrap();
    other.check_bucket(&database, &[7; 32]).await.unwrap();
    assert_eq!(repository.files_bucket().await.unwrap(), None);
}

#[tokio::test]
async fn a_recorded_bucket_without_credentials_stays_until_the_operator_forgets_it() {
    use orbit_server::repositories::instance_settings::{
        BucketLocation, InstanceSettingsRepository,
    };

    let fixture = fixture().await;
    let database = (*fixture.database).clone();
    let repository = InstanceSettingsRepository::new(database.clone());
    let first = start_fake_s3().await;
    repository
        .keep_files_bucket(&BucketLocation {
            endpoint: first.endpoint.clone(),
            bucket: first.bucket.clone(),
            prefix: first.prefix.clone(),
        })
        .await
        .unwrap();

    environment_with(first)
        .check_bucket(&database, &[7; 32])
        .await
        .unwrap();
    let second = environment_with(start_fake_s3().await);
    let error = second.check_bucket(&database, &[7; 32]).await.unwrap_err();
    assert!(error.contains("no credentials"), "{error}");
    assert!(error.contains("forget-bucket"), "{error}");

    assert!(
        repository
            .forget_files_bucket("test", TimestampMillis::now())
            .await
            .unwrap()
            .is_some()
    );
    second.check_bucket(&database, &[7; 32]).await.unwrap();
    let audited = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM audit_events WHERE action = 'instance.files_bucket_forgotten'",
    )
    .fetch_one(database.pool())
    .await
    .unwrap();
    assert_eq!(audited, 1);
}

#[tokio::test]
async fn a_mover_that_saw_attachments_off_keeps_the_record_once_they_are_on_again() {
    use orbit_server::repositories::instance_settings::InstanceSettingsRepository;

    let fixture = fixture().await;
    let repository = InstanceSettingsRepository::new((*fixture.database).clone());
    let (status, _) = fixture
        .call("PUT", "/api/v1/admin/settings/s3", Some(s3_body().await))
        .await;
    assert_eq!(status, StatusCode::OK);
    let options = |attachments: bool| {
        json!({"attachments_in_s3": attachments, "backups_in_s3": false, "backup_schedule": "off"})
    };
    let (status, _) = fixture
        .call("PUT", "/api/v1/admin/settings/storage", Some(options(true)))
        .await;
    assert_eq!(status, StatusCode::OK);
    let recorded = repository.files_bucket().await.unwrap().unwrap();

    // The mover checked the empty bucket while attachments were off; the root user turned them on before it
    // cleared the record.
    assert!(!repository.files_bucket_emptied(&recorded, true).await.unwrap());
    assert_eq!(repository.files_bucket().await.unwrap(), Some(recorded.clone()));

    let (status, _) = fixture
        .call("PUT", "/api/v1/admin/settings/storage", Some(options(false)))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(repository.files_bucket_emptied(&recorded, true).await.unwrap());
    assert_eq!(repository.files_bucket().await.unwrap(), None);
}
