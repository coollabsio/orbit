//! Insights: the numbers behind the project charts.
// The fixture has the shape of the one in `milestones_api.rs`; not every helper is used here.
#![allow(dead_code)]

use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{AuthenticatedUser, Id, PasswordService, TestDatabase, TimestampMillis};
use orbit_server::auth_routes::CookieMode;
use orbit_server::page_routes::{PageState, page_router};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::task_routes::{TaskState, task_router};
use serde_json::{Value, json};
use tower::ServiceExt;

const DAY: i64 = 24 * 60 * 60 * 1_000;

struct Fixture {
    database: TestDatabase,
    identity: Arc<IdentityRepository>,
    app: axum::Router,
    workspace_id: String,
    project_id: String,
    owner_cookie: String,
    owner_id: Id,
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
        let app = task_router(TaskState::new(Arc::clone(&identity), CookieMode::secure())).merge(
            page_router(PageState::new(Arc::clone(&identity), CookieMode::secure())),
        );
        Self {
            database,
            identity,
            app,
            workspace_id: setup.workspace_id.to_string(),
            project_id: setup.project_id.to_string(),
            owner_cookie: format!("__Host-orbit_session={}", setup.session.token),
            owner_id: setup.user_id,
        }
    }

    fn uri(&self, path: &str) -> String {
        format!("/api/v1/workspaces/{}{path}", self.workspace_id)
    }

    fn project_uri(&self, path: &str) -> String {
        self.uri(&format!("/projects/{}{path}", self.project_id))
    }

    async fn call(&self, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        self.call_as(&self.owner_cookie, method, uri, body).await
    }

    async fn call_as(
        &self,
        cookie: &str,
        method: &str,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::COOKIE, cookie);
        let request = match body {
            Some(value) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(value.to_string()))
                .unwrap(),
            None => builder.body(Body::empty()).unwrap(),
        };
        let response = self.app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        (status, value)
    }

    async fn project(&self) -> Value {
        let (status, project) = self.call("GET", &self.project_uri(""), None).await;
        assert_eq!(status, StatusCode::OK);
        project
    }

    async fn patch_project(&self, changes: Value) -> (StatusCode, Value) {
        let current = self.project().await;
        let mut body = json!({
            "name": current["name"],
            "key": current["key"],
            "color": current["color"],
            "expected_version": current["version"],
        });
        for (key, value) in changes.as_object().unwrap() {
            body[key] = value.clone();
        }
        self.call("PATCH", &self.project_uri(""), Some(body)).await
    }

    async fn status_id(&self, project_id: &str, category: &str) -> String {
        sqlx::query_scalar(
            "SELECT id FROM task_statuses WHERE project_id = ? AND category = ? ORDER BY position LIMIT 1",
        )
        .bind(project_id)
        .bind(category)
        .fetch_one(self.database.pool())
        .await
        .unwrap()
    }

    async fn create_task(&self, title: &str, category: &str, extra: Value) -> Value {
        let mut body = json!({
            "project_id": self.project_id,
            "status_id": self.status_id(&self.project_id, category).await,
            "title": title,
        });
        for (key, value) in extra.as_object().unwrap() {
            body[key] = value.clone();
        }
        let (status, task) = self.call("POST", &self.uri("/tasks"), Some(body)).await;
        assert_eq!(status, StatusCode::CREATED, "{task}");
        task
    }

    async fn create_milestone(&self, name: &str) -> Value {
        let (status, milestone) = self
            .call(
                "POST",
                &self.project_uri("/milestones"),
                Some(json!({"name": name})),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{milestone}");
        milestone
    }

    async fn add_member(&self, email: &str, role: &str) -> (Id, String) {
        let id = Id::new_v7();
        let now = TimestampMillis::now();
        sqlx::query(
            "INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at) \
             VALUES (?, ?, ?, 'Member', 'unused', ?, ?)",
        )
        .bind(id.to_string())
        .bind(email)
        .bind(email)
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(self.database.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at) \
             VALUES (?, ?, ?, ?, 0, ?, ?)",
        )
        .bind(Id::new_v7().to_string())
        .bind(&self.workspace_id)
        .bind(id.to_string())
        .bind(role)
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(self.database.pool())
        .await
        .unwrap();
        let session = self
            .identity
            .create_session(
                &AuthenticatedUser {
                    id,
                    email: email.to_owned(),
                    display_name: "Member".to_owned(),
                },
                now,
            )
            .await
            .unwrap();
        (id, format!("__Host-orbit_session={}", session.token))
    }
}

fn id_of(value: &Value) -> &str {
    value["id"].as_str().unwrap()
}

fn iso(millis: i64) -> Value {
    serde_json::to_value(TimestampMillis::from_millis(millis)).unwrap()
}

const WEEK: i64 = 7 * DAY;

/// Monday 00:00 UTC of the week of `millis`. 1970-01-01 was a Thursday.
fn monday_utc(millis: i64) -> i64 {
    let day = millis.div_euclid(DAY);
    (day - (day + 3).rem_euclid(7)) * DAY
}

fn day_text(millis: i64) -> String {
    iso(millis).as_str().unwrap()[..10].to_owned()
}

impl Fixture {
    async fn set_created(&self, task: &Value, millis: i64) {
        sqlx::query("UPDATE tasks SET created_at = ? WHERE id = ?")
            .bind(millis)
            .bind(id_of(task))
            .execute(self.database.pool())
            .await
            .unwrap();
    }

    /// Open, done, a parent with one open sub-issue, and tasks that no chart counts.
    async fn seed(&self, extra: Value) -> (Value, Value, Value) {
        let with = |fields: Value| {
            let mut merged = extra.clone();
            for (key, value) in fields.as_object().unwrap() {
                merged[key] = value.clone();
            }
            merged
        };
        let open = self
            .create_task("open", "unstarted", with(json!({"estimate": 3})))
            .await;
        let done = self
            .create_task("done", "completed", with(json!({"estimate": 5})))
            .await;
        let parent = self
            .create_task("parent", "started", with(json!({"estimate": 13})))
            .await;
        let sub = self
            .create_task(
                "sub",
                "started",
                with(json!({"parent_task_id": id_of(&parent)})),
            )
            .await;
        self.create_task("dropped", "cancelled", with(json!({"estimate": 8})))
            .await;
        self.create_task("waiting", "triage", with(json!({"estimate": 8})))
            .await;
        let trashed = self
            .create_task("trashed", "unstarted", with(json!({"estimate": 8})))
            .await;
        let (status, _) = self
            .call(
                "DELETE",
                &self.uri(&format!("/tasks/{}?expected_version=0", id_of(&trashed))),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        (open, done, sub)
    }
}

#[tokio::test]
async fn open_tasks_group_by_each_dimension_with_the_counting_rules() {
    let fixture = Fixture::new().await;
    let (first, _) = fixture.add_member("first@example.com", "member").await;
    let (second, _) = fixture.add_member("second@example.com", "member").await;
    let mut labels = Vec::new();
    for name in ["bug", "ui"] {
        let (_, label) = fixture
            .call(
                "POST",
                &fixture.uri("/labels"),
                Some(json!({"name": name, "color": "#123456"})),
            )
            .await;
        labels.push(id_of(&label).to_owned());
    }
    let (open, _, sub) = fixture.seed(json!({})).await;
    let (status, updated) = fixture
        .call(
            "PATCH",
            &fixture.uri(&format!("/tasks/{}", id_of(&open))),
            Some(json!({
                "expected_version": 0, "assignee_ids": [first, second], "label_ids": labels,
                "priority": "high"
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{updated}");

    let by = |dimension: &'static str| {
        let fixture = &fixture;
        async move {
            let (status, body) = fixture
                .call(
                    "GET",
                    &fixture.project_uri(&format!("/insights/open?by={dimension}")),
                    None,
                )
                .await;
            assert_eq!(status, StatusCode::OK, "{body}");
            // Open, leaf tasks only: "open" and "sub". "sub" has no estimate.
            assert_eq!(body["unestimated_count"], 1, "{dimension}");
            body["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|group| {
                    (
                        group["key"].as_str().unwrap().to_owned(),
                        group["count"].as_i64().unwrap(),
                        group["points"].as_i64().unwrap(),
                    )
                })
                .collect::<Vec<_>>()
        }
    };
    let unstarted = fixture.status_id(&fixture.project_id, "unstarted").await;
    let started = fixture.status_id(&fixture.project_id, "started").await;
    assert_eq!(by("status").await, [(unstarted, 1, 3), (started, 1, 0)]);
    assert_eq!(
        by("priority").await,
        [("high".to_owned(), 1, 3), ("none".to_owned(), 1, 0)]
    );
    // A task with two assignees or two labels counts in each.
    let mut assignees = by("assignee").await;
    assignees.sort();
    let mut expected = vec![
        (first.to_string(), 1, 3),
        (second.to_string(), 1, 3),
        ("none".to_owned(), 1, 0),
    ];
    expected.sort();
    assert_eq!(assignees, expected);
    let mut by_label = by("label").await;
    by_label.sort();
    let mut expected = vec![
        (labels[0].clone(), 1, 3),
        (labels[1].clone(), 1, 3),
        ("none".to_owned(), 1, 0),
    ];
    expected.sort();
    assert_eq!(by_label, expected);
    assert_eq!(id_of(&sub).len(), 36);

    let (status, _) = fixture
        .call(
            "GET",
            &fixture.project_uri("/insights/open?by=colour"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn throughput_counts_created_and_completed_tasks_in_the_viewers_weeks() {
    let fixture = Fixture::new().await;
    let (_, done, _) = fixture.seed(json!({})).await;
    let this_monday = monday_utc(TimestampMillis::now().as_millis());

    let (status, body) = fixture
        .call(
            "GET",
            &fixture.project_uri("/insights/throughput?weeks=2"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let items = body["items"].as_array().unwrap();
    // Weeks with no data are present, so the chart has no gap.
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["week"], day_text(this_monday - WEEK));
    assert_eq!(items[0]["created_count"], 0);
    assert_eq!(items[1]["week"], day_text(this_monday));
    // Created: open (3), done (5), sub (no estimate). Not: parent, cancelled, triage, trashed.
    assert_eq!(items[1]["created_count"], 3);
    assert_eq!(items[1]["created_points"], 8);
    assert_eq!(items[1]["completed_count"], 1);
    assert_eq!(items[1]["completed_points"], 5);
    assert_eq!(body["unestimated_count"], 1);

    // Sunday 20:00 UTC is in the week before for a viewer in UTC, and already Monday in Tokyo.
    fixture
        .set_created(&done, this_monday - 4 * 60 * 60 * 1_000)
        .await;
    let created_in = |zone: &'static str, week: String| {
        let fixture = &fixture;
        async move {
            let (status, body) = fixture
                .call(
                    "GET",
                    &fixture.project_uri(&format!("/insights/throughput?weeks=3&tz={zone}")),
                    None,
                )
                .await;
            assert_eq!(status, StatusCode::OK, "{body}");
            body["items"]
                .as_array()
                .unwrap()
                .iter()
                .find(|item| item["week"] == week.as_str())
                .map(|item| item["created_count"].as_i64().unwrap())
        }
    };
    assert_eq!(
        created_in("UTC", day_text(this_monday - WEEK)).await,
        Some(1)
    );
    assert_eq!(created_in("UTC", day_text(this_monday)).await, Some(2));
    assert_eq!(
        created_in("Asia/Tokyo", day_text(this_monday - WEEK)).await,
        Some(0)
    );
    assert!(
        created_in("Asia/Tokyo", day_text(this_monday))
            .await
            .unwrap()
            >= 1
    );

    for bad in ["weeks=0", "weeks=500", "tz=Mars/Olympus"] {
        let (status, _) = fixture
            .call(
                "GET",
                &fixture.project_uri(&format!("/insights/throughput?{bad}")),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{bad}");
    }
}

#[tokio::test]
async fn a_milestone_burns_up_from_the_week_of_its_first_task() {
    let fixture = Fixture::new().await;
    let milestone = fixture.create_milestone("v1").await;
    let burnup = fixture.project_uri(&format!("/milestones/{}/burnup", id_of(&milestone)));

    // A milestone with no tasks has no weeks.
    let (status, empty) = fixture.call("GET", &burnup, None).await;
    assert_eq!(status, StatusCode::OK, "{empty}");
    assert_eq!(empty["items"], json!([]));

    let (open, _, _) = fixture
        .seed(json!({"milestone_id": id_of(&milestone)}))
        .await;
    // A task outside the milestone is not in its scope.
    fixture
        .create_task("other", "unstarted", json!({"estimate": 2}))
        .await;
    let (_, body) = fixture.call("GET", &burnup, None).await;
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    // Scope: open (3), done (5), sub (no estimate). Done: "done".
    assert_eq!(items[0]["scope_count"], 3);
    assert_eq!(items[0]["scope_points"], 8);
    assert_eq!(items[0]["done_count"], 1);
    assert_eq!(items[0]["done_points"], 5);
    assert_eq!(body["unestimated_count"], 1);

    // A task created three weeks ago starts the chart there; the scope grows to this week.
    let this_monday = monday_utc(TimestampMillis::now().as_millis());
    fixture
        .set_created(&open, this_monday - 3 * WEEK + DAY)
        .await;
    let (_, body) = fixture.call("GET", &burnup, None).await;
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 4);
    assert_eq!(items[0]["week"], day_text(this_monday - 3 * WEEK));
    assert_eq!(
        (
            items[0]["scope_count"].clone(),
            items[0]["done_count"].clone()
        ),
        (json!(1), json!(0))
    );
    assert_eq!(items[2]["scope_count"], 1);
    assert_eq!(items[3]["scope_count"], 3);
    assert_eq!(items[3]["done_count"], 1);

    let unknown = fixture.project_uri(&format!("/milestones/{}/burnup", Id::new_v7()));
    let (status, _) = fixture.call("GET", &unknown, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // Velocity needs no route: the cycles list has the done totals of each completed cycle.
    let (status, cycles) = fixture
        .call("GET", &fixture.project_uri("/cycles"), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cycles["items"], json!([]));
}
