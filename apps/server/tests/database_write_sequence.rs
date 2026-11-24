//! Every write that changes `GET /databases/{id}` bumps the workspace's realtime sequence, the
//! key of the planned snapshot cache (spec 2026-11-24 section 4.1, rollout step 1).
mod database_fixture;

use std::fmt::Debug;

use axum::http::StatusCode;
use database_fixture::{Fixture, id_of, property};
use serde_json::{Value, json};

struct Probe<'a> {
    fixture: &'a Fixture,
    before: i64,
    misses: Vec<String>,
}

impl<'a> Probe<'a> {
    async fn sequence(&self) -> i64 {
        sqlx::query_scalar::<_, i64>(
            "SELECT CAST(sequence AS BIGINT) FROM realtime_sequences WHERE workspace_id = $1",
        )
        .bind(&self.fixture.workspace_id)
        .fetch_optional(self.fixture.database.pool())
        .await
        .unwrap()
        .unwrap_or(0)
    }

    async fn begin(&mut self) {
        self.before = self.sequence().await;
    }

    /// The write under test; a failed request is a miss of `label`.
    async fn send(&mut self, label: &str, method: &str, path: &str, body: Option<Value>) -> Value {
        let (status, value) = self.fixture.call(method, path, body).await;
        if !status.is_success() {
            self.misses
                .push(format!("{label}: request failed {status} {value}"));
        }
        value
    }

    async fn end<T: PartialEq + Debug>(&mut self, label: &str, actual: T, expected: T) {
        let after = self.sequence().await;
        if after <= self.before {
            self.misses.push(format!(
                "{label}: sequence not bumped (stays {})",
                self.before
            ));
        }
        if actual != expected {
            self.misses.push(format!(
                "{label}: GET shows {actual:?}, expected {expected:?}"
            ));
        }
    }
}

fn row<'a>(detail: &'a Value, id: &str) -> Option<&'a Value> {
    detail["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == id)
}

fn titles(detail: &Value) -> Vec<String> {
    detail["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["title"].as_str().unwrap().to_owned())
        .collect()
}

fn named<'a>(items: &'a Value, name: &str) -> Option<&'a Value> {
    items
        .as_array()
        .and_then(|items| items.iter().find(|item| item["name"] == name))
}

async fn version(fixture: &Fixture, page: &str) -> Value {
    fixture.page(page).await["version"].clone()
}

#[tokio::test]
async fn every_database_write_bumps_the_workspace_sequence() {
    let f = Fixture::new().await;
    let db = f.create_database("Tasks").await;
    let other = f.create_database("Other").await;
    let score = f.property_id(&db, "Score", "number", None).await;
    let double = f
        .property_id(
            &db,
            "Double",
            "formula",
            Some(json!({"expression": "prop(\"Score\") * 2"})),
        )
        .await;
    let title = id_of(property(&f.database(&db).await, "Name")).to_owned();
    let a = id_of(&f.add_row(&db, json!({"title": "A"})).await).to_owned();
    let target = id_of(&f.add_row(&other, json!({"title": "Target"})).await).to_owned();
    let host = id_of(&f.create_page(json!({"title": "Host"})).await).to_owned();
    let loose = id_of(&f.create_page(json!({"title": "Loose"})).await).to_owned();
    let mut p = Probe {
        fixture: &f,
        before: 0,
        misses: Vec::new(),
    };
    let dbp = |rest: &str| format!("databases/{db}/{rest}");

    // Properties.
    p.begin().await;
    p.send(
        "property create",
        "POST",
        &dbp("properties"),
        Some(json!({"name": "Notes", "type": "text"})),
    )
    .await;
    let detail = f.database(&db).await;
    p.end(
        "property create",
        named(&detail["properties"], "Notes").is_some(),
        true,
    )
    .await;
    let notes = id_of(named(&detail["properties"], "Notes").unwrap()).to_owned();

    p.begin().await;
    p.send(
        "property update",
        "PATCH",
        &dbp(&format!("properties/{notes}")),
        Some(json!({"name": "Remarks"})),
    )
    .await;
    let detail = f.database(&db).await;
    p.end(
        "property update",
        named(&detail["properties"], "Remarks").is_some(),
        true,
    )
    .await;

    p.begin().await;
    p.send(
        "property delete",
        "DELETE",
        &dbp(&format!("properties/{notes}")),
        None,
    )
    .await;
    let detail = f.database(&db).await;
    p.end(
        "property delete",
        named(&detail["properties"], "Remarks").is_some(),
        false,
    )
    .await;

    p.begin().await;
    p.send(
        "two-way relation create (target database gets the synced property)",
        "POST",
        &dbp("properties"),
        Some(json!({"name": "Rel", "type": "relation",
                    "config": {"database_id": other, "limit": "none", "two_way": true}})),
    )
    .await;
    let other_detail = f.database(&other).await;
    p.end(
        "two-way relation create (target database gets the synced property)",
        named(&other_detail["properties"], "Tasks").is_some(),
        true,
    )
    .await;
    let rel = id_of(property(&f.database(&db).await, "Rel")).to_owned();
    let count = f
        .property_id(
            &db,
            "Count",
            "rollup",
            Some(json!({"relation_property_id": rel,
                        "target_property_id": id_of(property(&other_detail, "Name")),
                        "function": "count_all"})),
        )
        .await;

    // Views.
    p.begin().await;
    let view = p
        .send(
            "view create",
            "POST",
            &dbp("views"),
            Some(json!({"name": "Board", "type": "table"})),
        )
        .await;
    let detail = f.database(&db).await;
    p.end(
        "view create",
        named(&detail["views"], "Board").is_some(),
        true,
    )
    .await;
    let view = view["id"].as_str().unwrap_or_default().to_owned();

    p.begin().await;
    p.send(
        "view update",
        "PATCH",
        &dbp(&format!("views/{view}")),
        Some(json!({"name": "Planning"})),
    )
    .await;
    let detail = f.database(&db).await;
    p.end(
        "view update",
        named(&detail["views"], "Planning").is_some(),
        true,
    )
    .await;

    p.begin().await;
    p.send(
        "view delete",
        "DELETE",
        &dbp(&format!("views/{view}")),
        None,
    )
    .await;
    let detail = f.database(&db).await;
    p.end(
        "view delete",
        named(&detail["views"], "Planning").is_some(),
        false,
    )
    .await;

    // Links (`GET …?link=`).
    p.begin().await;
    let link = p
        .send(
            "link create",
            "POST",
            &dbp("links"),
            Some(json!({"host_page_id": host})),
        )
        .await;
    let link = link["link"]["id"].as_str().unwrap_or_default().to_owned();
    let (status, _) = f
        .call("GET", &format!("databases/{db}?link={link}"), None)
        .await;
    p.end("link create", status, StatusCode::OK).await;

    p.begin().await;
    p.send(
        "link delete",
        "DELETE",
        &dbp(&format!("links/{link}")),
        None,
    )
    .await;
    let (status, _) = f
        .call("GET", &format!("databases/{db}?link={link}"), None)
        .await;
    p.end("link delete", status, StatusCode::NOT_FOUND).await;

    // Rows.
    p.begin().await;
    let b = p
        .send(
            "row create",
            "POST",
            &dbp("rows"),
            Some(json!({"title": "B"})),
        )
        .await;
    let b = id_of(&b).to_owned();
    p.end(
        "row create",
        titles(&f.database(&db).await),
        vec!["A".into(), "B".into()],
    )
    .await;

    p.begin().await;
    p.send(
        "row values",
        "PATCH",
        &dbp(&format!("rows/{a}/values")),
        Some(json!({&score: 5})),
    )
    .await;
    let detail = f.database(&db).await;
    p.end(
        "row values (with formula input)",
        (
            row(&detail, &a).unwrap()["values"][&score].clone(),
            row(&detail, &a).unwrap()["values"][&double].clone(),
        ),
        (json!(5.0), json!(10.0)),
    )
    .await;

    p.begin().await;
    p.send(
        "relation value",
        "PATCH",
        &dbp(&format!("rows/{a}/values")),
        Some(json!({&rel: [target]})),
    )
    .await;
    let detail = f.database(&db).await;
    p.end(
        "relation value (with rollup)",
        (
            row(&detail, &a).unwrap()["values"][&rel].clone(),
            row(&detail, &a).unwrap()["values"][&count].clone(),
        ),
        (json!([target]), json!(1)),
    )
    .await;

    p.begin().await;
    let v = version(&f, &target).await;
    p.send(
        "related row renamed in the other database",
        "PATCH",
        &format!("pages/{target}"),
        Some(json!({"expected_version": v, "title": "Renamed"})),
    )
    .await;
    let detail = f.database(&db).await;
    p.end(
        "related row renamed in the other database",
        detail["related_pages"][&target]["title"].clone(),
        json!("Renamed"),
    )
    .await;

    p.begin().await;
    p.send(
        "row move",
        "POST",
        &dbp(&format!("rows/{b}/move")),
        Some(json!({"after_row_id": null})),
    )
    .await;
    p.end(
        "row move",
        titles(&f.database(&db).await),
        vec!["B".into(), "A".into()],
    )
    .await;

    let run = f
        .property_id(
            &db,
            "Run",
            "button",
            Some(
                json!({"label": "Go", "actions": [{"type": "set_values", "values": {&score: 7}}]}),
            ),
        )
        .await;
    p.begin().await;
    p.send(
        "button press",
        "POST",
        &dbp(&format!("rows/{a}/press/{run}")),
        Some(json!({})),
    )
    .await;
    let detail = f.database(&db).await;
    p.end(
        "button press",
        row(&detail, &a).unwrap()["values"][&score].clone(),
        json!(7.0),
    )
    .await;

    p.begin().await;
    p.send(
        "rows trash",
        "POST",
        &dbp("rows/trash"),
        Some(json!({"row_ids": [b]})),
    )
    .await;
    p.end(
        "rows trash",
        titles(&f.database(&db).await),
        vec!["A".into()],
    )
    .await;

    // Form submit.
    let form = id_of(
        &f.add_view(
            &db,
            "Intake",
            "form",
            Some(json!({"form": {"questions": [{"property_id": title, "required": true}]}})),
        )
        .await,
    )
    .to_owned();
    p.begin().await;
    p.send(
        "form submit",
        "POST",
        &dbp(&format!("forms/{form}/submit")),
        Some(json!({"answers": {&title: "From form"}})),
    )
    .await;
    p.end(
        "form submit",
        titles(&f.database(&db).await).contains(&"From form".to_owned()),
        true,
    )
    .await;

    // CSV.
    p.begin().await;
    p.send(
        "csv import into the database",
        "POST",
        &dbp("import"),
        Some(
            json!({"columns": [{"name": "Title", "target": {"property_id": title}}],
                    "rows": [["Imported"]]}),
        ),
    )
    .await;
    p.end(
        "csv import into the database",
        titles(&f.database(&db).await).contains(&"Imported".to_owned()),
        true,
    )
    .await;

    p.begin().await;
    let created = p
        .send(
            "csv creates a database",
            "POST",
            "databases/import",
            Some(json!({"title": "Contacts", "title_column": 0,
                        "columns": [{"name": "Name", "target": "skip"}],
                        "rows": [["Ada"]]})),
        )
        .await;
    let contacts = created["database"]["id"].as_str().unwrap_or_default();
    p.end(
        "csv creates a database",
        titles(&f.database(contacts).await),
        vec!["Ada".into()],
    )
    .await;

    // Templates.
    p.begin().await;
    let template = p
        .send(
            "template create",
            "POST",
            &dbp("templates"),
            Some(json!({"name": "Weekly"})),
        )
        .await;
    let template = id_of(&template).to_owned();
    let detail = f.database(&db).await;
    p.end(
        "template create",
        detail["templates"][0]["name"].clone(),
        json!("Weekly"),
    )
    .await;

    p.begin().await;
    p.send(
        "template update",
        "PATCH",
        &dbp(&format!("templates/{template}")),
        Some(json!({"name": "Daily"})),
    )
    .await;
    p.end(
        "template update",
        f.database(&db).await["templates"][0]["name"].clone(),
        json!("Daily"),
    )
    .await;

    p.begin().await;
    let v = version(&f, &template).await;
    p.send(
        "template title via the page route",
        "PATCH",
        &format!("pages/{template}"),
        Some(json!({"expected_version": v, "title": "Standup"})),
    )
    .await;
    p.end(
        "template title via the page route",
        f.database(&db).await["templates"][0]["title"].clone(),
        json!("Standup"),
    )
    .await;

    p.begin().await;
    p.send(
        "template delete",
        "DELETE",
        &dbp(&format!("templates/{template}")),
        None,
    )
    .await;
    p.end(
        "template delete",
        f.database(&db).await.get("templates").cloned(),
        None,
    )
    .await;

    // Page routes on rows and on the database page.
    p.begin().await;
    let v = version(&f, &a).await;
    p.send(
        "row title via the page route",
        "PATCH",
        &format!("pages/{a}"),
        Some(json!({"expected_version": v, "title": "Alpha"})),
    )
    .await;
    let detail = f.database(&db).await;
    p.end(
        "row title via the page route",
        row(&detail, &a).unwrap()["title"].clone(),
        json!("Alpha"),
    )
    .await;

    p.begin().await;
    let v = version(&f, &a).await;
    p.send(
        "row icon via the page route",
        "PATCH",
        &format!("pages/{a}"),
        Some(json!({"expected_version": v, "icon": "📄"})),
    )
    .await;
    let detail = f.database(&db).await;
    p.end(
        "row icon via the page route",
        row(&detail, &a).unwrap()["icon"].clone(),
        json!("📄"),
    )
    .await;

    p.begin().await;
    let v = version(&f, &a).await;
    p.send(
        "row content via the page route",
        "PATCH",
        &format!("pages/{a}"),
        Some(json!({"expected_version": v, "content": [
            {"id": "b1", "type": "paragraph", "content": [{"type": "text", "text": "body", "styles": {}}]}
        ]})),
    )
    .await;
    let detail = f.database(&db).await;
    p.end(
        "row content via the page route (has_content)",
        row(&detail, &a).unwrap()["has_content"].clone(),
        json!(true),
    )
    .await;

    p.begin().await;
    let v = version(&f, &a).await;
    p.send(
        "row content cleared",
        "PATCH",
        &format!("pages/{a}"),
        Some(json!({"expected_version": v, "content": []})),
    )
    .await;
    let detail = f.database(&db).await;
    p.end(
        "row content cleared (has_content)",
        row(&detail, &a).unwrap()["has_content"].clone(),
        json!(false),
    )
    .await;

    // Version restore: backdate so the next title edit keeps "Alpha" as a version.
    sqlx::query(
        "UPDATE pages SET created_at = created_at - 3600000, updated_at = updated_at - 3600000 WHERE id = $1",
    )
    .bind(&a)
    .execute(f.database.pool())
    .await
    .unwrap();
    let v = version(&f, &a).await;
    f.ok(
        "PATCH",
        &format!("pages/{a}"),
        Some(json!({"expected_version": v, "title": "Beta"})),
    )
    .await;
    let versions = f.ok("GET", &format!("pages/{a}/versions"), None).await;
    let version_id = versions["items"][0]["id"].as_str().unwrap().to_owned();
    p.begin().await;
    let v = version(&f, &a).await;
    p.send(
        "row version restore",
        "POST",
        &format!("pages/{a}/versions/{version_id}/restore"),
        Some(json!({"expected_version": v})),
    )
    .await;
    let detail = f.database(&db).await;
    p.end(
        "row version restore",
        row(&detail, &a).unwrap()["title"].clone(),
        json!("Alpha"),
    )
    .await;

    p.begin().await;
    let v = version(&f, &db).await;
    p.send(
        "database title via the page route",
        "PATCH",
        &format!("pages/{db}"),
        Some(json!({"expected_version": v, "title": "Work"})),
    )
    .await;
    p.end(
        "database title via the page route",
        f.database(&db).await["database"]["title"].clone(),
        json!("Work"),
    )
    .await;

    p.begin().await;
    p.send(
        "database page lock",
        "POST",
        &format!("pages/{db}/lock"),
        Some(json!({"locked": true})),
    )
    .await;
    p.end(
        "database page lock",
        f.database(&db).await["database"]["locked"].clone(),
        json!(true),
    )
    .await;
    f.ok(
        "POST",
        &format!("pages/{db}/lock"),
        Some(json!({"locked": false})),
    )
    .await;

    p.begin().await;
    let v = version(&f, &a).await;
    p.send(
        "row trash via the page route",
        "DELETE",
        &format!("pages/{a}?expected_version={v}"),
        None,
    )
    .await;
    p.end(
        "row trash via the page route",
        row(&f.database(&db).await, &a).is_some(),
        false,
    )
    .await;

    p.begin().await;
    let trash = f.ok("GET", "pages/trash", None).await;
    let v = trash["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == a)
        .map(|item| item["version"].clone())
        .unwrap_or_default();
    p.send(
        "row restore",
        "POST",
        &format!("pages/{a}/restore"),
        Some(json!({"expected_version": v})),
    )
    .await;
    p.end(
        "row restore",
        row(&f.database(&db).await, &a).is_some(),
        true,
    )
    .await;

    p.begin().await;
    p.send(
        "row duplicate",
        "POST",
        &format!("pages/{a}/duplicate"),
        Some(json!({})),
    )
    .await;
    let alphas = titles(&f.database(&db).await)
        .iter()
        .filter(|title| title.starts_with("Alpha"))
        .count();
    p.end("row duplicate", alphas, 2).await;

    p.begin().await;
    let v = version(&f, &loose).await;
    p.send(
        "page moved into the database",
        "POST",
        &format!("pages/{loose}/move"),
        Some(json!({"expected_version": v, "parent_id": db, "position": 0})),
    )
    .await;
    p.end(
        "page moved into the database",
        row(&f.database(&db).await, &loose).is_some(),
        true,
    )
    .await;

    p.begin().await;
    let v = version(&f, &loose).await;
    p.send(
        "row moved out of the database",
        "POST",
        &format!("pages/{loose}/move"),
        Some(json!({"expected_version": v, "parent_id": null, "position": 0})),
    )
    .await;
    p.end(
        "row moved out of the database",
        row(&f.database(&db).await, &loose).is_some(),
        false,
    )
    .await;

    p.begin().await;
    let v = version(&f, &a).await;
    p.send(
        "row moved to another database",
        "POST",
        &format!("pages/{a}/move"),
        Some(json!({"expected_version": v, "parent_id": other, "position": 0})),
    )
    .await;
    p.end(
        "row moved to another database",
        (
            row(&f.database(&db).await, &a).is_some(),
            row(&f.database(&other).await, &a).is_some(),
        ),
        (false, true),
    )
    .await;

    p.begin().await;
    let v = version(&f, &target).await;
    p.send(
        "related row trashed in the other database",
        "DELETE",
        &format!("pages/{target}?expected_version={v}"),
        None,
    )
    .await;
    let detail = f.database(&db).await;
    p.end(
        "related row trashed in the other database",
        detail["related_pages"].get(&target).is_some(),
        false,
    )
    .await;

    assert!(
        p.misses.is_empty(),
        "write paths that miss the bump or the effect:\n{}",
        p.misses.join("\n")
    );
}
