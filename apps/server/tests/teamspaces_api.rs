use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{AuthenticatedUser, Id, PasswordService, TestDatabase, TimestampMillis};
use orbit_server::auth_routes::CookieMode;
use orbit_server::page_routes::{PageState, page_router};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::workspaces::WorkspaceRepository;
use orbit_server::teamspace_routes::{TeamspaceState, teamspace_router};
use serde_json::{Value, json};
use tower::ServiceExt;

struct Fixture {
    database: TestDatabase,
    identity: Arc<IdentityRepository>,
    app: axum::Router,
    workspace_id: String,
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
        let app = page_router(PageState::new(Arc::clone(&identity), CookieMode::secure())).merge(
            teamspace_router(TeamspaceState::new(
                Arc::clone(&identity),
                CookieMode::secure(),
            )),
        );
        Self {
            database,
            identity,
            app,
            workspace_id: setup.workspace_id.to_string(),
            owner_cookie: format!("__Host-orbit_session={}", setup.session.token),
            owner_id: setup.user_id,
        }
    }

    fn teamspaces_uri(&self) -> String {
        teamspaces_uri(&self.workspace_id)
    }

    fn teamspace_uri(&self, teamspace_id: &str) -> String {
        format!("{}/{teamspace_id}", self.teamspaces_uri())
    }

    async fn call(&self, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        call_as(self, &self.owner_cookie, method, uri, body).await
    }

    async fn list(&self) -> Vec<Value> {
        let (status, list) = self.call("GET", &self.teamspaces_uri(), None).await;
        assert_eq!(status, StatusCode::OK, "{list}");
        list["items"].as_array().unwrap().clone()
    }

    async fn create(&self, name: &str) -> Value {
        let (status, teamspace) = self
            .call("POST", &self.teamspaces_uri(), Some(json!({"name": name})))
            .await;
        assert_eq!(status, StatusCode::CREATED, "{teamspace}");
        teamspace
    }

    async fn delete_as(&self, cookie: &str, teamspace: &Value) -> (StatusCode, Value) {
        call_as(
            self,
            cookie,
            "DELETE",
            &format!(
                "{}?expected_version={}",
                self.teamspace_uri(id_of(teamspace)),
                teamspace["version"]
            ),
            None,
        )
        .await
    }

    async fn create_page(&self, body: Value) -> Value {
        let (status, page) = self
            .call(
                "POST",
                &format!("/api/v1/workspaces/{}/pages", self.workspace_id),
                Some(body),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{page}");
        page
    }

    async fn trash_page(&self, page: &Value) {
        let (status, body) = self
            .call(
                "DELETE",
                &format!(
                    "/api/v1/workspaces/{}/pages/{}?expected_version={}",
                    self.workspace_id,
                    id_of(page),
                    page["version"]
                ),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    }
}

#[tokio::test]
async fn teamspaces_are_listed_created_and_renamed_with_a_derived_default() {
    let fixture = Fixture::new().await;
    let initial = fixture.list().await;
    assert_eq!(initial.len(), 1, "setup creates the default teamspace");
    let general = &initial[0];
    assert_eq!(general["name"], "General");
    assert_eq!(general["icon"], Value::Null);
    assert_eq!(general["position"], 0);
    assert_eq!(general["version"], 0);
    assert_eq!(general["is_default"], true);
    assert_eq!(general["workspace_id"], fixture.workspace_id);

    let (status, design) = fixture
        .call(
            "POST",
            &fixture.teamspaces_uri(),
            Some(json!({"name": "  Design  ", "icon": "🎨"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{design}");
    assert_eq!(design["name"], "Design");
    assert_eq!(design["icon"], "🎨");
    assert_eq!(design["position"], 1);
    assert_eq!(design["version"], 0);
    assert_eq!(design["is_default"], false);
    let engineering = fixture.create("Engineering").await;
    assert_eq!(engineering["position"], 2);
    assert_eq!(
        names(&fixture.list().await),
        ["General", "Design", "Engineering"]
    );

    let (status, renamed) = fixture
        .call(
            "PATCH",
            &fixture.teamspace_uri(id_of(&design)),
            Some(json!({"expected_version": 0, "name": "Product", "icon": null})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{renamed}");
    assert_eq!(renamed["name"], "Product");
    assert_eq!(renamed["icon"], Value::Null);
    assert_eq!(renamed["version"], 1);
    assert_eq!(renamed["position"], 1);
    assert_eq!(fixture.list().await[1], renamed);

    let (status, conflict) = fixture
        .call(
            "PATCH",
            &fixture.teamspace_uri(id_of(&design)),
            Some(json!({"expected_version": 0, "name": "Stale"})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{conflict}");
    assert_eq!(conflict["code"], "conflict");
    assert_eq!(conflict["conflict"]["current_version"], 1);
    assert_eq!(
        conflict["conflict"]["refresh"],
        fixture.teamspaces_uri().as_str()
    );

    for body in [
        json!({"name": ""}),
        json!({"name": "   "}),
        json!({"name": "x".repeat(101)}),
        json!({"name": "Ok", "icon": "x".repeat(65)}),
    ] {
        let (status, problem) = fixture
            .call("POST", &fixture.teamspaces_uri(), Some(body.clone()))
            .await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{body}: {problem}"
        );
        assert_eq!(problem["code"], "validation_failed");
    }
    let (status, _) = fixture
        .call(
            "PATCH",
            &fixture.teamspace_uri(id_of(&design)),
            Some(json!({"expected_version": 1, "name": " "})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        fixture.create(&"y".repeat(100)).await["name"],
        "y".repeat(100)
    );

    // Deleting the default promotes the next teamspace by position.
    let (status, body) = fixture.delete_as(&fixture.owner_cookie, general).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let remaining = fixture.list().await;
    assert_eq!(names(&remaining)[..2], ["Product", "Engineering"]);
    assert_eq!(remaining[0]["is_default"], true);
    assert!(
        remaining[1..]
            .iter()
            .all(|item| item["is_default"] == false)
    );
    let page = fixture
        .create_page(json!({"title": "Lands in Product"}))
        .await;
    assert_eq!(page["teamspace_id"], design["id"]);

    let actions = sqlx::query_as::<_, (String, String, i64)>(
        "SELECT audit_events.action, audit_events.resource_type, \
         (SELECT COUNT(*) FROM outbox_events WHERE outbox_events.id = audit_events.id AND outbox_events.topic = 'workspace.changed') \
         FROM audit_events WHERE workspace_id = ? AND resource_type = 'teamspace' AND resource_id IN (?, ?) \
         ORDER BY occurred_at, id",
    )
    .bind(&fixture.workspace_id)
    .bind(id_of(general))
    .bind(id_of(&design))
    .fetch_all(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(
        actions,
        [
            ("teamspace.created".to_owned(), "teamspace".to_owned(), 1),
            ("teamspace.updated".to_owned(), "teamspace".to_owned(), 1),
            ("teamspace.deleted".to_owned(), "teamspace".to_owned(), 1),
        ]
    );
}

#[tokio::test]
async fn members_manage_teamspaces_but_only_owners_and_admins_delete_them() {
    let fixture = Fixture::new().await;
    let (member_id, member_cookie) =
        add_member(&fixture, &fixture.workspace_id, "member@example.com").await;
    let (status, created) = call_as(
        &fixture,
        &member_cookie,
        "POST",
        &fixture.teamspaces_uri(),
        Some(json!({"name": "Member space"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let (status, renamed) = call_as(
        &fixture,
        &member_cookie,
        "PATCH",
        &fixture.teamspace_uri(id_of(&created)),
        Some(json!({"expected_version": 0, "name": "Renamed"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{renamed}");

    let (status, problem) = fixture.delete_as(&member_cookie, &renamed).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{problem}");
    assert_eq!(problem["code"], "workspace_action_forbidden");
    assert_eq!(fixture.list().await.len(), 2);

    sqlx::query("UPDATE memberships SET role = 'admin' WHERE user_id = ?")
        .bind(member_id.to_string())
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let (status, body) = fixture.delete_as(&member_cookie, &renamed).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    assert_eq!(names(&fixture.list().await), ["General"]);
}

#[tokio::test]
async fn deletion_requires_an_empty_teamspace_and_keeps_the_last_one() {
    let fixture = Fixture::new().await;
    let general = fixture.list().await.remove(0);
    let (status, problem) = fixture.delete_as(&fixture.owner_cookie, &general).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");
    assert_eq!(problem["code"], "last_teamspace");

    let archive = fixture.create("Archive").await;
    let parent = fixture
        .create_page(json!({"teamspace_id": id_of(&archive), "title": "Parent"}))
        .await;
    let child = fixture
        .create_page(json!({"parent_id": id_of(&parent), "title": "Child"}))
        .await;
    assert_eq!(child["teamspace_id"], archive["id"]);
    let private = fixture
        .create_page(json!({"private": true, "title": "Mine"}))
        .await;

    let (status, problem) = fixture.delete_as(&fixture.owner_cookie, &archive).await;
    assert_eq!(status, StatusCode::CONFLICT, "{problem}");
    assert_eq!(problem["code"], "teamspace_not_empty");
    let (status, problem) = fixture
        .call(
            "DELETE",
            &format!(
                "{}?expected_version=7",
                fixture.teamspace_uri(id_of(&archive))
            ),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{problem}");
    assert_eq!(problem["code"], "conflict");

    // Trashed pages (a parent with its child) go with the teamspace.
    fixture.trash_page(&parent).await;
    let (status, body) = fixture.delete_as(&fixture.owner_cookie, &archive).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let remaining: Vec<String> = sqlx::query_scalar("SELECT id FROM pages ORDER BY id")
        .fetch_all(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(remaining, [id_of(&private)]);
    let (status, trash) = fixture
        .call(
            "GET",
            &format!("/api/v1/workspaces/{}/pages/trash", fixture.workspace_id),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(trash["items"].as_array().unwrap().is_empty());
    let (status, problem) = fixture.delete_as(&fixture.owner_cookie, &archive).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{problem}");
    assert_eq!(problem["code"], "teamspace_not_found");
}

#[tokio::test]
async fn teamspaces_are_isolated_between_workspaces() {
    let fixture = Fixture::new().await;
    let general = fixture.list().await.remove(0);
    let foreign_workspace = WorkspaceRepository::new((*fixture.database).clone())
        .create(
            fixture.owner_id,
            "Foreign".to_owned(),
            "teamspaces-test",
            TimestampMillis::now(),
        )
        .await
        .unwrap()
        .id
        .to_string();
    let (status, foreign) = fixture
        .call("GET", &teamspaces_uri(&foreign_workspace), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    let foreign = foreign["items"].as_array().unwrap();
    assert_eq!(names(foreign), ["General"], "new workspaces get General");
    assert_ne!(foreign[0]["id"], general["id"]);

    let (_, outsider_cookie) =
        add_member(&fixture, &foreign_workspace, "outsider@example.com").await;
    for (method, uri, body) in [
        ("GET", fixture.teamspaces_uri(), None),
        (
            "POST",
            fixture.teamspaces_uri(),
            Some(json!({"name": "Intruder"})),
        ),
        (
            "PATCH",
            fixture.teamspace_uri(id_of(&general)),
            Some(json!({"expected_version": 0, "name": "Hijacked"})),
        ),
        (
            "DELETE",
            format!(
                "{}?expected_version=0",
                fixture.teamspace_uri(id_of(&general))
            ),
            None,
        ),
    ] {
        let (status, problem) = call_as(&fixture, &outsider_cookie, method, &uri, body).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {uri}: {problem}");
        assert_eq!(problem["code"], "teamspace_not_found");
    }

    // The owner belongs to both, but a teamspace is only reachable through its own workspace.
    let wrong_scope = format!("{}/{}", teamspaces_uri(&foreign_workspace), id_of(&general));
    let (status, _) = fixture
        .call(
            "PATCH",
            &wrong_scope,
            Some(json!({"expected_version": 0, "name": "Moved"})),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, problem) = fixture
        .call(
            "POST",
            &format!("/api/v1/workspaces/{foreign_workspace}/pages"),
            Some(json!({"teamspace_id": id_of(&general)})),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{problem}");
    assert_eq!(problem["code"], "teamspace_not_found");
    assert_eq!(fixture.list().await[0]["name"], "General");
}

fn names(teamspaces: &[Value]) -> Vec<&str> {
    teamspaces
        .iter()
        .map(|teamspace| teamspace["name"].as_str().unwrap())
        .collect()
}

fn id_of(value: &Value) -> &str {
    value["id"].as_str().unwrap()
}

fn teamspaces_uri(workspace_id: &str) -> String {
    format!("/api/v1/workspaces/{workspace_id}/teamspaces")
}

async fn call_as(
    fixture: &Fixture,
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
            .body(Body::from(value.to_string())),
        None => builder.body(Body::empty()),
    }
    .unwrap();
    let response = fixture.app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, value)
}

async fn add_member(fixture: &Fixture, workspace_id: &str, email: &str) -> (Id, String) {
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
    .execute(fixture.database.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at) \
         VALUES (?, ?, ?, 'member', 0, ?, ?)",
    )
    .bind(Id::new_v7().to_string())
    .bind(workspace_id)
    .bind(id.to_string())
    .bind(now.as_millis())
    .bind(now.as_millis())
    .execute(fixture.database.pool())
    .await
    .unwrap();
    let session = fixture
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
