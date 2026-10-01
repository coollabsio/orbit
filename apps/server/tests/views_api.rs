use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{AuthenticatedUser, Id, PasswordService, TestDatabase, TimestampMillis};
use orbit_server::auth_routes::CookieMode;
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::workspaces::WorkspaceRepository;
use orbit_server::task_routes::TaskState;
use orbit_server::view_routes::view_router;
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
        let app = view_router(TaskState::new(Arc::clone(&identity), CookieMode::secure()));
        Self {
            database,
            identity,
            app,
            workspace_id: setup.workspace_id.to_string(),
            owner_cookie: format!("__Host-orbit_session={}", setup.session.token),
            owner_id: setup.user_id,
        }
    }

    /// Adds a user with a live session. `role: None` leaves the user outside the workspace.
    async fn add_user(&self, email: &str, display_name: &str, role: Option<&str>) -> (Id, String) {
        let id = Id::new_v7();
        let now = TimestampMillis::now();
        sqlx::query(
            "INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at) \
             VALUES (?, ?, ?, ?, 'unused', ?, ?)",
        )
        .bind(id.to_string())
        .bind(email)
        .bind(email)
        .bind(display_name)
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(self.database.pool())
        .await
        .unwrap();
        if let Some(role) = role {
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
        }
        let session = self
            .identity
            .create_session(
                &AuthenticatedUser {
                    id,
                    email: email.to_owned(),
                    display_name: display_name.to_owned(),
                },
                now,
            )
            .await
            .unwrap();
        (id, format!("__Host-orbit_session={}", session.token))
    }

    async fn call(
        &self,
        method: &str,
        path: &str,
        cookie: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        self.call_in(&self.workspace_id, method, path, cookie, body)
            .await
    }

    async fn call_in(
        &self,
        workspace_id: &str,
        method: &str,
        path: &str,
        cookie: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let builder = Request::builder()
            .method(method)
            .uri(format!("/api/v1/workspaces/{workspace_id}{path}"))
            .header(header::COOKIE, cookie);
        let request = match body {
            Some(body) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
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

    async fn create_view(&self, cookie: &str, name: &str, visibility: &str) -> Value {
        let (status, body) = self
            .call(
                "POST",
                "/views",
                cookie,
                Some(json!({"name": name, "visibility": visibility, "state": view_state()})),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        body
    }

    /// Workspace realtime sequence; every broadcast write bumps it by one.
    async fn sequence(&self) -> i64 {
        sqlx::query_scalar(
            "SELECT COALESCE((SELECT sequence FROM realtime_sequences WHERE workspace_id = ?), 0)",
        )
        .bind(&self.workspace_id)
        .fetch_one(self.database.pool())
        .await
        .unwrap()
    }
}

fn view_state() -> Value {
    json!({
        "filter": {"op": "and", "children": [
            {"field": "priority", "operator": "is", "value": ["high", "urgent"]}
        ]},
        "display": {
            "layout": "list",
            "group_by": "status",
            "sub_group_by": "none",
            "order_by": "manual",
            "order_direction": "asc",
            "properties": ["id", "status", "assignee", "priority", "project", "due_date", "labels"],
            "show_completed": "all",
            "show_empty_groups": false,
            "sub_issues": "nested"
        }
    })
}

fn other_state() -> Value {
    json!({
        "filter": {"op": "or", "children": [
            {"field": "assignee", "operator": "is", "value": ["me"]},
            {"op": "and", "children": [{"field": "label", "operator": "is_empty"}]}
        ]},
        "display": {
            "layout": "board",
            "group_by": "priority",
            "sub_group_by": "none",
            "order_by": "due_date",
            "order_direction": "desc",
            "properties": ["id", "status"],
            "show_completed": "past_week",
            "show_empty_groups": true,
            "sub_issues": "flat"
        }
    })
}

/// A state whose only condition is a `text contains` of `length` characters (limit: 200).
fn state_with_text(length: usize) -> Value {
    let mut state = view_state();
    state["filter"] = json!({"op": "and", "children": [
        {"field": "text", "operator": "contains", "value": "x".repeat(length)}
    ]});
    state
}

fn id_of(view: &Value) -> String {
    view["id"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn owner_creates_reads_updates_and_deletes_a_personal_view() {
    let fixture = Fixture::new().await;
    let cookie = fixture.owner_cookie.clone();
    let (status, created) = fixture
        .call(
            "POST",
            "/views",
            &cookie,
            Some(json!({
                "name": "  Bugs  ",
                "description": "Open bugs",
                "icon": "bug",
                "color": "#FF0088",
                "visibility": "personal",
                "state": view_state()
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["name"], "Bugs");
    assert_eq!(created["description"], "Open bugs");
    assert_eq!(created["icon"], "bug");
    assert_eq!(created["color"], "#ff0088");
    assert_eq!(created["visibility"], "personal");
    assert_eq!(created["owner"]["user_id"], fixture.owner_id.to_string());
    assert_eq!(created["owner"]["display_name"], "Owner");
    assert_eq!(created["state"], view_state());
    assert_eq!(created["state_error"], Value::Null);
    assert_eq!(created["version"], 1);
    assert_eq!(created["is_favorite"], false);
    assert_eq!(created["favorite_position"], Value::Null);
    assert_eq!(created["can_edit"], true);
    for field in ["created_at", "updated_at"] {
        let stamp = created[field].as_str().unwrap();
        assert!(stamp.ends_with('Z'), "{stamp}");
        serde_json::from_value::<TimestampMillis>(created[field].clone()).unwrap();
    }
    let id = id_of(&created);

    let (status, list) = fixture.call("GET", "/views", &cookie, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["id"], id.as_str());

    let (status, updated) = fixture
        .call(
            "PATCH",
            &format!("/views/{id}"),
            &cookie,
            Some(json!({
                "expected_version": 1,
                "name": "Bug triage",
                "icon": null,
                "state": other_state()
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["name"], "Bug triage");
    assert_eq!(updated["icon"], Value::Null);
    assert_eq!(updated["color"], "#ff0088");
    assert_eq!(updated["description"], "Open bugs");
    assert_eq!(updated["state"], other_state());
    assert_eq!(updated["version"], 2);

    let (status, fetched) = fixture
        .call("GET", &format!("/views/{id}"), &cookie, None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fetched, updated);

    sqlx::query(
        "INSERT INTO saved_view_favorites (view_id, user_id, position, created_at) VALUES (?, ?, 0, 0)",
    )
    .bind(&id)
    .bind(fixture.owner_id.to_string())
    .execute(fixture.database.pool())
    .await
    .unwrap();
    let (status, _) = fixture
        .call("DELETE", &format!("/views/{id}"), &cookie, None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let favorites: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM saved_view_favorites WHERE view_id = ?")
            .bind(&id)
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    assert_eq!(favorites, 0);
    let (status, problem) = fixture
        .call("GET", &format!("/views/{id}"), &cookie, None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(problem["code"], "task_resource_not_found");
}

#[tokio::test]
async fn personal_views_are_not_found_for_everyone_but_their_owner() {
    let fixture = Fixture::new().await;
    let (_, member) = fixture
        .add_user("member@example.com", "Member", Some("member"))
        .await;
    let (_, admin) = fixture
        .add_user("admin@example.com", "Admin", Some("admin"))
        .await;
    let view = fixture
        .create_view(&fixture.owner_cookie, "Mine", "personal")
        .await;
    let path = format!("/views/{}", id_of(&view));
    for cookie in [&member, &admin] {
        let (status, list) = fixture.call("GET", "/views", cookie, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(list, json!([]));
        assert_eq!(
            fixture.call("GET", &path, cookie, None).await.0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            fixture
                .call(
                    "PATCH",
                    &path,
                    cookie,
                    Some(json!({"expected_version": 1, "name": "Taken"}))
                )
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            fixture.call("DELETE", &path, cookie, None).await.0,
            StatusCode::NOT_FOUND
        );
    }
}

#[tokio::test]
async fn workspace_views_are_readable_by_members_and_editable_by_their_owner_or_admins() {
    let fixture = Fixture::new().await;
    let (author_id, author) = fixture
        .add_user("author@example.com", "Author", Some("member"))
        .await;
    let (_, reader) = fixture
        .add_user("reader@example.com", "Reader", Some("member"))
        .await;
    let (_, admin) = fixture
        .add_user("admin@example.com", "Admin", Some("admin"))
        .await;
    let view = fixture
        .create_view(&author, "Team board", "workspace")
        .await;
    assert_eq!(view["description"], "");
    let id = id_of(&view);
    let path = format!("/views/{id}");

    let (status, seen) = fixture.call("GET", &path, &reader, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(seen["can_edit"], false);
    assert_eq!(seen["owner"]["user_id"], author_id.to_string());
    assert_eq!(seen["owner"]["display_name"], "Author");
    let (status, list) = fixture.call("GET", "/views", &reader, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list[0]["id"], id.as_str());
    assert_eq!(list[0]["can_edit"], false);
    assert_eq!(
        fixture
            .call(
                "PATCH",
                &path,
                &reader,
                Some(json!({"expected_version": 1, "name": "Mine now"}))
            )
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        fixture.call("DELETE", &path, &reader, None).await.0,
        StatusCode::FORBIDDEN
    );

    let (status, seen) = fixture.call("GET", &path, &admin, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(seen["can_edit"], true);
    // An admin edits a shared view but only its owner decides who sees it.
    assert_eq!(seen["can_change_visibility"], false);
    let (status, renamed) = fixture
        .call(
            "PATCH",
            &path,
            &admin,
            Some(json!({"expected_version": 1, "name": "Admin renamed"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{renamed}");
    assert_eq!(renamed["name"], "Admin renamed");
    assert_eq!(renamed["owner"]["user_id"], author_id.to_string());

    let (status, seen) = fixture
        .call("GET", &path, &fixture.owner_cookie, None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(seen["can_edit"], true);
    assert_eq!(
        fixture
            .call("DELETE", &path, &fixture.owner_cookie, None)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        fixture.call("GET", &path, &reader, None).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn only_the_view_owner_changes_visibility() {
    let fixture = Fixture::new().await;
    let (_, author) = fixture
        .add_user("author@example.com", "Author", Some("member"))
        .await;
    let view = fixture.create_view(&author, "Shared", "workspace").await;
    let path = format!("/views/{}", id_of(&view));

    let (status, _) = fixture
        .call(
            "PATCH",
            &path,
            &fixture.owner_cookie,
            Some(json!({"expected_version": 1, "visibility": "personal"})),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, same) = fixture
        .call(
            "PATCH",
            &path,
            &fixture.owner_cookie,
            Some(json!({"expected_version": 1, "visibility": "workspace", "description": "Kept shared"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{same}");
    assert_eq!(same["visibility"], "workspace");
    assert_eq!(same["description"], "Kept shared");

    let (status, private) = fixture
        .call(
            "PATCH",
            &path,
            &author,
            Some(json!({"expected_version": 2, "visibility": "personal"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{private}");
    assert_eq!(private["visibility"], "personal");
    assert_eq!(
        fixture
            .call("GET", &path, &fixture.owner_cookie, None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn making_a_view_personal_drops_other_users_favorites() {
    let fixture = Fixture::new().await;
    let (author_id, author) = fixture
        .add_user("author@example.com", "Author", Some("member"))
        .await;
    let (reader_id, reader) = fixture
        .add_user("reader@example.com", "Reader", Some("member"))
        .await;
    let id = id_of(&fixture.create_view(&author, "Shared", "workspace").await);
    for (user_id, position) in [(author_id, 0), (reader_id, 0), (fixture.owner_id, 3)] {
        sqlx::query(
            "INSERT INTO saved_view_favorites (view_id, user_id, position, created_at) VALUES (?, ?, ?, 0)",
        )
        .bind(&id)
        .bind(user_id.to_string())
        .bind(position)
        .execute(fixture.database.pool())
        .await
        .unwrap();
    }

    let (status, body) = fixture
        .call(
            "PATCH",
            &format!("/views/{id}"),
            &author,
            Some(json!({"expected_version": 1, "visibility": "personal"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["is_favorite"], true);
    let remaining: Vec<String> =
        sqlx::query_scalar("SELECT user_id FROM saved_view_favorites WHERE view_id = ?")
            .bind(&id)
            .fetch_all(fixture.database.pool())
            .await
            .unwrap();
    assert_eq!(remaining, vec![author_id.to_string()]);
    // the former reader's bookmarked URL now shows "View not found"
    let (status, _) = fixture
        .call("GET", &format!("/views/{id}"), &reader, None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn stale_versions_conflict_with_the_current_view() {
    let fixture = Fixture::new().await;
    let cookie = fixture.owner_cookie.clone();
    let id = id_of(&fixture.create_view(&cookie, "Versioned", "workspace").await);
    let path = format!("/views/{id}");
    let (status, _) = fixture
        .call(
            "PATCH",
            &path,
            &cookie,
            Some(json!({"expected_version": 1, "name": "First"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    let (status, problem) = fixture
        .call(
            "PATCH",
            &path,
            &cookie,
            Some(json!({"expected_version": 1, "name": "Second"})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{problem}");
    assert_eq!(problem["code"], "conflict");
    assert_eq!(problem["conflict"]["current_version"], 2);
    assert_eq!(problem["conflict"]["current"]["name"], "First");
    assert_eq!(
        problem["conflict"]["refresh"],
        format!("/api/v1/workspaces/{}/views/{id}", fixture.workspace_id)
    );

    let (_, reader) = fixture
        .add_user("reader@example.com", "Reader", Some("member"))
        .await;
    assert_eq!(
        fixture
            .call(
                "PATCH",
                &path,
                &reader,
                Some(json!({"expected_version": 1, "name": "Stale and forbidden"}))
            )
            .await
            .0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn view_fields_are_validated() {
    let fixture = Fixture::new().await;
    let cookie = fixture.owner_cookie.clone();
    for (body, field) in [
        (
            json!({"name": "   ", "visibility": "personal", "state": view_state()}),
            "name",
        ),
        (
            json!({"name": "n".repeat(81), "visibility": "personal", "state": view_state()}),
            "name",
        ),
        (
            json!({"name": "Ok", "description": "d".repeat(501), "visibility": "personal", "state": view_state()}),
            "description",
        ),
        (
            json!({"name": "Ok", "color": "pink", "visibility": "personal", "state": view_state()}),
            "color",
        ),
        (
            json!({"name": "Ok", "icon": "not an icon!", "visibility": "personal", "state": view_state()}),
            "icon",
        ),
        (
            json!({"name": "Ok", "icon": "i".repeat(65), "visibility": "personal", "state": view_state()}),
            "icon",
        ),
    ] {
        let (status, problem) = fixture.call("POST", "/views", &cookie, Some(body)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");
        assert_eq!(problem["code"], "validation_failed");
        assert_eq!(problem["detail"], field);
    }

    let (status, created) = fixture
        .call(
            "POST",
            "/views",
            &cookie,
            Some(json!({"name": "n".repeat(80), "visibility": "workspace", "state": view_state()})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let (status, problem) = fixture
        .call(
            "PATCH",
            &format!("/views/{}", id_of(&created)),
            &cookie,
            Some(json!({"expected_version": 1, "name": ""})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(problem["detail"], "name");

    let (status, _) = fixture
        .call(
            "POST",
            "/views",
            &cookie,
            Some(json!({"name": "Ok", "visibility": "team", "state": view_state()})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn invalid_view_state_returns_the_json_path() {
    let fixture = Fixture::new().await;
    let cookie = fixture.owner_cookie.clone();
    let (status, problem) = fixture
        .call(
            "POST",
            "/views",
            &cookie,
            Some(json!({"name": "Bad", "visibility": "personal", "state": state_with_text(201)})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");
    assert!(
        problem["path"]
            .as_str()
            .unwrap()
            .starts_with("filter.children[0]"),
        "{problem}"
    );

    let (status, created) = fixture
        .call(
            "POST",
            "/views",
            &cookie,
            Some(json!({"name": "Edge", "visibility": "personal", "state": state_with_text(200)})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let (status, problem) = fixture
        .call(
            "PATCH",
            &format!("/views/{}", id_of(&created)),
            &cookie,
            Some(json!({"expected_version": 1, "state": state_with_text(201)})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");
    assert!(
        problem["path"]
            .as_str()
            .unwrap()
            .starts_with("filter.children[0]"),
        "{problem}"
    );
    let (status, unchanged) = fixture
        .call("GET", &format!("/views/{}", id_of(&created)), &cookie, None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(unchanged["version"], 1);
    assert_eq!(unchanged["state"], state_with_text(200));
}

#[tokio::test]
async fn unreadable_stored_state_is_returned_with_a_state_error() {
    let fixture = Fixture::new().await;
    let cookie = fixture.owner_cookie.clone();
    let mut ids = Vec::new();
    for (name, stored) in [
        (
            "Broken enum",
            r#"{"filter":{"op":"xor","children":[]},"display":{}}"#,
        ),
        ("Broken json", r#"{"filter":"#),
    ] {
        let id = Id::new_v7().to_string();
        sqlx::query(
            "INSERT INTO saved_views (id, workspace_id, owner_user_id, name, description, icon, color, \
             visibility, state_json, version, created_at, updated_at) \
             VALUES (?, ?, ?, ?, '', NULL, NULL, 'workspace', ?, 1, 0, 0)",
        )
        .bind(&id)
        .bind(&fixture.workspace_id)
        .bind(fixture.owner_id.to_string())
        .bind(name)
        .bind(stored)
        .execute(fixture.database.pool())
        .await
        .unwrap();
        ids.push(id);
    }

    let mut errors = Vec::new();
    for id in &ids {
        let (status, view) = fixture
            .call("GET", &format!("/views/{id}"), &cookie, None)
            .await;
        assert_eq!(status, StatusCode::OK, "{view}");
        assert_eq!(view["state"], Value::Null);
        errors.push(view["state_error"].as_str().unwrap().to_owned());
    }
    assert!(errors[0].starts_with("filter"), "{}", errors[0]);
    assert_eq!(errors[1], "is not valid JSON");
    let (status, list) = fixture.call("GET", "/views", &cookie, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list.as_array().unwrap().len(), 2);
    assert!(
        list.as_array()
            .unwrap()
            .iter()
            .all(|view| view["state"].is_null())
    );

    let path = format!("/views/{}", ids[0]);
    let (status, renamed) = fixture
        .call(
            "PATCH",
            &path,
            &cookie,
            Some(json!({"expected_version": 1, "name": "Still broken"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{renamed}");
    assert_eq!(renamed["state"], Value::Null);
    assert!(renamed["state_error"].is_string());

    let (status, repaired) = fixture
        .call(
            "PATCH",
            &path,
            &cookie,
            Some(json!({"expected_version": 2, "state": view_state()})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{repaired}");
    assert_eq!(repaired["state"], view_state());
    assert_eq!(repaired["state_error"], Value::Null);
}

#[tokio::test]
async fn outsiders_and_other_workspaces_get_not_found() {
    let fixture = Fixture::new().await;
    let (_, outsider) = fixture
        .add_user("outsider@example.com", "Outsider", None)
        .await;
    let id = id_of(
        &fixture
            .create_view(&fixture.owner_cookie, "Shared", "workspace")
            .await,
    );
    assert_eq!(
        fixture.call("GET", "/views", &outsider, None).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        fixture
            .call("GET", &format!("/views/{id}"), &outsider, None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        fixture
            .call(
                "POST",
                "/views",
                &outsider,
                Some(json!({"name": "Nope", "visibility": "personal", "state": view_state()}))
            )
            .await
            .0,
        StatusCode::NOT_FOUND
    );

    let other = WorkspaceRepository::new((*fixture.database).clone())
        .create(
            fixture.owner_id,
            "Elsewhere".to_owned(),
            "views-test",
            TimestampMillis::now(),
        )
        .await
        .unwrap()
        .id
        .to_string();
    let (status, _) = fixture
        .call_in(
            &other,
            "GET",
            &format!("/views/{id}"),
            &fixture.owner_cookie,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, list) = fixture
        .call_in(&other, "GET", "/views", &fixture.owner_cookie, None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list, json!([]));
    assert_eq!(
        fixture
            .call("GET", "/views/not-a-view-id", &fixture.owner_cookie, None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn view_writes_are_audited_and_broadcast() {
    let fixture = Fixture::new().await;
    let cookie = fixture.owner_cookie.clone();
    let start = fixture.sequence().await;
    let id = id_of(&fixture.create_view(&cookie, "Audited", "workspace").await);
    assert_eq!(fixture.sequence().await, start + 1);
    let (status, _) = fixture
        .call(
            "PATCH",
            &format!("/views/{id}"),
            &cookie,
            Some(json!({"expected_version": 1, "name": "Audited again"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fixture.sequence().await, start + 2);
    let (status, _) = fixture
        .call("DELETE", &format!("/views/{id}"), &cookie, None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(fixture.sequence().await, start + 3);

    let actions: Vec<String> = sqlx::query_scalar(
        "SELECT action FROM audit_events WHERE resource_type = 'saved_view' AND resource_id = ? ORDER BY rowid",
    )
    .bind(&id)
    .fetch_all(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(
        actions,
        [
            "saved_view.created",
            "saved_view.updated",
            "saved_view.deleted"
        ]
    );
}

#[tokio::test]
async fn views_are_listed_by_name_ignoring_case() {
    let fixture = Fixture::new().await;
    let cookie = fixture.owner_cookie.clone();
    for name in ["beta", "Gamma", "alpha"] {
        fixture.create_view(&cookie, name, "workspace").await;
    }
    let (status, list) = fixture.call("GET", "/views", &cookie, None).await;
    assert_eq!(status, StatusCode::OK);
    let names: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|view| view["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["alpha", "beta", "Gamma"]);
}

async fn set_favorite(
    fixture: &Fixture,
    cookie: &str,
    view_id: &str,
    favorite: bool,
) -> StatusCode {
    let method = if favorite { "PUT" } else { "DELETE" };
    fixture
        .call(method, &format!("/views/{view_id}/favorite"), cookie, None)
        .await
        .0
}

async fn reorder(fixture: &Fixture, cookie: &str, view_ids: Value) -> (StatusCode, Value) {
    fixture
        .call(
            "PUT",
            "/view-favorites/order",
            cookie,
            Some(json!({"view_ids": view_ids})),
        )
        .await
}

/// Favorite view names in sidebar order.
async fn favorite_names(fixture: &Fixture, cookie: &str) -> Vec<String> {
    let (status, list) = fixture.call("GET", "/views", cookie, None).await;
    assert_eq!(status, StatusCode::OK);
    let mut favorites: Vec<(i64, String)> = list
        .as_array()
        .unwrap()
        .iter()
        .filter(|view| view["is_favorite"] == true)
        .map(|view| {
            (
                view["favorite_position"].as_i64().unwrap(),
                view["name"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    favorites.sort();
    favorites.into_iter().map(|(_, name)| name).collect()
}

#[tokio::test]
async fn favorites_are_idempotent_and_append_at_the_end() {
    let fixture = Fixture::new().await;
    let cookie = fixture.owner_cookie.clone();
    let a = id_of(&fixture.create_view(&cookie, "A", "workspace").await);
    let b = id_of(&fixture.create_view(&cookie, "B", "personal").await);
    let c = id_of(&fixture.create_view(&cookie, "C", "workspace").await);

    assert_eq!(
        set_favorite(&fixture, &cookie, &a, true).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        set_favorite(&fixture, &cookie, &a, true).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        set_favorite(&fixture, &cookie, &c, true).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(favorite_names(&fixture, &cookie).await, ["A", "C"]);
    let (status, view_b) = fixture
        .call("GET", &format!("/views/{b}"), &cookie, None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view_b["is_favorite"], false);
    assert_eq!(view_b["favorite_position"], Value::Null);

    assert_eq!(
        set_favorite(&fixture, &cookie, &a, false).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        set_favorite(&fixture, &cookie, &a, false).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(favorite_names(&fixture, &cookie).await, ["C"]);

    assert_eq!(
        set_favorite(&fixture, &cookie, &b, true).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        set_favorite(&fixture, &cookie, &a, true).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(favorite_names(&fixture, &cookie).await, ["C", "B", "A"]);
}

#[tokio::test]
async fn favorites_are_per_user_and_limited_to_visible_views() {
    let fixture = Fixture::new().await;
    let (_, member) = fixture
        .add_user("member@example.com", "Member", Some("member"))
        .await;
    let shared = id_of(
        &fixture
            .create_view(&fixture.owner_cookie, "Shared", "workspace")
            .await,
    );
    let private = id_of(
        &fixture
            .create_view(&fixture.owner_cookie, "Private", "personal")
            .await,
    );

    assert_eq!(
        set_favorite(&fixture, &member, &shared, true).await,
        StatusCode::NO_CONTENT
    );
    let (status, as_member) = fixture
        .call("GET", &format!("/views/{shared}"), &member, None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(as_member["is_favorite"], true);
    assert_eq!(as_member["favorite_position"], 0);
    let (status, as_owner) = fixture
        .call(
            "GET",
            &format!("/views/{shared}"),
            &fixture.owner_cookie,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(as_owner["is_favorite"], false);

    assert_eq!(
        set_favorite(&fixture, &member, &private, true).await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        set_favorite(&fixture, &member, &private, false).await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        set_favorite(&fixture, &member, &Id::new_v7().to_string(), true).await,
        StatusCode::NOT_FOUND
    );
    let (_, outsider) = fixture
        .add_user("outsider@example.com", "Outsider", None)
        .await;
    assert_eq!(
        set_favorite(&fixture, &outsider, &shared, true).await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        reorder(&fixture, &outsider, json!([])).await.0,
        StatusCode::NOT_FOUND
    );
}

/// Deferred from Task 5's review: a favorite is scoped to the caller who set it, both on the
/// single-view fetch and in the list.
#[tokio::test]
async fn a_favorite_is_not_visible_to_other_callers() {
    let fixture = Fixture::new().await;
    let (_, member) = fixture
        .add_user("member@example.com", "Member", Some("member"))
        .await;
    let shared = id_of(
        &fixture
            .create_view(&fixture.owner_cookie, "Shared", "workspace")
            .await,
    );

    assert_eq!(
        set_favorite(&fixture, &fixture.owner_cookie, &shared, true).await,
        StatusCode::NO_CONTENT
    );

    let (status, as_member) = fixture
        .call("GET", &format!("/views/{shared}"), &member, None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(as_member["is_favorite"], false);
    assert_eq!(as_member["favorite_position"], Value::Null);

    let (status, list) = fixture.call("GET", "/views", &member, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["id"], shared.as_str());
    assert_eq!(list[0]["is_favorite"], false);
    assert_eq!(list[0]["favorite_position"], Value::Null);

    let (status, as_owner) = fixture
        .call(
            "GET",
            &format!("/views/{shared}"),
            &fixture.owner_cookie,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(as_owner["is_favorite"], true);
    assert_eq!(as_owner["favorite_position"], 0);
}

#[tokio::test]
async fn reordering_requires_exactly_the_callers_favorites() {
    let fixture = Fixture::new().await;
    let cookie = fixture.owner_cookie.clone();
    let a = id_of(&fixture.create_view(&cookie, "A", "workspace").await);
    let b = id_of(&fixture.create_view(&cookie, "B", "personal").await);
    let c = id_of(&fixture.create_view(&cookie, "C", "workspace").await);
    let d = id_of(&fixture.create_view(&cookie, "D", "workspace").await);
    for id in [&a, &b, &c] {
        assert_eq!(
            set_favorite(&fixture, &cookie, id, true).await,
            StatusCode::NO_CONTENT
        );
    }

    let (status, _) = reorder(&fixture, &cookie, json!([c, a, b])).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(favorite_names(&fixture, &cookie).await, ["C", "A", "B"]);

    for bad in [
        json!([c, a]),
        json!([c, a, b, d]),
        json!([c, c, a, b]),
        json!([]),
        json!([c, a, "not-an-id"]),
    ] {
        let (status, problem) = reorder(&fixture, &cookie, bad).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");
        assert_eq!(problem["code"], "validation_failed");
        assert_eq!(problem["detail"], "view_ids");
    }
    assert_eq!(favorite_names(&fixture, &cookie).await, ["C", "A", "B"]);

    let (_, member) = fixture
        .add_user("member@example.com", "Member", Some("member"))
        .await;
    assert_eq!(
        reorder(&fixture, &member, json!([])).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        reorder(&fixture, &member, json!([c])).await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
}

#[tokio::test]
async fn favorites_follow_view_visibility_and_deletion() {
    let fixture = Fixture::new().await;
    let (_, author) = fixture
        .add_user("author@example.com", "Author", Some("member"))
        .await;
    let (_, reader) = fixture
        .add_user("reader@example.com", "Reader", Some("member"))
        .await;
    let shared = id_of(&fixture.create_view(&author, "Shared", "workspace").await);
    let other = id_of(&fixture.create_view(&author, "Other", "workspace").await);
    assert_eq!(
        set_favorite(&fixture, &reader, &shared, true).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        set_favorite(&fixture, &reader, &other, true).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        set_favorite(&fixture, &author, &shared, true).await,
        StatusCode::NO_CONTENT
    );

    let (status, _) = fixture
        .call(
            "PATCH",
            &format!("/views/{shared}"),
            &author,
            Some(json!({"expected_version": 1, "visibility": "personal"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(favorite_names(&fixture, &reader).await, ["Other"]);
    assert_eq!(favorite_names(&fixture, &author).await, ["Shared"]);
    assert_eq!(
        reorder(&fixture, &reader, json!([other])).await.0,
        StatusCode::NO_CONTENT
    );

    assert_eq!(
        fixture
            .call(
                "DELETE",
                &format!("/views/{other}"),
                &fixture.owner_cookie,
                None
            )
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert!(favorite_names(&fixture, &reader).await.is_empty());
    assert_eq!(
        reorder(&fixture, &reader, json!([])).await.0,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn favorite_changes_are_broadcast_only_when_something_changes() {
    let fixture = Fixture::new().await;
    let cookie = fixture.owner_cookie.clone();
    let a = id_of(&fixture.create_view(&cookie, "A", "workspace").await);
    let start = fixture.sequence().await;
    assert_eq!(
        set_favorite(&fixture, &cookie, &a, true).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(fixture.sequence().await, start + 1);
    assert_eq!(
        set_favorite(&fixture, &cookie, &a, true).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(fixture.sequence().await, start + 1);
    assert_eq!(
        reorder(&fixture, &cookie, json!([a])).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(fixture.sequence().await, start + 1);
    assert_eq!(
        set_favorite(&fixture, &cookie, &a, false).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(fixture.sequence().await, start + 2);
    let changes: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events WHERE action = 'saved_view.favorites_changed'",
    )
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(changes, 2);
}

async fn preference(
    fixture: &Fixture,
    method: &str,
    cookie: &str,
    page_key: &str,
    state: Option<Value>,
) -> (StatusCode, Value) {
    fixture
        .call(
            method,
            &format!("/view-preferences/{page_key}"),
            cookie,
            state.map(|state| json!({"state": state})),
        )
        .await
}

#[tokio::test]
async fn view_preferences_round_trip_and_upsert() {
    let fixture = Fixture::new().await;
    let cookie = fixture.owner_cookie.clone();
    let (status, problem) = preference(&fixture, "GET", &cookie, "all", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(problem["code"], "task_resource_not_found");

    let (status, saved) = preference(&fixture, "PUT", &cookie, "all", Some(view_state())).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["page_key"], "all");
    assert_eq!(saved["state"], view_state());
    assert_eq!(saved["state_error"], Value::Null);
    let (status, fetched) = preference(&fixture, "GET", &cookie, "all", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fetched, saved);

    let (status, _) = preference(&fixture, "PUT", &cookie, "all", Some(other_state())).await;
    assert_eq!(status, StatusCode::OK);
    let (status, fetched) = preference(&fixture, "GET", &cookie, "all", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fetched["state"], other_state());
    let rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM view_preferences WHERE workspace_id = ? AND user_id = ?",
    )
    .bind(&fixture.workspace_id)
    .bind(fixture.owner_id.to_string())
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(rows, 1);
}

#[tokio::test]
async fn view_preference_page_keys_are_validated() {
    let fixture = Fixture::new().await;
    let cookie = fixture.owner_cookie.clone();
    let project_id: String =
        sqlx::query_scalar("SELECT id FROM projects WHERE workspace_id = ? LIMIT 1")
            .bind(&fixture.workspace_id)
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    let invalid = [
        "everything".to_owned(),
        "all:extra".to_owned(),
        "project:".to_owned(),
        "project:nope".to_owned(),
        format!("project:{}", project_id.to_uppercase()),
        "preset:".to_owned(),
        "preset:bogus".to_owned(),
    ];
    for key in &invalid {
        for (method, state) in [("GET", None), ("PUT", Some(view_state()))] {
            let (status, problem) = preference(&fixture, method, &cookie, key, state).await;
            assert_eq!(
                status,
                StatusCode::UNPROCESSABLE_ENTITY,
                "{method} {key}: {problem}"
            );
            assert_eq!(problem["detail"], "page_key");
        }
    }

    let valid = [
        format!("project:{project_id}"),
        "preset:mine".to_owned(),
        "preset:overdue".to_owned(),
        "preset:due_soon".to_owned(),
        "preset:current_week".to_owned(),
        "preset:my_week".to_owned(),
    ];
    for key in &valid {
        let (status, saved) = preference(&fixture, "PUT", &cookie, key, Some(view_state())).await;
        assert_eq!(status, StatusCode::OK, "{key}: {saved}");
        assert_eq!(saved["page_key"], key.as_str());
    }
    let (status, encoded) = preference(
        &fixture,
        "GET",
        &cookie,
        &format!("project%3A{project_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{encoded}");
    assert_eq!(encoded["page_key"], format!("project:{project_id}"));

    // a well-formed id that is not a project of this workspace gets no row
    let unknown = format!("project:{}", Id::new_v7());
    let (status, problem) =
        preference(&fixture, "PUT", &cookie, &unknown, Some(view_state())).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{problem}");
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM view_preferences WHERE page_key = ?")
        .bind(&unknown)
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(rows, 0);
}

#[tokio::test]
async fn view_preferences_are_per_caller() {
    let fixture = Fixture::new().await;
    let owner = fixture.owner_cookie.clone();
    let (_, member) = fixture
        .add_user("member@example.com", "Member", Some("member"))
        .await;
    let (_, outsider) = fixture
        .add_user("outsider@example.com", "Outsider", None)
        .await;

    assert_eq!(
        preference(&fixture, "PUT", &owner, "all", Some(view_state()))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        preference(&fixture, "GET", &member, "all", None).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        preference(&fixture, "PUT", &member, "all", Some(other_state()))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        preference(&fixture, "GET", &owner, "all", None).await.1["state"],
        view_state()
    );
    assert_eq!(
        preference(&fixture, "GET", &member, "all", None).await.1["state"],
        other_state()
    );
    assert_eq!(
        preference(&fixture, "GET", &outsider, "all", None).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        preference(&fixture, "PUT", &outsider, "all", Some(view_state()))
            .await
            .0,
        StatusCode::NOT_FOUND
    );

    let other = WorkspaceRepository::new((*fixture.database).clone())
        .create(
            fixture.owner_id,
            "Elsewhere".to_owned(),
            "views-test",
            TimestampMillis::now(),
        )
        .await
        .unwrap()
        .id
        .to_string();
    let (status, _) = fixture
        .call_in(&other, "GET", "/view-preferences/all", &owner, None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn view_preference_state_is_validated_and_never_broadcast() {
    let fixture = Fixture::new().await;
    let cookie = fixture.owner_cookie.clone();
    let start = fixture.sequence().await;
    let (status, problem) =
        preference(&fixture, "PUT", &cookie, "all", Some(state_with_text(201))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");
    assert!(
        problem["path"]
            .as_str()
            .unwrap()
            .starts_with("filter.children[0]"),
        "{problem}"
    );
    assert_eq!(
        preference(&fixture, "PUT", &cookie, "all", Some(view_state()))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(fixture.sequence().await, start);
    let (status, _) = fixture
        .call(
            "PUT",
            "/view-preferences/all",
            &cookie,
            Some(json!({"state": view_state(), "extra": 1})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn unreadable_stored_preference_is_returned_with_a_state_error() {
    let fixture = Fixture::new().await;
    sqlx::query(
        "INSERT INTO view_preferences (workspace_id, user_id, page_key, state_json, updated_at) \
         VALUES (?, ?, 'preset:overdue', '{\"filter\":', 0)",
    )
    .bind(&fixture.workspace_id)
    .bind(fixture.owner_id.to_string())
    .execute(fixture.database.pool())
    .await
    .unwrap();
    let (status, stored) = preference(
        &fixture,
        "GET",
        &fixture.owner_cookie,
        "preset:overdue",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{stored}");
    assert_eq!(stored["page_key"], "preset:overdue");
    assert_eq!(stored["state"], Value::Null);
    assert!(!stored["state_error"].as_str().unwrap().is_empty());
}

#[tokio::test]
async fn views_store_the_sub_issues_display_and_default_it_to_nested() {
    let fixture = Fixture::new().await;
    let cookie = fixture.owner_cookie.clone();
    let mut hidden = view_state();
    hidden["display"]["sub_issues"] = json!("hidden");
    hidden["display"]["properties"] = json!(["id", "status", "sub_issue_progress"]);
    let (status, created) = fixture
        .call(
            "POST",
            "/views",
            &cookie,
            Some(json!({"name": "Roots", "visibility": "personal", "state": hidden})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["state"], hidden);

    // States saved before sub-issues existed read as nested.
    let mut legacy = view_state();
    legacy["display"]
        .as_object_mut()
        .unwrap()
        .remove("sub_issues");
    let (status, created) = fixture
        .call(
            "POST",
            "/views",
            &cookie,
            Some(json!({"name": "Legacy", "visibility": "personal", "state": legacy})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["state"]["display"]["sub_issues"], "nested");

    // POST /views deserializes `state` straight into the typed `ViewState` (as it already does
    // for every other display enum, e.g. `layout`); an unknown enum literal fails that typed
    // parse before `validate_view_state` runs, so it surfaces as the generic 400 used for any
    // malformed body, not a path-carrying 422. (`parse_view_state`'s path-based errors are used
    // when re-reading stored state, e.g. `decode_state`, not on this write path.)
    let mut invalid = view_state();
    invalid["display"]["sub_issues"] = json!("tree");
    let (status, _problem) = fixture
        .call(
            "POST",
            "/views",
            &cookie,
            Some(json!({"name": "Bad", "visibility": "personal", "state": invalid})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
