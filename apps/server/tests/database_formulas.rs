mod database_fixture;

use axum::http::StatusCode;
use database_fixture::{Fixture, id_of, problem, property};
use serde_json::{Value, json};

/// A database with a number `Price`, a number `Qty` and three rows, in manual order.
async fn priced(fixture: &Fixture) -> (String, String, String, Vec<String>) {
    let db = fixture.create_database("Orders").await;
    let price = fixture.property_id(&db, "Price", "number", None).await;
    let qty = fixture.property_id(&db, "Qty", "number", None).await;
    let mut rows = Vec::new();
    for (title, values) in [
        ("A", json!({&price: 2, &qty: 3})),
        ("B", json!({&price: 5})),
        ("C", json!({&price: 1.5, &qty: 4})),
    ] {
        let row = fixture
            .add_row(&db, json!({"title": title, "values": values}))
            .await;
        rows.push(id_of(&row).to_owned());
    }
    (db, price, qty, rows)
}

fn values_of(database: &Value, key: &str) -> Vec<Value> {
    database["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["values"].get(key).cloned().unwrap_or(Value::Null))
        .collect()
}

#[tokio::test]
async fn a_formula_saves_with_ids_and_its_type_and_reads_on_every_row() {
    let fixture = Fixture::new().await;
    let (db, price, qty, _) = priced(&fixture).await;
    let total = fixture
        .add_property(
            &db,
            json!({"name": "Total", "type": "formula", "config": {"expression": "prop(\"Price\") * prop(\"Qty\")"}}),
        )
        .await;
    assert_eq!(
        total["config"],
        json!({
            "expression": format!("prop(\"{price}\") * prop(\"{qty}\")"),
            "result_type": "number",
        })
    );
    let total = id_of(&total).to_owned();
    let label = fixture
        .add_property(
            &db,
            json!({"name": "Label", "type": "formula", "config": {"expression": "if(prop(\"Total\") > 5, \"big\", \"small\")"}}),
        )
        .await;
    assert_eq!(label["config"]["result_type"], "text");
    let label = id_of(&label).to_owned();

    let database = fixture.database(&db).await;
    assert_eq!(
        values_of(&database, &total),
        [json!(6.0), Value::Null, json!(6.0)]
    );
    assert_eq!(
        values_of(&database, &label),
        [json!("big"), json!("small"), json!("big")],
        "no value is less than any number"
    );

    // A write changes the result on the next read; formulas store nothing.
    let row = database["rows"][1]["id"].as_str().unwrap().to_owned();
    let (status, body) = fixture.set_values(&db, &row, json!({&qty: 2})).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let database = fixture.database(&db).await;
    assert_eq!(
        values_of(&database, &total),
        [json!(6.0), json!(10.0), json!(6.0)]
    );
    assert_eq!(
        fixture
            .count(
                "SELECT CAST(COUNT(*) AS BIGINT) FROM page_property_values WHERE property_id = $1",
                &total,
            )
            .await,
        0
    );
    let (status, body) = fixture.set_values(&db, &row, json!({&total: 3})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
}

#[tokio::test]
async fn an_invalid_formula_is_422_with_its_place_in_utf16() {
    let fixture = Fixture::new().await;
    let (db, _, _, _) = priced(&fixture).await;
    let created = |expression: &str| json!({"name": "Bad", "type": "formula", "config": {"expression": expression}});
    for (expression, message, start, end) in [
        (
            "prop(\"Price\") + \"€\"",
            "`+` cannot add number and text. Use format() to make text.",
            0,
            19,
        ),
        (
            "\"é\" + if(true, 1",
            "Expected `)` after the arguments of `if`, found the end of the formula.",
            16,
            16,
        ),
        ("prop(\"Statis\")", "No property named \"Statis\".", 5, 13),
    ] {
        let response = fixture
            .call(
                "POST",
                &format!("databases/{db}/properties"),
                Some(created(expression)),
            )
            .await;
        assert_eq!(
            problem(&response),
            (StatusCode::UNPROCESSABLE_ENTITY, Some("formula_invalid")),
            "{expression}"
        );
        assert_eq!(response.1["detail"], message, "{expression}");
        assert_eq!(
            response.1["formula"],
            json!({"message": message, "start": start, "end": end}),
            "{expression}"
        );
    }
    assert_eq!(
        fixture.database(&db).await["properties"]
            .as_array()
            .unwrap()
            .len(),
        4,
        "nothing was created"
    );

    // A formula that would use itself through another is refused on PATCH too.
    let a = fixture
        .add_property(
            &db,
            json!({"name": "A", "type": "formula", "config": {"expression": "prop(\"Price\")"}}),
        )
        .await;
    fixture
        .add_property(
            &db,
            json!({"name": "B", "type": "formula", "config": {"expression": "prop(\"A\") + 1"}}),
        )
        .await;
    let response = fixture
        .call(
            "PATCH",
            &format!("databases/{db}/properties/{}", id_of(&a)),
            Some(json!({"config": {"expression": "prop(\"B\")"}})),
        )
        .await;
    assert_eq!(
        problem(&response),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("formula_invalid"))
    );
    assert_eq!(
        response.1["detail"],
        "This formula uses itself through \"B\"."
    );
}

#[tokio::test]
async fn today_and_dates_read_in_the_requested_time_zone() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("Days").await;
    let meeting = fixture.property_id(&db, "Meeting", "date", None).await;
    fixture
        .add_row(
            &db,
            json!({"title": "Standup", "values": {&meeting: {"start": "2026-03-28T23:30:00Z", "end": null}}}),
        )
        .await;
    let today = id_of(
        &fixture
            .add_property(
                &db,
                json!({"name": "Today", "type": "formula", "config": {"expression": "today()"}}),
            )
            .await,
    )
    .to_owned();
    let local = id_of(
        &fixture
            .add_property(
                &db,
                json!({"name": "Local", "type": "formula", "config": {"expression": "formatDate(prop(\"Meeting\"), \"YYYY-MM-DD HH:mm\")"}}),
            )
            .await,
    )
    .to_owned();

    for zone in ["Pacific/Kiritimati", "Pacific/Pago_Pago"] {
        let database = fixture
            .ok("GET", &format!("databases/{db}?tz={zone}"), None)
            .await;
        let expected = jiff::Timestamp::now()
            .to_zoned(jiff::tz::TimeZone::get(zone).unwrap())
            .date()
            .to_string();
        assert_eq!(
            database["rows"][0]["values"][&today],
            json!({"start": expected, "end": null}),
            "{zone}"
        );
    }
    let read = |zone: &'static str| {
        let fixture = &fixture;
        let db = db.clone();
        async move {
            fixture
                .ok("GET", &format!("databases/{db}?tz={zone}"), None)
                .await
        }
    };
    assert_eq!(
        read("Europe/Budapest").await["rows"][0]["values"][&local],
        "2026-03-29 00:30"
    );
    assert_eq!(
        read("America/New_York").await["rows"][0]["values"][&local],
        "2026-03-28 19:30"
    );
    assert_eq!(
        fixture.database(&db).await["rows"][0]["values"][&local],
        "2026-03-28 23:30",
        "UTC without ?tz"
    );
    assert_eq!(
        read("Mars/Olympus").await["rows"][0]["values"][&local],
        "2026-03-28 23:30",
        "an unknown zone reads as UTC"
    );
    let response = fixture
        .call("GET", &format!("databases/{db}?tz=Europe%3BBudapest"), None)
        .await;
    assert_eq!(
        problem(&response),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("validation_failed")),
        "{}",
        response.1
    );
}

#[tokio::test]
async fn a_preview_types_the_expression_and_computes_the_first_rows() {
    let fixture = Fixture::new().await;
    let (db, _, _, rows) = priced(&fixture).await;
    let path = format!("databases/{db}/formula/preview");
    let preview = fixture
        .ok(
            "POST",
            &path,
            Some(json!({"expression": "prop(\"Price\") * 2"})),
        )
        .await;
    assert_eq!(
        preview,
        json!({
            "result_type": "number",
            "samples": [
                {"row_id": rows[0], "value": 4.0},
                {"row_id": rows[1], "value": 10.0},
                {"row_id": rows[2], "value": 3.0},
            ],
        })
    );

    let preview = fixture
        .ok(
            "POST",
            &path,
            Some(json!({"expression": "prop(\"Price\") +"})),
        )
        .await;
    assert_eq!(
        preview,
        json!({
            "error": {"message": "Expected a value, found the end of the formula.", "start": 15, "end": 15},
            "samples": [],
        })
    );

    // As an existing formula: it may not read itself, and other formulas see the new type.
    let total = id_of(
        &fixture
            .add_property(
                &db,
                json!({"name": "Total", "type": "formula", "config": {"expression": "prop(\"Price\")"}}),
            )
            .await,
    )
    .to_owned();
    let preview = fixture
        .ok(
            "POST",
            &path,
            Some(json!({"expression": "prop(\"Total\") + 1", "property_id": total})),
        )
        .await;
    assert_eq!(
        preview["error"]["message"],
        "This formula uses itself through \"Total\"."
    );
    let preview = fixture
        .ok(
            "POST",
            &path,
            Some(json!({"expression": "format(prop(\"Price\"))", "property_id": total})),
        )
        .await;
    assert_eq!(preview["result_type"], "text");
    assert_eq!(preview["samples"][0]["value"], "2");

    let response = fixture
        .call(
            "POST",
            &path,
            Some(json!({"expression": "1", "property_id": rows[0]})),
        )
        .await;
    assert_eq!(
        problem(&response),
        (StatusCode::NOT_FOUND, Some("database_property_not_found"))
    );
    assert_eq!(
        fixture.database(&db).await["properties"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "Total")
            .unwrap()["config"]["result_type"],
        "number",
        "a preview writes nothing"
    );

    // Only readers of the database may preview.
    let (_, member) = fixture.add_member("member@example.com").await;
    let private = id_of(
        &fixture
            .create_page(json!({"kind": "database", "title": "Diary", "private": true}))
            .await,
    )
    .to_owned();
    let response = fixture
        .call_as(
            &member,
            "POST",
            &format!("databases/{private}/formula/preview"),
            Some(json!({"expression": "1"})),
        )
        .await;
    assert_eq!(
        problem(&response),
        (StatusCode::NOT_FOUND, Some("page_not_found"))
    );
}

#[tokio::test]
async fn a_preview_samples_at_most_ten_rows() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("Many").await;
    fixture.seed_rows(&db, 12).await;
    let preview = fixture
        .ok(
            "POST",
            &format!("databases/{db}/formula/preview"),
            Some(json!({"expression": "prop(\"Name\")"})),
        )
        .await;
    let samples = preview["samples"].as_array().unwrap();
    assert_eq!(samples.len(), 10);
    assert!(samples.iter().all(|sample| sample["value"] == "Row"));
}

#[tokio::test]
async fn deleting_a_read_property_shows_an_error_in_every_cell() {
    let fixture = Fixture::new().await;
    let (db, price, _, _) = priced(&fixture).await;
    let total = id_of(
        &fixture
            .add_property(
                &db,
                json!({"name": "Total", "type": "formula", "config": {"expression": "prop(\"Price\") * 2"}}),
            )
            .await,
    )
    .to_owned();
    let echo = id_of(
        &fixture
            .add_property(
                &db,
                json!({"name": "Echo", "type": "formula", "config": {"expression": "prop(\"Qty\")"}}),
            )
            .await,
    )
    .to_owned();
    fixture
        .ok(
            "DELETE",
            &format!("databases/{db}/properties/{price}"),
            None,
        )
        .await;
    let database = fixture.database(&db).await;
    let error = json!({"error": "A property was deleted."});
    assert_eq!(
        values_of(&database, &total),
        [error.clone(), error.clone(), error]
    );
    assert_eq!(
        property(&database, "Total")["config"]["expression"],
        format!("prop(\"{price}\") * 2"),
        "the dangling id marks the reference"
    );

    // A type change of a read property retypes the formula that reads it.
    let qty = id_of(property(&database, "Qty")).to_owned();
    fixture
        .ok(
            "PATCH",
            &format!("databases/{db}/properties/{qty}"),
            Some(json!({"type": "text"})),
        )
        .await;
    let database = fixture.database(&db).await;
    assert_eq!(property(&database, "Echo")["config"]["result_type"], "text");
    assert_eq!(
        values_of(&database, &echo),
        [json!("3"), json!(""), json!("4")]
    );
}
