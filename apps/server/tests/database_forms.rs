mod database_fixture;

use axum::http::StatusCode;
use database_fixture::{Fixture, id_of, problem, property};
use serde_json::{Value, json};

struct Form {
    db: String,
    view: String,
    title: String,
    score: String,
    stage: String,
}

/// A database with a form asking for the title (required), a number and a select.
async fn form(fixture: &Fixture) -> Form {
    let db = fixture.create_database("Requests").await;
    let score = fixture.property_id(&db, "Score", "number", None).await;
    let stage = fixture
        .property_id(
            &db,
            "Stage",
            "select",
            Some(json!({"options": [{"id": "s1", "name": "New", "color": "blue"}]})),
        )
        .await;
    let title = id_of(property(&fixture.database(&db).await, "Name")).to_owned();
    let view = fixture
        .add_view(
            &db,
            "Intake",
            "form",
            Some(json!({"form": {"questions": [
                {"property_id": title, "required": true, "label": "What happened?"},
                {"property_id": score},
                {"property_id": stage, "required": true},
            ]}})),
        )
        .await;
    Form {
        db,
        view: id_of(&view).to_owned(),
        title,
        score,
        stage,
    }
}

fn submit_path(form: &Form) -> String {
    format!("databases/{}/forms/{}/submit", form.db, form.view)
}

#[tokio::test]
async fn a_member_who_can_read_submits_a_row_they_created() {
    let fixture = Fixture::new().await;
    let form = form(&fixture).await;
    let (member_id, member) = fixture.add_member("member@example.com").await;
    let response = fixture
        .call_as(
            &member,
            "POST",
            &submit_path(&form),
            Some(json!({"answers": {&form.title: "  Printer jam  ", &form.score: 3, &form.stage: "s1"}})),
        )
        .await;
    assert_eq!(response.0, StatusCode::CREATED, "{}", response.1);
    let row = &response.1;
    assert_eq!(row["title"], "Printer jam");
    assert_eq!(row["created_by"], member_id.to_string());
    assert_eq!(row["values"], json!({&form.score: 3.0, &form.stage: "s1"}));
    let database = fixture.database(&form.db).await;
    assert_eq!(database["rows"][0]["id"], row["id"]);
    assert_eq!(database["rows"][0]["created_by"], member_id.to_string());
    assert_eq!(
        fixture
            .count(
                "SELECT CAST(COUNT(*) AS BIGINT) FROM audit_events WHERE action = 'database.form_submitted' AND actor_id = $1",
                &member_id.to_string(),
            )
            .await,
        1
    );

    // A private database's form is not found for other members.
    let private = id_of(
        &fixture
            .create_page(json!({"kind": "database", "title": "Diary", "private": true}))
            .await,
    )
    .to_owned();
    let private_view = id_of(&fixture.add_view(&private, "Form", "form", None).await).to_owned();
    let response = fixture
        .call_as(
            &member,
            "POST",
            &format!("databases/{private}/forms/{private_view}/submit"),
            Some(json!({"answers": {}})),
        )
        .await;
    assert_eq!(
        problem(&response),
        (StatusCode::NOT_FOUND, Some("page_not_found"))
    );
}

#[tokio::test]
async fn answers_are_checked_against_the_questions() {
    let fixture = Fixture::new().await;
    let form = form(&fixture).await;
    let other = fixture.property_id(&form.db, "Other", "text", None).await;
    for (answers, field) in [
        (
            json!({&form.stage: "s1"}),
            format!("answers.{}", form.title),
        ),
        (
            json!({&form.title: "   ", &form.stage: "s1"}),
            format!("answers.{}", form.title),
        ),
        (
            json!({&form.title: "Jam"}),
            format!("answers.{}", form.stage),
        ),
        (
            json!({&form.title: "Jam", &form.stage: "nope"}),
            format!("answers.{}", form.stage),
        ),
        (
            json!({&form.title: "Jam", &form.stage: "s1", &form.score: "three"}),
            format!("answers.{}", form.score),
        ),
        (
            json!({&form.title: "Jam", &form.stage: "s1", &other: "not asked"}),
            format!("answers.{other}"),
        ),
        (
            json!({&form.title: 7, &form.stage: "s1"}),
            format!("answers.{}", form.title),
        ),
        (
            json!({&form.title: "x".repeat(501), &form.stage: "s1"}),
            format!("answers.{}", form.title),
        ),
        (json!({"not-an-id": 1}), "answers.not-an-id".to_owned()),
    ] {
        let response = fixture
            .call(
                "POST",
                &submit_path(&form),
                Some(json!({"answers": answers})),
            )
            .await;
        assert_eq!(
            (response.0, response.1["detail"].as_str()),
            (StatusCode::UNPROCESSABLE_ENTITY, Some(field.as_str())),
            "{answers}"
        );
    }
    assert_eq!(
        fixture.database(&form.db).await["rows"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    // An optional question may stay empty.
    let response = fixture
        .call(
            "POST",
            &submit_path(&form),
            Some(json!({"answers": {&form.title: "Jam", &form.stage: "s1", &form.score: null}})),
        )
        .await;
    assert_eq!(response.0, StatusCode::CREATED, "{}", response.1);
    assert_eq!(response.1["values"], json!({&form.stage: "s1"}));
}

#[tokio::test]
async fn a_closed_form_or_another_view_takes_no_answers() {
    let fixture = Fixture::new().await;
    let form = form(&fixture).await;
    let answers = json!({"answers": {&form.title: "Jam", &form.stage: "s1"}});
    let view = fixture.database(&form.db).await["views"][1].clone();
    let mut config = view["config"].clone();
    config["form"]["access"] = json!("closed");
    fixture
        .ok(
            "PATCH",
            &format!("databases/{}/views/{}", form.db, form.view),
            Some(json!({"config": config})),
        )
        .await;
    let response = fixture
        .call("POST", &submit_path(&form), Some(answers.clone()))
        .await;
    assert_eq!(
        problem(&response),
        (StatusCode::CONFLICT, Some("database_form_closed"))
    );

    let table = fixture.database(&form.db).await["views"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let response = fixture
        .call(
            "POST",
            &format!("databases/{}/forms/{table}/submit", form.db),
            Some(answers),
        )
        .await;
    assert_eq!(
        problem(&response),
        (StatusCode::NOT_FOUND, Some("database_view_not_found"))
    );
    assert_eq!(
        fixture.database(&form.db).await["rows"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
}

#[tokio::test]
async fn a_member_submits_at_most_30_forms_an_hour() {
    let fixture = Fixture::new().await;
    let form = form(&fixture).await;
    let (_, member) = fixture.add_member("member@example.com").await;
    let answers = json!({"answers": {&form.title: "Jam", &form.stage: "s1"}});
    for at in 0..30 {
        let response = fixture
            .call_as(&member, "POST", &submit_path(&form), Some(answers.clone()))
            .await;
        assert_eq!(
            response.0,
            StatusCode::CREATED,
            "submission {at}: {}",
            response.1
        );
    }
    let response = fixture
        .call_as(&member, "POST", &submit_path(&form), Some(answers.clone()))
        .await;
    assert_eq!(
        problem(&response),
        (
            StatusCode::TOO_MANY_REQUESTS,
            Some("database_form_rate_limited")
        )
    );
    // The limit is per member, and submissions older than an hour no longer count.
    let response = fixture
        .call("POST", &submit_path(&form), Some(answers.clone()))
        .await;
    assert_eq!(response.0, StatusCode::CREATED, "{}", response.1);
    sqlx::query(
        "UPDATE audit_events SET occurred_at = occurred_at - 3600001 WHERE action = 'database.form_submitted'",
    )
    .execute(fixture.database.pool())
    .await
    .unwrap();
    let response = fixture
        .call_as(&member, "POST", &submit_path(&form), Some(answers))
        .await;
    assert_eq!(response.0, StatusCode::CREATED, "{}", response.1);
    assert_eq!(
        fixture.database(&form.db).await["rows"]
            .as_array()
            .unwrap()
            .len(),
        32
    );
}

#[tokio::test]
async fn a_submission_to_a_full_database_is_refused() {
    let fixture = Fixture::new().await;
    let form = form(&fixture).await;
    fixture.seed_rows(&form.db, 5_000).await;
    let response = fixture
        .call(
            "POST",
            &submit_path(&form),
            Some(json!({"answers": {&form.title: "Jam", &form.stage: "s1"}})),
        )
        .await;
    assert_eq!(
        problem(&response),
        (StatusCode::CONFLICT, Some("database_row_limit"))
    );
    assert_eq!(
        fixture
            .count(
                "SELECT CAST(COUNT(*) AS BIGINT) FROM audit_events WHERE action = 'database.form_submitted' AND workspace_id = $1",
                &fixture.workspace_id,
            )
            .await,
        0
    );
}

fn questions(view: &Value) -> Vec<(String, bool)> {
    view["config"]["form"]["questions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|question| {
            (
                question["property_id"].as_str().unwrap().to_owned(),
                question["required"].as_bool().unwrap(),
            )
        })
        .collect()
}

#[tokio::test]
async fn form_configs_are_repaired_and_checked() {
    let fixture = Fixture::new().await;
    let form = form(&fixture).await;
    let db = &form.db;

    // A new form view asks for the title, required.
    let fresh = fixture.add_view(db, "Fresh", "form", None).await;
    assert_eq!(questions(&fresh), [(form.title.clone(), true)]);
    assert_eq!(fresh["config"]["form"]["access"], "members");

    // Computed, generated, files and repeated questions are dropped; others keep their order.
    let created = fixture
        .property_id(db, "Created", "created_time", None)
        .await;
    let files = fixture.property_id(db, "Files", "files", None).await;
    let number = fixture.property_id(db, "Ticket", "unique_id", None).await;
    let formula = id_of(
        &fixture
            .add_property(
                db,
                json!({"name": "Twice", "type": "formula", "config": {"expression": "prop(\"Score\") * 2"}}),
            )
            .await,
    )
    .to_owned();
    let saved = fixture
        .add_view(
            db,
            "Mixed",
            "form",
            Some(json!({"form": {"questions": [
                {"property_id": form.score},
                {"property_id": created},
                {"property_id": files},
                {"property_id": number},
                {"property_id": formula},
                {"property_id": form.score, "required": true},
                {"property_id": form.title, "required": true},
            ]}})),
        )
        .await;
    assert_eq!(
        questions(&saved),
        [(form.score.clone(), false), (form.title.clone(), true)]
    );

    // A deleted property's question goes; a table view keeps no form.
    fixture
        .ok(
            "DELETE",
            &format!("databases/{db}/properties/{}", form.stage),
            None,
        )
        .await;
    let database = fixture.database(db).await;
    let intake = database["views"]
        .as_array()
        .unwrap()
        .iter()
        .find(|view| view["id"] == form.view.as_str())
        .unwrap();
    assert_eq!(
        questions(intake),
        [(form.title.clone(), true), (form.score.clone(), false)]
    );
    let table = fixture
        .add_view(
            db,
            "Table",
            "table",
            Some(json!({"form": {"questions": [{"property_id": form.title}]}})),
        )
        .await;
    assert_eq!(table["config"].get("form"), None);

    // Limits name the field.
    for (config, field) in [
        (
            json!({"form": {"questions": [{"property_id": form.title, "label": "x".repeat(201)}]}}),
            "config.form.questions[0].label",
        ),
        (
            json!({"form": {"questions": [], "description": "x".repeat(2001)}}),
            "config.form.description",
        ),
        (
            json!({"form": {"questions": [], "submit_label": "x".repeat(201)}}),
            "config.form.submit_label",
        ),
        (json!({"form": {"access": "public"}}), "config"),
    ] {
        let response = fixture
            .call(
                "POST",
                &format!("databases/{db}/views"),
                Some(json!({"name": "Bad", "type": "form", "config": config})),
            )
            .await;
        assert_eq!(
            (response.0, response.1["detail"].as_str()),
            (StatusCode::UNPROCESSABLE_ENTITY, Some(field)),
            "{config}"
        );
    }
}
