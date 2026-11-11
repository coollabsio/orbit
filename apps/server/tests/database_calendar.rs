//! List and calendar views (docs databases P3 item 11, spec 13): their configs and repairs.

mod db_fixture;

use axum::http::StatusCode;
use db_fixture::{Fixture, id_of, property};
use serde_json::{Value, json};

async fn add(fixture: &Fixture, db: &str, name: &str, kind: &str) -> String {
    id_of(
        &fixture
            .add_property(db, json!({"name": name, "type": kind}))
            .await,
    )
    .to_owned()
}

async fn view_config(fixture: &Fixture, db: &str, view: &str) -> Value {
    fixture.database(db).await["views"]
        .as_array()
        .unwrap()
        .iter()
        .find(|candidate| candidate["id"] == view)
        .unwrap()["config"]
        .clone()
}

async fn patch_view(fixture: &Fixture, db: &str, view: &str, config: Value) -> (StatusCode, Value) {
    fixture
        .call(
            "PATCH",
            &format!("databases/{db}/views/{view}"),
            Some(json!({ "config": config })),
        )
        .await
}

#[tokio::test]
async fn a_calendar_places_rows_by_its_date_property_or_the_first_date_property() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("Events").await;
    let notes = add(&fixture, &db, "Notes", "text").await;
    let view = id_of(
        &fixture
            .ok(
                "POST",
                &format!("databases/{db}/views"),
                Some(json!({"name": "Calendar", "type": "calendar"})),
            )
            .await,
    )
    .to_owned();
    let config = view_config(&fixture, &db, &view).await;
    assert_eq!(
        (config.get("date_property_id"), config.get("calendar_mode")),
        (None, None),
        "no date property: none (the client asks to choose one), and month: {config}"
    );

    let due = add(&fixture, &db, "Due", "date").await;
    let start = add(&fixture, &db, "Start", "date").await;
    let created = add(&fixture, &db, "Created", "created_time").await;
    assert_eq!(
        view_config(&fixture, &db, &view).await["date_property_id"],
        due.as_str(),
        "repair takes the first date property"
    );

    let mut config = view_config(&fixture, &db, &view).await;
    config["date_property_id"] = json!(start);
    config["calendar_mode"] = json!("week");
    config["card_properties"] = json!([notes]);
    let (status, body) = patch_view(&fixture, &db, &view, config.clone()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let saved = view_config(&fixture, &db, &view).await;
    assert_eq!(
        (
            &saved["date_property_id"],
            &saved["calendar_mode"],
            &saved["card_properties"]
        ),
        (&json!(start), &json!("week"), &json!([notes]))
    );

    config["calendar_mode"] = json!("month");
    config["date_property_id"] = json!(created);
    let (status, body) = patch_view(&fixture, &db, &view, config.clone()).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a created time places rows too: {body}"
    );
    let saved = view_config(&fixture, &db, &view).await;
    assert_eq!(saved["date_property_id"], created.as_str());
    assert_eq!(saved.get("calendar_mode"), None, "month is the default");

    config["date_property_id"] = json!(notes);
    let (status, body) = patch_view(&fixture, &db, &view, config.clone()).await;
    assert_eq!(
        (status, body["detail"].as_str()),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Some("config.date_property_id")
        )
    );

    for gone in [&created, &due] {
        fixture
            .ok("DELETE", &format!("databases/{db}/properties/{gone}"), None)
            .await;
    }
    assert_eq!(
        view_config(&fixture, &db, &view).await["date_property_id"],
        start.as_str(),
        "a deleted date property gives way to the first one left"
    );
    fixture
        .ok(
            "DELETE",
            &format!("databases/{db}/properties/{start}"),
            None,
        )
        .await;
    let config = view_config(&fixture, &db, &view).await;
    assert_eq!(config.get("date_property_id"), None, "{config}");
    assert_eq!(config["card_properties"], json!([notes]));
}

#[tokio::test]
async fn a_list_view_keeps_its_columns_sorts_and_filter() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("Reading").await;
    let detail = fixture.database(&db).await;
    let name = id_of(property(&detail, "Name")).to_owned();
    let tags = id_of(property(&detail, "Tags")).to_owned();
    let pages = add(&fixture, &db, "Pages", "number").await;
    let config = json!({
        "columns": [
            {"property_id": name, "width": 280, "hidden": false},
            {"property_id": pages, "width": 120, "hidden": false},
            {"property_id": tags, "width": 200, "hidden": true},
        ],
        "sorts": [{"property_id": pages, "direction": "desc"}],
        "filter": {"op": "and", "children": [
            {"property_id": pages, "operator": ">", "value": 100.0}
        ]},
        "wrap_cells": false,
    });
    let view = fixture
        .ok(
            "POST",
            &format!("databases/{db}/views"),
            Some(json!({"name": "List", "type": "list", "config": config})),
        )
        .await;
    assert_eq!(view["type"], "list");
    let saved = view_config(&fixture, &db, id_of(&view)).await;
    assert_eq!(saved, config);

    fixture
        .ok(
            "DELETE",
            &format!("databases/{db}/properties/{pages}"),
            None,
        )
        .await;
    let saved = view_config(&fixture, &db, id_of(&view)).await;
    assert_eq!(
        saved,
        json!({
            "columns": [
                {"property_id": name, "width": 280, "hidden": false},
                {"property_id": tags, "width": 200, "hidden": true},
            ],
            "sorts": [],
            "filter": {"op": "and", "children": []},
            "wrap_cells": false,
        }),
        "a deleted property leaves the list's columns, sorts and filter"
    );
}
