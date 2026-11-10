mod database_fixture;

use axum::http::StatusCode;
use database_fixture::{Fixture, id_of, problem, property};
use orbit_platform::TimestampMillis;
use serde_json::{Value, json};

fn press_path(db: &str, row: &str, button: &str) -> String {
    format!("databases/{db}/rows/{row}/press/{button}")
}

async fn button(fixture: &Fixture, db: &str, config: Value) -> String {
    fixture.property_id(db, "Run", "button", Some(config)).await
}

fn row_in<'a>(database: &'a Value, id: &str) -> &'a Value {
    database["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == id)
        .unwrap_or_else(|| panic!("no row {id}"))
}

fn utc_day(now: TimestampMillis) -> String {
    jiff::Timestamp::from_millisecond(now.as_millis())
        .unwrap()
        .strftime("%Y-%m-%d")
        .to_string()
}

#[tokio::test]
async fn a_press_sets_values_adds_rows_and_returns_the_url() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("Tasks").await;
    let done = fixture.property_id(&db, "Done", "checkbox", None).await;
    let owner = fixture.property_id(&db, "Owner", "person", None).await;
    let due = fixture.property_id(&db, "Due", "date", None).await;
    let closed = fixture.property_id(&db, "Closed", "date", None).await;
    let note = fixture.property_id(&db, "Note", "text", None).await;
    let log = fixture.create_database("Log").await;
    let log_database = fixture.database(&log).await;
    let log_title = id_of(property(&log_database, "Name")).to_owned();
    let when = fixture.property_id(&log, "When", "date", None).await;
    let title = id_of(property(&fixture.database(&db).await, "Name")).to_owned();

    let run = button(
        &fixture,
        &db,
        json!({
            "label": "  Close  ",
            "confirm": "Close this task?",
            "actions": [
                {"type": "set_values", "values": {&done: true, &owner: "me", &due: "today", &closed: {"start": "now", "end": null}, &note: "closed"}},
                {"type": "add_row", "database_id": db, "values": {&title: "Follow-up", &owner: ["me"]}},
                {"type": "add_row", "database_id": log, "values": {&log_title: "Closed a task", &when: "today"}},
                {"type": "open_url", "url": "https://orbit.so/done"},
            ],
        }),
    )
    .await;
    let database = fixture.database(&db).await;
    assert_eq!(property(&database, "Run")["config"]["label"], "Close");
    let task = id_of(&fixture.add_row(&db, json!({"title": "Ship"})).await).to_owned();

    let before = TimestampMillis::now();
    let pressed = fixture
        .ok(
            "POST",
            &press_path(&db, &task, &run),
            Some(json!({"today": "2026-11-09"})),
        )
        .await;
    let after = TimestampMillis::now();
    let me = fixture.owner_id.to_string();
    let values = &pressed["row"]["values"];
    assert_eq!(pressed["row"]["id"], task.as_str());
    assert_eq!(values[&done], true);
    assert_eq!(values[&owner], json!([me]));
    assert_eq!(values[&due], json!({"start": "2026-11-09", "end": null}));
    assert_eq!(values[&note], "closed");
    let moment = values[&closed]["start"].as_str().unwrap();
    let moment_ms = moment.parse::<jiff::Timestamp>().unwrap().as_millisecond();
    assert!(
        (before.as_millis() / 1000 * 1000..=after.as_millis()).contains(&moment_ms),
        "now is the press time in UTC: {moment}"
    );
    assert_eq!(pressed["open_url"], "https://orbit.so/done");
    let created = pressed["created_rows"].as_array().unwrap();
    assert_eq!(created.len(), 2);
    assert_eq!(created[0]["database_id"], db.as_str());
    assert_eq!(created[0]["row"]["title"], "Follow-up");
    assert_eq!(created[0]["row"]["values"][&owner], json!([me]));
    assert_eq!(created[1]["database_id"], log.as_str());
    assert_eq!(created[1]["row"]["title"], "Closed a task");
    assert_eq!(
        created[1]["row"]["values"][&when],
        json!({"start": "2026-11-09", "end": null})
    );

    // The writes are stored; the new row goes last.
    let database = fixture.database(&db).await;
    assert_eq!(row_in(&database, &task)["values"], *values);
    let titles: Vec<&str> = database["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["Ship", "Follow-up"]);
    assert_eq!(
        fixture.database(&log).await["rows"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    // Without `today`, "today" is the UTC day.
    let other = id_of(&fixture.add_row(&db, json!({"title": "Other"})).await).to_owned();
    let before = TimestampMillis::now();
    let pressed = fixture
        .ok("POST", &press_path(&db, &other, &run), Some(json!({})))
        .await;
    let day = pressed["row"]["values"][&due]["start"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        [utc_day(before), utc_day(TimestampMillis::now())].contains(&day),
        "{day}"
    );
    let response = fixture
        .call(
            "POST",
            &press_path(&db, &other, &run),
            Some(json!({"today": "tomorrow"})),
        )
        .await;
    assert_eq!(
        response.0,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        response.1
    );
}

#[tokio::test]
async fn a_failed_action_rolls_back_every_earlier_one() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("Tasks").await;
    let done = fixture.property_id(&db, "Done", "checkbox", None).await;
    let score = fixture.property_id(&db, "Score", "number", None).await;
    let title = id_of(property(&fixture.database(&db).await, "Name")).to_owned();
    let run = button(
        &fixture,
        &db,
        json!({"actions": [
            {"type": "set_values", "values": {&done: true}},
            {"type": "add_row", "database_id": db, "values": {&title: "Made"}},
            {"type": "set_values", "values": {&score: "not a number"}},
        ]}),
    )
    .await;
    let task = id_of(&fixture.add_row(&db, json!({"title": "Ship"})).await).to_owned();
    let response = fixture
        .call("POST", &press_path(&db, &task, &run), Some(json!({})))
        .await;
    assert_eq!(
        problem(&response),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("validation_failed")),
        "{}",
        response.1
    );
    assert_eq!(response.1["detail"], format!("actions[2].values.{score}"));
    let database = fixture.database(&db).await;
    assert_eq!(database["rows"].as_array().unwrap().len(), 1, "no new row");
    assert_eq!(database["rows"][0]["values"], json!({}), "Done stays unset");
    assert_eq!(
        fixture
            .count(
                "SELECT CAST(COUNT(*) AS BIGINT) FROM audit_events WHERE action = 'database.button_pressed' AND workspace_id = $1",
                &fixture.workspace_id,
            )
            .await,
        0
    );

    // A full target database also rolls the press back.
    fixture.seed_rows(&db, 4_999).await;
    let response = fixture
        .call("POST", &press_path(&db, &task, &run), Some(json!({})))
        .await;
    assert_eq!(
        problem(&response),
        (StatusCode::CONFLICT, Some("database_row_limit"))
    );
    assert_eq!(
        row_in(&fixture.database(&db).await, &task)["values"],
        json!({})
    );
}

#[tokio::test]
async fn add_row_needs_access_to_its_target_database() {
    let fixture = Fixture::new().await;
    let (_, member) = fixture.add_member("member@example.com").await;
    let db = fixture.create_database("Shared").await;
    let done = fixture.property_id(&db, "Done", "checkbox", None).await;
    let private = id_of(
        &fixture
            .create_page(json!({"kind": "database", "title": "Diary", "private": true}))
            .await,
    )
    .to_owned();
    let run = button(
        &fixture,
        &db,
        json!({"actions": [
            {"type": "set_values", "values": {&done: true}},
            {"type": "add_row", "database_id": private},
        ]}),
    )
    .await;
    let task = id_of(&fixture.add_row(&db, json!({"title": "Ship"})).await).to_owned();

    let response = fixture
        .call_as(
            &member,
            "POST",
            &press_path(&db, &task, &run),
            Some(json!({})),
        )
        .await;
    assert_eq!(
        problem(&response),
        (StatusCode::NOT_FOUND, Some("page_not_found")),
        "{}",
        response.1
    );
    assert_eq!(
        row_in(&fixture.database(&db).await, &task)["values"],
        json!({})
    );
    assert_eq!(
        fixture.database(&private).await["rows"]
            .as_array()
            .unwrap()
            .len(),
        0
    );

    // The owner can read both, so the same press works for them.
    let pressed = fixture
        .ok("POST", &press_path(&db, &task, &run), Some(json!({})))
        .await;
    assert_eq!(pressed["created_rows"][0]["database_id"], private.as_str());

    // A non-button property and an unknown row are 404.
    let response = fixture
        .call("POST", &press_path(&db, &task, &done), Some(json!({})))
        .await;
    assert_eq!(
        problem(&response),
        (StatusCode::NOT_FOUND, Some("database_property_not_found"))
    );
    let response = fixture
        .call("POST", &press_path(&db, &db, &run), Some(json!({})))
        .await;
    assert_eq!(response.0, StatusCode::NOT_FOUND, "{}", response.1);
}

#[tokio::test]
async fn presses_past_ten_a_second_are_429() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("Tasks").await;
    let run = button(&fixture, &db, json!({"actions": []})).await;
    let task = id_of(&fixture.add_row(&db, json!({"title": "Ship"})).await).to_owned();
    // Start at the beginning of a second, so all presses land in one window.
    let millis = TimestampMillis::now().as_millis() % 1_000;
    tokio::time::sleep(std::time::Duration::from_millis(
        u64::try_from(1_000 - millis).unwrap(),
    ))
    .await;
    let start = TimestampMillis::now();
    let mut statuses = Vec::new();
    for _ in 0..11 {
        let response = fixture
            .call("POST", &press_path(&db, &task, &run), Some(json!({})))
            .await;
        statuses.push((response.0, response.1["code"].as_str().map(str::to_owned)));
    }
    let elapsed = TimestampMillis::now().as_millis() - start.as_millis();
    assert!(elapsed < 900, "the presses took {elapsed} ms");
    assert!(
        statuses[..10]
            .iter()
            .all(|(status, _)| *status == StatusCode::OK),
        "{statuses:?}"
    );
    assert_eq!(
        statuses[10],
        (
            StatusCode::TOO_MANY_REQUESTS,
            Some("database_button_rate_limit".to_owned())
        )
    );
    tokio::time::sleep(std::time::Duration::from_millis(1_000)).await;
    let response = fixture
        .call("POST", &press_path(&db, &task, &run), Some(json!({})))
        .await;
    assert_eq!(response.0, StatusCode::OK, "the next second admits again");
}

#[tokio::test]
async fn button_configs_are_checked_when_saved() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("Tasks").await;
    let url = json!({"type": "open_url", "url": "https://orbit.so"});
    let set = json!({"type": "set_values", "values": {}});
    for (config, field) in [
        (
            json!({"actions": [set, set, set, set, set, set]}),
            "config.actions",
        ),
        (json!({"actions": [url, url]}), "config.actions[1].url"),
        (
            json!({"actions": [{"type": "open_url", "url": "javascript:alert(1)"}]}),
            "config.actions[0].url",
        ),
        (json!({"label": "x".repeat(201)}), "config.label"),
        (json!({"confirm": "x".repeat(501)}), "config.confirm"),
        (json!({"actions": [{"type": "notify"}]}), "config"),
    ] {
        let response = fixture
            .call(
                "POST",
                &format!("databases/{db}/properties"),
                Some(json!({"name": "Run", "type": "button", "config": config})),
            )
            .await;
        assert_eq!(
            (response.0, response.1["detail"].as_str()),
            (StatusCode::UNPROCESSABLE_ENTITY, Some(field)),
            "{config}"
        );
    }
    let saved = fixture
        .add_property(
            &db,
            json!({"name": "Run", "type": "button", "config": {"confirm": "   ", "actions": [set, set, set, set, url]}}),
        )
        .await;
    assert_eq!(saved["config"]["confirm"], Value::Null);
    assert_eq!(saved["config"]["actions"].as_array().unwrap().len(), 5);
}

#[tokio::test]
async fn a_button_is_not_a_value_a_filter_a_csv_column_or_a_form_question() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("Tasks").await;
    let run = button(&fixture, &db, json!({"actions": []})).await;
    let task = id_of(&fixture.add_row(&db, json!({"title": "Ship"})).await).to_owned();
    let database = fixture.database(&db).await;
    let view = id_of(&database["views"][0]).to_owned();
    let name = id_of(property(&database, "Name")).to_owned();

    let (status, body) = fixture
        .set_values(&db, &task, json!({&run: "pressed"}))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let response = fixture
        .call(
            "POST",
            &format!("databases/{db}/rows"),
            Some(json!({"title": "New", "values": {&run: true}})),
        )
        .await;
    assert_eq!(
        response.0,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        response.1
    );

    for config in [
        json!({"filter": {"op": "and", "children": [{"property_id": run, "operator": "is", "value": "x"}]}}),
        json!({"filter": {"op": "and", "children": [{"property_id": run, "operator": "contains", "value": "x"}]}}),
    ] {
        let response = fixture
            .call(
                "PATCH",
                &format!("databases/{db}/views/{view}"),
                Some(json!({"config": config})),
            )
            .await;
        assert_eq!(
            response.0,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{config}: {}",
            response.1
        );
    }

    let response = fixture
        .call(
            "POST",
            &format!("databases/{db}/import"),
            Some(json!({
                "columns": [
                    {"name": "Name", "target": {"property_id": name}},
                    {"name": "Run", "target": {"property_id": run}},
                ],
                "rows": [["Imported", "x"]],
            })),
        )
        .await;
    assert_eq!(
        response.0,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        response.1
    );
    let response = fixture
        .call(
            "POST",
            &format!("databases/{db}/import"),
            Some(json!({
                "columns": [
                    {"name": "Name", "target": {"property_id": name}},
                    {"name": "Run", "target": {"new": "button"}},
                ],
                "rows": [["Imported", "x"]],
            })),
        )
        .await;
    assert_eq!(
        response.0,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        response.1
    );

    let form = fixture
        .add_view(
            &db,
            "Form",
            "form",
            Some(json!({"form": {"questions": [
                {"property_id": name, "required": true},
                {"property_id": run},
            ]}})),
        )
        .await;
    assert_eq!(
        form["config"]["form"]["questions"],
        json!([{"property_id": name, "required": true}])
    );
    assert_eq!(
        fixture.database(&db).await["rows"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
