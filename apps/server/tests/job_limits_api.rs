//! Background job limits in Admin: the API, and a change that reaches the worker of another
//! server of the same database without a restart.

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{
    Config, Database, EnvironmentMode, Id, PasswordService, TestDatabase, TimestampMillis,
};
use orbit_server::app::App;
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::workspace_routes::{WorkspaceState, workspace_router};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

mod shared_postgres;

const ORIGIN: &str = "http://127.0.0.1:8080";

/// Completes the setup and returns the root user's session token.
async fn root_session(database: &Database) -> String {
    let identity = IdentityRepository::new(database.clone());
    let now = TimestampMillis::now();
    identity
        .store_setup_token(
            "operator-secret",
            TimestampMillis::from_millis(now.as_millis() + 60_000),
        )
        .await
        .unwrap();
    identity
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
        .unwrap()
        .session
        .token
}

async fn call(
    app: &Router,
    cookie: &str,
    method: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(method)
        .uri("/api/v1/admin/jobs/limits")
        .header(header::COOKIE, cookie)
        .header(header::ORIGIN, ORIGIN);
    let request = match body {
        Some(body) => request
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string())),
        None => request.body(Body::empty()),
    }
    .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn the_root_user_changes_and_resets_job_limits() {
    let database = TestDatabase::new().await.unwrap();
    let token = root_session(&database).await;
    let app = workspace_router(WorkspaceState::new(
        Arc::new(IdentityRepository::new((*database).clone())),
        ORIGIN.to_owned(),
        orbit_server::auth_routes::CookieMode::secure(),
    ));
    let cookie = format!("__Host-orbit_session={token}");
    let defaults = json!({"kinds": [
        {"kind": "notion.scan", "concurrency": 2, "workspace_concurrency": 1,
         "default_concurrency": 2, "default_workspace_concurrency": 1},
        {"kind": "notion.import", "concurrency": 2, "workspace_concurrency": 1,
         "default_concurrency": 2, "default_workspace_concurrency": 1},
    ]});

    let (status, view) = call(&app, &cookie, "GET", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view, defaults);

    let (status, view) = call(
        &app,
        &cookie,
        "PUT",
        Some(json!({"kinds": [
            {"kind": "notion.import", "concurrency": 6, "workspace_concurrency": null},
        ]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["kinds"][0], defaults["kinds"][0]);
    assert_eq!(
        view["kinds"][1],
        json!({"kind": "notion.import", "concurrency": 6, "workspace_concurrency": null,
               "default_concurrency": 2, "default_workspace_concurrency": 1})
    );

    for invalid in [
        json!({"kind": "backup.scheduled", "concurrency": 2, "workspace_concurrency": 1}),
        json!({"kind": "notion.import", "concurrency": 0, "workspace_concurrency": 1}),
        json!({"kind": "notion.import", "concurrency": 2, "workspace_concurrency": 101}),
    ] {
        let (status, problem) = call(&app, &cookie, "PUT", Some(json!({"kinds": [invalid]}))).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{invalid}");
        assert_eq!(problem["code"], "invalid_job_limits");
    }

    let (status, view) = call(
        &app,
        &cookie,
        "PUT",
        Some(json!({"kinds": [
            {"kind": "notion.import", "concurrency": 2, "workspace_concurrency": 1},
        ]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view, defaults);
    let rows: i64 = database
        .scalar("SELECT COUNT(*) FROM job_limits")
        .await
        .unwrap();
    assert_eq!(rows, 0, "limits equal to the defaults keep no row");
}

fn server_config(url: &str, root: &std::path::Path) -> Config {
    let mut config = Config {
        environment: EnvironmentMode::Development,
        ..Config::default()
    };
    config.data.database = root.join("data/orbit.sqlite");
    config.data.attachments = root.join("data/attachments");
    config.data.backups = root.join("backups");
    config.data.database_url = Some(url.to_owned());
    config.secrets.insert(
        "app_key".to_owned(),
        orbit_platform::Secret::new(shared_postgres::APP_KEY),
    );
    config
}

async fn insert_import_job(database: &Database, workspace: &str, state: &str) -> String {
    let id = Id::new_v7().to_string();
    let now = TimestampMillis::now().as_millis();
    let lease = (state == "running").then_some(now + 60 * 60 * 1_000);
    sqlx::query(
        "INSERT INTO jobs (id, workspace_id, kind, payload_json, state, max_attempts, available_at, \
         lease_owner, claim_token, lease_expires_at, attempt_count, created_at, updated_at) \
         VALUES ($1, $2, 'notion.import', '{}', $3, 5, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(&id)
    .bind(workspace)
    .bind(state)
    .bind(now)
    .bind(lease.map(|_| "another-server"))
    .bind(lease.map(|_| "token"))
    .bind(lease)
    .bind(i64::from(lease.is_some()))
    .bind(now)
    .bind(now)
    .execute(database.pool())
    .await
    .unwrap();
    id
}

#[tokio::test]
async fn a_limit_saved_on_one_server_applies_to_the_worker_of_another() {
    let Some(shared) = shared_postgres::SharedDatabase::create().await else {
        return; // several servers need a Postgres database
    };
    let (root_one, root_two) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    // Server one only answers HTTP (its worker never starts); server two only runs jobs.
    let one = App::build(server_config(&shared.url, root_one.path()))
        .await
        .unwrap();
    let two = App::build(server_config(&shared.url, root_two.path()))
        .await
        .unwrap();
    let database = one.database().clone();
    let cookie = format!("orbit_session_dev={}", root_session(&database).await);
    let router = one.router();
    let shutdown = CancellationToken::new();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let task = tokio::spawn(two.serve(listener, shutdown.clone()));
    let set_concurrency = |concurrency: u32| {
        let (router, cookie) = (router.clone(), cookie.clone());
        async move {
            let body = json!({"kinds": [{"kind": "notion.import", "concurrency": concurrency,
                                         "workspace_concurrency": 1}]});
            let (status, view) = call(&router, &cookie, "PUT", Some(body)).await;
            assert_eq!(status, StatusCode::OK, "{view}");
        }
    };
    let attempts = |id: String| {
        let database = database.clone();
        async move {
            sqlx::query_scalar::<_, i64>("SELECT attempt_count FROM jobs WHERE id = $1")
                .bind(id)
                .fetch_one(database.pool())
                .await
                .unwrap()
        }
    };

    // One import runs elsewhere; the default limit of two would start the waiting one.
    set_concurrency(1).await;
    insert_import_job(&database, "workspace-a", "running").await;
    let waiting = insert_import_job(&database, "workspace-b", "queued").await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(
        attempts(waiting.clone()).await,
        0,
        "server two keeps the limit saved on server one"
    );

    set_concurrency(2).await;
    tokio::time::timeout(Duration::from_secs(10), async {
        while attempts(waiting.clone()).await == 0 {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("server two starts the import once the limit is raised");

    shutdown.cancel();
    task.await.unwrap().unwrap();
    drop(one);
    shared.drop().await;
}
