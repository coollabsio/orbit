//! Relation and rollup properties (docs databases P3 item 9, spec 11): links between rows, both
//! sides of a two-way pair, access, trash, templates, duplicates and rollup functions.

mod db_fixture;

use axum::http::StatusCode;
use db_fixture::{Fixture, id_of, property, row, value};
use serde_json::{Value, json};

/// Adds a relation from `database` to `target`; returns the relation property.
async fn relate(
    fixture: &Fixture,
    database: &str,
    name: &str,
    target: &str,
    two_way: bool,
) -> Value {
    fixture
        .add_property(
            database,
            json!({"name": name, "type": "relation",
                   "config": {"database_id": target, "limit": "none", "two_way": two_way}}),
        )
        .await
}

async fn link(fixture: &Fixture, database: &str, row: &str, property: &str, pages: &[&str]) {
    let (status, body) = fixture
        .set_values(database, row, json!({ property: pages }))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

fn ids(pages: &[&str]) -> Value {
    json!(pages)
}

/// Tasks (t1, t2) relate to Projects (p1, p2) through a two-way pair "Project" / "Tasks".
struct Pair {
    tasks: String,
    projects: String,
    a: String,
    b: String,
    t1: String,
    t2: String,
    p1: String,
    p2: String,
}

async fn pair(fixture: &Fixture) -> Pair {
    let tasks = fixture.create_database("Tasks").await;
    let projects = fixture.create_database("Projects").await;
    let a = relate(fixture, &tasks, "Project", &projects, true).await;
    let b = a["config"]["synced_property_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let a = id_of(&a).to_owned();
    let t1 = fixture.add_row(&tasks, "Write").await;
    let t2 = fixture.add_row(&tasks, "Ship").await;
    let p1 = fixture.add_row(&projects, "Apollo").await;
    let p2 = fixture.add_row(&projects, "Gemini").await;
    Pair {
        tasks,
        projects,
        a,
        b,
        t1,
        t2,
        p1,
        p2,
    }
}

#[tokio::test]
async fn a_two_way_relation_reads_and_writes_from_both_sides() {
    let fixture = Fixture::new().await;
    let p = pair(&fixture).await;

    let projects = fixture.database(&p.projects).await;
    let synced = property(&projects, "Tasks");
    assert_eq!(id_of(synced), p.b);
    assert_eq!(
        (&synced["type"], &synced["config"]),
        (
            &json!("relation"),
            &json!({"database_id": p.tasks, "limit": "none", "synced_property_id": p.a, "primary": false})
        ),
        "the synced property is named after the source database and points back"
    );

    link(&fixture, &p.tasks, &p.t1, &p.a, &[&p.p2, &p.p1]).await;
    link(&fixture, &p.tasks, &p.t2, &p.a, &[&p.p1]).await;
    let tasks = fixture.database(&p.tasks).await;
    assert_eq!(
        value(&tasks, &p.t1, &p.a),
        ids(&[&p.p2, &p.p1]),
        "cell order"
    );
    assert_eq!(value(&tasks, &p.t2, &p.a), ids(&[&p.p1]));
    assert_eq!(tasks["related_pages"][&p.p1]["title"], "Apollo");
    assert_eq!(
        tasks["related_pages"][&p.p2]["database_id"],
        p.projects.as_str()
    );

    let projects = fixture.database(&p.projects).await;
    assert_eq!(value(&projects, &p.p1, &p.b), ids(&[&p.t1, &p.t2]));
    assert_eq!(value(&projects, &p.p2, &p.b), ids(&[&p.t1]));
    assert_eq!(projects["related_pages"][&p.t2]["title"], "Ship");

    // A write on the synced side changes the same links.
    link(&fixture, &p.projects, &p.p2, &p.b, &[&p.t2]).await;
    let tasks = fixture.database(&p.tasks).await;
    assert_eq!(value(&tasks, &p.t1, &p.a), ids(&[&p.p1]));
    assert_eq!(value(&tasks, &p.t2, &p.a), ids(&[&p.p1, &p.p2]));

    let related_changed = |actions: Vec<String>| {
        actions
            .iter()
            .filter(|action| *action == "database.related_changed")
            .count()
    };
    assert_eq!(
        related_changed(fixture.audit_actions(&p.projects).await),
        2,
        "each value write in Tasks tells Projects"
    );
    assert_eq!(related_changed(fixture.audit_actions(&p.tasks).await), 1);
}

#[tokio::test]
async fn deleting_the_primary_hands_its_links_to_the_synced_property() {
    let fixture = Fixture::new().await;
    let p = pair(&fixture).await;
    link(&fixture, &p.tasks, &p.t1, &p.a, &[&p.p1, &p.p2]).await;
    link(&fixture, &p.tasks, &p.t2, &p.a, &[&p.p1]).await;

    fixture
        .ok(
            "DELETE",
            &format!("databases/{}/properties/{}", p.tasks, p.a),
            None,
        )
        .await;
    let tasks = fixture.database(&p.tasks).await;
    assert!(
        tasks["properties"]
            .as_array()
            .unwrap()
            .iter()
            .all(|property| property["id"] != p.a.as_str())
    );
    let projects = fixture.database(&p.projects).await;
    assert_eq!(
        property(&projects, "Tasks")["config"],
        json!({"database_id": p.tasks, "limit": "none", "synced_property_id": null, "primary": true})
    );
    assert_eq!(value(&projects, &p.p1, &p.b), ids(&[&p.t1, &p.t2]));
    assert_eq!(value(&projects, &p.p2, &p.b), ids(&[&p.t1]));

    // The new primary takes writes alone.
    link(&fixture, &p.projects, &p.p2, &p.b, &[&p.t2, &p.t1]).await;
    let projects = fixture.database(&p.projects).await;
    assert_eq!(value(&projects, &p.p2, &p.b), ids(&[&p.t2, &p.t1]));
}

#[tokio::test]
async fn turning_two_way_off_deletes_the_synced_property_and_keeps_the_links() {
    let fixture = Fixture::new().await;
    let p = pair(&fixture).await;
    link(&fixture, &p.tasks, &p.t1, &p.a, &[&p.p1]).await;

    fixture
        .ok(
            "PATCH",
            &format!("databases/{}/properties/{}", p.tasks, p.a),
            Some(json!({"config": {"database_id": p.projects, "limit": "none", "two_way": false}})),
        )
        .await;
    let projects = fixture.database(&p.projects).await;
    let names: Vec<&str> = projects["properties"]
        .as_array()
        .unwrap()
        .iter()
        .map(|property| property["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Name", "Tags"]);
    let tasks = fixture.database(&p.tasks).await;
    assert_eq!(value(&tasks, &p.t1, &p.a), ids(&[&p.p1]));
    assert_eq!(
        property(&tasks, "Project")["config"]["synced_property_id"],
        Value::Null
    );
}

#[tokio::test]
async fn a_two_way_self_relation_gives_blocked_by_and_blocking() {
    let fixture = Fixture::new().await;
    let issues = fixture.create_database("Issues").await;
    let blocked_by = relate(&fixture, &issues, "Blocked by", &issues, true).await;
    let blocking = blocked_by["config"]["synced_property_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let blocked_by = id_of(&blocked_by).to_owned();
    fixture
        .ok(
            "PATCH",
            &format!("databases/{issues}/properties/{blocking}"),
            Some(json!({"name": "Blocking"})),
        )
        .await;
    let first = fixture.add_row(&issues, "Design").await;
    let second = fixture.add_row(&issues, "Build").await;
    link(&fixture, &issues, &second, &blocked_by, &[&first]).await;

    let detail = fixture.database(&issues).await;
    assert_eq!(
        id_of(property(&detail, "Blocking")),
        blocking,
        "both properties live in the one database"
    );
    assert_eq!(value(&detail, &second, &blocked_by), ids(&[&first]));
    assert_eq!(value(&detail, &first, &blocking), ids(&[&second]));
    assert_eq!(value(&detail, &first, &blocked_by), Value::Null);
    assert_eq!(value(&detail, &second, &blocking), Value::Null);
}

#[tokio::test]
async fn trashed_pages_leave_values_and_rollups_until_restored_and_purged_pages_lose_their_links() {
    let fixture = Fixture::new().await;
    let p = pair(&fixture).await;
    let count = id_of(
        &fixture
            .add_property(
                &p.tasks,
                json!({"name": "Count", "type": "rollup", "config": {
                    "relation_property_id": p.a,
                    "target_property_id": id_of(property(&fixture.database(&p.projects).await, "Name")),
                    "function": "count_all"}}),
            )
            .await,
    )
    .to_owned();
    link(&fixture, &p.tasks, &p.t1, &p.a, &[&p.p1, &p.p2]).await;

    fixture.trash_page(&p.p1).await;
    let tasks = fixture.database(&p.tasks).await;
    assert_eq!(value(&tasks, &p.t1, &p.a), ids(&[&p.p2]));
    assert_eq!(value(&tasks, &p.t1, &count), json!(1));
    assert_eq!(tasks["related_pages"].get(&p.p1), None);

    fixture.restore_page(&p.p1).await;
    let tasks = fixture.database(&p.tasks).await;
    assert_eq!(value(&tasks, &p.t1, &p.a), ids(&[&p.p1, &p.p2]));
    assert_eq!(value(&tasks, &p.t1, &count), json!(2));

    fixture.purge_page(&p.p2).await;
    let tasks = fixture.database(&p.tasks).await;
    assert_eq!(value(&tasks, &p.t1, &p.a), ids(&[&p.p1]));
    let stored: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM page_relations WHERE page_id = $1 OR related_page_id = $1",
    )
    .bind(&p.p2)
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(stored, 0, "a page deleted forever takes its links along");

    // A trashed row keeps its own links too.
    fixture.trash_page(&p.t1).await;
    let projects = fixture.database(&p.projects).await;
    assert_eq!(value(&projects, &p.p1, &p.b), Value::Null);
    fixture.restore_page(&p.t1).await;
    let projects = fixture.database(&p.projects).await;
    assert_eq!(value(&projects, &p.p1, &p.b), ids(&[&p.t1]));
}

#[tokio::test]
async fn a_relation_needs_read_access_and_two_way_an_unlocked_target() {
    let fixture = Fixture::new().await;
    let (_, member) = fixture.add_member("member@example.com").await;
    let tasks = fixture.create_database("Tasks").await;
    let diary = fixture
        .create_database_as(
            &member,
            json!({"kind": "database", "title": "Diary", "private": true}),
        )
        .await;
    for two_way in [false, true] {
        let (status, body) = fixture
            .call(
                "POST",
                &format!("databases/{tasks}/properties"),
                Some(json!({"name": "Diary", "type": "relation",
                            "config": {"database_id": diary, "limit": "none", "two_way": two_way}})),
            )
            .await;
        assert_eq!(
            (status, body["detail"].as_str()),
            (StatusCode::UNPROCESSABLE_ENTITY, Some("config.database_id")),
            "a database the creator cannot read: {body}"
        );
    }

    let archive = fixture.create_database("Archive").await;
    fixture
        .ok(
            "POST",
            &format!("pages/{archive}/lock"),
            Some(json!({"locked": true})),
        )
        .await;
    let (status, body) = fixture
        .call(
            "POST",
            &format!("databases/{tasks}/properties"),
            Some(json!({"name": "Archive", "type": "relation",
                        "config": {"database_id": archive, "limit": "none", "two_way": true}})),
        )
        .await;
    assert_eq!(
        (status, body["code"].as_str()),
        (StatusCode::LOCKED, Some("page_locked")),
        "two-way adds a property to the locked target: {body}"
    );
    let one_way = relate(&fixture, &tasks, "Archive", &archive, false).await;
    assert_eq!(one_way["config"]["synced_property_id"], Value::Null);
    assert_eq!(
        fixture.database(&archive).await["properties"]
            .as_array()
            .unwrap()
            .len(),
        2,
        "a one-way relation leaves the target alone"
    );
}

#[tokio::test]
async fn links_to_pages_the_reader_cannot_read_are_hidden_kept_and_left_out_of_rollups() {
    let fixture = Fixture::new().await;
    let (_, member) = fixture.add_member("member@example.com").await;
    let tasks = fixture.create_database("Tasks").await;
    let task = fixture.add_row(&tasks, "Shared task").await;
    let diary = fixture
        .create_database_as(
            &member,
            json!({"kind": "database", "title": "Diary", "private": true}),
        )
        .await;
    let points = id_of(
        &fixture
            .add_property_as(&member, &diary, json!({"name": "Points", "type": "number"}))
            .await,
    )
    .to_owned();
    let entry = id_of(
        &fixture
            .ok_as(
                &member,
                "POST",
                &format!("databases/{diary}/rows"),
                Some(json!({"title": "Private entry", "values": {&points: 5}})),
            )
            .await,
    )
    .to_owned();
    let relation = id_of(
        &fixture
            .add_property_as(
                &member,
                &tasks,
                json!({"name": "Entries", "type": "relation",
                       "config": {"database_id": diary, "limit": "none", "two_way": false}}),
            )
            .await,
    )
    .to_owned();
    let sum = id_of(
        &fixture
            .add_property_as(
                &member,
                &tasks,
                json!({"name": "Points", "type": "rollup", "config": {
                    "relation_property_id": relation, "target_property_id": points, "function": "sum"}}),
            )
            .await,
    )
    .to_owned();
    let (status, body) = fixture
        .set_values_as(&member, &tasks, &task, json!({&relation: [&entry]}))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let mine = fixture.database_as(&member, &tasks).await;
    assert_eq!(value(&mine, &task, &relation), ids(&[&entry]));
    assert_eq!(value(&mine, &task, &sum), json!(5.0));
    assert_eq!(row(&mine, &task).get("hidden_links"), None);

    let theirs = fixture.database(&tasks).await;
    assert_eq!(value(&theirs, &task, &relation), Value::Null);
    assert_eq!(row(&theirs, &task)["hidden_links"], json!({&relation: 1}));
    assert_eq!(theirs["related_pages"].get(&entry), None);
    assert_eq!(
        value(&theirs, &task, &sum),
        json!(0.0),
        "hidden pages do not count"
    );

    // The owner can neither link the private page nor drop it by writing the cell.
    let (status, body) = fixture
        .set_values(&tasks, &task, json!({&relation: [&entry]}))
        .await;
    assert_eq!(
        (status, body["detail"].as_str()),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Some(format!("values.{relation}").as_str())
        )
    );
    let other = fixture.add_row(&tasks, "Unrelated").await;
    let (status, body) = fixture
        .set_values(&tasks, &task, json!({&relation: [&other]}))
        .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a row of another database: {body}"
    );
    let (status, _) = fixture
        .set_values(&tasks, &task, json!({&relation: []}))
        .await;
    assert_eq!(status, StatusCode::OK);
    let mine = fixture.database_as(&member, &tasks).await;
    assert_eq!(
        value(&mine, &task, &relation),
        ids(&[&entry]),
        "the hidden link stays"
    );
}

#[tokio::test]
async fn a_cell_holds_100_links_and_the_reverse_side_refuses_writes_beyond() {
    let fixture = Fixture::new().await;
    let p = pair(&fixture).await;
    let mut rows = Vec::new();
    for n in 0..101 {
        rows.push(
            fixture
                .add_row_with(
                    &p.tasks,
                    json!({"title": format!("Task {n}"), "values": {&p.a: [&p.p1]}}),
                )
                .await,
        );
    }
    let projects = fixture.database(&p.projects).await;
    assert_eq!(
        value(&projects, &p.p1, &p.b).as_array().unwrap().len(),
        101,
        "the reverse side shows every link"
    );
    let (status, body) = fixture
        .set_values(&p.projects, &p.p1, json!({&p.b: [&rows[0]]}))
        .await;
    assert_eq!(
        (status, body["detail"].as_str()),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Some(format!("values.{}.too_many_links", p.b).as_str())
        )
    );
    let (status, body) = fixture
        .set_values(&p.projects, &p.p2, json!({&p.b: rows}))
        .await;
    assert_eq!(
        (status, body["detail"].as_str()),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Some(format!("values.{}", p.b).as_str())
        ),
        "101 links in one write"
    );
    let projects = fixture.database(&p.projects).await;
    assert_eq!(value(&projects, &p.p1, &p.b).as_array().unwrap().len(), 101);
    assert_eq!(value(&projects, &p.p2, &p.b), Value::Null);
}

#[tokio::test]
async fn rollups_compute_numbers_dates_checkboxes_and_lists_and_empty_cells() {
    let fixture = Fixture::new().await;
    let orders = fixture.create_database("Orders").await;
    let items = fixture.create_database("Items").await;
    let add = async |name: &str, kind: &str| {
        id_of(
            &fixture
                .add_property(&items, json!({"name": name, "type": kind}))
                .await,
        )
        .to_owned()
    };
    let points = add("Points", "number").await;
    let when = add("When", "date").await;
    let done = add("Done", "checkbox").await;
    let label = add("Label", "text").await;
    let item = async |title: &str, values: Value| {
        fixture
            .add_row_with(&items, json!({"title": title, "values": values}))
            .await
    };
    let first = item(
        "First",
        json!({&points: 2, &when: {"start": "2026-11-01", "end": null}, &done: true, &label: "a"}),
    )
    .await;
    let second = item(
        "Second",
        json!({&points: 4, &when: {"start": "2026-11-05", "end": "2026-11-07"}, &label: "b"}),
    )
    .await;
    let third = item("Third", json!({&done: true, &label: "a"})).await;
    let relation = id_of(&relate(&fixture, &orders, "Items", &items, false).await).to_owned();
    let full = fixture.add_row(&orders, "Full").await;
    let empty = fixture.add_row(&orders, "Empty").await;
    link(
        &fixture,
        &orders,
        &full,
        &relation,
        &[&first, &second, &third],
    )
    .await;

    let third_of = 1.0 / 3.0;
    let cases: [(&str, &str, Value, Value); 22] = [
        ("sum", &points, json!(6.0), json!(0.0)),
        ("average", &points, json!(3.0), Value::Null),
        ("median", &points, json!(3.0), Value::Null),
        ("min", &points, json!(2.0), Value::Null),
        ("max", &points, json!(4.0), Value::Null),
        ("range", &points, json!(2.0), Value::Null),
        ("count_all", &points, json!(3), json!(0)),
        ("count_values", &points, json!(2), json!(0)),
        ("count_unique", &label, json!(2), json!(0)),
        ("count_empty", &points, json!(1), json!(0)),
        ("count_not_empty", &points, json!(2), json!(0)),
        ("percent_empty", &points, json!(third_of), json!(0.0)),
        ("percent_not_empty", &points, json!(2.0 / 3.0), json!(0.0)),
        (
            "earliest_date",
            &when,
            json!({"start": "2026-11-01", "end": null}),
            Value::Null,
        ),
        (
            "latest_date",
            &when,
            json!({"start": "2026-11-07", "end": null}),
            Value::Null,
        ),
        (
            "date_range",
            &when,
            json!({"start": "2026-11-01", "end": "2026-11-07"}),
            Value::Null,
        ),
        ("checked", &done, json!(2), json!(0)),
        ("unchecked", &done, json!(1), json!(0)),
        ("percent_checked", &done, json!(2.0 / 3.0), json!(0.0)),
        ("percent_unchecked", &done, json!(third_of), json!(0.0)),
        ("show_original", &label, json!(["a", "b", "a"]), Value::Null),
        ("show_unique", &label, json!(["a", "b"]), Value::Null),
    ];
    let mut rollups = Vec::new();
    for (function, target, _, _) in &cases {
        rollups.push(
            id_of(
                &fixture
                    .add_property(
                        &orders,
                        json!({"name": function, "type": "rollup", "config": {
                            "relation_property_id": relation, "target_property_id": target,
                            "function": function}}),
                    )
                    .await,
            )
            .to_owned(),
        );
    }
    let detail = fixture.database(&orders).await;
    for ((function, _, full_value, empty_value), rollup) in cases.iter().zip(&rollups) {
        assert_eq!(
            (
                value(&detail, &full, rollup),
                value(&detail, &empty, rollup)
            ),
            (full_value.clone(), empty_value.clone()),
            "{function}"
        );
    }

    // A rollup over a rollup works one level deep; a deeper chain is refused.
    let lists = fixture.create_database("Lists").await;
    let to_orders = id_of(&relate(&fixture, &lists, "Orders", &orders, false).await).to_owned();
    let nested = fixture
        .add_property(
            &lists,
            json!({"name": "Sums", "type": "rollup", "config": {
                "relation_property_id": to_orders, "target_property_id": rollups[0], "function": "sum"}}),
        )
        .await;
    let list = fixture.add_row(&lists, "Both orders").await;
    link(&fixture, &lists, &list, &to_orders, &[&full, &empty]).await;
    assert_eq!(
        value(&fixture.database(&lists).await, &list, id_of(&nested)),
        json!(6.0)
    );
    let shelves = fixture.create_database("Shelves").await;
    let to_lists = id_of(&relate(&fixture, &shelves, "Lists", &lists, false).await).to_owned();
    let (status, body) = fixture
        .call(
            "POST",
            &format!("databases/{shelves}/properties"),
            Some(json!({"name": "Deep", "type": "rollup", "config": {
                "relation_property_id": to_lists, "target_property_id": id_of(&nested),
                "function": "sum"}})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
}

#[tokio::test]
async fn a_template_keeps_its_links_and_gives_them_to_new_rows() {
    let fixture = Fixture::new().await;
    let p = pair(&fixture).await;
    link(&fixture, &p.tasks, &p.t1, &p.a, &[&p.p1]).await;
    let template = fixture
        .ok(
            "POST",
            &format!("databases/{}/templates", p.tasks),
            Some(json!({"name": "Like Write", "from_row_id": p.t1})),
        )
        .await;
    let template = id_of(&template).to_owned();
    let tasks = fixture.database(&p.tasks).await;
    let kept = tasks["templates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == template.as_str())
        .unwrap();
    assert_eq!(kept["values"][&p.a], ids(&[&p.p1]));

    let made = fixture
        .add_row_with(&p.tasks, json!({"title": "Made", "template_id": template}))
        .await;
    let tasks = fixture.database(&p.tasks).await;
    assert_eq!(value(&tasks, &made, &p.a), ids(&[&p.p1]));
    let projects = fixture.database(&p.projects).await;
    assert_eq!(
        value(&projects, &p.p1, &p.b),
        ids(&[&p.t1, &made]),
        "the reverse cell leaves the template out"
    );
}

#[tokio::test]
async fn a_duplicated_database_keeps_targets_and_links_but_turns_one_way() {
    let fixture = Fixture::new().await;
    let p = pair(&fixture).await;
    link(&fixture, &p.tasks, &p.t1, &p.a, &[&p.p1, &p.p2]).await;

    let copy = fixture
        .ok(
            "POST",
            &format!("pages/{}/duplicate", p.tasks),
            Some(json!({"include_children": true})),
        )
        .await;
    let copy = fixture.database(id_of(&copy)).await;
    let relation = property(&copy, "Project");
    assert_ne!(id_of(relation), p.a);
    assert_eq!(
        relation["config"],
        json!({"database_id": p.projects, "limit": "none", "synced_property_id": null, "primary": true})
    );
    let copied = copy["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["title"] == "Write")
        .unwrap();
    assert_eq!(copied["values"][id_of(relation)], ids(&[&p.p1, &p.p2]));

    let projects = fixture.database(&p.projects).await;
    assert_eq!(
        projects["properties"].as_array().unwrap().len(),
        3,
        "no second synced property"
    );
    assert_eq!(value(&projects, &p.p1, &p.b), ids(&[&p.t1]));
}

#[tokio::test]
async fn a_type_change_to_a_relation_is_refused_and_from_one_clears_the_links() {
    let fixture = Fixture::new().await;
    let p = pair(&fixture).await;
    link(&fixture, &p.tasks, &p.t1, &p.a, &[&p.p1]).await;
    let notes = id_of(
        &fixture
            .add_property(&p.tasks, json!({"name": "Notes", "type": "text"}))
            .await,
    )
    .to_owned();
    let (status, body) = fixture
        .call(
            "PATCH",
            &format!("databases/{}/properties/{notes}", p.tasks),
            Some(json!({"type": "relation", "config": {"database_id": p.projects}})),
        )
        .await;
    assert_eq!(
        (status, body["detail"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("type"))
    );

    fixture
        .ok(
            "PATCH",
            &format!("databases/{}/properties/{}", p.tasks, p.a),
            Some(json!({"type": "text"})),
        )
        .await;
    let tasks = fixture.database(&p.tasks).await;
    assert_eq!(value(&tasks, &p.t1, &p.a), Value::Null);
    let projects = fixture.database(&p.projects).await;
    assert_eq!(
        property(&projects, "Tasks")["config"],
        json!({"database_id": p.tasks, "limit": "none", "synced_property_id": null, "primary": true}),
        "the synced property becomes one-way"
    );
    assert_eq!(value(&projects, &p.p1, &p.b), Value::Null);
}

#[tokio::test]
async fn a_row_moved_out_of_its_database_loses_its_links_on_both_sides() {
    let fixture = Fixture::new().await;
    let p = pair(&fixture).await;
    link(&fixture, &p.tasks, &p.t1, &p.a, &[&p.p1]).await;
    link(&fixture, &p.tasks, &p.t2, &p.a, &[&p.p1, &p.p2]).await;

    for (row, database) in [(&p.t1, &p.tasks), (&p.p2, &p.projects)] {
        let version = fixture.page(row).await["version"].clone();
        let (status, body) = fixture
            .call(
                "POST",
                &format!("pages/{row}/move"),
                Some(json!({"parent_id": null, "position": 0, "expected_version": version})),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{database}: {body}");
    }
    let tasks = fixture.database(&p.tasks).await;
    assert_eq!(value(&tasks, &p.t2, &p.a), ids(&[&p.p1]));
    let projects = fixture.database(&p.projects).await;
    assert_eq!(value(&projects, &p.p1, &p.b), ids(&[&p.t2]));
    let stored: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM page_relations WHERE page_id IN ($1, $2) OR related_page_id IN ($1, $2)",
    )
    .bind(&p.t1)
    .bind(&p.p2)
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(stored, 0);
}
