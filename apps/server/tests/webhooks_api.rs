use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{PasswordService, TestDatabase, TimestampMillis};
use orbit_server::auth_routes::CookieMode;
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::task_routes::{TaskState, task_router};
use orbit_server::webhooks::routes::{WebhookState, webhook_router};
use serde_json::{Value, json};
use tower::ServiceExt;

struct Fixture {
    database: TestDatabase,
    app: axum::Router,
    workspace_id: String,
    project_id: String,
    status_id: String,
    cookie: String,
}

impl Fixture {
    async fn new() -> Self {
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
            "SELECT id FROM task_statuses WHERE project_id = $1 ORDER BY position LIMIT 1",
        )
        .bind(setup.project_id.to_string())
        .fetch_one(database.pool())
        .await
        .unwrap();
        let app = task_router(TaskState::new(Arc::clone(&identity), CookieMode::secure())).merge(
            webhook_router(WebhookState::new(
                identity,
                CookieMode::secure(),
                Some([7; 32]),
                false,
            )),
        );
        Self {
            database,
            app,
            workspace_id: setup.workspace_id.to_string(),
            project_id: setup.project_id.to_string(),
            status_id,
            cookie: format!("__Host-orbit_session={}", setup.session.token),
        }
    }

    async fn call(&self, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(format!("/api/v1/workspaces/{}{path}", self.workspace_id))
            .header(header::COOKIE, &self.cookie);
        let body = match body {
            Some(value) => {
                request = request.header(header::CONTENT_TYPE, "application/json");
                Body::from(value.to_string())
            }
            None => Body::empty(),
        };
        let response = self
            .app
            .clone()
            .oneshot(request.body(body).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        (status, value)
    }

    async fn create_webhook(&self, body: Value) -> Value {
        let (status, webhook) = self.call("POST", "/webhooks", Some(body)).await;
        assert_eq!(status, StatusCode::CREATED, "{webhook}");
        webhook
    }

    async fn create_project(&self, key: &str) -> (String, String) {
        let (status, project) = self
            .call(
                "POST",
                "/projects",
                Some(json!({"name": key, "key": key, "color": "#123456"})),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED);
        let project_id = project["id"].as_str().unwrap().to_owned();
        let status_id: String = sqlx::query_scalar(
            "SELECT id FROM task_statuses WHERE project_id = $1 ORDER BY position LIMIT 1",
        )
        .bind(&project_id)
        .fetch_one(self.database.pool())
        .await
        .unwrap();
        (project_id, status_id)
    }

    async fn create_task(&self, project_id: &str, status_id: &str, title: &str) -> Value {
        let (status, task) = self
            .call(
                "POST",
                "/tasks",
                Some(json!({"project_id": project_id, "status_id": status_id, "title": title})),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED);
        task
    }

    /// `(event_type, resource_id, state, body)` of every delivery, oldest first.
    async fn deliveries(&self) -> Vec<(String, String, String, Value)> {
        let rows: Vec<(String, String, String, String)> = sqlx::query_as(
            "SELECT event_type, resource_id, state, body FROM webhook_deliveries ORDER BY created_at, id",
        )
        .fetch_all(self.database.pool())
        .await
        .unwrap();
        rows.into_iter()
            .map(|(event_type, resource_id, state, body)| {
                (
                    event_type,
                    resource_id,
                    state,
                    serde_json::from_str(&body).unwrap(),
                )
            })
            .collect()
    }

    async fn delivery_jobs(&self) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM jobs WHERE kind = 'webhook.deliver'")
            .fetch_one(self.database.pool())
            .await
            .unwrap()
    }
}

const ALL_EVENTS: [&str; 3] = ["task.created", "task.updated", "task.deleted"];

#[tokio::test]
async fn a_task_moved_out_of_a_selected_project_reaches_its_old_subscribers() {
    let fixture = Fixture::new().await;
    let (other_project, other_status) = fixture.create_project("OTHER").await;
    for (name, project) in [("Old", &fixture.project_id), ("New", &other_project)] {
        fixture
            .create_webhook(json!({
                "name": name,
                "url": "https://hooks.example.com/orbit",
                "event_types": ["task.updated"],
                "project_ids": [project],
            }))
            .await;
    }
    let task = fixture
        .create_task(&fixture.project_id, &fixture.status_id, "Moving")
        .await;
    let task_id = task["id"].as_str().unwrap();
    let (status, moved) = fixture
        .call(
            "PATCH",
            &format!("/tasks/{task_id}"),
            Some(json!({
                "expected_version": task["version"],
                "project_id": other_project,
                "status_id": other_status,
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{moved}");

    let bodies: Vec<Value> = fixture
        .deliveries()
        .await
        .into_iter()
        .map(|(_, _, _, body)| body)
        .collect();
    assert_eq!(bodies.len(), 2);
    assert_eq!(
        bodies[0]["id"], bodies[1]["id"],
        "one event, two deliveries"
    );
    let mut old = bodies
        .iter()
        .find(|body| body["data"]["moved_out"] == true)
        .unwrap()
        .clone();
    assert!(old["data"]["task"]["version"].is_u64());
    old["data"]["task"]["version"] = json!(0);
    assert_eq!(
        old["data"],
        json!({
            "project_id": fixture.project_id,
            "moved_out": true,
            "task": {"id": task_id, "version": 0},
            "changes": {"project": {"from": fixture.project_id, "to": null}},
        })
    );
    let new = bodies
        .iter()
        .find(|body| body["data"]["moved_out"].is_null())
        .unwrap();
    assert_eq!(new["data"]["project_id"], other_project);
    assert_eq!(new["data"]["task"]["title"], "Moving");
    assert_eq!(
        new["data"]["changes"]["project"],
        json!({"from": null, "to": other_project})
    );
    assert_eq!(
        new["data"]["changes"]["status"],
        json!({"from": null, "to": other_status})
    );
}

#[tokio::test]
async fn task_events_of_selected_projects_become_signed_deliveries() {
    let fixture = Fixture::new().await;
    let (other_project, other_status) = fixture.create_project("OTHER").await;
    let webhook = fixture
        .create_webhook(json!({
            "name": "Sync",
            "url": "https://hooks.example.com/orbit",
            "event_types": ALL_EVENTS,
            "project_ids": [fixture.project_id],
        }))
        .await;
    let secret = webhook["signing_secret"].as_str().unwrap();
    assert!(secret.starts_with("whsec_"), "{secret}");
    assert_eq!(webhook["enabled"], true);
    assert_eq!(webhook["project_ids"], json!([fixture.project_id]));
    let webhook_id = webhook["id"].as_str().unwrap();
    let (status, read) = fixture
        .call("GET", &format!("/webhooks/{webhook_id}"), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        read.get("signing_secret").is_none(),
        "the secret is shown once"
    );

    let task = fixture
        .create_task(&fixture.project_id, &fixture.status_id, "Watched")
        .await;
    let task_id = task["id"].as_str().unwrap();
    let (status, updated) = fixture
        .call(
            "PATCH",
            &format!("/tasks/{task_id}"),
            Some(json!({"expected_version": task["version"], "priority": "high"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = fixture
        .call(
            "DELETE",
            &format!("/tasks/{task_id}?expected_version={}", updated["version"]),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    fixture
        .create_task(&other_project, &other_status, "Not watched")
        .await;

    let deliveries = fixture.deliveries().await;
    let summary: Vec<(&str, &str, &str)> = deliveries
        .iter()
        .map(|(event_type, resource_id, state, _)| {
            (event_type.as_str(), resource_id.as_str(), state.as_str())
        })
        .collect();
    assert_eq!(
        summary,
        [
            ("task.created", task_id, "pending"),
            ("task.updated", task_id, "pending"),
            ("task.deleted", task_id, "pending"),
        ]
    );
    assert_eq!(fixture.delivery_jobs().await, 3);

    let created = &deliveries[0].3;
    assert_eq!(created["type"], "task.created");
    assert_eq!(created["schema_version"], 1);
    assert_eq!(created["workspace_id"], fixture.workspace_id);
    assert_eq!(created["resource"], json!({"type": "task", "id": task_id}));
    assert_eq!(created["origin"], json!({"action": "task.created"}));
    assert_eq!(created["data"]["project_id"], fixture.project_id);
    assert_eq!(created["data"]["task"]["title"], "Watched");
    assert_eq!(created["data"]["task"]["parent_task_id"], Value::Null);
    assert_eq!(created["data"]["changes"], json!({}));
    let updated_body = &deliveries[1].3;
    assert_eq!(
        updated_body["data"]["changes"],
        json!({"priority": {"from": "none", "to": "high"}})
    );
    assert_ne!(updated_body["id"], created["id"], "one event id per event");
    let deleted = &deliveries[2].3;
    assert_eq!(deleted["data"]["task"]["id"], task_id);
    assert!(deleted["data"]["task"]["deleted_at"].is_string());
    assert!(deleted["data"]["task"].get("title").is_none());

    let (status, listed) = fixture
        .call("GET", &format!("/webhooks/{webhook_id}/deliveries"), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    let listed: Vec<(&str, &str)> = listed
        .as_array()
        .unwrap()
        .iter()
        .map(|delivery| {
            (
                delivery["event_type"].as_str().unwrap(),
                delivery["state"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        listed,
        [
            ("task.deleted", "pending"),
            ("task.updated", "pending"),
            ("task.created", "pending"),
        ]
    );
}

#[tokio::test]
async fn disabled_subscriptions_and_other_event_types_produce_no_deliveries() {
    let fixture = Fixture::new().await;
    fixture
        .create_webhook(json!({
            "name": "Off",
            "url": "https://hooks.example.com/off",
            "event_types": ALL_EVENTS,
            "project_ids": [fixture.project_id],
            "enabled": false,
        }))
        .await;
    fixture
        .create_webhook(json!({
            "name": "Deletes only",
            "url": "https://hooks.example.com/deletes",
            "event_types": ["task.deleted"],
            "project_ids": [fixture.project_id],
        }))
        .await;
    fixture
        .create_task(&fixture.project_id, &fixture.status_id, "Quiet")
        .await;
    assert!(fixture.deliveries().await.is_empty());
    assert_eq!(fixture.delivery_jobs().await, 0);
}

#[tokio::test]
async fn a_rolled_back_write_leaves_no_delivery() {
    let fixture = Fixture::new().await;
    fixture
        .create_webhook(json!({
            "name": "Updates",
            "url": "https://hooks.example.com/updates",
            "event_types": ["task.updated"],
            "project_ids": [fixture.project_id],
        }))
        .await;
    let first = fixture
        .create_task(&fixture.project_id, &fixture.status_id, "First")
        .await;
    let second = fixture
        .create_task(&fixture.project_id, &fixture.status_id, "Second")
        .await;
    // The first update is written and recorded, then the stale second one rolls both back.
    let (status, _) = fixture
        .call(
            "POST",
            "/tasks/bulk",
            Some(json!({"updates": [
                {"id": first["id"], "expected_version": first["version"], "priority": "high"},
                {"id": second["id"], "expected_version": 99, "priority": "urgent"},
            ]})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(fixture.deliveries().await.is_empty());
    assert_eq!(fixture.delivery_jobs().await, 0);
}

#[tokio::test]
async fn only_failed_or_cancelled_deliveries_can_be_retried() {
    let fixture = Fixture::new().await;
    let webhook = fixture
        .create_webhook(json!({
            "name": "Retry",
            "url": "https://hooks.example.com/retry",
            "event_types": ["task.created"],
            "project_ids": [fixture.project_id],
        }))
        .await;
    let webhook_id = webhook["id"].as_str().unwrap();
    fixture
        .create_task(&fixture.project_id, &fixture.status_id, "Retried")
        .await;
    let delivery_id: String = sqlx::query_scalar("SELECT id FROM webhook_deliveries")
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    let retry = format!("/webhooks/{webhook_id}/deliveries/{delivery_id}/retry");

    let (status, problem) = fixture.call("POST", &retry, None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(problem["code"], "webhook_delivery_not_retryable");

    let (status, _) = fixture
        .call(
            "PATCH",
            &format!("/webhooks/{webhook_id}"),
            Some(json!({"enabled": false})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fixture.deliveries().await[0].2, "cancelled");

    let (status, retried) = fixture.call("POST", &retry, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(retried["state"], "pending");
    assert_eq!(retried["attempt_count"], 0);
    assert_eq!(fixture.delivery_jobs().await, 2);
    let current_job: String =
        sqlx::query_scalar("SELECT job_id FROM webhook_deliveries WHERE id = $1")
            .bind(&delivery_id)
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    let newest_job: String = sqlx::query_scalar(
        "SELECT id FROM jobs WHERE kind = 'webhook.deliver' ORDER BY created_at DESC, id DESC LIMIT 1",
    )
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(current_job, newest_job, "only the newest job may send it");
}

#[tokio::test]
async fn subscriptions_are_validated() {
    let fixture = Fixture::new().await;
    let valid = json!({
        "name": "Valid",
        "url": "https://hooks.example.com/valid",
        "event_types": ["task.created"],
        "project_ids": [fixture.project_id],
    });
    for (field, value) in [
        ("url", json!("http://hooks.example.com/plain")),
        ("url", json!("https://127.0.0.1/local")),
        ("event_types", json!([])),
        ("project_ids", json!([])),
        (
            "project_ids",
            json!([orbit_platform::Id::new_v7().to_string()]),
        ),
        ("name", json!("  ")),
    ] {
        let mut body = valid.clone();
        body[field] = value.clone();
        let (status, problem) = fixture.call("POST", "/webhooks", Some(body)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{field} {value}");
        assert_eq!(problem["detail"], field);
    }
    let mut unknown = valid.clone();
    unknown["event_types"] = json!(["task.purged"]);
    let (status, _) = fixture.call("POST", "/webhooks", Some(unknown)).await;
    assert!(status.is_client_error());
    let (status, list) = fixture.call("GET", "/webhooks", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list, json!([]));
}
