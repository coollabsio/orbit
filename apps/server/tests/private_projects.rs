//! Private projects (#37): a project that is private answers like a missing one to every member
//! who is neither on its member list nor a workspace owner or admin. Everything goes through
//! HTTP, as a client sees it.

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
        let app = App::build(config).await.unwrap();
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
}

impl Server {
    async fn status(&self, session: &str, method: &str, path: &str, body: Value) -> u16 {
        let body = (method != "GET" && method != "DELETE").then_some(body);
        self.call(session, method, path, body.as_ref()).await.0
    }

    async fn get(&self, session: &str, path: &str) -> Value {
        self.ok(session, "GET", path, Value::Null).await
    }

    async fn bearer(&self, token: &str, path: &str) -> (u16, Value) {
        let response = self
            .http
            .get(format!("{}{path}", self.base))
            .header("authorization", format!("Bearer {token}"))
            .send()
            .await
            .unwrap();
        let status = response.status().as_u16();
        (status, response.json().await.unwrap_or(Value::Null))
    }
}

fn id(record: &Value) -> String {
    record["id"].as_str().unwrap().to_owned()
}

/// The `field` of every item of a page (`{items: […]}`) or a plain list.
fn column(list: &Value, field: &str) -> Vec<String> {
    list.get("items")
        .unwrap_or(list)
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item[field].as_str().unwrap().to_owned())
        .collect()
}

/// A workspace with a public project "General" and a private project "Secret" (key `SEC`) that
/// Alice created. Bob is a member of the workspace only.
struct World {
    server: Server,
    base: String,
    workspace: String,
    owner: String,
    admin: String,
    admin_id: Id,
    alice: String,
    alice_id: Id,
    bob: String,
    bob_id: Id,
    guest_id: Id,
    general: String,
    general_status: String,
    secret: Value,
    secret_status: String,
    /// `SEC-1`, with one comment.
    hidden: String,
}

impl World {
    async fn new() -> Self {
        let server = Server::start().await;
        let identity = IdentityRepository::new(server.database.clone());
        let now = TimestampMillis::now();
        identity
            .store_setup_token(
                "private-projects",
                TimestampMillis::from_millis(now.as_millis() + 60_000),
            )
            .await
            .unwrap();
        let setup = identity
            .complete_setup(
                SetupRequest {
                    token: "private-projects".to_owned(),
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
        let owner = setup.session.token.clone();
        let workspace = setup.workspace_id.to_string();
        let base = format!("/api/v1/workspaces/{workspace}");
        let (admin_id, admin) = server.person(&workspace, "admin", Some("admin")).await;
        let (alice_id, alice) = server.person(&workspace, "alice", Some("member")).await;
        let (bob_id, bob) = server.person(&workspace, "bob", Some("member")).await;
        let (guest_id, _) = server.person(&workspace, "guest", Some("guest")).await;

        let general =
            column(&server.get(&owner, &format!("{base}/projects")).await, "id").remove(0);
        let general_status = column(
            &server
                .get(&owner, &format!("{base}/projects/{general}/statuses"))
                .await,
            "id",
        )
        .remove(0);
        let secret = server
            .ok(
                &alice,
                "POST",
                &format!("{base}/projects"),
                json!({"name": "Secret", "key": "SEC", "color": "#336699", "private": true}),
            )
            .await;
        let secret_id = id(&secret);
        let secret_status = column(
            &server
                .get(&alice, &format!("{base}/projects/{secret_id}/statuses"))
                .await,
            "id",
        )
        .remove(0);
        let mut world = Self {
            server,
            base,
            workspace,
            owner,
            admin,
            admin_id,
            alice,
            alice_id,
            bob,
            bob_id,
            guest_id,
            general,
            general_status,
            secret,
            secret_status,
            hidden: String::new(),
        };
        world.hidden = world
            .task(&world.alice, true, json!({"title": "Hidden"}))
            .await;
        let hidden = world.hidden.clone();
        world
            .server
            .ok(
                &world.alice,
                "POST",
                &world.path(&format!("/tasks/{hidden}/comments")),
                json!({"body": "between us"}),
            )
            .await;
        world
    }

    fn path(&self, suffix: &str) -> String {
        format!("{}{suffix}", self.base)
    }

    fn secret_id(&self) -> String {
        id(&self.secret)
    }

    /// Creates a task in Secret (`private`) or General; `fields` adds to the body.
    async fn task(&self, session: &str, private: bool, fields: Value) -> String {
        let mut body = if private {
            json!({"project_id": self.secret_id(), "status_id": self.secret_status})
        } else {
            json!({"project_id": self.general, "status_id": self.general_status})
        };
        body.as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        id(&self
            .server
            .ok(session, "POST", &self.path("/tasks"), body)
            .await)
    }

    async fn get_task(&self, session: &str, task: &str) -> Value {
        self.server
            .get(session, &self.path(&format!("/tasks/{task}")))
            .await
    }

    async fn add_member(&self, session: &str, user: Id) -> (u16, Value) {
        self.server
            .call(
                session,
                "POST",
                &self.path(&format!("/projects/{}/members", self.secret_id())),
                Some(&json!({"user_id": user})),
            )
            .await
    }
}

#[tokio::test]
async fn a_private_project_answers_like_a_missing_one_to_other_members() {
    let world = World::new().await;
    let (server, bob) = (&world.server, world.bob.as_str());
    let secret = world.secret_id();
    let hidden = world.hidden.as_str();
    assert_eq!(world.secret["private"], json!(true));
    server
        .ok(
            &world.alice,
            "POST",
            &world.path(&format!("/projects/{secret}/labels")),
            json!({"name": "hush", "color": "#336699"}),
        )
        .await;

    for (method, suffix, body) in [
        ("GET", format!("/projects/{secret}/statuses"), Value::Null),
        ("GET", format!("/projects/{secret}/labels"), Value::Null),
        ("GET", format!("/projects/{secret}/members"), Value::Null),
        (
            "POST",
            format!("/projects/{secret}/members"),
            json!({"user_id": world.bob_id}),
        ),
        (
            "DELETE",
            format!("/projects/{secret}/members/{}", world.alice_id),
            Value::Null,
        ),
        (
            "PATCH",
            format!("/projects/{secret}"),
            json!({"name": "Mine", "key": "SEC", "color": "#336699", "private": false, "expected_version": 0}),
        ),
        (
            "DELETE",
            format!("/projects/{secret}?expected_version=0"),
            Value::Null,
        ),
        (
            "POST",
            format!("/projects/{secret}/statuses"),
            json!({"name": "Mine", "color": "#336699", "category": "unstarted"}),
        ),
        (
            "POST",
            format!("/projects/{secret}/labels"),
            json!({"name": "mine", "color": "#336699"}),
        ),
        ("GET", format!("/tasks/{hidden}"), Value::Null),
        ("GET", "/tasks/SEC-1".to_owned(), Value::Null),
        ("GET", format!("/tasks/{hidden}/comments"), Value::Null),
        (
            "POST",
            format!("/tasks/{hidden}/comments"),
            json!({"body": "let me in"}),
        ),
        ("GET", format!("/tasks/{hidden}/attachments"), Value::Null),
        ("GET", format!("/tasks/{hidden}/activity"), Value::Null),
        ("GET", format!("/tasks/{hidden}/relations"), Value::Null),
        ("GET", format!("/tasks/{hidden}/github-links"), Value::Null),
        (
            "PATCH",
            format!("/tasks/{hidden}"),
            json!({"title": "Mine", "expected_version": 0}),
        ),
        (
            "DELETE",
            format!("/tasks/{hidden}?expected_version=0"),
            Value::Null,
        ),
        (
            "POST",
            "/tasks".to_owned(),
            json!({"project_id": secret, "status_id": world.secret_status, "title": "Mine"}),
        ),
    ] {
        assert_eq!(
            server.status(bob, method, &world.path(&suffix), body).await,
            404,
            "{method} {suffix}"
        );
    }

    let projects = server.get(bob, &world.path("/projects")).await;
    assert_eq!(column(&projects, "name"), ["General"]);
    assert!(column(&server.get(bob, &world.path("/tasks")).await, "title").is_empty());
    assert!(column(&server.get(bob, &world.path("/labels")).await, "name").is_empty());

    // The same requests answer for a project member, an admin and the owner.
    for session in [&world.alice, &world.admin, &world.owner] {
        assert_eq!(world.get_task(session, "SEC-1").await["title"], "Hidden");
        assert_eq!(
            column(&server.get(session, &world.path("/projects")).await, "name"),
            ["General", "Secret"]
        );
        assert_eq!(
            column(&server.get(session, &world.path("/tasks")).await, "title"),
            ["Hidden"]
        );
        assert_eq!(
            column(&server.get(session, &world.path("/labels")).await, "name"),
            ["hush"]
        );
        assert_eq!(
            column(
                &server
                    .get(session, &world.path(&format!("/tasks/{hidden}/comments")))
                    .await,
                "body"
            ),
            ["between us"]
        );
    }
    world.server.stop().await;
}

#[tokio::test]
async fn a_visible_task_hides_what_it_says_about_private_tasks() {
    let world = World::new().await;
    let (server, alice, bob) = (&world.server, world.alice.as_str(), world.bob.as_str());
    let hidden = world.hidden.as_str();
    // Open tasks that point into the private project: a child of, a duplicate of and a task
    // related to the hidden task, and a parent with one hidden and one open sub-issue.
    let child = world
        .task(
            alice,
            false,
            json!({"title": "Child", "parent_task_id": hidden}),
        )
        .await;
    let duplicate = world
        .task(alice, false, json!({"title": "Duplicate"}))
        .await;
    server
        .ok(
            alice,
            "PATCH",
            &world.path(&format!("/tasks/{duplicate}")),
            json!({"duplicate_of_id": hidden, "expected_version": 0}),
        )
        .await;
    let related = world.task(alice, false, json!({"title": "Related"})).await;
    server
        .ok(
            alice,
            "POST",
            &world.path(&format!("/tasks/{related}/relations")),
            json!({"type": "related", "task_id": hidden}),
        )
        .await;
    let parent = world.task(alice, false, json!({"title": "Parent"})).await;
    world
        .task(
            alice,
            true,
            json!({"title": "Hidden sub", "parent_task_id": parent}),
        )
        .await;
    world
        .task(
            alice,
            false,
            json!({"title": "Open sub", "parent_task_id": parent}),
        )
        .await;

    let seen = world.get_task(alice, &child).await;
    assert_eq!(seen["parent"]["title"], "Hidden");
    assert_eq!(column(&seen["ancestors"], "title"), ["Hidden"]);
    assert_eq!(
        world.get_task(alice, &duplicate).await["duplicate_of"]["title"],
        "Hidden"
    );
    assert_eq!(world.get_task(alice, &parent).await["sub_issue_count"], 2);
    let relations = server
        .get(alice, &world.path(&format!("/tasks/{related}/relations")))
        .await;
    assert_eq!(relations.as_array().unwrap().len(), 1);

    let seen = world.get_task(bob, &child).await;
    assert_eq!(seen["parent"], Value::Null);
    assert_eq!(seen["ancestors"], json!([]));
    assert_eq!(
        world.get_task(bob, &duplicate).await["duplicate_of"],
        Value::Null
    );
    assert_eq!(world.get_task(bob, &parent).await["sub_issue_count"], 1);
    assert_eq!(
        server
            .get(bob, &world.path(&format!("/tasks/{related}/relations")))
            .await,
        json!([])
    );
    let listed = server.get(bob, &world.path("/tasks")).await;
    let mut titles = column(&listed, "title");
    titles.sort();
    assert_eq!(
        titles,
        ["Child", "Duplicate", "Open sub", "Parent", "Related"]
    );
    for task in listed["items"].as_array().unwrap() {
        assert_eq!(
            task["parent"]["title"].as_str().unwrap_or("Parent"),
            "Parent"
        );
        assert_eq!(task["duplicate_of"], Value::Null);
    }

    // Bob cannot point a task into the project, or move one there.
    let mine = world.task(bob, false, json!({"title": "Mine"})).await;
    let patch = world.path(&format!("/tasks/{mine}"));
    for (body, expected) in [
        (
            json!({"parent_task_id": hidden, "expected_version": 0}),
            422,
        ),
        (
            json!({"duplicate_of_id": hidden, "expected_version": 0}),
            422,
        ),
        (
            json!({"project_id": world.secret_id(), "expected_version": 0}),
            404,
        ),
    ] {
        assert_eq!(
            server.status(bob, "PATCH", &patch, body.clone()).await,
            expected,
            "{body}"
        );
    }
    assert_eq!(
        server
            .status(
                bob,
                "POST",
                &world.path(&format!("/tasks/{mine}/relations")),
                json!({"type": "related", "task_id": hidden}),
            )
            .await,
        422
    );
    assert_eq!(world.get_task(bob, &mine).await["version"], 0);
    world.server.stop().await;
}

#[tokio::test]
async fn members_are_added_and_removed_and_removal_revokes_access() {
    let world = World::new().await;
    let (server, alice, bob) = (&world.server, world.alice.as_str(), world.bob.as_str());
    let secret = world.secret_id();
    let hidden = world.hidden.as_str();
    let members = world.path(&format!("/projects/{secret}/members"));
    // Creating a private project made its creator the only member; the admin is not listed.
    assert_eq!(
        column(&server.get(&world.admin, &members).await, "user_id"),
        [world.alice_id.to_string()]
    );

    // Nobody outside the project is assigned to its tasks.
    let assign = json!({"assignee_ids": [world.bob_id], "expected_version": 0});
    let patch = world.path(&format!("/tasks/{hidden}"));
    assert_eq!(
        server.status(alice, "PATCH", &patch, assign.clone()).await,
        422
    );
    assert_eq!(
        server
            .status(
                alice,
                "POST",
                &world.path("/tasks"),
                json!({"project_id": secret, "status_id": world.secret_status, "title": "For Bob", "assignee_ids": [world.bob_id]}),
            )
            .await,
        422
    );
    // A mention of someone outside the project notifies nobody.
    server
        .ok(
            alice,
            "POST",
            &world.path(&format!("/tasks/{hidden}/comments")),
            json!({"body": "ask bob", "mentioned_user_ids": [world.bob_id]}),
        )
        .await;
    let inbox = world.path("/notifications");
    assert_eq!(server.get(bob, &inbox).await["items"], json!([]));

    assert_eq!(world.add_member(alice, world.guest_id).await.0, 422);
    assert_eq!(world.add_member(alice, Id::new_v7()).await.0, 422);
    let (status, added) = world.add_member(alice, world.bob_id).await;
    assert_eq!(status, 201);
    assert_eq!(added["user_id"], json!(world.bob_id));
    assert_eq!(added["project_id"], json!(secret));
    // Adding again changes nothing.
    assert_eq!(world.add_member(alice, world.bob_id).await.0, 201);
    assert_eq!(
        column(&server.get(bob, &members).await, "user_id"),
        [world.alice_id.to_string(), world.bob_id.to_string()]
    );
    assert_eq!(world.get_task(bob, hidden).await["title"], "Hidden");
    assert_eq!(server.status(alice, "PATCH", &patch, assign).await, 200);
    assert_eq!(
        column(&server.get(bob, &inbox).await, "kind"),
        ["task_assigned"]
    );

    // Bob, a member like Alice, removes himself.
    let bob_row = format!("{members}/{}", world.bob_id);
    assert_eq!(
        server.status(bob, "DELETE", &bob_row, Value::Null).await,
        204
    );
    assert_eq!(
        server
            .status(
                bob,
                "GET",
                &world.path(&format!("/tasks/{hidden}")),
                Value::Null
            )
            .await,
        404
    );
    assert_eq!(server.status(bob, "GET", &members, Value::Null).await, 404);
    assert_eq!(server.get(bob, &inbox).await["items"], json!([]));
    assert_eq!(
        server.status(alice, "DELETE", &bob_row, Value::Null).await,
        404
    );
    assert_eq!(
        column(&server.get(alice, &members).await, "user_id"),
        [world.alice_id.to_string()]
    );

    // A membership that ends takes its project rows along.
    assert_eq!(world.add_member(alice, world.bob_id).await.0, 201);
    sqlx::query("DELETE FROM task_assignees WHERE user_id = $1")
        .bind(world.bob_id.to_string())
        .execute(server.database.pool())
        .await
        .unwrap();
    sqlx::query("DELETE FROM memberships WHERE workspace_id = $1 AND user_id = $2")
        .bind(&world.workspace)
        .bind(world.bob_id.to_string())
        .execute(server.database.pool())
        .await
        .unwrap();
    assert_eq!(
        column(&server.get(alice, &members).await, "user_id"),
        [world.alice_id.to_string()]
    );
    world.server.stop().await;
}

#[tokio::test]
async fn making_a_project_private_adds_whoever_did_it() {
    let world = World::new().await;
    let (server, alice, bob) = (&world.server, world.alice.as_str(), world.bob.as_str());
    let open = server
        .ok(
            bob,
            "POST",
            &world.path("/projects"),
            json!({"name": "Open", "key": "OPN", "color": "#336699"}),
        )
        .await;
    assert_eq!(open["private"], json!(false));
    let project = world.path(&format!("/projects/{}", id(&open)));
    let statuses = format!("{project}/statuses");
    assert_eq!(
        server.status(alice, "GET", &statuses, Value::Null).await,
        200
    );
    assert_eq!(
        server.get(bob, &format!("{project}/members")).await,
        json!([])
    );

    let body = |private: bool, version: u64| json!({"name": "Open", "key": "OPN", "color": "#336699", "private": private, "expected_version": version});
    let closed = server.ok(bob, "PATCH", &project, body(true, 0)).await;
    assert_eq!(closed["private"], json!(true));
    assert_eq!(
        column(
            &server.get(bob, &format!("{project}/members")).await,
            "user_id"
        ),
        [world.bob_id.to_string()]
    );
    assert_eq!(
        server.status(alice, "GET", &statuses, Value::Null).await,
        404
    );
    assert_eq!(
        server
            .status(alice, "PATCH", &project, body(false, 1))
            .await,
        404
    );

    // Without `private` the project stays as it is.
    let renamed = server
        .ok(
            bob,
            "PATCH",
            &project,
            json!({"name": "Closed", "key": "OPN", "color": "#336699", "expected_version": 1}),
        )
        .await;
    assert_eq!(renamed["private"], json!(true));
    let reopened = server.ok(bob, "PATCH", &project, body(false, 2)).await;
    assert_eq!(reopened["private"], json!(false));
    assert_eq!(
        server.status(alice, "GET", &statuses, Value::Null).await,
        200
    );
    world.server.stop().await;
}

#[tokio::test]
async fn an_all_projects_token_covers_what_its_creator_sees() {
    let world = World::new().await;
    let server = &world.server;
    let hidden = world.hidden.as_str();
    let issued = server
        .ok(
            &world.admin,
            "POST",
            &world.path("/api-tokens"),
            json!({"name": "all", "scopes": ["tasks:read"], "all_projects": true}),
        )
        .await;
    let token = issued["token"].as_str().unwrap();
    let public = format!("/api/public/v1/workspaces/{}", world.workspace);
    let projects = format!("{public}/projects");
    let task = format!("{public}/tasks/{hidden}");

    let (_, listed) = server.bearer(token, &projects).await;
    assert_eq!(column(&listed, "name"), ["General", "Secret"]);
    assert_eq!(server.bearer(token, &task).await.0, 200);

    // The creator is no longer an admin, so the token loses the private project with them.
    sqlx::query("UPDATE memberships SET role = 'member' WHERE user_id = $1")
        .bind(world.admin_id.to_string())
        .execute(server.database.pool())
        .await
        .unwrap();
    let (_, listed) = server.bearer(token, &projects).await;
    assert_eq!(column(&listed, "name"), ["General"]);
    assert_eq!(server.bearer(token, &task).await.0, 404);

    assert_eq!(world.add_member(&world.alice, world.admin_id).await.0, 201);
    let (_, listed) = server.bearer(token, &projects).await;
    assert_eq!(column(&listed, "name"), ["General", "Secret"]);
    world.server.stop().await;
}
