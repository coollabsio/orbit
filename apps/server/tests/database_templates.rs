//! Row templates (docs databases P2 item 5): a template is a hidden page of its database that
//! fills new rows, and is never a row itself.

use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{Id, PasswordService, TestDatabase, TimestampMillis};
use orbit_server::auth_routes::CookieMode;
use orbit_server::database_routes::{DatabaseState, database_router};
use orbit_server::page_routes::{PageState, page_router};
use orbit_server::repositories::databases::MAX_ROWS;
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::page_export::PageExportRepository;
use serde_json::{Value, json};
use tower::ServiceExt;

struct Fixture {
    database: TestDatabase,
    app: axum::Router,
    workspace_id: String,
    cookie: String,
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
            database_router(DatabaseState::new(
                Arc::clone(&identity),
                CookieMode::secure(),
            )),
        );
        Self {
            database,
            app,
            workspace_id: setup.workspace_id.to_string(),
            cookie: format!("__Host-orbit_session={}", setup.session.token),
            owner_id: setup.user_id,
        }
    }

    async fn call(&self, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        let builder = Request::builder()
            .method(method)
            .uri(format!("/api/v1/workspaces/{}/{path}", self.workspace_id))
            .header(header::COOKIE, &self.cookie);
        let request = match body {
            Some(value) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(value.to_string())),
            None => builder.body(Body::empty()),
        }
        .unwrap();
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

    async fn ok(&self, method: &str, path: &str, body: Option<Value>) -> Value {
        let (status, value) = self.call(method, path, body).await;
        assert!(status.is_success(), "{method} {path}: {status} {value}");
        value
    }

    async fn create_database(&self, title: &str) -> String {
        let page = self
            .ok(
                "POST",
                "pages",
                Some(json!({"kind": "database", "title": title})),
            )
            .await;
        id_of(&page)
    }

    async fn database(&self, id: &str) -> Value {
        self.ok("GET", &format!("databases/{id}"), None).await
    }

    async fn add_property(&self, database: &str, name: &str, kind: &str) -> String {
        let property = self
            .ok(
                "POST",
                &format!("databases/{database}/properties"),
                Some(json!({"name": name, "type": kind})),
            )
            .await;
        id_of(&property)
    }

    async fn add_row(&self, database: &str, body: Value) -> Value {
        self.ok("POST", &format!("databases/{database}/rows"), Some(body))
            .await
    }

    async fn add_template(&self, database: &str, body: Value) -> Value {
        self.ok(
            "POST",
            &format!("databases/{database}/templates"),
            Some(body),
        )
        .await
    }

    async fn set_values(&self, database: &str, row: &str, values: Value) -> (StatusCode, Value) {
        self.call(
            "PATCH",
            &format!("databases/{database}/rows/{row}/values"),
            Some(values),
        )
        .await
    }

    async fn edit_page(&self, page: &str, changes: Value) {
        let version = self.ok("GET", &format!("pages/{page}"), None).await["version"].clone();
        let mut body = changes;
        body["expected_version"] = version;
        self.ok("PATCH", &format!("pages/{page}"), Some(body)).await;
    }

    async fn first_view(&self, database: &str) -> Value {
        self.database(database).await["views"][0].clone()
    }

    async fn set_default(&self, database: &str, template: Option<&str>) -> (StatusCode, Value) {
        let view = self.first_view(database).await;
        let mut config = view["config"].clone();
        config["default_template_id"] = json!(template);
        self.call(
            "PATCH",
            &format!("databases/{database}/views/{}", id_of(&view)),
            Some(json!({"config": config})),
        )
        .await
    }
}

fn id_of(value: &Value) -> String {
    value["id"].as_str().unwrap().to_owned()
}

fn ids(items: &Value) -> Vec<String> {
    items.as_array().unwrap().iter().map(id_of).collect()
}

fn paragraph(text: &str) -> Value {
    json!([{
        "id": "b1",
        "type": "paragraph",
        "props": {"backgroundColor": "default", "textColor": "default", "textAlignment": "left"},
        "content": [{"type": "text", "text": text, "styles": {}}],
        "children": [],
    }])
}

/// A database with a row "Kept row" and a template "Weekly" titled "Hidden template" whose body,
/// title and text value all hold the word "zebra".
async fn database_with_template(fixture: &Fixture) -> (String, String, String, String) {
    let db = fixture.create_database("ITEM5 test").await;
    let notes = fixture.add_property(&db, "Notes", "text").await;
    let row = id_of(&fixture.add_row(&db, json!({"title": "Kept row"})).await);
    let template = id_of(&fixture.add_template(&db, json!({"name": "Weekly"})).await);
    fixture
        .edit_page(
            &template,
            json!({"title": "Hidden zebra template", "content": paragraph("zebra body")}),
        )
        .await;
    let (status, _) = fixture
        .set_values(&db, &template, json!({ notes.clone(): "zebra notes" }))
        .await;
    assert_eq!(status, StatusCode::OK);
    (db, notes, row, template)
}

#[tokio::test]
async fn a_template_is_never_a_row() {
    let fixture = Fixture::new().await;
    let (db, _, row, template) = database_with_template(&fixture).await;

    let detail = fixture.database(&db).await;
    assert_eq!(ids(&detail["rows"]), vec![row.clone()]);
    assert_eq!(detail["database"]["row_count"], 1);
    assert_eq!(ids(&detail["templates"]), vec![template.clone()]);
    assert_eq!(detail["templates"][0]["name"], "Weekly");
    assert_eq!(detail["templates"][0]["title"], "Hidden zebra template");
    assert_eq!(detail["templates"][0]["has_content"], true);

    // The page itself opens (its editor is the row page) and names its database.
    let page = fixture.ok("GET", &format!("pages/{template}"), None).await;
    assert_eq!(page["database_id"], json!(db));

    // Not in search (title, body or value), the tree, recent pages or favorites.
    let found = fixture.ok("GET", "pages/search?q=zebra", None).await;
    assert_eq!(found["items"], json!([]), "search finds no template");
    let tree = fixture.ok("GET", "pages", None).await;
    assert!(!tree.to_string().contains(&template), "tree: {tree}");
    fixture
        .ok("POST", &format!("pages/{template}/visit"), None)
        .await;
    let recent = fixture.ok("GET", "pages/recent", None).await;
    assert!(!recent.to_string().contains(&template), "recent: {recent}");
    let (status, _) = fixture
        .call("PUT", &format!("pages/{template}/favorite"), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "a template is no favorite");
    let favorites = fixture.ok("GET", "pages/favorites", None).await;
    assert_eq!(favorites["items"], json!([]));

    // Row writes do not reach it: trash skips it, a move cannot go after it, and the row move
    // route does not know it.
    fixture
        .ok(
            "POST",
            &format!("databases/{db}/rows/trash"),
            Some(json!({"row_ids": [template]})),
        )
        .await;
    assert_eq!(
        ids(&fixture.database(&db).await["templates"]),
        vec![template.clone()]
    );
    let (status, _) = fixture
        .call(
            "POST",
            &format!("databases/{db}/rows/{row}/move"),
            Some(json!({"after_row_id": template})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = fixture
        .call(
            "POST",
            &format!("databases/{db}/rows/{template}/move"),
            Some(json!({"after_row_id": null})),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // Nor does a page move: a template stays in its database.
    let version = fixture.ok("GET", &format!("pages/{template}"), None).await["version"].clone();
    let (status, _) = fixture
        .call(
            "POST",
            &format!("pages/{template}/move"),
            Some(json!({"expected_version": version, "parent_id": null, "position": 0})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // Export leaves it out: the database page with its sub-pages, and the whole workspace.
    let export = PageExportRepository::new((*fixture.database).clone());
    let workspace_id: Id = fixture.workspace_id.parse().unwrap();
    let source = export
        .load(
            workspace_id,
            db.parse().unwrap(),
            fixture.owner_id,
            true,
            100,
        )
        .await
        .unwrap();
    let exported: Vec<String> = source
        .pages
        .iter()
        .map(|page| page.id.to_string())
        .collect();
    assert_eq!(exported, vec![db.clone(), row.clone()]);
    let all = export
        .load_workspace(workspace_id, fixture.owner_id, "pages", false)
        .await
        .unwrap();
    assert!(all.pages.iter().all(|page| page.id.to_string() != template));
}

#[tokio::test]
async fn templates_do_not_count_toward_the_row_cap() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("ITEM5 cap").await;
    fixture.add_template(&db, json!({"name": "A"})).await;
    let (teamspace, owner_column): (Option<String>, Option<String>) =
        sqlx::query_as("SELECT teamspace_id, owner_id FROM pages WHERE id = $1")
            .bind(&db)
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    let literal =
        |value: Option<String>| value.map_or_else(|| "NULL".to_owned(), |id| format!("'{id}'"));
    let (teamspace, owner_column) = (literal(teamspace), literal(owner_column));
    // The database's rows but one.
    let now = TimestampMillis::now().as_millis();
    let ids: Vec<String> = (0..MAX_ROWS - 1)
        .map(|_| Id::new_v7().to_string())
        .collect();
    for chunk in ids.chunks(500) {
        let values = chunk
            .iter()
            .enumerate()
            .map(|(index, id)| {
                format!(
                    "('{id}', '{}', '{db}', {teamspace}, {owner_column}, 'Row', {index}, '{owner}', '{owner}', {now}, {now})",
                    fixture.workspace_id,
                    owner = fixture.owner_id,
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "INSERT INTO pages (id, workspace_id, parent_id, teamspace_id, owner_id, title, position, creator_id, \
             updated_by, created_at, updated_at) VALUES {values}"
        )))
        .execute(fixture.database.pool())
        .await
        .unwrap();
    }
    fixture.add_row(&db, json!({"title": "Last"})).await;
    let (status, body) = fixture
        .call("POST", &format!("databases/{db}/rows"), Some(json!({})))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "database_row_limit");
    // A template is not a row: one more can still be made.
    fixture.add_template(&db, json!({"name": "B"})).await;
}

#[tokio::test]
async fn a_row_from_a_template_copies_title_icon_values_and_body() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("ITEM5 fill").await;
    let notes = fixture.add_property(&db, "Notes", "text").await;
    let owner = fixture.add_property(&db, "Owner", "person").await;
    let due = fixture.add_property(&db, "Due", "date").await;
    let score = fixture.add_property(&db, "Score", "number").await;
    let template = id_of(&fixture.add_template(&db, json!({"name": "Bug"})).await);
    fixture
        .edit_page(
            &template,
            json!({"title": "Bug report", "icon": "🐞", "content": paragraph("Steps to reproduce")}),
        )
        .await;
    let (status, saved) = fixture
        .set_values(
            &db,
            &template,
            json!({
                notes.clone(): "Fill me in",
                owner.clone(): ["me"],
                due.clone(): {"start": "today"},
                score.clone(): 3,
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    // The template reads its placeholders as they were sent; the stored values hold none.
    assert_eq!(saved["values"][&owner], json!(["me"]));
    assert_eq!(
        saved["values"][&due],
        json!({"start": "today", "end": null})
    );
    let stored: i64 = sqlx::query_scalar(
        "SELECT CAST(COUNT(*) AS BIGINT) FROM page_property_values WHERE page_id = $1",
    )
    .bind(&template)
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(stored, 2, "only Notes and Score are stored values");

    let row = fixture
        .add_row(
            &db,
            json!({"template_id": template, "today": "2026-11-10", "values": {score.clone(): 5}}),
        )
        .await;
    assert_eq!(row["title"], "Bug report");
    assert_eq!(row["icon"], "🐞");
    assert_eq!(row["has_content"], true);
    assert_eq!(
        row["values"],
        json!({
            notes.clone(): "Fill me in",
            owner.clone(): [fixture.owner_id.to_string()],
            due.clone(): {"start": "2026-11-10", "end": null},
            score.clone(): 5.0,
        })
    );
    let page = fixture
        .ok("GET", &format!("pages/{}", id_of(&row)), None)
        .await;
    assert_eq!(
        page["content"][0]["content"][0]["text"],
        "Steps to reproduce"
    );

    // The request's title wins; without `today` the date is the UTC day.
    let titled = fixture
        .add_row(
            &db,
            json!({"template_id": template, "title": "Crash on start"}),
        )
        .await;
    assert_eq!(titled["title"], "Crash on start");
    assert_eq!(
        titled["values"][&due]["start"].as_str().unwrap().len(),
        "2026-11-10".len()
    );

    // Rows refuse the placeholders; an unknown template or a bad day is a validation error.
    let (status, _) = fixture
        .set_values(&db, &id_of(&row), json!({ owner.clone(): ["me"] }))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    for body in [
        json!({"template_id": Id::new_v7().to_string()}),
        json!({"template_id": id_of(&row)}),
        json!({"template_id": template, "today": "11/10/2026"}),
    ] {
        let (status, _) = fixture
            .call("POST", &format!("databases/{db}/rows"), Some(body))
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    }

    // Clearing a placeholder: a written date replaces "today", a person list without "me" drops it.
    let (_, saved) = fixture
        .set_values(
            &db,
            &template,
            json!({ due.clone(): {"start": "2026-12-01"}, owner.clone(): [] }),
        )
        .await;
    assert_eq!(
        saved["values"][&due],
        json!({"start": "2026-12-01", "end": null})
    );
    assert_eq!(saved["values"].get(&owner), None);
}

#[tokio::test]
async fn a_template_from_a_row_copies_it() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("ITEM5 from").await;
    let notes = fixture.add_property(&db, "Notes", "text").await;
    let row = fixture
        .add_row(
            &db,
            json!({"title": "Example", "icon": "⭐", "values": {notes.clone(): "Copied"}}),
        )
        .await;
    fixture
        .edit_page(&id_of(&row), json!({"content": paragraph("Body text")}))
        .await;
    let template = fixture
        .add_template(&db, json!({"name": "From row", "from_row_id": id_of(&row)}))
        .await;
    assert_eq!(template["name"], "From row");
    assert_eq!(template["title"], "Example");
    assert_eq!(template["icon"], "⭐");
    assert_eq!(template["has_content"], true);
    assert_eq!(template["values"], json!({ notes.clone(): "Copied" }));
    assert_eq!(fixture.database(&db).await["database"]["row_count"], 1);
}

#[tokio::test]
async fn templates_rename_reorder_and_stop_at_twenty() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("ITEM5 limit").await;
    let mut made = Vec::new();
    for index in 0..20 {
        made.push(id_of(
            &fixture
                .add_template(&db, json!({"name": format!("T{index}")}))
                .await,
        ));
    }
    let (status, body) = fixture
        .call(
            "POST",
            &format!("databases/{db}/templates"),
            Some(json!({"name": "One too many"})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "database_template_limit");

    let moved = fixture
        .ok(
            "PATCH",
            &format!("databases/{db}/templates/{}", made[19]),
            Some(json!({"name": "First", "position": 0})),
        )
        .await;
    assert_eq!(moved["name"], "First");
    assert_eq!(moved["position"], 0);
    let order = ids(&fixture.database(&db).await["templates"]);
    assert_eq!(order[0], made[19]);
    assert_eq!(order[1], made[0]);
    let (status, _) = fixture
        .call(
            "PATCH",
            &format!("databases/{db}/templates/{}", Id::new_v7()),
            Some(json!({"name": "Nope"})),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn deleting_a_template_clears_the_views_that_use_it() {
    let fixture = Fixture::new().await;
    let (db, _, _, template) = database_with_template(&fixture).await;
    let (status, view) = fixture.set_default(&db, Some(&template)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["config"]["default_template_id"], json!(template));
    // An id that is not a template of this database is dropped on save.
    let (_, view) = fixture
        .set_default(&db, Some(&Id::new_v7().to_string()))
        .await;
    assert_eq!(view["config"].get("default_template_id"), None);
    fixture.set_default(&db, Some(&template)).await;

    fixture
        .ok(
            "DELETE",
            &format!("databases/{db}/templates/{template}"),
            None,
        )
        .await;
    let detail = fixture.database(&db).await;
    assert_eq!(
        detail.get("templates"),
        None,
        "no templates: the field is left out"
    );
    assert_eq!(
        detail["views"][0]["config"].get("default_template_id"),
        None
    );
    let stored: String = sqlx::query_scalar("SELECT config_json FROM database_views WHERE id = $1")
        .bind(id_of(&detail["views"][0]))
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert!(!stored.contains("default_template_id"), "{stored}");
    // The page is gone for good, not in the trash.
    let (status, _) = fixture
        .call("GET", &format!("pages/{template}"), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let trash = fixture.ok("GET", "pages/trash", None).await;
    assert!(!trash.to_string().contains(&template));
}

#[tokio::test]
async fn duplicating_a_database_copies_its_templates_as_templates() {
    let fixture = Fixture::new().await;
    let (db, notes, _, template) = database_with_template(&fixture).await;
    let owner = fixture.add_property(&db, "Owner", "person").await;
    fixture
        .set_values(&db, &template, json!({ owner.clone(): ["me"] }))
        .await;
    fixture.set_default(&db, Some(&template)).await;

    let copy = fixture
        .ok("POST", &format!("pages/{db}/duplicate"), Some(json!({})))
        .await;
    let detail = fixture.database(&id_of(&copy)).await;
    assert_eq!(detail["rows"].as_array().unwrap().len(), 1);
    assert_eq!(detail["database"]["row_count"], 1);
    let templates = detail["templates"].as_array().unwrap();
    assert_eq!(templates.len(), 1);
    let copied = &templates[0];
    assert_ne!(id_of(copied), template);
    assert_eq!(copied["name"], "Weekly");
    assert_eq!(copied["title"], "Hidden zebra template");
    let notes_copy = id_of(
        detail["properties"]
            .as_array()
            .unwrap()
            .iter()
            .find(|property| property["name"] == "Notes")
            .unwrap(),
    );
    let owner_copy = id_of(
        detail["properties"]
            .as_array()
            .unwrap()
            .iter()
            .find(|property| property["name"] == "Owner")
            .unwrap(),
    );
    assert_ne!(notes_copy, notes);
    assert_eq!(
        copied["values"],
        json!({ notes_copy: "zebra notes", owner_copy: ["me"] })
    );
    assert_eq!(
        detail["views"][0]["config"]["default_template_id"],
        json!(id_of(copied))
    );
    // A template duplicated on its own becomes a row with its stored values.
    let alone = fixture
        .ok(
            "POST",
            &format!("pages/{template}/duplicate"),
            Some(json!({})),
        )
        .await;
    let detail = fixture.database(&db).await;
    assert!(ids(&detail["rows"]).contains(&id_of(&alone)));
    assert_eq!(detail["templates"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn trashing_a_database_takes_its_templates_and_restore_brings_them_back() {
    let fixture = Fixture::new().await;
    let (db, _, _, template) = database_with_template(&fixture).await;
    let version = fixture.ok("GET", &format!("pages/{db}"), None).await["version"].clone();
    fixture
        .ok(
            "DELETE",
            &format!("pages/{db}?expected_version={version}"),
            None,
        )
        .await;
    let trash = fixture.ok("GET", "pages/trash", None).await;
    assert_eq!(ids(&trash["items"]), vec![db.clone()]);
    let version = trash["items"][0]["version"].clone();
    fixture
        .ok(
            "POST",
            &format!("pages/{db}/restore"),
            Some(json!({"expected_version": version})),
        )
        .await;
    let detail = fixture.database(&db).await;
    assert_eq!(ids(&detail["templates"]), vec![template]);
    assert_eq!(detail["database"]["row_count"], 1);
}
