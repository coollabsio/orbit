//! Custom workspace roles (#73): a role adds permissions to an ordinary member, only the owner
//! and admins manage roles, and a role never carries the owner's powers. Everything goes through
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
}

fn id(record: &Value) -> String {
    record["id"].as_str().unwrap().to_owned()
}

struct World {
    server: Server,
    base: String,
    owner: String,
    admin: String,
    alice: String,
    alice_membership: String,
    bob: String,
    bob_membership: String,
}

impl World {
    async fn new() -> Self {
        let server = Server::start().await;
        let identity = IdentityRepository::new(server.database.clone());
        let now = TimestampMillis::now();
        identity
            .store_setup_token(
                "workspace-roles",
                TimestampMillis::from_millis(now.as_millis() + 60_000),
            )
            .await
            .unwrap();
        let setup = identity
            .complete_setup(
                SetupRequest {
                    token: "workspace-roles".to_owned(),
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
        let (_, alice) = server.person(&workspace, "alice", Some("member")).await;
        let (_, bob) = server.person(&workspace, "bob", Some("member")).await;
        let owner = setup.session.token.clone();
        let members = server.get(&owner, &format!("{base}/members")).await;
        let membership = |name: &str| {
            members["items"]
                .as_array()
                .unwrap()
                .iter()
                .find(|member| member["display_name"] == name)
                .map(id)
                .unwrap()
        };
        Self {
            alice_membership: membership("alice"),
            bob_membership: membership("bob"),
            server,
            base,
            owner,
            admin,
            alice,
            bob,
        }
    }

    async fn create_role(&self, name: &str, permissions: &[&str]) -> Value {
        self.server
            .ok(
                &self.owner,
                "POST",
                &format!("{}/roles", self.base),
                json!({"name": name, "permissions": permissions}),
            )
            .await
    }

    async fn assign(&self, membership: &str, role: &str) {
        let version = self.version(membership).await;
        self.server
            .ok(
                &self.owner,
                "PUT",
                &format!("{}/members/{membership}/custom-role", self.base),
                json!({"role_id": role, "expected_version": version}),
            )
            .await;
    }

    async fn member(&self, membership: &str) -> Value {
        let members = self
            .server
            .get(&self.owner, &format!("{}/members", self.base))
            .await;
        members["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|member| id(member) == membership)
            .cloned()
            .unwrap()
    }

    async fn version(&self, membership: &str) -> u64 {
        self.member(membership).await["version"].as_u64().unwrap()
    }

    async fn permissions(&self, session: &str) -> Vec<String> {
        let workspace = self.server.get(session, &self.base).await;
        workspace["permissions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|permission| permission.as_str().unwrap().to_owned())
            .collect()
    }

    async fn status(&self, session: &str, method: &str, path: &str, body: Value) -> u16 {
        self.server
            .status(session, method, &format!("{}{path}", self.base), body)
            .await
    }
}

#[tokio::test]
async fn a_custom_role_gives_a_member_its_permissions() {
    let world = World::new().await;
    assert_eq!(
        world.status(&world.bob, "GET", "/audit", Value::Null).await,
        403
    );
    assert_eq!(
        world.permissions(&world.bob).await,
        ["workspace.export_personal"]
    );

    let role = world
        .create_role("Auditor", &["chat.manage", "audit.view", "audit.view"])
        .await;
    assert_eq!(role["name"], "Auditor");
    assert_eq!(role["permissions"], json!(["audit.view", "chat.manage"]));
    assert_eq!(role["member_count"], 0);
    world.assign(&world.bob_membership, &id(&role)).await;

    // The permission works on the server, and the workspace names it for the client.
    assert_eq!(
        world.status(&world.bob, "GET", "/audit", Value::Null).await,
        200
    );
    assert_eq!(
        world.permissions(&world.bob).await,
        ["audit.view", "chat.manage", "workspace.export_personal"]
    );
    assert_eq!(
        world
            .status(&world.bob, "GET", "/mail/mailboxes", Value::Null)
            .await,
        403
    );
    // Someone else with no role is unchanged.
    assert_eq!(
        world
            .status(&world.alice, "GET", "/audit", Value::Null)
            .await,
        403
    );

    let bob = world.member(&world.bob_membership).await;
    assert_eq!(bob["custom_role"]["name"], "Auditor");
    let roles = world
        .server
        .get(&world.owner, &format!("{}/roles", world.base))
        .await;
    assert_eq!(roles["items"][0]["member_count"], 1);
    assert!(
        !roles["grantable_permissions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|permission| permission == "billing.manage")
    );

    // Taking the role away, or deleting it, takes the permissions away.
    world
        .server
        .ok(
            &world.owner,
            "PUT",
            &format!("{}/members/{}/custom-role", world.base, world.bob_membership),
            json!({"role_id": null, "expected_version": world.version(&world.bob_membership).await}),
        )
        .await;
    assert_eq!(
        world.status(&world.bob, "GET", "/audit", Value::Null).await,
        403
    );
    world.assign(&world.bob_membership, &id(&role)).await;
    assert_eq!(
        world.status(&world.bob, "GET", "/audit", Value::Null).await,
        200
    );
    world
        .server
        .ok(
            &world.admin,
            "DELETE",
            &format!("{}/roles/{}", world.base, id(&role)),
            Value::Null,
        )
        .await;
    assert_eq!(
        world.status(&world.bob, "GET", "/audit", Value::Null).await,
        403
    );
    assert!(world.member(&world.bob_membership).await["custom_role"].is_null());
    world.server.stop().await;
}

#[tokio::test]
async fn only_owners_and_admins_manage_roles() {
    let world = World::new().await;
    let role = world.create_role("People", &["members.manage"]).await;
    world.assign(&world.alice_membership, &id(&role)).await;
    let create = json!({"name": "Mine", "permissions": ["audit.view"]});
    for session in [&world.bob, &world.alice] {
        assert_eq!(
            world
                .status(session, "POST", "/roles", create.clone())
                .await,
            403
        );
        assert_eq!(
            world
                .status(
                    session,
                    "PATCH",
                    &format!("/roles/{}", id(&role)),
                    json!({"expected_version": 0, "permissions": []})
                )
                .await,
            403
        );
        assert_eq!(
            world
                .status(
                    session,
                    "DELETE",
                    &format!("/roles/{}", id(&role)),
                    Value::Null
                )
                .await,
            403
        );
        let assign = json!({"role_id": id(&role), "expected_version": 0});
        assert_eq!(
            world
                .status(
                    session,
                    "PUT",
                    &format!("/members/{}/custom-role", world.bob_membership),
                    assign
                )
                .await,
            403
        );
    }
    // Any member reads the list; an admin writes it.
    assert_eq!(
        world.status(&world.bob, "GET", "/roles", Value::Null).await,
        200
    );
    assert_eq!(
        world.status(&world.admin, "POST", "/roles", create).await,
        201
    );

    // The denials are audited.
    let audit = world
        .server
        .get(&world.owner, &format!("{}/audit", world.base))
        .await;
    assert!(
        audit["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["action"] == "role.manage_denied")
    );
    assert!(
        audit["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["action"] == "role.created")
    );
    world.server.stop().await;
}

#[tokio::test]
async fn a_role_cannot_carry_owner_powers_or_a_bad_name() {
    let world = World::new().await;
    for permission in [
        "workspace.delete",
        "workspace.transfer",
        "billing.manage",
        "roles.manage",
    ] {
        assert_eq!(
            world
                .status(
                    &world.owner,
                    "POST",
                    "/roles",
                    json!({"name": "Sneaky", "permissions": [permission]})
                )
                .await,
            422,
            "{permission}"
        );
    }
    // An unknown permission name is not even valid JSON for the body.
    assert_eq!(
        world
            .status(
                &world.owner,
                "POST",
                "/roles",
                json!({"name": "Sneaky", "permissions": ["nope"]})
            )
            .await,
        400
    );
    for name in ["", "   ", "Admin", "owner", &"x".repeat(41)] {
        assert_eq!(
            world
                .status(
                    &world.owner,
                    "POST",
                    "/roles",
                    json!({"name": name, "permissions": []})
                )
                .await,
            422,
            "{name:?}"
        );
    }
    world.create_role("Support", &[]).await;
    assert_eq!(
        world
            .status(
                &world.owner,
                "POST",
                "/roles",
                json!({"name": " support ", "permissions": []})
            )
            .await,
        409
    );
    // A rename to its own name in another case is the same role, not a clash.
    let roles = world
        .server
        .get(&world.owner, &format!("{}/roles", world.base))
        .await;
    let role = &roles["items"][0];
    assert_eq!(
        world
            .status(
                &world.owner,
                "PATCH",
                &format!("/roles/{}", id(role)),
                json!({"expected_version": 0, "name": "SUPPORT"})
            )
            .await,
        200
    );
    assert_eq!(
        world
            .status(
                &world.owner,
                "PATCH",
                &format!("/roles/{}", id(role)),
                json!({"expected_version": 0, "name": "Again"})
            )
            .await,
        409
    );
    world.server.stop().await;
}

#[tokio::test]
async fn a_member_manager_cannot_make_admins_and_a_role_ends_with_the_member_role() {
    let world = World::new().await;
    let people = world.create_role("People", &["members.manage"]).await;
    world.assign(&world.alice_membership, &id(&people)).await;
    let bob = format!("/members/{}", world.bob_membership);
    let bob_version = world.version(&world.bob_membership).await;

    // Alice manages members but cannot lift anyone to admin.
    assert_eq!(
        world
            .status(
                &world.alice,
                "PATCH",
                &bob,
                json!({"role": "admin", "expected_version": bob_version})
            )
            .await,
        403
    );
    assert_eq!(
        world
            .status(
                &world.alice,
                "POST",
                "/invitations",
                json!({"email": "x@example.com", "role": "admin", "delivery": "manual"})
            )
            .await,
        403
    );
    // Alice cannot touch the admin either.
    let members = world
        .server
        .get(&world.owner, &format!("{}/members", world.base))
        .await;
    let admin = members["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|member| member["role"] == "admin")
        .unwrap();
    assert_eq!(
        world
            .status(
                &world.alice,
                "PATCH",
                &format!("/members/{}", id(admin)),
                json!({"role": "member", "expected_version": admin["version"]})
            )
            .await,
        403
    );
    assert_eq!(
        world
            .status(
                &world.alice,
                "DELETE",
                &format!(
                    "/members/{}?expected_version={}",
                    id(admin),
                    admin["version"]
                ),
                Value::Null
            )
            .await,
        403
    );

    // Keeping the member role keeps the custom role.
    world.assign(&world.bob_membership, &id(&people)).await;
    let version = world.version(&world.bob_membership).await;
    assert_eq!(
        world
            .status(
                &world.owner,
                "PATCH",
                &bob,
                json!({"role": "member", "expected_version": version})
            )
            .await,
        204
    );
    assert_eq!(
        world.member(&world.bob_membership).await["custom_role"]["name"],
        "People"
    );
    // Making them an admin ends it, and an admin holds none.
    let version = world.version(&world.bob_membership).await;
    assert_eq!(
        world
            .status(
                &world.owner,
                "PATCH",
                &bob,
                json!({"role": "admin", "expected_version": version})
            )
            .await,
        204
    );
    assert!(world.member(&world.bob_membership).await["custom_role"].is_null());
    let version = world.version(&world.bob_membership).await;
    assert_eq!(
        world
            .status(
                &world.owner,
                "PUT",
                &format!("/members/{}/custom-role", world.bob_membership),
                json!({"role_id": id(&people), "expected_version": version})
            )
            .await,
        422
    );
    // A stale version is a conflict, and an unknown role is not found.
    let version = world.version(&world.alice_membership).await;
    assert_eq!(
        world
            .status(
                &world.owner,
                "PUT",
                &format!("/members/{}/custom-role", world.alice_membership),
                json!({"role_id": id(&people), "expected_version": version + 1})
            )
            .await,
        409
    );
    assert_eq!(
        world
            .status(
                &world.owner,
                "PUT",
                &format!("/members/{}/custom-role", world.alice_membership),
                json!({"role_id": Id::new_v7().to_string(), "expected_version": version})
            )
            .await,
        404
    );
    world.server.stop().await;
}
