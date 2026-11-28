//! Time tracking on tasks (#95): members log their own time, `time.manage` (owners and admins)
//! covers other members' time, and the task shows its own and its sub-issues' totals. A project
//! takes new time only while it shows the workspace's time tracking field. Everything goes
//! through HTTP, as a client sees it.

use orbit_platform::{
    AuthenticatedUser, Config, EnvironmentMode, Id, PasswordService, TimestampMillis,
};
use orbit_server::app::App;
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

const COOKIE: &str = "orbit_session_dev";

struct Server {
    base: String,
    database: orbit_platform::Database,
    shutdown: CancellationToken,
    task: tokio::task::JoinHandle<()>,
    http: reqwest::Client,
    _root: TempDir,
}

impl Server {
    async fn start() -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut config = Config {
            environment: EnvironmentMode::Development,
            app_key: Some(orbit_platform::AppKey::new([1; 32])),
            ..Config::default()
        };
        config.data.database = root.path().join("data/orbit.sqlite");
        config.data.attachments = root.path().join("data/attachments");
        config.data.backups = root.path().join("backups");
        config.rate_limits.general_per_minute = 100_000;
        let app = App::build(config, &crate::FRONTEND).await.unwrap();
        let database = app.database().clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let shutdown = CancellationToken::new();
        let token = shutdown.clone();
        let task = tokio::spawn(async move {
            app.serve(listener, token).await.unwrap();
        });
        Self {
            base,
            database,
            shutdown,
            task,
            http: reqwest::Client::new(),
            _root: root,
        }
    }

    async fn stop(self) {
        self.shutdown.cancel();
        self.task.await.unwrap();
    }

    async fn call(
        &self,
        session: &str,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> (u16, Value) {
        let mut request = self
            .http
            .request(method.parse().unwrap(), format!("{}{path}", self.base))
            .header("origin", &self.base)
            .header("cookie", format!("{COOKIE}={session}"));
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request.send().await.unwrap();
        let status = response.status().as_u16();
        let text = response.text().await.unwrap_or_default();
        (
            status,
            serde_json::from_str(&text).unwrap_or(Value::String(text)),
        )
    }

    /// A call that must succeed; returns its body.
    async fn ok(&self, session: &str, method: &str, path: &str, body: Value) -> Value {
        let body = (method != "GET").then_some(body);
        let (status, response) = self.call(session, method, path, body.as_ref()).await;
        assert!(
            (200..300).contains(&status),
            "{method} {path}: {status} {response}"
        );
        response
    }

    /// A user with a session and, unless `role` is `None`, a membership row.
    async fn person(&self, workspace: &str, name: &str, role: Option<&str>) -> (Id, String) {
        let id = Id::new_v7();
        let now = TimestampMillis::now().as_millis();
        let email = format!("{name}@example.com");
        sqlx::query(
            "INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, 'unused', $5, $6)",
        )
        .bind(id.to_string())
        .bind(&email)
        .bind(&email)
        .bind(name)
        .bind(now)
        .bind(now)
        .execute(self.database.pool())
        .await
        .unwrap();
        if let Some(role) = role {
            sqlx::query(
                "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at) \
                 VALUES ($1, $2, $3, $4, 0, $5, $6)",
            )
            .bind(Id::new_v7().to_string())
            .bind(workspace)
            .bind(id.to_string())
            .bind(role)
            .bind(now)
            .bind(now)
            .execute(self.database.pool())
            .await
            .unwrap();
        }
        let session = IdentityRepository::new(self.database.clone())
            .create_session(
                &AuthenticatedUser {
                    id,
                    email,
                    display_name: name.to_owned(),
                },
                TimestampMillis::now(),
            )
            .await
            .unwrap();
        (id, session.token)
    }

    /// Adds a time tracking field to the gallery and shows it in `project`; returns its id.
    async fn turn_on_time_tracking(&self, session: &str, base: &str, project: &str) -> String {
        let field = self
            .ok(
                session,
                "POST",
                &format!("{base}/task-fields"),
                json!({"name": "Time tracking", "kind": "time_tracking"}),
            )
            .await;
        let id = field["id"].as_str().unwrap().to_owned();
        self.ok(
            session,
            "PUT",
            &format!("{base}/projects/{project}/fields"),
            json!({"field_ids": [id]}),
        )
        .await;
        id
    }
}

/// A server with a set-up workspace and its admin: (workspace id, API base, the General
/// project, its first status, the admin's session).
async fn workspace(server: &Server) -> (String, String, String, String, String) {
    let identity = IdentityRepository::new(server.database.clone());
    let now = TimestampMillis::now();
    identity
        .store_setup_token(
            "time",
            TimestampMillis::from_millis(now.as_millis() + 60_000),
        )
        .await
        .unwrap();
    let setup = identity
        .complete_setup(
            SetupRequest {
                token: "time".to_owned(),
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
    let workspace = setup.workspace_id.to_string();
    let base = format!("/api/v1/workspaces/{workspace}");
    let (_, admin) = server.person(&workspace, "admin", Some("admin")).await;
    let projects = server
        .ok(&admin, "GET", &format!("{base}/projects"), Value::Null)
        .await;
    let project = projects["items"].get(0).unwrap_or(&projects[0])["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let statuses = server
        .ok(
            &admin,
            "GET",
            &format!("{base}/projects/{project}/statuses"),
            Value::Null,
        )
        .await;
    let status = statuses["items"].get(0).unwrap_or(&statuses[0])["id"]
        .as_str()
        .unwrap()
        .to_owned();
    (workspace, base, project, status, admin)
}

#[tokio::test]
async fn members_log_their_own_time_and_admins_manage_everyones() {
    let server = Server::start().await;
    let (workspace, base, project, status, admin) = workspace(&server).await;
    let (alice_id, alice) = server.person(&workspace, "alice", Some("member")).await;
    let (bob_id, bob) = server.person(&workspace, "bob", Some("member")).await;
    let (guest_id, _) = server.person(&workspace, "guest", Some("guest")).await;
    server.turn_on_time_tracking(&admin, &base, &project).await;
    let task = |parent: Option<String>| json!({"project_id": project, "status_id": status, "title": "Work", "parent_task_id": parent});
    let parent = server
        .ok(&alice, "POST", &format!("{base}/tasks"), task(None))
        .await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let child = server
        .ok(
            &alice,
            "POST",
            &format!("{base}/tasks"),
            task(Some(parent.clone())),
        )
        .await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let entries = |task: &str| format!("{base}/tasks/{task}/time-entries");

    // Alice logs her own time; the day defaults to today when left out.
    let own = server
        .ok(
            &alice,
            "POST",
            &entries(&parent),
            json!({"minutes": 90, "worked_on": "2026-11-10", "note": " Fixed the retry ", "billable": true}),
        )
        .await;
    assert_eq!(own["user_id"], alice_id.to_string());
    assert_eq!(own["note"], "Fixed the retry");
    assert_eq!(own["billable"], true);
    assert_eq!(own["can_edit"], true);
    let today = server
        .ok(&alice, "POST", &entries(&child), json!({"minutes": 30}))
        .await;
    assert_eq!(today["worked_on"].as_str().unwrap().len(), 10);

    // A member cannot log time for someone else, nor for a guest; an admin can for a member.
    let for_bob = json!({"minutes": 60, "worked_on": "2026-11-11", "user_id": bob_id.to_string()});
    assert_eq!(
        server
            .call(&alice, "POST", &entries(&parent), Some(&for_bob))
            .await
            .0,
        403
    );
    let for_guest =
        json!({"minutes": 60, "worked_on": "2026-11-11", "user_id": guest_id.to_string()});
    assert_eq!(
        server
            .call(&admin, "POST", &entries(&parent), Some(&for_guest))
            .await
            .0,
        422
    );
    let bobs = server.ok(&admin, "POST", &entries(&parent), for_bob).await;
    assert_eq!(bobs["user_id"], bob_id.to_string());

    // Invalid durations and days are refused.
    for body in [
        json!({"minutes": 0, "worked_on": "2026-11-10"}),
        json!({"minutes": 1441, "worked_on": "2026-11-10"}),
        json!({"minutes": 10, "worked_on": "2026-02-30"}),
    ] {
        assert_eq!(
            server
                .call(&alice, "POST", &entries(&parent), Some(&body))
                .await
                .0,
            422,
            "{body}"
        );
    }

    // The task shows its own total and its sub-issues' total; Alice cannot edit Bob's entry.
    let list = server
        .ok(&alice, "GET", &entries(&parent), Value::Null)
        .await;
    assert_eq!(list["total_minutes"], 150);
    assert_eq!(list["sub_issues_minutes"], 30);
    assert_eq!(list["items"].as_array().unwrap().len(), 2);
    let bob_entry = format!("{base}/time-entries/{}", bobs["id"].as_str().unwrap());
    assert_eq!(list["items"][1]["can_edit"], false);
    assert_eq!(
        server
            .call(&alice, "PATCH", &bob_entry, Some(&json!({"minutes": 5})))
            .await
            .0,
        403
    );
    assert_eq!(server.call(&alice, "DELETE", &bob_entry, None).await.0, 403);

    // Bob edits his own entry; a stale version conflicts; the owner of the entry deletes it.
    let edited = server
        .ok(
            &bob,
            "PATCH",
            &bob_entry,
            json!({"minutes": 45, "note": "Pairing", "expected_version": 0}),
        )
        .await;
    assert_eq!(edited["minutes"], 45);
    assert_eq!(edited["version"], 1);
    assert_eq!(
        server
            .call(
                &bob,
                "PATCH",
                &bob_entry,
                Some(&json!({"minutes": 50, "expected_version": 0}))
            )
            .await
            .0,
        409
    );
    server.ok(&bob, "DELETE", &bob_entry, Value::Null).await;
    let list = server
        .ok(&alice, "GET", &entries(&parent), Value::Null)
        .await;
    assert_eq!(list["total_minutes"], 90);

    // A trashed sub-issue no longer counts.
    let version = server
        .ok(&alice, "GET", &format!("{base}/tasks/{child}"), Value::Null)
        .await["version"]
        .as_u64()
        .unwrap();
    server
        .ok(
            &alice,
            "DELETE",
            &format!("{base}/tasks/{child}?expected_version={version}"),
            Value::Null,
        )
        .await;
    let list = server
        .ok(&alice, "GET", &entries(&parent), Value::Null)
        .await;
    assert_eq!(list["sub_issues_minutes"], 0);

    // Every write is in the audit log, without the note.
    let actions: Vec<String> = sqlx::query_scalar(
        "SELECT action FROM audit_events WHERE resource_type = 'time_entry' ORDER BY id",
    )
    .fetch_all(server.database.pool())
    .await
    .unwrap();
    assert_eq!(
        actions,
        [
            "time_entry.created",
            "time_entry.created",
            "time_entry.created",
            "time_entry.updated",
            "time_entry.deleted"
        ]
    );

    // `/spend` in the composer: the comment and Alice's time are one write, linked. A bad
    // duration writes neither.
    let comments = format!("{base}/tasks/{parent}/comments");
    let bad = json!({"body": "Nope", "time": {"minutes": 0}});
    assert_eq!(
        server.call(&alice, "POST", &comments, Some(&bad)).await.0,
        422
    );
    let comment = server
        .ok(
            &alice,
            "POST",
            &comments,
            json!({"body": "Paired on the retry", "time": {"minutes": 45, "worked_on": "2026-11-12"}}),
        )
        .await;
    let list = server
        .ok(&alice, "GET", &entries(&parent), Value::Null)
        .await;
    assert_eq!(list["total_minutes"], 135, "{list}");
    let spent = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["comment_id"] == comment["id"])
        .unwrap()
        .clone();
    assert_eq!(spent["minutes"], 45);
    assert_eq!(spent["worked_on"], "2026-11-12");
    assert_eq!(spent["note"], "Paired on the retry");
    assert_eq!(spent["user_id"], alice_id.to_string());
    let comment_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM task_comments WHERE body = 'Nope'")
            .fetch_one(server.database.pool())
            .await
            .unwrap();
    assert_eq!(comment_count, 0);

    // Deleting the comment keeps the logged time, without the link.
    server
        .ok(
            &alice,
            "DELETE",
            &format!(
                "{comments}/{}?expected_version=0",
                comment["id"].as_str().unwrap()
            ),
            Value::Null,
        )
        .await;
    let list = server
        .ok(&alice, "GET", &entries(&parent), Value::Null)
        .await;
    assert_eq!(list["total_minutes"], 135);
    let kept = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == spent["id"])
        .unwrap();
    assert_eq!(kept["comment_id"], Value::Null);
    assert_eq!(kept["note"], "Paired on the retry");
    server.stop().await;
}

#[tokio::test]
async fn time_tracking_is_a_field_the_project_shows() {
    let server = Server::start().await;
    let (_, base, project, status, admin) = workspace(&server).await;
    let task = server
        .ok(
            &admin,
            "POST",
            &format!("{base}/tasks"),
            json!({"project_id": project, "status_id": status, "title": "Work"}),
        )
        .await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let entries = format!("{base}/tasks/{task}/time-entries");
    let comments = format!("{base}/tasks/{task}/comments");
    let off = "Time tracking is off in this project. Add the Time tracking field to the project.";

    // Off: no new time, and `/spend` writes no comment either.
    let (status_code, body) = server
        .call(&admin, "POST", &entries, Some(&json!({"minutes": 30})))
        .await;
    assert_eq!((status_code, body["detail"].as_str()), (422, Some(off)));
    let (status_code, body) = server
        .call(
            &admin,
            "POST",
            &comments,
            Some(&json!({"body": "Spent", "time": {"minutes": 30}})),
        )
        .await;
    assert_eq!((status_code, body["detail"].as_str()), (422, Some(off)));
    let written: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM task_comments")
        .fetch_one(server.database.pool())
        .await
        .unwrap();
    assert_eq!(written, 0);

    // On: a workspace has one time tracking field, whatever its name.
    let field = server.turn_on_time_tracking(&admin, &base, &project).await;
    let (status_code, body) = server
        .call(
            &admin,
            "POST",
            &format!("{base}/task-fields"),
            Some(&json!({"name": "Hours", "kind": "time_tracking"})),
        )
        .await;
    assert_eq!(
        (status_code, body["detail"].as_str()),
        (
            422,
            Some("The workspace already has a Time tracking field.")
        )
    );
    server
        .ok(
            &admin,
            "POST",
            &entries,
            json!({"minutes": 90, "worked_on": "2026-11-10"}),
        )
        .await;
    server
        .ok(
            &admin,
            "POST",
            &comments,
            json!({"body": "Spent", "time": {"minutes": 30}}),
        )
        .await;

    // The task's value is its logged minutes, in the detail and in the list; nobody sets it.
    let detail = server
        .ok(&admin, "GET", &format!("{base}/tasks/{task}"), Value::Null)
        .await;
    assert_eq!(
        detail["field_values"],
        json!([{"field_id": field, "value": 120}])
    );
    let list = server
        .ok(
            &admin,
            "GET",
            &format!("{base}/tasks?project_id={project}"),
            Value::Null,
        )
        .await;
    let listed = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == task.as_str())
        .unwrap();
    assert_eq!(
        listed["field_values"],
        json!([{"field_id": field, "value": 120}])
    );
    let (status_code, body) = server
        .call(
            &admin,
            "PUT",
            &format!("{base}/tasks/{task}/fields/{field}"),
            Some(&json!({"value": 5})),
        )
        .await;
    assert_eq!(
        (status_code, body["detail"].as_str()),
        (
            422,
            Some(
                "A Time tracking field shows the time logged on the task. Log time instead of setting a value."
            )
        )
    );

    // Off again: logged time stays readable and editable, and the task still shows it.
    server
        .ok(
            &admin,
            "PUT",
            &format!("{base}/projects/{project}/fields"),
            json!({"field_ids": []}),
        )
        .await;
    assert_eq!(
        server
            .call(&admin, "POST", &entries, Some(&json!({"minutes": 30})))
            .await
            .0,
        422
    );
    let logged = server.ok(&admin, "GET", &entries, Value::Null).await;
    let first = logged["items"][0]["id"].as_str().unwrap();
    server
        .ok(
            &admin,
            "PATCH",
            &format!("{base}/time-entries/{first}"),
            json!({"minutes": 60}),
        )
        .await;
    let detail = server
        .ok(&admin, "GET", &format!("{base}/tasks/{task}"), Value::Null)
        .await;
    assert_eq!(
        detail["field_values"],
        json!([{"field_id": field, "value": 90}])
    );
    server.stop().await;
}
