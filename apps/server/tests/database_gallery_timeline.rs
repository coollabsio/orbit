mod database_fixture;

use axum::http::StatusCode;
use database_fixture::{Fixture, id_of, property};
use serde_json::{Value, json};

fn view<'a>(database: &'a Value, id: &str) -> &'a Value {
    database["views"]
        .as_array()
        .unwrap()
        .iter()
        .find(|view| view["id"] == id)
        .unwrap_or_else(|| panic!("no view {id}"))
}

async fn invalid(fixture: &Fixture, db: &str, kind: &str, config: Value) -> Option<String> {
    let (status, body) = fixture
        .call(
            "POST",
            &format!("databases/{db}/views"),
            Some(json!({"name": "Bad", "type": kind, "config": config})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{config}: {body}");
    body["detail"].as_str().map(str::to_owned)
}

fn image(url: &str) -> Value {
    json!({"id": format!("img-{url}"), "type": "image", "props": {"url": url}, "content": [], "children": []})
}

fn paragraph(text: &str, children: Vec<Value>) -> Value {
    json!({"id": format!("p-{text}"), "type": "paragraph", "props": {}, "content": [{"type": "text", "text": text, "styles": {}}], "children": children})
}

#[tokio::test]
async fn rows_carry_a_preview_only_when_a_gallery_shows_one() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("Moodboard").await;
    let pictured = id_of(&fixture.add_row(&db, json!({"title": "Pictured"})).await).to_owned();
    let written = id_of(&fixture.add_row(&db, json!({"title": "Written"})).await).to_owned();
    let blank = id_of(&fixture.add_row(&db, json!({"title": "Blank"})).await).to_owned();
    fixture
        .set_content(
            &pictured,
            json!([
                paragraph("Intro", vec![image("https://example.com/nested.png")]),
                image("https://example.com/second.png"),
            ]),
        )
        .await;
    let long = "word ".repeat(100);
    fixture
        .set_content(&written, json!([paragraph(long.trim(), vec![])]))
        .await;

    let rows = |database: &Value| database["rows"].as_array().unwrap().clone();
    let database = fixture.database(&db).await;
    assert!(
        rows(&database)
            .iter()
            .all(|row| row.get("preview").is_none()),
        "no gallery, no preview"
    );
    let none = fixture
        .add_view(
            &db,
            "Plain",
            "gallery",
            Some(json!({"card_preview": "none"})),
        )
        .await;
    assert_eq!(none["config"]["card_preview"], "none");
    let database = fixture.database(&db).await;
    assert!(
        rows(&database)
            .iter()
            .all(|row| row.get("preview").is_none())
    );

    let gallery = fixture.add_view(&db, "Cards", "gallery", None).await;
    assert_eq!(
        gallery["config"].get("card_preview"),
        None,
        "content is the default and is not stored"
    );
    let database = fixture.database(&db).await;
    let by_title = |title: &str| {
        rows(&database)
            .into_iter()
            .find(|row| row["title"] == title)
            .unwrap()
    };
    assert_eq!(
        by_title("Pictured")["preview"],
        json!({"image_url": "https://example.com/nested.png", "text": "Intro"})
    );
    let text = by_title("Written")["preview"]["text"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(text.chars().count(), 299, "300 characters, trimmed");
    assert!(long.starts_with(&text));
    assert_eq!(by_title("Blank")["preview"], json!({}));
    assert_eq!(by_title("Blank")["id"], blank.as_str());
}

#[tokio::test]
async fn a_gallery_checks_and_repairs_its_files_preview() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("Moodboard").await;
    let files = fixture.property_id(&db, "Photos", "files", None).await;
    let text = fixture.property_id(&db, "Caption", "text", None).await;

    assert_eq!(
        invalid(
            &fixture,
            &db,
            "gallery",
            json!({"card_preview": {"files": text}})
        )
        .await,
        Some("config.card_preview".to_owned())
    );
    assert_eq!(
        invalid(&fixture, &db, "gallery", json!({"card_size": "huge"})).await,
        Some("config".to_owned())
    );
    let gallery = fixture
        .add_view(
            &db,
            "Photos",
            "gallery",
            Some(
                json!({"card_preview": {"files": files}, "card_size": "large", "fit_image": true}),
            ),
        )
        .await;
    assert_eq!(gallery["config"]["card_preview"], json!({"files": files}));
    assert_eq!(gallery["config"]["card_size"], "large");
    assert_eq!(gallery["config"]["fit_image"], true);
    fixture
        .ok(
            "DELETE",
            &format!("databases/{db}/properties/{files}"),
            None,
        )
        .await;
    let database = fixture.database(&db).await;
    let repaired = view(&database, id_of(&gallery));
    assert_eq!(
        repaired["config"].get("card_preview"),
        None,
        "back to the content default"
    );
    assert_eq!(repaired["config"]["card_size"], "large");
}

#[tokio::test]
async fn a_timeline_needs_date_properties_and_repairs_when_they_go() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("Roadmap").await;
    let start = fixture.property_id(&db, "Start", "date", None).await;
    let end = fixture.property_id(&db, "End", "date", None).await;
    let created = fixture
        .property_id(&db, "Created", "created_time", None)
        .await;
    let owner = fixture.property_id(&db, "Owner", "person", None).await;
    let database = fixture.database(&db).await;
    let title = id_of(property(&database, "Name")).to_owned();

    for (config, field) in [
        (
            json!({"start_property_id": created}),
            "config.start_property_id",
        ),
        (
            json!({"start_property_id": owner}),
            "config.start_property_id",
        ),
        (
            json!({"end_property_id": created}),
            "config.end_property_id",
        ),
        (json!({"zoom": "decade"}), "config"),
    ] {
        assert_eq!(
            invalid(&fixture, &db, "timeline", config.clone()).await,
            Some(field.to_owned()),
            "{config}"
        );
    }

    // No start: the first date property. Defaults are not stored.
    let plain = fixture.add_view(&db, "Plain", "timeline", None).await;
    assert_eq!(plain["config"]["start_property_id"], start.as_str());
    assert_eq!(plain["config"].get("zoom"), None);
    assert_eq!(plain["config"].get("end_property_id"), None);

    let timeline = fixture
        .add_view(
            &db,
            "Plan",
            "timeline",
            Some(json!({
                "start_property_id": end,
                "end_property_id": end,
                "zoom": "quarter",
                "show_table": false,
                "table_columns": [owner, title, owner, start],
            })),
        )
        .await;
    assert_eq!(timeline["config"]["start_property_id"], end.as_str());
    assert_eq!(
        timeline["config"].get("end_property_id"),
        None,
        "an end equal to the start is dropped"
    );
    assert_eq!(timeline["config"]["zoom"], "quarter");
    assert_eq!(timeline["config"]["show_table"], false);
    assert_eq!(
        timeline["config"]["table_columns"],
        json!([owner, start]),
        "the title and repeats are dropped"
    );

    let ranged = fixture
        .add_view(
            &db,
            "Ranged",
            "timeline",
            Some(json!({"start_property_id": start, "end_property_id": end})),
        )
        .await;
    assert_eq!(ranged["config"]["end_property_id"], end.as_str());
    fixture
        .ok("DELETE", &format!("databases/{db}/properties/{end}"), None)
        .await;
    let database = fixture.database(&db).await;
    let ranged = view(&database, id_of(&ranged));
    assert_eq!(ranged["config"]["start_property_id"], start.as_str());
    assert_eq!(ranged["config"].get("end_property_id"), None);
    let plan = view(&database, id_of(&timeline));
    assert_eq!(
        plan["config"]["start_property_id"],
        start.as_str(),
        "a deleted start falls back to the first date property"
    );
    fixture
        .ok(
            "DELETE",
            &format!("databases/{db}/properties/{owner}"),
            None,
        )
        .await;
    let database = fixture.database(&db).await;
    assert_eq!(
        view(&database, id_of(&timeline))["config"]["table_columns"],
        json!([start])
    );
}
