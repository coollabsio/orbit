//! Project details, milestones and owned pages (migration 0045).

use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{AuthenticatedUser, Id, PasswordService, TestDatabase, TimestampMillis};
use orbit_server::auth_routes::CookieMode;
use orbit_server::page_routes::{PageState, page_router};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::workspaces::WorkspaceRepository;
use orbit_server::task_routes::{TaskState, task_router};
use orbit_server::view_routes::view_router;
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
        let app = task_router(TaskState::new(Arc::clone(&identity), CookieMode::secure()))
            .merge(page_router(PageState::new(
                Arc::clone(&identity),
                CookieMode::secure(),
            )))
            .merge(view_router(TaskState::new(
                Arc::clone(&identity),
                CookieMode::secure(),
            )));
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

#[tokio::test]
async fn lead_and_members_are_active_members_and_follow_removal_and_suspension() {
    let fixture = Fixture::new().await;
    let (lead, _) = fixture.add_member("lead@example.com", "member").await;
    let (member, _) = fixture.add_member("member@example.com", "member").await;

    let project = fixture.project().await;
    assert_eq!(project["lead_user_id"], Value::Null);
    assert_eq!(project["member_ids"], json!([]));

    let (status, project) = fixture
        .patch_project(json!({"lead_user_id": lead, "member_ids": [lead, member]}))
        .await;
    assert_eq!(status, StatusCode::OK, "{project}");
    assert_eq!(project["lead_user_id"], json!(lead));
    assert_eq!(project["member_ids"].as_array().unwrap().len(), 2);

    // Absent fields change nothing.
    let (_, project) = fixture.patch_project(json!({})).await;
    assert_eq!(project["lead_user_id"], json!(lead));
    assert_eq!(project["member_ids"].as_array().unwrap().len(), 2);

    // A person who is not a member of the workspace is refused.
    let outsider = Id::new_v7();
    let (status, _) = fixture
        .patch_project(json!({"lead_user_id": outsider}))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = fixture
        .patch_project(json!({"member_ids": [outsider]}))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // The audit row names the changed people.
    let metadata: String = sqlx::query_scalar(
        "SELECT metadata_json FROM audit_events WHERE action = 'project.updated' \
         AND metadata_json LIKE '%lead%' LIMIT 1",
    )
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    assert!(metadata.contains("members"), "{metadata}");

    // Suspension clears the lead and the membership row.
    sqlx::query("UPDATE users SET suspended_at = 1 WHERE id = ?")
        .bind(lead.to_string())
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let project = fixture.project().await;
    assert_eq!(project["lead_user_id"], Value::Null);
    assert_eq!(project["member_ids"], json!([member]));

    // Removal from the workspace clears both too.
    let (_, project) = fixture.patch_project(json!({"lead_user_id": member})).await;
    assert_eq!(project["lead_user_id"], json!(member));
    sqlx::query("DELETE FROM memberships WHERE user_id = ?")
        .bind(member.to_string())
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let project = fixture.project().await;
    assert_eq!(project["lead_user_id"], Value::Null);
    assert_eq!(project["member_ids"], json!([]));

    // `null` clears the lead.
    let (_, project) = fixture
        .patch_project(json!({"lead_user_id": fixture.owner_id}))
        .await;
    assert_eq!(project["lead_user_id"], json!(fixture.owner_id));
    let (_, project) = fixture.patch_project(json!({"lead_user_id": null})).await;
    assert_eq!(project["lead_user_id"], Value::Null);
}

#[tokio::test]
async fn project_counts_and_milestone_progress_follow_the_status_categories() {
    let fixture = Fixture::new().await;
    let milestone = fixture.create_milestone("v1").await;
    let milestone_id = id_of(&milestone);
    let with = json!({"milestone_id": milestone_id});
    fixture.create_task("open", "unstarted", with.clone()).await;
    fixture.create_task("done", "completed", with.clone()).await;
    fixture
        .create_task("dropped", "cancelled", with.clone())
        .await;
    let trashed = fixture.create_task("trashed", "completed", with).await;
    fixture.create_task("other", "started", json!({})).await;
    let (status, _) = fixture
        .call(
            "DELETE",
            &fixture.uri(&format!("/tasks/{}?expected_version=0", id_of(&trashed))),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let project = fixture.project().await;
    assert_eq!(
        project["task_counts"],
        json!({"unstarted": 1, "started": 1, "completed": 1, "cancelled": 1})
    );
    let (_, list) = fixture.call("GET", &fixture.uri("/projects"), None).await;
    assert_eq!(list["items"][0]["task_counts"]["completed"], 1);

    // Total excludes cancelled, duplicate and trashed tasks; done is the completed category.
    let (_, milestones) = fixture.call("GET", &fixture.uri("/milestones"), None).await;
    let item = &milestones["items"][0];
    assert_eq!(item["id"], milestone_id);
    assert_eq!(item["task_count"], 2);
    assert_eq!(item["task_done_count"], 1);
    assert_eq!(item["health"], Value::Null);
}

#[tokio::test]
async fn milestone_rules_dates_status_versions_and_delete() {
    let fixture = Fixture::new().await;
    let now = TimestampMillis::now().as_millis();

    let (status, _) = fixture
        .call(
            "POST",
            &fixture.project_uri("/milestones"),
            Some(json!({"name": "bad", "start_at": iso(now + DAY), "target_at": iso(now)})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = fixture
        .call(
            "POST",
            &fixture.project_uri("/milestones"),
            Some(json!({"name": "bad", "status": "done"})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let milestone = fixture.create_milestone("v1").await;
    assert_eq!(milestone["status"], "planned");
    assert_eq!(milestone["position"], 0);
    assert_eq!(fixture.create_milestone("v2").await["position"], 1);
    let uri = fixture.project_uri(&format!("/milestones/{}", id_of(&milestone)));

    let (status, updated) = fixture
        .call(
            "PATCH",
            &uri,
            Some(json!({
                "expected_version": 0, "status": "completed",
                "start_at": iso(now), "target_at": iso(now + 7 * DAY)
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["version"], 1);
    assert!(updated["completed_at"].is_string());
    assert_eq!(updated["target_at"], iso(now + 7 * DAY));

    // A stale version returns the current record; the target cannot move before the start.
    let (status, conflict) = fixture
        .call(
            "PATCH",
            &uri,
            Some(json!({"expected_version": 0, "name": "x"})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(conflict["conflict"]["current"]["version"], 1);
    let (status, _) = fixture
        .call(
            "PATCH",
            &uri,
            Some(json!({"expected_version": 1, "target_at": iso(now - DAY)})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // Reopening clears the completion time; `null` clears a date.
    let (_, reopened) = fixture
        .call(
            "PATCH",
            &uri,
            Some(json!({"expected_version": 1, "status": "in_progress", "start_at": null})),
        )
        .await;
    assert_eq!(reopened["completed_at"], Value::Null);
    assert_eq!(reopened["start_at"], Value::Null);
    assert_eq!(reopened["target_at"], iso(now + 7 * DAY));

    // Delete keeps the tasks and clears their milestone.
    let task = fixture
        .create_task("t", "unstarted", json!({"milestone_id": id_of(&milestone)}))
        .await;
    assert_eq!(task["milestone_id"], milestone["id"]);
    let (status, _) = fixture
        .call("DELETE", &format!("{uri}?expected_version=2"), None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, task) = fixture
        .call(
            "GET",
            &fixture.uri(&format!("/tasks/{}", id_of(&task))),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(task["milestone_id"], Value::Null);
}

#[tokio::test]
async fn task_milestone_is_in_the_task_project_and_a_move_clears_it() {
    let fixture = Fixture::new().await;
    let milestone = fixture.create_milestone("v1").await;
    let (status, other) = fixture
        .call(
            "POST",
            &fixture.uri("/projects"),
            Some(json!({"name": "Other", "key": "OTH", "color": "#123456"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let other_id = id_of(&other).to_owned();
    let (_, foreign) = fixture
        .call(
            "POST",
            &fixture.uri(&format!("/projects/{other_id}/milestones")),
            Some(json!({"name": "foreign"})),
        )
        .await;

    // A milestone of a different project is refused on create and on update.
    let unstarted = fixture.status_id(&fixture.project_id, "unstarted").await;
    let (status, _) = fixture
        .call(
            "POST",
            &fixture.uri("/tasks"),
            Some(json!({
                "project_id": fixture.project_id, "status_id": unstarted, "title": "x",
                "milestone_id": id_of(&foreign)
            })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let task = fixture.create_task("t", "unstarted", json!({})).await;
    let task_uri = fixture.uri(&format!("/tasks/{}", id_of(&task)));
    let (status, _) = fixture
        .call(
            "PATCH",
            &task_uri,
            Some(json!({"expected_version": 0, "milestone_id": id_of(&foreign)})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let (status, set) = fixture
        .call(
            "PATCH",
            &task_uri,
            Some(json!({"expected_version": 0, "milestone_id": id_of(&milestone)})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{set}");
    assert_eq!(set["milestone_id"], milestone["id"]);

    // The activity feed names the change.
    let (_, activity) = fixture
        .call("GET", &format!("{task_uri}/activity"), None)
        .await;
    assert!(activity.to_string().contains("\"milestone\""), "{activity}");

    // The filter finds it; `is_empty` does not.
    let query = |filter: Value| {
        let fixture = &fixture;
        async move {
            let (status, page) = fixture
                .call(
                    "POST",
                    &fixture.uri("/tasks/query"),
                    Some(json!({
                        "filter": {"op": "and", "children": [filter]},
                        "order_by": "created", "order_direction": "asc", "show_completed": "all"
                    })),
                )
                .await;
            assert_eq!(status, StatusCode::OK, "{page}");
            page["items"].as_array().unwrap().len()
        }
    };
    let by_id = json!({"field": "milestone", "operator": "is", "value": [id_of(&milestone)]});
    assert_eq!(query(by_id).await, 1);
    assert_eq!(
        query(json!({"field": "milestone", "operator": "is_empty"})).await,
        0
    );

    // The bulk route sets and clears it.
    let (status, bulk) = fixture
        .call(
            "POST",
            &fixture.uri("/tasks/bulk"),
            Some(json!({"updates": [{"id": id_of(&task), "expected_version": 1, "milestone_id": null}]})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{bulk}");
    let (_, set) = fixture
        .call(
            "PATCH",
            &task_uri,
            Some(json!({"expected_version": 2, "milestone_id": id_of(&milestone)})),
        )
        .await;
    assert_eq!(set["milestone_id"], milestone["id"]);

    // A move to a different project clears the milestone.
    let (status, moved) = fixture
        .call(
            "PATCH",
            &task_uri,
            Some(json!({"expected_version": 3, "project_id": other_id})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{moved}");
    assert_eq!(moved["project_id"], other["id"]);
    assert_eq!(moved["milestone_id"], Value::Null);

    // The database refuses a milestone of a different project.
    let error = sqlx::query("UPDATE tasks SET milestone_id = ? WHERE id = ?")
        .bind(id_of(&milestone))
        .bind(id_of(&task))
        .execute(fixture.database.pool())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("task milestone"), "{error}");
}

#[tokio::test]
async fn updates_set_the_health_and_only_the_author_or_a_moderator_changes_them() {
    let fixture = Fixture::new().await;
    let (author, member_cookie) = fixture.add_member("author@example.com", "member").await;
    let (other, other_cookie) = fixture.add_member("other@example.com", "member").await;
    fixture.add_member("outside@example.com", "member").await;
    let (status, _) = fixture
        .patch_project(json!({"lead_user_id": fixture.owner_id, "member_ids": [author, other]}))
        .await;
    assert_eq!(status, StatusCode::OK);
    let milestone = fixture.create_milestone("v1").await;
    let updates = fixture.project_uri(&format!("/milestones/{}/updates", id_of(&milestone)));

    let (status, _) = fixture
        .call(
            "POST",
            &updates,
            Some(json!({"health": "great", "body": "x"})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let (status, first) = fixture
        .call_as(
            &member_cookie,
            "POST",
            &updates,
            Some(json!({"health": "on_track", "body": "All good"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{first}");
    assert_eq!(first["can_edit"], true);
    let (_, second) = fixture
        .call_as(
            &member_cookie,
            "POST",
            &updates,
            Some(json!({"health": "at_risk", "body": "Slipping"})),
        )
        .await;

    // Each update notifies the project lead and members, and not the author: one row for each
    // update. The owner is the lead; the other member is a member; a person with no role in the
    // project gets nothing.
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT users.email, notifications.kind FROM notifications \
         JOIN users ON users.id = notifications.recipient_user_id \
         WHERE notifications.milestone_id = ? ORDER BY notifications.created_at, users.email",
    )
    .bind(id_of(&milestone))
    .fetch_all(fixture.database.pool())
    .await
    .unwrap();
    let recipients: Vec<&str> = rows.iter().map(|row| row.0.as_str()).collect();
    assert_eq!(
        recipients,
        [
            "other@example.com",
            "owner@example.com",
            "other@example.com",
            "owner@example.com"
        ]
    );
    assert!(rows.iter().all(|row| row.1 == "milestone_update_posted"));
    let (status, inbox) = fixture
        .call("GET", &fixture.uri("/notifications"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{inbox}");
    let item = &inbox["items"][0];
    assert_eq!(item["kind"], "milestone_update_posted");
    assert_eq!(item["milestone_id"], milestone["id"]);
    assert_eq!(item["milestone_name"], "v1");
    assert_eq!(item["milestone_project_id"], fixture.project_id.as_str());

    // The milestone shows the health of the most recent update.
    let health = || async {
        let (_, list) = fixture
            .call("GET", &fixture.project_uri("/milestones"), None)
            .await;
        list["items"][0]["health"].clone()
    };
    assert_eq!(health().await, "at_risk");

    // Newest first; a different member cannot change them.
    let (_, feed) = fixture.call_as(&other_cookie, "GET", &updates, None).await;
    assert_eq!(feed["items"][0]["id"], second["id"]);
    assert_eq!(feed["items"][0]["can_edit"], false);
    assert_eq!(feed["items"][0]["can_delete"], false);
    let second_uri = format!("{updates}/{}", id_of(&second));
    let (status, _) = fixture
        .call_as(
            &other_cookie,
            "PATCH",
            &second_uri,
            Some(json!({"expected_version": 0, "health": "on_track", "body": "no"})),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = fixture
        .call_as(
            &other_cookie,
            "DELETE",
            &format!("{second_uri}?expected_version=0"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // The author edits; the owner (a moderator) deletes.
    let (status, edited) = fixture
        .call_as(
            &member_cookie,
            "PATCH",
            &second_uri,
            Some(json!({"expected_version": 0, "health": "off_track", "body": "Late"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{edited}");
    assert_eq!(health().await, "off_track");
    let (status, _) = fixture
        .call("DELETE", &format!("{second_uri}?expected_version=1"), None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(health().await, "on_track");
}

#[tokio::test]
async fn owned_pages_are_hidden_from_docs_and_deleted_with_their_owner() {
    let fixture = Fixture::new().await;
    let (_, member_cookie) = fixture.add_member("member@example.com", "member").await;
    let milestone = fixture.create_milestone("v1").await;
    let milestone_uri = fixture.project_uri(&format!("/milestones/{}", id_of(&milestone)));

    let (status, overview) = fixture
        .call("POST", &fixture.project_uri("/overview-page"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{overview}");
    let overview_id = overview["page_id"].as_str().unwrap().to_owned();
    // The second call returns the same page.
    let (_, again) = fixture
        .call_as(
            &member_cookie,
            "POST",
            &fixture.project_uri("/overview-page"),
            None,
        )
        .await;
    assert_eq!(again["page_id"], overview_id.as_str());
    assert_eq!(
        fixture.project().await["overview_page_id"],
        overview_id.as_str()
    );
    let (status, description) = fixture
        .call("POST", &format!("{milestone_uri}/description-page"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{description}");
    let description_id = description["page_id"].as_str().unwrap().to_owned();

    // Give the page text, so that the search would find it.
    sqlx::query(
        "UPDATE pages SET title = 'Zebracorn', content_text = 'zebracorn plan' WHERE id = ?",
    )
    .bind(&overview_id)
    .execute(fixture.database.pool())
    .await
    .unwrap();

    // Every workspace member reads an owned page by id.
    for cookie in [&fixture.owner_cookie, &member_cookie] {
        let (status, page) = fixture
            .call_as(
                cookie,
                "GET",
                &fixture.uri(&format!("/pages/{overview_id}")),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{page}");
    }

    // Docs does not list it anywhere.
    for path in [
        "/pages",
        "/pages/trash",
        "/pages/recent",
        "/pages/search?q=zebracorn",
    ] {
        let (status, body) = fixture.call("GET", &fixture.uri(path), None).await;
        assert_eq!(status, StatusCode::OK, "{path}: {body}");
        assert!(
            !body.to_string().contains(&overview_id),
            "{path} lists an owned page: {body}"
        );
    }
    // Docs actions refuse it: favourite, trash, sub-page, export.
    let (status, _) = fixture
        .call(
            "PUT",
            &fixture.uri(&format!("/pages/{overview_id}/favorite")),
            None,
        )
        .await;
    assert_ne!(status, StatusCode::OK);
    assert_ne!(status, StatusCode::NO_CONTENT);
    let (status, _) = fixture
        .call(
            "DELETE",
            &fixture.uri(&format!("/pages/{overview_id}?expected_version=0")),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = fixture
        .call(
            "POST",
            &fixture.uri("/pages"),
            Some(json!({"title": "child", "parent_id": overview_id})),
        )
        .await;
    assert!(status.is_client_error(), "{status}");

    let page_exists = |id: String| {
        let pool = fixture.database.pool().clone();
        async move {
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM pages WHERE id = ?")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap()
        }
    };

    // The delete of a milestone deletes its page.
    let (status, _) = fixture
        .call(
            "DELETE",
            &format!("{milestone_uri}?expected_version=0"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(page_exists(description_id).await, 0);

    // The purge of a project deletes its page, and the page of each milestone.
    let second = fixture.create_milestone("v2").await;
    let (_, second_page) = fixture
        .call(
            "POST",
            &fixture.project_uri(&format!("/milestones/{}/description-page", id_of(&second))),
            None,
        )
        .await;
    let second_page_id = second_page["page_id"].as_str().unwrap().to_owned();
    let version = fixture.project().await["version"].as_u64().unwrap();
    let (status, _) = fixture
        .call(
            "DELETE",
            &fixture.project_uri(&format!("?expected_version={version}")),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(page_exists(overview_id.clone()).await, 1);
    sqlx::query("UPDATE projects SET deleted_at = 1 WHERE id = ?")
        .bind(&fixture.project_id)
        .execute(fixture.database.pool())
        .await
        .unwrap();
    WorkspaceRepository::new((*fixture.database).clone())
        .purge_retention(TimestampMillis::now())
        .await
        .unwrap();
    assert_eq!(page_exists(overview_id).await, 0);
    assert_eq!(page_exists(second_page_id).await, 0);
}

#[tokio::test]
async fn archived_tasks_stay_in_the_project_counts_and_the_milestone_progress() {
    let fixture = Fixture::new().await;
    let milestone = fixture.create_milestone("v1").await;
    let with = json!({"milestone_id": id_of(&milestone)});
    fixture.create_task("open", "unstarted", with.clone()).await;
    let done = fixture.create_task("done", "completed", with).await;

    let (status, body) = fixture
        .call(
            "POST",
            &fixture.uri("/tasks/archive"),
            Some(json!({"task_ids": [id_of(&done)]})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["count"], 1);
    // The archived task is out of the lists.
    let (_, list) = fixture.call("GET", &fixture.uri("/tasks"), None).await;
    assert_eq!(list["items"].as_array().unwrap().len(), 1);

    // An archived task is finished work: the numbers do not fall.
    let project = fixture.project().await;
    assert_eq!(
        project["task_counts"],
        json!({"unstarted": 1, "completed": 1})
    );
    let (_, milestones) = fixture.call("GET", &fixture.uri("/milestones"), None).await;
    assert_eq!(milestones["items"][0]["task_count"], 2);
    assert_eq!(milestones["items"][0]["task_done_count"], 1);
}

#[tokio::test]
async fn projects_and_milestones_are_favorites_with_title_colour_and_path() {
    let fixture = Fixture::new().await;
    let milestone = fixture.create_milestone("v1").await;
    let milestone_id = id_of(&milestone).to_owned();
    let project = fixture.project().await;
    let set = async |method: &str, kind: &str, id: &str| {
        fixture
            .call(
                method,
                &fixture.uri(&format!("/favorites/{kind}/{id}")),
                None,
            )
            .await
            .0
    };
    assert_eq!(
        set("PUT", "milestone", &milestone_id).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        set("PUT", "project", &fixture.project_id).await,
        StatusCode::NO_CONTENT
    );
    // An id of a different kind is not found.
    assert_eq!(
        set("PUT", "project", &milestone_id).await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        set("PUT", "milestone", &fixture.project_id).await,
        StatusCode::NOT_FOUND
    );

    let (status, list) = fixture.call("GET", &fixture.uri("/favorites"), None).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(
        list["items"],
        json!([
            {"kind": "milestone", "target_id": milestone_id, "title": "v1", "identifier": project["key"],
             "color": project["color"], "icon": null, "position": 0,
             "path": format!("/tasks/projects/{}/milestones/{milestone_id}", fixture.project_id)},
            {"kind": "project", "target_id": fixture.project_id, "title": project["name"],
             "identifier": project["key"], "color": project["color"], "icon": null, "position": 1,
             "path": format!("/tasks/projects/{}", fixture.project_id)},
        ])
    );

    // A deleted milestone takes its favorite with it; a project in the trash is not listed.
    let (status, _) = fixture
        .call(
            "DELETE",
            &fixture.project_uri(&format!(
                "/milestones/{milestone_id}?expected_version={}",
                milestone["version"]
            )),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM favorites WHERE kind = 'milestone'")
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(rows, 0);
    sqlx::query("UPDATE projects SET deleted_at = 5 WHERE id = ?")
        .bind(&fixture.project_id)
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let (_, list) = fixture.call("GET", &fixture.uri("/favorites"), None).await;
    assert_eq!(list["items"], json!([]));
}
