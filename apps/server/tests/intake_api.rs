//! Backlog, triage, templates and recurring tasks (migration 0046).

use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{AuthenticatedUser, Id, PasswordService, TestDatabase, TimestampMillis};
use orbit_server::auth_routes::CookieMode;
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::tasks::{DiscordTask, GithubWorkItem, TaskRepository};
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
        let app = task_router(TaskState::new(Arc::clone(&identity), CookieMode::secure()));
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

impl Fixture {
    fn repository(&self) -> TaskRepository {
        TaskRepository::new((*self.database).clone())
    }

    async fn set_triage(&self, enabled: bool) -> (StatusCode, Value) {
        self.patch_project(json!({"triage_enabled": enabled})).await
    }

    async fn github_issue(&self, number: i64, state: &'static str) -> String {
        self.repository()
            .sync_github_work_item(
                self.workspace_id.parse().unwrap(),
                self.owner_id,
                GithubWorkItem {
                    repository: "acme/repo".to_owned(),
                    number,
                    project_id: self.project_id.parse().unwrap(),
                    title: format!("Issue {number}"),
                    description: String::new(),
                    kind: "issue",
                    state,
                    state_changed: false,
                },
                "github-test",
                TimestampMillis::now(),
            )
            .await
            .unwrap()
            .to_string()
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

    async fn count_titled(&self, title: &str) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM tasks WHERE title = ?")
            .bind(title)
            .fetch_one(self.database.pool())
            .await
            .unwrap()
    }

    async fn task(&self, task_id: &str) -> Value {
        let (status, task) = self
            .call("GET", &self.uri(&format!("/tasks/{task_id}")), None)
            .await;
        assert_eq!(status, StatusCode::OK, "{task}");
        task
    }
}

#[tokio::test]
async fn new_projects_get_a_backlog_and_a_triage_status_and_both_are_guarded() {
    let fixture = Fixture::new().await;
    let (_, statuses) = fixture
        .call("GET", &fixture.project_uri("/statuses"), None)
        .await;
    let categories: Vec<(&str, &str)> = statuses["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|status| {
            (
                status["name"].as_str().unwrap(),
                status["category"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        categories,
        [
            ("Backlog", "backlog"),
            ("Todo", "unstarted"),
            ("In Progress", "started"),
            ("Done", "completed"),
            ("Cancelled", "cancelled"),
            ("Triage", "triage"),
            ("Duplicate", "duplicate"),
        ]
    );
    let triage = statuses["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|status| status["category"] == "triage")
        .unwrap();
    let triage_uri = fixture.project_uri(&format!("/statuses/{}", id_of(triage)));

    // A second Triage status, a change of its category and its delete are refused.
    let (status, _) = fixture
        .call(
            "POST",
            &fixture.project_uri("/statuses"),
            Some(json!({"name": "Intake", "color": "#123456", "category": "triage"})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = fixture
        .call(
            "PATCH",
            &triage_uri,
            Some(json!({"name": "Triage", "color": "#123456", "category": "backlog", "position": 6, "expected_version": 0})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = fixture
        .call("DELETE", &format!("{triage_uri}?expected_version=0"), None)
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // `backlog` is a normal category for a new status.
    let (status, created) = fixture
        .call(
            "POST",
            &fixture.project_uri("/statuses"),
            Some(json!({"name": "Icebox", "color": "#123456", "category": "backlog"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");

    // A task in the backlog is open: it blocks, and it is not done in the progress.
    let milestone = fixture.create_milestone("v1").await;
    let blocker = fixture
        .create_task(
            "blocker",
            "backlog",
            json!({"milestone_id": id_of(&milestone)}),
        )
        .await;
    let blocked = fixture.create_task("blocked", "unstarted", json!({})).await;
    let (status, relation) = fixture
        .call(
            "POST",
            &fixture.uri(&format!("/tasks/{}/relations", id_of(&blocker))),
            Some(json!({"type": "blocks", "task_id": id_of(&blocked)})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{relation}");
    assert_eq!(fixture.task(id_of(&blocked)).await["blocked"], true);
    let (_, milestones) = fixture.call("GET", &fixture.uri("/milestones"), None).await;
    assert_eq!(milestones["items"][0]["task_count"], 1);
    assert_eq!(milestones["items"][0]["task_done_count"], 0);
    assert_eq!(fixture.project().await["task_counts"]["backlog"], 1);
}

#[tokio::test]
async fn integration_tasks_enter_triage_only_while_the_project_has_it_on() {
    let fixture = Fixture::new().await;
    assert_eq!(fixture.project().await["triage_enabled"], false);

    // Off: the normal status.
    let before = fixture.github_issue(1, "open").await;
    assert_ne!(fixture.category_of(&before).await, "triage");

    let (status, project) = fixture.set_triage(true).await;
    assert_eq!(status, StatusCode::OK, "{project}");
    assert_eq!(project["triage_enabled"], true);

    // On: open work from GitHub and Discord waits in triage; a closed issue does not.
    let open = fixture.github_issue(2, "open").await;
    assert_eq!(fixture.category_of(&open).await, "triage");
    let closed = fixture.github_issue(3, "closed").await;
    assert_eq!(fixture.category_of(&closed).await, "completed");
    let (discord, _) = fixture
        .repository()
        .create_discord_task(
            fixture.workspace_id.parse().unwrap(),
            fixture.owner_id,
            None,
            DiscordTask {
                source_url: "https://discord.com/channels/1/2/3".to_owned(),
                event_id: "event-1".to_owned(),
                payload_hash: [7; 32],
                project_id: fixture.project_id.parse().unwrap(),
                title: "From Discord".to_owned(),
                description: String::new(),
            },
            "discord-test",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    assert_eq!(fixture.category_of(&discord.id.to_string()).await, "triage");

    // A member's task starts in the status the member selects, and the member can select Triage.
    let own = fixture.create_task("Mine", "unstarted", json!({})).await;
    assert_eq!(fixture.category_of(id_of(&own)).await, "unstarted");
    let chosen = fixture.create_task("Unsure", "triage", json!({})).await;
    assert_eq!(fixture.category_of(id_of(&chosen)).await, "triage");

    // Each task that enters triage notifies the project lead and members, and not its actor.
    // Before the project had a lead or members, nobody was notified.
    let triage_rows = || async {
        sqlx::query_as::<_, (String, String)>(
            "SELECT users.email, tasks.title FROM notifications \
             JOIN users ON users.id = notifications.recipient_user_id \
             JOIN tasks ON tasks.id = notifications.task_id \
             WHERE notifications.kind = 'task_triage_new' ORDER BY tasks.title, users.email",
        )
        .fetch_all(fixture.database.pool())
        .await
        .unwrap()
    };
    assert!(triage_rows().await.is_empty());
    let (member, _) = fixture.add_member("member@example.com", "member").await;
    fixture.add_member("outside@example.com", "member").await;
    let (status, _) = fixture
        .patch_project(json!({"lead_user_id": fixture.owner_id, "member_ids": [member]}))
        .await;
    assert_eq!(status, StatusCode::OK);
    let notified = fixture.github_issue(9, "open").await;
    assert_eq!(fixture.category_of(&notified).await, "triage");
    // The owner creates one in Triage: the owner is the actor, so only the member is notified.
    fixture
        .create_task("By the lead", "triage", json!({}))
        .await;
    let pairs = triage_rows().await;
    let pairs: Vec<(&str, &str)> = pairs
        .iter()
        .map(|row| (row.0.as_str(), row.1.as_str()))
        .collect();
    assert_eq!(
        pairs,
        [
            ("member@example.com", "By the lead"),
            ("member@example.com", "Issue 9"),
            ("owner@example.com", "Issue 9"),
        ]
    );
    let (status, _) = fixture
        .call(
            "POST",
            &fixture.uri(&format!("/tasks/{notified}/triage")),
            Some(json!({"expected_version": 0, "action": "accept"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    // Default queries leave triage out; the explicit filter shows only it.
    let all = fixture.query(json!([])).await;
    assert_eq!(all, ["Issue 1", "Issue 3", "Mine", "Issue 9"]);
    let queue = fixture
        .query(json!([{"field": "status_category", "operator": "is", "value": ["triage"]}]))
        .await;
    assert_eq!(queue, ["Issue 2", "From Discord", "Unsure", "By the lead"]);
    let (_, legacy) = fixture.call("GET", &fixture.uri("/tasks"), None).await;
    assert_eq!(legacy["items"].as_array().unwrap().len(), 4);
    // A task in triage is still a normal task: it opens and it can be edited.
    let (status, edited) = fixture
        .call(
            "PATCH",
            &fixture.uri(&format!("/tasks/{open}")),
            Some(json!({"expected_version": 0, "priority": "high"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{edited}");

    // The switch cannot go off while the queue has tasks; the answer has the count.
    let (status, problem) = fixture.set_triage(false).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(problem["code"], "triage_not_empty");
    assert_eq!(problem["conflict"]["count"], 4);
    assert_eq!(fixture.project().await["triage_enabled"], true);
}

#[tokio::test]
async fn triage_accept_and_decline_are_status_changes() {
    let fixture = Fixture::new().await;
    fixture.set_triage(true).await;
    let accepted = fixture.github_issue(1, "open").await;
    let chosen = fixture.github_issue(2, "open").await;
    let declined = fixture.github_issue(3, "open").await;
    let triage = |task_id: &str, body: Value| {
        let uri = fixture.uri(&format!("/tasks/{task_id}/triage"));
        let fixture = &fixture;
        async move { fixture.call("POST", &uri, Some(body)).await }
    };

    // Accept with no status: the first backlog status.
    let (status, task) = triage(
        &accepted,
        json!({"expected_version": 0, "action": "accept"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{task}");
    assert_eq!(fixture.category_of(&accepted).await, "backlog");

    // Accept with a status; never back to Triage or to Duplicate.
    let started = fixture.status_id(&fixture.project_id, "started").await;
    let duplicate = fixture.status_id(&fixture.project_id, "duplicate").await;
    let (status, _) = triage(
        &chosen,
        json!({"expected_version": 0, "action": "accept", "status_id": duplicate}),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = triage(
        &chosen,
        json!({"expected_version": 0, "action": "accept", "status_id": started}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fixture.category_of(&chosen).await, "started");

    // Decline: the first cancelled status, with the comment.
    let (status, _) = triage(
        &declined,
        json!({"expected_version": 0, "action": "decline", "comment": "Not a bug"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fixture.category_of(&declined).await, "cancelled");
    let (_, comments) = fixture
        .call(
            "GET",
            &fixture.uri(&format!("/tasks/{declined}/comments")),
            None,
        )
        .await;
    assert_eq!(comments["items"][0]["body"], "Not a bug");

    // A task that is not in triage, a stale version and an unknown action are refused.
    let (status, _) = triage(
        &accepted,
        json!({"expected_version": 1, "action": "accept"}),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let stale = fixture.github_issue(4, "open").await;
    let (status, _) = triage(&stale, json!({"expected_version": 9, "action": "accept"})).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = triage(&stale, json!({"expected_version": 0, "action": "snooze"})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // With the queue empty, the switch goes off.
    triage(&stale, json!({"expected_version": 0, "action": "accept"})).await;
    let (status, project) = fixture.set_triage(false).await;
    assert_eq!(status, StatusCode::OK, "{project}");
    assert_eq!(project["triage_enabled"], false);
}

#[tokio::test]
async fn templates_store_a_payload_and_a_task_from_a_payload_is_all_or_nothing() {
    let fixture = Fixture::new().await;
    let (member, _) = fixture.add_member("member@example.com", "member").await;
    let milestone = fixture.create_milestone("v1").await;
    let started = fixture.status_id(&fixture.project_id, "started").await;
    let (_, label) = fixture
        .call(
            "POST",
            &fixture.uri("/labels"),
            Some(json!({"name": "release", "color": "#123456"})),
        )
        .await;
    let gone = Id::new_v7();
    let payload = json!({
        "title": "Release",
        "description": "Ship it",
        "status_id": started,
        "priority": "high",
        "label_ids": [id_of(&label), gone],
        "assignee_ids": [member, gone],
        "milestone_id": id_of(&milestone),
        "due_offset_days": 3,
        "sub_issues": [{"title": "Changelog"}, {"title": "Tag", "status_id": gone}],
    });
    let templates = fixture.project_uri("/templates");

    // Shape errors are refused: a second level of sub-issues, a bad priority.
    for bad in [
        json!({"title": "x", "sub_issues": [{"title": "y", "sub_issues": [{"title": "z"}]}]}),
        json!({"title": "x", "priority": "soon"}),
        json!({"title": " "}),
    ] {
        let (status, _) = fixture
            .call(
                "POST",
                &templates,
                Some(json!({"name": "Bad", "payload": bad})),
            )
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    }

    let (status, template) = fixture
        .call(
            "POST",
            &templates,
            Some(json!({"name": "Release", "payload": payload})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{template}");
    assert_eq!(template["payload"]["sub_issues"][0]["title"], "Changelog");
    // The name is unique in the project.
    let (status, _) = fixture
        .call(
            "POST",
            &templates,
            Some(json!({"name": "Release", "payload": {"title": "x"}})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let template_uri = format!("{templates}/{}", id_of(&template));
    let (status, renamed) = fixture
        .call(
            "PATCH",
            &template_uri,
            Some(json!({"expected_version": 0, "name": "Release train", "payload": payload})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{renamed}");
    let (_, list) = fixture.call("GET", &templates, None).await;
    assert_eq!(list["items"][0]["name"], "Release train");
    assert_eq!(list["items"][0]["version"], 1);

    // Use: references that no longer exist are dropped; a missing status becomes the default.
    let before = TimestampMillis::now().as_millis();
    let (status, task) = fixture
        .call(
            "POST",
            &fixture.uri("/tasks/from-payload"),
            Some(json!({"project_id": fixture.project_id, "payload": payload})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{task}");
    assert_eq!(task["title"], "Release");
    assert_eq!(task["status_id"], started.as_str());
    assert_eq!(task["priority"], "high");
    assert_eq!(task["label_ids"], json!([id_of(&label)]));
    assert_eq!(task["assignee_ids"], json!([member]));
    assert_eq!(task["milestone_id"], milestone["id"]);
    assert_eq!(task["sub_issue_count"], 2);
    let due: i64 = sqlx::query_scalar("SELECT due_at FROM tasks WHERE id = ?")
        .bind(id_of(&task))
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert!((due - before - 3 * DAY).abs() < 60_000, "{due}");
    let children: Vec<(String, String)> = sqlx::query_as(
        "SELECT tasks.title, task_statuses.category FROM tasks JOIN task_statuses ON task_statuses.id = tasks.status_id \
         WHERE tasks.parent_task_id = ? ORDER BY tasks.title",
    )
    .bind(id_of(&task))
    .fetch_all(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(
        children,
        [
            ("Changelog".to_owned(), "unstarted".to_owned()),
            ("Tag".to_owned(), "unstarted".to_owned()),
        ]
    );

    // A failure in a sub-issue leaves no task: the database refuses the second insert.
    sqlx::query(
        "CREATE TRIGGER fail_second_sub_issue BEFORE INSERT ON tasks WHEN NEW.title = 'Boom' \
         BEGIN SELECT RAISE(ABORT, 'boom'); END",
    )
    .execute(fixture.database.pool())
    .await
    .unwrap();
    let count = || async {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM tasks")
            .fetch_one(fixture.database.pool())
            .await
            .unwrap()
    };
    let tasks_before = count().await;
    let (status, _) = fixture
        .call(
            "POST",
            &fixture.uri("/tasks/from-payload"),
            Some(json!({
                "project_id": fixture.project_id,
                "payload": {"title": "Parent", "sub_issues": [{"title": "Fine"}, {"title": "Boom"}]}
            })),
        )
        .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(count().await, tasks_before);

    let (status, _) = fixture
        .call(
            "DELETE",
            &format!("{template_uri}?expected_version=1"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn recurring_tasks_run_on_a_schedule_without_drift_or_replay() {
    let fixture = Fixture::new().await;
    let repository = fixture.repository();
    let start = TimestampMillis::now().as_millis();
    let at = |millis: i64| TimestampMillis::from_millis(millis);
    let recurring = fixture.project_uri("/recurring-tasks");

    for bad in [
        json!({"payload": {"title": "x"}, "mode": "hourly", "every_count": 1, "every_unit": "day"}),
        json!({"payload": {"title": "x"}, "mode": "schedule", "every_count": 0, "every_unit": "day"}),
        json!({"payload": {"title": "x"}, "mode": "schedule", "every_count": 1, "every_unit": "year"}),
    ] {
        let (status, _) = fixture.call("POST", &recurring, Some(bad)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    }

    let (status, routine) = fixture
        .call(
            "POST",
            &recurring,
            Some(json!({
                "payload": {"title": "Weekly review", "sub_issues": [{"title": "Notes"}]},
                "mode": "schedule", "every_count": 1, "every_unit": "week",
                "starts_at": iso(start + DAY),
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{routine}");
    assert_eq!(routine["next_run_at"], iso(start + DAY));
    let routine_uri = format!("{recurring}/{}", id_of(&routine));
    let routine_now = || async {
        let (_, list) = fixture.call("GET", &recurring, None).await;
        list["items"][0].clone()
    };

    // Not due: nothing.
    assert_eq!(
        repository
            .run_recurring_tasks(at(start))
            .await
            .unwrap()
            .created,
        0
    );
    assert_eq!(fixture.count_titled("Weekly review").await, 0);

    // Due, and run two hours late: one task with its sub-issue, made by the service account; the
    // next run stays on the anchor's time.
    let late = start + DAY + 2 * 60 * 60 * 1_000;
    assert_eq!(
        repository
            .run_recurring_tasks(at(late))
            .await
            .unwrap()
            .created,
        1
    );
    assert_eq!(fixture.count_titled("Weekly review").await, 1);
    assert_eq!(fixture.count_titled("Notes").await, 1);
    let current = routine_now().await;
    assert_eq!(current["next_run_at"], iso(start + 8 * DAY));
    let task = fixture
        .task(current["last_task_id"].as_str().unwrap())
        .await;
    assert_eq!(task["creator_service_account_name"], "Recurring");
    assert_eq!(task["sub_issue_count"], 1);
    // The same minute again: nothing.
    assert_eq!(
        repository
            .run_recurring_tasks(at(late))
            .await
            .unwrap()
            .created,
        0
    );

    // After four weeks of downtime: one task, not four, although the last one is still open.
    let after_downtime = start + 30 * DAY;
    assert_eq!(
        repository
            .run_recurring_tasks(at(after_downtime))
            .await
            .unwrap()
            .created,
        1
    );
    assert_eq!(fixture.count_titled("Weekly review").await, 2);
    assert_eq!(routine_now().await["next_run_at"], iso(start + 36 * DAY));

    // Paused: nothing; resumed: it skips what it missed.
    let version = routine_now().await["version"].as_u64().unwrap();
    let (status, paused) = fixture
        .call(
            "PATCH",
            &routine_uri,
            Some(json!({"expected_version": version, "paused": true})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{paused}");
    assert_eq!(
        repository
            .run_recurring_tasks(at(start + 40 * DAY))
            .await
            .unwrap()
            .created,
        0
    );
    let (_, resumed) = fixture
        .call(
            "PATCH",
            &routine_uri,
            Some(json!({"expected_version": version + 1, "paused": false})),
        )
        .await;
    assert_eq!(resumed["paused"], false);
    assert!(resumed["next_run_at"].is_string());

    // A project in the trash stops its routines.
    let project_version = fixture.project().await["version"].as_u64().unwrap();
    fixture
        .call(
            "DELETE",
            &fixture.project_uri(&format!("?expected_version={project_version}")),
            None,
        )
        .await;
    assert_eq!(
        repository
            .run_recurring_tasks(at(start + 400 * DAY))
            .await
            .unwrap()
            .created,
        0
    );
    assert_eq!(fixture.count_titled("Weekly review").await, 2);
}

#[tokio::test]
async fn an_after_completion_routine_waits_for_its_last_task() {
    let fixture = Fixture::new().await;
    let repository = fixture.repository();
    let start = TimestampMillis::now().as_millis();
    let at = |millis: i64| TimestampMillis::from_millis(millis);
    let recurring = fixture.project_uri("/recurring-tasks");
    let (status, routine) = fixture
        .call(
            "POST",
            &recurring,
            Some(json!({
                "payload": {"title": "Rotate keys"},
                "mode": "after_completion", "every_count": 2, "every_unit": "day",
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{routine}");
    let routine_now = || async {
        let (_, list) = fixture.call("GET", &recurring, None).await;
        list["items"][0].clone()
    };
    let count = || async {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM tasks WHERE title = 'Rotate keys'")
            .fetch_one(fixture.database.pool())
            .await
            .unwrap()
    };

    // The first task is created at once; then the routine waits while that task is open.
    assert_eq!(
        repository
            .run_recurring_tasks(at(start + 1_000))
            .await
            .unwrap()
            .created,
        1
    );
    let first = routine_now().await;
    assert_eq!(first["next_run_at"], Value::Null);
    assert_eq!(
        repository
            .run_recurring_tasks(at(start + 90 * DAY))
            .await
            .unwrap()
            .created,
        0
    );
    assert_eq!(count().await, 1);

    // Completion starts the interval, from the completion time.
    let task_id = first["last_task_id"].as_str().unwrap().to_owned();
    let done = fixture.status_id(&fixture.project_id, "completed").await;
    let (status, closed) = fixture
        .call(
            "PATCH",
            &fixture.uri(&format!("/tasks/{task_id}")),
            Some(json!({"expected_version": 0, "status_id": done})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{closed}");
    let completed_at: i64 = sqlx::query_scalar("SELECT completed_at FROM tasks WHERE id = ?")
        .bind(&task_id)
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(
        repository
            .run_recurring_tasks(at(completed_at + DAY))
            .await
            .unwrap()
            .created,
        0
    );
    assert_eq!(
        routine_now().await["next_run_at"],
        iso(completed_at + 2 * DAY)
    );
    assert_eq!(
        repository
            .run_recurring_tasks(at(completed_at + 2 * DAY))
            .await
            .unwrap()
            .created,
        1
    );
    assert_eq!(count().await, 2);

    // A task that goes to the trash continues the routine in the same way.
    let second = routine_now().await;
    assert_eq!(second["next_run_at"], Value::Null);
    let second_id = second["last_task_id"].as_str().unwrap().to_owned();
    let (status, _) = fixture
        .call(
            "DELETE",
            &fixture.uri(&format!("/tasks/{second_id}?expected_version=0")),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    repository
        .run_recurring_tasks(TimestampMillis::now())
        .await
        .unwrap();
    assert!(routine_now().await["next_run_at"].is_string());

    let version = routine_now().await["version"].as_u64().unwrap();
    let (status, _) = fixture
        .call(
            "DELETE",
            &format!("{recurring}/{}?expected_version={version}", id_of(&routine)),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn a_task_from_a_payload_has_one_label_of_a_label_group() {
    let fixture = Fixture::new().await;
    let (status, group) = fixture
        .call(
            "POST",
            &fixture.uri("/label-groups"),
            Some(json!({"name": "Type", "color": "#112233"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{group}");
    let mut labels = Vec::new();
    for (name, grouped) in [("Bug", true), ("Feature", true), ("Urgent", false)] {
        let (status, label) = fixture
            .call(
                "POST",
                &fixture.uri("/labels"),
                Some(json!({"name": name, "color": "#445566",
                    "group_id": grouped.then(|| id_of(&group).to_owned())})),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{label}");
        labels.push(id_of(&label).to_owned());
    }
    // The payload names both labels of the group: the last one stays, as in a task edit.
    let (status, task) = fixture
        .call(
            "POST",
            &fixture.uri("/tasks/from-payload"),
            Some(json!({"project_id": fixture.project_id,
                "payload": {"title": "From a template", "label_ids": labels}})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{task}");
    let mut stored: Vec<String> =
        sqlx::query_scalar("SELECT label_id FROM task_labels WHERE task_id = ?")
            .bind(id_of(&task))
            .fetch_all(fixture.database.pool())
            .await
            .unwrap();
    stored.sort();
    let mut expected = vec![labels[1].clone(), labels[2].clone()];
    expected.sort();
    assert_eq!(stored, expected);
}

#[tokio::test]
async fn the_search_leaves_out_tasks_in_triage() {
    let fixture = Fixture::new().await;
    fixture.set_triage(true).await;
    let waiting = fixture.github_issue(1, "open").await;
    assert_eq!(fixture.category_of(&waiting).await, "triage");
    let search = async || {
        let (status, found) = fixture
            .call("GET", &fixture.uri("/tasks/search?q=issue"), None)
            .await;
        assert_eq!(status, StatusCode::OK, "{found}");
        found["items"].as_array().unwrap().len()
    };
    // The search follows the default visibility rule: a task in triage is not found.
    assert_eq!(search().await, 0);
    let (status, task) = fixture
        .call(
            "POST",
            &fixture.uri(&format!("/tasks/{waiting}/triage")),
            Some(json!({"expected_version": 0, "action": "accept"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{task}");
    assert_eq!(search().await, 1);
}
