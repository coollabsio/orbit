//! Estimates and cycles (migration 0047).

use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{Id, PasswordService, TestDatabase, TimestampMillis};
use orbit_server::auth_routes::CookieMode;
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::tasks::{TaskChanges, TaskRepository, TaskUpdate};
use orbit_server::task_routes::{TaskState, task_router};
use serde_json::{Value, json};
use tower::ServiceExt;

const DAY: i64 = 24 * 60 * 60 * 1_000;

struct Fixture {
    database: TestDatabase,
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
        let app = task_router(TaskState::new(Arc::clone(&identity), CookieMode::secure()));
        Self {
            database,
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
}

fn id_of(value: &Value) -> &str {
    value["id"].as_str().unwrap()
}

fn iso(millis: i64) -> Value {
    serde_json::to_value(TimestampMillis::from_millis(millis)).unwrap()
}

impl Fixture {
    fn repository(&self) -> TaskRepository {
        TaskRepository::new((*self.database).clone())
    }

    async fn category_of(&self, task_id: &str) -> String {
        sqlx::query_scalar(
            "SELECT category FROM task_statuses JOIN tasks ON tasks.status_id = task_statuses.id WHERE tasks.id = ?",
        )
        .bind(task_id)
        .fetch_one(self.database.pool())
        .await
        .unwrap()
    }

    async fn query(&self, filter: Value) -> Vec<String> {
        let (status, page) = self
            .call(
                "POST",
                &self.uri("/tasks/query"),
                Some(json!({
                    "filter": {"op": "and", "children": filter},
                    "order_by": "created", "order_direction": "asc", "show_completed": "all"
                })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{page}");
        page["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|task| task["title"].as_str().unwrap().to_owned())
            .collect()
    }

    /// Turns cycles on: one-week cycles in UTC that start on today's weekday, so the first cycle
    /// starts today. `extra` overrides fields of the settings body.
    async fn enable_cycles(&self, extra: Value) -> Value {
        let mut body = json!({
            "expected_version": 0, "enabled": true, "weeks": 1, "start_weekday": weekday_today(),
            "cooldown_weeks": 0, "cycles_ahead": 2, "timezone": "UTC",
            "auto_add_started": false, "auto_add_completed": false, "active_without_cycle": "off",
        });
        for (key, value) in extra.as_object().unwrap() {
            body[key] = value.clone();
        }
        let (status, settings) = self
            .call("PUT", &self.project_uri("/cycle-settings"), Some(body))
            .await;
        assert_eq!(status, StatusCode::OK, "{settings}");
        settings
    }

    async fn cycles(&self) -> Vec<Value> {
        let (status, page) = self.call("GET", &self.project_uri("/cycles"), None).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        page["items"].as_array().unwrap().clone()
    }

    /// Changes the status of a task at the time `now`, as the status change path does.
    async fn move_at(&self, task_id: &str, category: &str, now: i64) -> Value {
        let status_id = self.status_id(&self.project_id, category).await;
        let version = self.task(task_id).await["version"].as_u64().unwrap();
        self.repository()
            .update_task(
                self.workspace_id.parse().unwrap(),
                self.owner_id,
                &TaskUpdate {
                    id: task_id.parse().unwrap(),
                    expected_version: version,
                    changes: TaskChanges {
                        status_id: Some(status_id.parse().unwrap()),
                        ..TaskChanges::default()
                    },
                },
                "cycles-test",
                TimestampMillis::from_millis(now),
            )
            .await
            .unwrap();
        self.task(task_id).await
    }

    async fn task(&self, task_id: &str) -> Value {
        let (status, task) = self
            .call("GET", &self.uri(&format!("/tasks/{task_id}")), None)
            .await;
        assert_eq!(status, StatusCode::OK, "{task}");
        task
    }
}

/// 0 = Sunday ... 6 = Saturday, in UTC. 1970-01-01 was a Thursday.
fn weekday_today() -> i64 {
    (TimestampMillis::now().as_millis().div_euclid(DAY) + 4).rem_euclid(7)
}

fn millis(value: &Value) -> i64 {
    serde_json::from_value::<TimestampMillis>(value.clone())
        .unwrap()
        .as_millis()
}

fn at(millis: i64) -> TimestampMillis {
    TimestampMillis::from_millis(millis)
}

#[tokio::test]
async fn estimates_are_points_with_a_scale_for_each_project() {
    let fixture = Fixture::new().await;
    let project = fixture.project().await;
    assert_eq!(project["estimate_scale"], Value::Null);
    let (status, _) = fixture
        .patch_project(json!({"estimate_scale": "hours"}))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, project) = fixture
        .patch_project(json!({"estimate_scale": "fibonacci"}))
        .await;
    assert_eq!(status, StatusCode::OK, "{project}");
    assert_eq!(project["estimate_scale"], "fibonacci");
    // Absent: unchanged. Null: off.
    let (_, project) = fixture.patch_project(json!({})).await;
    assert_eq!(project["estimate_scale"], "fibonacci");

    let (status, _) = fixture
        .call(
            "POST",
            &fixture.uri("/tasks"),
            Some(json!({
                "project_id": fixture.project_id,
                "status_id": fixture.status_id(&fixture.project_id, "unstarted").await,
                "title": "bad", "estimate": -1
            })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let small = fixture
        .create_task("small", "unstarted", json!({"estimate": 1}))
        .await;
    let large = fixture
        .create_task("large", "unstarted", json!({"estimate": 8}))
        .await;
    let none = fixture.create_task("none", "unstarted", json!({})).await;
    assert_eq!(small["estimate"], 1);
    assert_eq!(none["estimate"], Value::Null);

    // Update, bulk and clear.
    let (status, updated) = fixture
        .call(
            "PATCH",
            &fixture.uri(&format!("/tasks/{}", id_of(&small))),
            Some(json!({"expected_version": 0, "estimate": 3})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["estimate"], 3);
    let (status, bulk) = fixture
        .call(
            "POST",
            &fixture.uri("/tasks/bulk"),
            Some(json!({"updates": [{"id": id_of(&none), "expected_version": 0, "estimate": 5}]})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{bulk}");
    let (_, cleared) = fixture
        .call(
            "PATCH",
            &fixture.uri(&format!("/tasks/{}", id_of(&none))),
            Some(json!({"expected_version": 1, "estimate": null})),
        )
        .await;
    assert_eq!(cleared["estimate"], Value::Null);

    // Filter and order: tasks with no estimate sort last in both directions.
    assert_eq!(
        fixture
            .query(json!([{"field": "estimate", "operator": "is", "value": ["3", "8"]}]))
            .await,
        ["small", "large"]
    );
    assert_eq!(
        fixture
            .query(json!([{"field": "estimate", "operator": "is_empty"}]))
            .await,
        ["none"]
    );
    for (direction, expected) in [
        ("asc", ["small", "large", "none"]),
        ("desc", ["large", "small", "none"]),
    ] {
        let (status, page) = fixture
            .call(
                "POST",
                &fixture.uri("/tasks/query"),
                Some(json!({
                    "filter": {"op": "and", "children": []},
                    "order_by": "estimate", "order_direction": direction, "show_completed": "all"
                })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{page}");
        let titles: Vec<&str> = page["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|task| task["title"].as_str().unwrap())
            .collect();
        assert_eq!(titles, expected, "{direction}");
    }

    // A parent shows the sum of its leaf sub-issues at every level; its own estimate is kept.
    let parent = fixture
        .create_task("parent", "unstarted", json!({"estimate": 13}))
        .await;
    let child = fixture
        .create_task(
            "child",
            "unstarted",
            json!({"estimate": 5, "parent_task_id": id_of(&parent)}),
        )
        .await;
    fixture
        .create_task(
            "leaf a",
            "unstarted",
            json!({"estimate": 2, "parent_task_id": id_of(&parent)}),
        )
        .await;
    fixture
        .create_task(
            "leaf b",
            "unstarted",
            json!({"estimate": 3, "parent_task_id": id_of(&child)}),
        )
        .await;
    fixture
        .create_task(
            "leaf c",
            "unstarted",
            json!({"parent_task_id": id_of(&child)}),
        )
        .await;
    let parent = fixture.task(id_of(&parent)).await;
    assert_eq!(parent["estimate"], 13);
    assert_eq!(parent["sub_issue_estimate"], 5);
    assert_eq!(fixture.task(id_of(&child)).await["sub_issue_estimate"], 3);
    assert_eq!(
        fixture.task(id_of(&large)).await["sub_issue_estimate"],
        Value::Null
    );

    // A payload carries an estimate.
    let (status, task) = fixture
        .call(
            "POST",
            &fixture.uri("/tasks/from-payload"),
            Some(json!({"project_id": fixture.project_id, "payload": {"title": "templated", "estimate": 2}})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{task}");
    assert_eq!(task["estimate"], 2);
}

#[tokio::test]
async fn cycles_are_created_ahead_closed_with_a_rollover_and_snapshotted() {
    let fixture = Fixture::new().await;
    let repository = fixture.repository();
    let (_, settings) = fixture
        .call("GET", &fixture.project_uri("/cycle-settings"), None)
        .await;
    assert_eq!(settings["enabled"], false);
    assert_eq!(settings["version"], 0);
    assert!(fixture.cycles().await.is_empty());

    // Bad settings and a stale version are refused.
    for (field, value) in [
        ("weeks", json!(9)),
        ("timezone", json!("Mars/Olympus")),
        ("cycles_ahead", json!(0)),
    ] {
        let mut body = json!({
            "expected_version": 0, "enabled": true, "weeks": 1, "start_weekday": 1, "cooldown_weeks": 0,
            "cycles_ahead": 2, "timezone": "UTC", "auto_add_started": false, "auto_add_completed": false,
            "active_without_cycle": "off",
        });
        body[field] = value;
        let (status, _) = fixture
            .call("PUT", &fixture.project_uri("/cycle-settings"), Some(body))
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{field}");
    }

    let settings = fixture.enable_cycles(json!({})).await;
    assert_eq!(settings["version"], 1);
    // One current cycle and two future ones, with no gap.
    let cycles = fixture.cycles().await;
    let states: Vec<&str> = cycles
        .iter()
        .map(|cycle| cycle["state"].as_str().unwrap())
        .collect();
    assert_eq!(states, ["current", "future", "future"]);
    assert_eq!(cycles[0]["number"], 1);
    assert_eq!(cycles[2]["number"], 3);
    assert_eq!(cycles[0]["ends_at"], cycles[1]["starts_at"]);
    assert_eq!(
        millis(&cycles[0]["ends_at"]) - millis(&cycles[0]["starts_at"]),
        7 * DAY
    );
    // 00:01 UTC.
    assert_eq!(millis(&cycles[0]["starts_at"]).rem_euclid(DAY), 60_000);
    let (_, current) = fixture
        .call("GET", &fixture.uri("/cycles/current"), None)
        .await;
    assert_eq!(current["items"][0]["id"], cycles[0]["id"]);
    // The job has nothing more to do.
    let now = TimestampMillis::now();
    let run = repository.run_cycles(now).await.unwrap();
    assert_eq!((run.created, run.closed, run.failed), (0, 0, 0));

    // Tasks of the cycle. The scope has leaf tasks only, without cancelled ones.
    let first = cycles[0]["id"].as_str().unwrap().to_owned();
    let second = cycles[1]["id"].as_str().unwrap().to_owned();
    let with = |estimate: Value| json!({"cycle_id": first, "estimate": estimate});
    let todo = fixture
        .create_task("todo", "unstarted", with(json!(2)))
        .await;
    let doing = fixture
        .create_task("doing", "started", with(json!(3)))
        .await;
    let done = fixture
        .create_task("done", "completed", with(json!(5)))
        .await;
    let backlog = fixture
        .create_task("backlog", "backlog", with(Value::Null))
        .await;
    fixture
        .create_task("dropped", "cancelled", with(json!(8)))
        .await;
    let parent = fixture
        .create_task("parent", "unstarted", with(json!(13)))
        .await;
    fixture
        .create_task(
            "sub",
            "unstarted",
            json!({"cycle_id": first, "estimate": 1, "parent_task_id": id_of(&parent)}),
        )
        .await;
    let cycle = &fixture.cycles().await[0];
    // todo, doing, done, backlog, sub (the parent is not a leaf; dropped is cancelled).
    assert_eq!(cycle["scope_count"], 5);
    assert_eq!(cycle["scope_points"], 11);
    assert_eq!(
        (
            cycle["started_count"].clone(),
            cycle["started_points"].clone()
        ),
        (json!(1), json!(3))
    );
    assert_eq!(
        (cycle["done_count"].clone(), cycle["done_points"].clone()),
        (json!(1), json!(5))
    );
    assert_eq!(cycle["unestimated_count"], 1);

    // The filter names the cycle by its place in time, or by id.
    let by = |value: &str| json!([{"field": "cycle", "operator": "is", "value": [value]}]);
    assert_eq!(fixture.query(by("current")).await.len(), 7);
    assert_eq!(fixture.query(by("next")).await.len(), 0);
    assert_eq!(fixture.query(by(&first)).await.len(), 7);
    let outside = fixture.create_task("outside", "unstarted", json!({})).await;
    assert_eq!(fixture.query(by("none")).await, ["outside"]);

    // A cycle of a different project is refused, and a project move clears the cycle.
    let (_, other) = fixture
        .call(
            "POST",
            &fixture.uri("/projects"),
            Some(json!({"name": "Other", "key": "OTH", "color": "#123456"})),
        )
        .await;
    let (status, _) = fixture
        .call(
            "POST",
            &fixture.uri("/tasks"),
            Some(json!({
                "project_id": id_of(&other),
                "status_id": fixture.status_id(id_of(&other), "unstarted").await,
                "title": "foreign", "cycle_id": first
            })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, moved) = fixture
        .call(
            "PATCH",
            &fixture.uri(&format!("/tasks/{}", id_of(&outside))),
            Some(json!({"expected_version": 0, "cycle_id": first})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{moved}");
    let (_, moved) = fixture
        .call(
            "PATCH",
            &fixture.uri(&format!("/tasks/{}", id_of(&outside))),
            Some(json!({"expected_version": 1, "project_id": id_of(&other)})),
        )
        .await;
    assert_eq!(moved["cycle_id"], Value::Null);

    // The end of the cycle: open planned work moves on, closed work stays, the backlog drops out.
    let after_first = millis(&cycles[0]["ends_at"]) + 60 * 60 * 1_000;
    let run = repository.run_cycles(at(after_first)).await.unwrap();
    assert_eq!((run.created, run.closed, run.failed), (1, 1, 0));
    assert_eq!(
        fixture.task(id_of(&todo)).await["cycle_id"],
        second.as_str()
    );
    assert_eq!(
        fixture.task(id_of(&doing)).await["cycle_id"],
        second.as_str()
    );
    assert_eq!(
        fixture.task(id_of(&parent)).await["cycle_id"],
        second.as_str()
    );
    assert_eq!(fixture.task(id_of(&done)).await["cycle_id"], first.as_str());
    assert_eq!(fixture.task(id_of(&backlog)).await["cycle_id"], Value::Null);
    let open_left: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tasks JOIN task_statuses ON task_statuses.id = tasks.status_id \
         WHERE tasks.cycle_id = ? AND task_statuses.category NOT IN ('completed', 'cancelled', 'duplicate')",
    )
    .bind(&first)
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(open_left, 0, "no open task stays in a closed cycle");
    // One audit row for each moved task; the activity feed of the task shows it.
    let moves: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM audit_events WHERE action = 'task.cycle_changed'")
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    assert_eq!(moves, 5);
    let stored: Vec<(String, Option<i64>)> = sqlx::query_as(
        "SELECT id, completed_at FROM cycles WHERE project_id = ? ORDER BY starts_at",
    )
    .bind(&fixture.project_id)
    .fetch_all(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(stored.len(), 4);
    assert_eq!(stored[0].1, Some(millis(&cycles[0]["ends_at"])));
    assert_eq!(stored[1].1, None);

    // Snapshots: one row for each day the job saw, and the last row at the close.
    let (status, days) = fixture
        .call(
            "GET",
            &fixture.project_uri(&format!("/cycles/{first}/burndown")),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{days}");
    let days = days["items"].as_array().unwrap().clone();
    assert_eq!(days.len(), 2, "{days:?}");
    assert_eq!(days[0]["scope_count"], 0, "the first day, before the tasks");
    // At the close: todo, doing, done, backlog, sub, outside is gone to the other project.
    assert_eq!(days[1]["done_points"], 5);
    assert_eq!(days[1]["scope_points"], 11);
    let second_days: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cycle_days WHERE cycle_id = ?")
        .bind(&second)
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(second_days, 1);
    // The same time again: nothing.
    let run = repository.run_cycles(at(after_first)).await.unwrap();
    assert_eq!((run.created, run.closed), (0, 0));

    // Downtime of many cycle lengths: every missed cycle closes, and the open work lands in the
    // cycle that is current then.
    let much_later = after_first + 70 * DAY;
    let run = repository.run_cycles(at(much_later)).await.unwrap();
    assert_eq!(run.failed, 0);
    assert!(run.closed >= 10, "{run:?}");
    let landed: (i64, i64, Option<i64>) = sqlx::query_as(
        "SELECT cycles.starts_at, cycles.ends_at, cycles.completed_at FROM tasks \
         JOIN cycles ON cycles.id = tasks.cycle_id WHERE tasks.id = ?",
    )
    .bind(id_of(&todo))
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    assert!(
        landed.0 <= much_later && much_later < landed.1,
        "{landed:?}"
    );
    assert_eq!(landed.2, None);
    let future: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM cycles WHERE project_id = ? AND starts_at > ?")
            .bind(&fixture.project_id)
            .bind(much_later)
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    assert_eq!(future, 2);
}

#[tokio::test]
async fn the_options_for_tasks_with_no_cycle_use_the_cooldown_rules() {
    let fixture = Fixture::new().await;
    let repository = fixture.repository();
    fixture
        .enable_cycles(json!({
            "cooldown_weeks": 1, "auto_add_started": true, "auto_add_completed": true,
            "active_without_cycle": "cycle"
        }))
        .await;
    let cycles = fixture.cycles().await;
    let first = cycles[0]["id"].as_str().unwrap().to_owned();
    let second = cycles[1]["id"].as_str().unwrap().to_owned();
    // One week of cooldown between two cycles.
    assert_eq!(
        millis(&cycles[1]["starts_at"]) - millis(&cycles[0]["ends_at"]),
        7 * DAY
    );
    let now = TimestampMillis::now().as_millis();

    // In a cycle: a task that starts, or is completed, with no cycle gets the current cycle.
    let started = fixture.create_task("started", "backlog", json!({})).await;
    assert_eq!(
        fixture.move_at(id_of(&started), "started", now).await["cycle_id"],
        first.as_str()
    );
    let completed = fixture.create_task("completed", "backlog", json!({})).await;
    assert_eq!(
        fixture.move_at(id_of(&completed), "completed", now).await["cycle_id"],
        first.as_str()
    );
    // A task that has a cycle keeps it.
    let planned = fixture
        .create_task("planned", "backlog", json!({"cycle_id": second}))
        .await;
    assert_eq!(
        fixture.move_at(id_of(&planned), "started", now).await["cycle_id"],
        second.as_str()
    );
    // Active work with no cycle: in the status path, and in the job for a task created with none.
    let activated = fixture.create_task("activated", "backlog", json!({})).await;
    assert_eq!(
        fixture.move_at(id_of(&activated), "unstarted", now).await["cycle_id"],
        first.as_str()
    );
    let created = fixture.create_task("created", "unstarted", json!({})).await;
    assert_eq!(created["cycle_id"], Value::Null);
    repository.run_cycles(at(now)).await.unwrap();
    assert_eq!(
        fixture.task(id_of(&created)).await["cycle_id"],
        first.as_str()
    );

    // In the cooldown there is no current cycle.
    let cooldown = millis(&cycles[0]["ends_at"]) + DAY;
    repository.run_cycles(at(cooldown)).await.unwrap();
    let (_, none) = fixture
        .call("GET", &fixture.uri("/cycles/current"), None)
        .await;
    // The route uses the real time, at which the first cycle was current before the job closed it.
    assert_eq!(none["items"].as_array().unwrap().len(), 0);
    // Started work goes to the next cycle; completed work to the cycle before.
    let late_start = fixture
        .create_task("late start", "backlog", json!({}))
        .await;
    assert_eq!(
        fixture
            .move_at(id_of(&late_start), "started", cooldown)
            .await["cycle_id"],
        second.as_str()
    );
    let late_done = fixture.create_task("late done", "backlog", json!({})).await;
    assert_eq!(
        fixture
            .move_at(id_of(&late_done), "completed", cooldown)
            .await["cycle_id"],
        first.as_str()
    );

    // The backlog option moves active work with no cycle to the backlog instead.
    let settings_version = 1;
    let (status, settings) = fixture
        .call(
            "PUT",
            &fixture.project_uri("/cycle-settings"),
            Some(json!({
                "expected_version": settings_version, "enabled": true, "weeks": 1,
                "start_weekday": weekday_today(), "cooldown_weeks": 1, "cycles_ahead": 2, "timezone": "UTC",
                "auto_add_started": false, "auto_add_completed": false, "active_without_cycle": "backlog"
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{settings}");
    let idle = fixture.create_task("idle", "started", json!({})).await;
    let idle = fixture.move_at(id_of(&idle), "unstarted", cooldown).await;
    assert_eq!(idle["cycle_id"], Value::Null);
    assert_eq!(fixture.category_of(id_of(&idle)).await, "backlog");
    let fresh = fixture.create_task("fresh", "unstarted", json!({})).await;
    repository.run_cycles(at(cooldown)).await.unwrap();
    assert_eq!(fixture.category_of(id_of(&fresh)).await, "backlog");
    // A stale settings version is refused.
    let (status, _) = fixture
        .call(
            "PUT",
            &fixture.project_uri("/cycle-settings"),
            Some(json!({
                "expected_version": settings_version, "enabled": true, "weeks": 1, "start_weekday": 1,
                "cooldown_weeks": 0, "cycles_ahead": 2, "timezone": "UTC", "auto_add_started": false,
                "auto_add_completed": false, "active_without_cycle": "off"
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn manual_cycle_actions_and_turning_cycles_off() {
    let fixture = Fixture::new().await;
    fixture.enable_cycles(json!({})).await;
    let cycles = fixture.cycles().await;
    let ids: Vec<String> = cycles.iter().map(|cycle| id_of(cycle).to_owned()).collect();
    let uri = |index: usize, suffix: &str| {
        fixture.project_uri(&format!("/cycles/{}{suffix}", ids[index]))
    };

    // Rename and describe; `null` returns to the default name.
    let (status, named) = fixture
        .call(
            "PATCH",
            &uri(0, ""),
            Some(json!({"expected_version": 0, "name": "Launch week", "description": "Ship it"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{named}");
    assert_eq!(named["name"], "Launch week");
    let (status, _) = fixture
        .call(
            "PATCH",
            &uri(0, ""),
            Some(json!({"expected_version": 0, "name": "stale"})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (_, unnamed) = fixture
        .call(
            "PATCH",
            &uri(0, ""),
            Some(json!({"expected_version": 1, "name": null})),
        )
        .await;
    assert_eq!(unnamed["name"], Value::Null);

    // The dates of a future cycle: no overlap with the cycle before it; the later ones move.
    let second_start = millis(&cycles[1]["starts_at"]);
    let second_end = millis(&cycles[1]["ends_at"]);
    let third_start = millis(&cycles[2]["starts_at"]);
    let (status, _) = fixture
        .call(
            "PATCH",
            &uri(1, ""),
            Some(json!({"expected_version": 0, "starts_at": iso(second_start - DAY)})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    // The current cycle cannot move.
    let (status, _) = fixture
        .call(
            "PATCH",
            &uri(0, ""),
            Some(json!({"expected_version": 2, "ends_at": iso(second_end)})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, longer) = fixture
        .call(
            "PATCH",
            &uri(1, ""),
            Some(json!({"expected_version": 0, "ends_at": iso(second_end + 2 * DAY)})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{longer}");
    let moved = fixture.cycles().await;
    assert_eq!(millis(&moved[2]["starts_at"]), third_start + 2 * DAY);

    // End the current cycle today: it ends before tomorrow 00:01 UTC; the next keeps its dates.
    let (status, _) = fixture.call("POST", &uri(1, "/end-today"), None).await;
    assert_eq!(status, StatusCode::CONFLICT, "only the current cycle");
    let (status, ended) = fixture.call("POST", &uri(0, "/end-today"), None).await;
    assert_eq!(status, StatusCode::OK, "{ended}");
    let now = TimestampMillis::now().as_millis();
    assert!(millis(&ended["ends_at"]) > now);
    assert!(millis(&ended["ends_at"]) <= now + DAY + 60_000);
    assert_eq!(
        millis(&fixture.cycles().await[1]["starts_at"]),
        second_start
    );

    // Start the next cycle today: the current one closes by the normal rules, the next one starts
    // now and keeps its length, and the later ones move with it.
    let open = fixture
        .create_task("open", "unstarted", json!({"cycle_id": ids[0]}))
        .await;
    let (status, _) = fixture.call("POST", &uri(2, "/start-today"), None).await;
    assert_eq!(status, StatusCode::CONFLICT, "only the next cycle");
    let (status, started) = fixture.call("POST", &uri(1, "/start-today"), None).await;
    assert_eq!(status, StatusCode::OK, "{started}");
    assert_eq!(started["state"], "current");
    assert_eq!(
        millis(&started["ends_at"]) - millis(&started["starts_at"]),
        second_end + 2 * DAY - second_start
    );
    let after = fixture.cycles().await;
    assert_eq!(after[0]["state"], "completed");
    assert_eq!(
        fixture.task(id_of(&open)).await["cycle_id"],
        ids[1].as_str()
    );
    assert_eq!(after[2]["starts_at"], started["ends_at"]);
    // A cycle that is created after the moved ones does not start before they end.
    for pair in after.windows(2) {
        assert!(
            millis(&pair[1]["starts_at"]) >= millis(&pair[0]["ends_at"]),
            "{after:?}"
        );
    }
    // A completed cycle does not change.
    let version = after[0]["version"].as_u64().unwrap();
    let (status, _) = fixture
        .call(
            "PATCH",
            &uri(0, ""),
            Some(json!({"expected_version": version, "name": "late"})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);

    // Cycles off: the current cycle is completed and its open tasks lose their cycle; the future
    // cycles are deleted; the completed ones stay.
    let future_task = fixture
        .create_task("future", "unstarted", json!({"cycle_id": id_of(&after[2])}))
        .await;
    let (status, off) = fixture
        .call(
            "PUT",
            &fixture.project_uri("/cycle-settings"),
            Some(json!({
                "expected_version": 1, "enabled": false, "weeks": 1, "start_weekday": weekday_today(),
                "cooldown_weeks": 0, "cycles_ahead": 2, "timezone": "UTC", "auto_add_started": false,
                "auto_add_completed": false, "active_without_cycle": "off"
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{off}");
    let left = fixture.cycles().await;
    assert_eq!(left.len(), 2);
    assert!(left.iter().all(|cycle| cycle["state"] == "completed"));
    assert_eq!(fixture.task(id_of(&open)).await["cycle_id"], Value::Null);
    assert_eq!(
        fixture.task(id_of(&future_task)).await["cycle_id"],
        Value::Null
    );
    // And the job leaves a project with cycles off alone.
    let run = fixture
        .repository()
        .run_cycles(at(now + 30 * DAY))
        .await
        .unwrap();
    assert_eq!((run.created, run.closed), (0, 0));
}
