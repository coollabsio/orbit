//! Files, email, phone and unique ID properties (docs databases P3 item 10, spec 12).

mod db_fixture;
mod shared_postgres;

use std::collections::BTreeSet;

use axum::http::StatusCode;
use db_fixture::{Fixture, PNG, id_of, property, value};
use orbit_platform::{Config, EnvironmentMode, PasswordService, TimestampMillis};
use orbit_server::app::App;
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

async fn add(fixture: &Fixture, db: &str, name: &str, kind: &str, config: Value) -> String {
    id_of(
        &fixture
            .add_property(db, json!({"name": name, "type": kind, "config": config}))
            .await,
    )
    .to_owned()
}

#[tokio::test]
async fn email_and_phone_values_must_look_like_an_address_and_a_number() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("Contacts").await;
    let email = add(&fixture, &db, "Email", "email", json!({})).await;
    let phone = add(&fixture, &db, "Phone", "phone", json!({})).await;
    let row = fixture.add_row(&db, "Ada").await;

    let longest_email = format!("{}@x.io", "a".repeat(315));
    let longest_phone = "1".repeat(50);
    for (property, good) in [
        (&email, "ada@orbit.dev"),
        (&email, longest_email.as_str()),
        (&phone, "+36 (1) 234-5678.9"),
        (&phone, longest_phone.as_str()),
    ] {
        let (status, body) = fixture
            .set_values(&db, &row, json!({ property: good }))
            .await;
        assert_eq!(status, StatusCode::OK, "{good}: {body}");
        assert_eq!(
            value(&fixture.database(&db).await, &row, property),
            json!(good)
        );
    }

    let too_long_email = format!("{}@x.io", "a".repeat(316));
    let too_long_phone = "1".repeat(51);
    for (property, bad) in [
        (&email, json!("no-at-sign")),
        (&email, json!("two@at@signs")),
        (&email, json!("with space@orbit.dev")),
        (&email, json!("@orbit.dev")),
        (&email, json!("ada@")),
        (&email, json!(too_long_email)),
        (&email, json!(3)),
        (&phone, json!("call me")),
        (&phone, json!("+-()")),
        (&phone, json!("123a")),
        (&phone, json!(too_long_phone)),
        (&phone, json!(["+36 1"])),
    ] {
        let (status, body) = fixture
            .set_values(&db, &row, json!({ property: bad }))
            .await;
        assert_eq!(
            (status, body["detail"].as_str()),
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                Some(format!("values.{property}").as_str())
            ),
            "{bad}"
        );
    }
    let detail = fixture.database(&db).await;
    assert_eq!(
        (value(&detail, &row, &email), value(&detail, &row, &phone)),
        (json!(longest_email), json!(longest_phone)),
        "refused writes change nothing"
    );

    let (status, _) = fixture
        .set_values(&db, &row, json!({&email: "", &phone: null}))
        .await;
    assert_eq!(status, StatusCode::OK);
    let detail = fixture.database(&db).await;
    assert_eq!(
        (value(&detail, &row, &email), value(&detail, &row, &phone)),
        (Value::Null, Value::Null)
    );
}

/// The unique ID of every row of a database detail, by row title.
fn numbers(detail: &Value, property: &str) -> Vec<(String, Value)> {
    detail["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            (
                row["title"].as_str().unwrap().to_owned(),
                row["values"][property].clone(),
            )
        })
        .collect()
}

#[tokio::test]
async fn unique_ids_number_old_and_new_rows_on_every_create_path_and_are_never_reused() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("Tickets").await;
    for title in ["One", "Two", "Three"] {
        fixture.add_row(&db, title).await;
    }
    let ticket = add(
        &fixture,
        &db,
        "Ticket",
        "unique_id",
        json!({"prefix": "TASK", "next": 50}),
    )
    .await;
    let detail = fixture.database(&db).await;
    assert_eq!(
        numbers(&detail, &ticket),
        [
            ("One".to_owned(), json!(1)),
            ("Two".to_owned(), json!(2)),
            ("Three".to_owned(), json!(3))
        ],
        "existing rows in creation order; a client's next is ignored"
    );
    assert_eq!(
        property(&detail, "Ticket")["config"],
        json!({"prefix": "TASK", "next": 4})
    );

    let four = fixture.add_row(&db, "Four").await;
    fixture.trash_page(&four).await;
    let five = fixture.add_row(&db, "Five").await;
    let three = detail["rows"][2]["id"].as_str().unwrap().to_owned();
    fixture.purge_page(&three).await;
    let six = fixture.add_row(&db, "Six").await;
    let copy = fixture
        .ok(
            "POST",
            &format!("pages/{six}/duplicate"),
            Some(json!({"include_children": false})),
        )
        .await;
    let template = id_of(
        &fixture
            .ok(
                "POST",
                &format!("databases/{db}/templates"),
                Some(json!({"name": "Like five", "from_row_id": five})),
            )
            .await,
    )
    .to_owned();
    let from_template = fixture
        .add_row_with(&db, json!({"title": "Templated", "template_id": template}))
        .await;
    let page = id_of(
        &fixture
            .ok("POST", "pages", Some(json!({"title": "Moved in"})))
            .await,
    )
    .to_owned();
    let version = fixture.page(&page).await["version"].clone();
    fixture
        .ok(
            "POST",
            &format!("pages/{page}/move"),
            Some(json!({"parent_id": db, "position": 0, "expected_version": version})),
        )
        .await;
    fixture.restore_page(&four).await;

    let detail = fixture.database(&db).await;
    let by_title: std::collections::BTreeMap<String, Value> =
        numbers(&detail, &ticket).into_iter().collect();
    assert_eq!(
        by_title,
        [
            ("One", 1),
            ("Two", 2),
            ("Four", 4),
            ("Five", 5),
            ("Six", 6),
            (copy["title"].as_str().unwrap(), 7),
            ("Templated", 8),
            ("Moved in", 9),
        ]
        .into_iter()
        .map(|(title, number)| (title.to_owned(), json!(number)))
        .collect(),
        "a restored row keeps its number; 3 is never given again"
    );
    assert_eq!(value(&detail, &from_template, &ticket), json!(8));
    let templates = &detail["templates"];
    assert_eq!(
        templates[0]["values"].get(&ticket),
        None,
        "a template has no number"
    );
    assert_eq!(
        property(&detail, "Ticket")["config"],
        json!({"prefix": "TASK", "next": 10})
    );

    let (status, body) = fixture.set_values(&db, &five, json!({&ticket: 99})).await;
    assert_eq!(
        (status, body["detail"].as_str()),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Some(format!("values.{ticket}").as_str())
        ),
        "clients never write numbers"
    );
    let (status, body) = fixture
        .call(
            "POST",
            &format!("databases/{db}/properties"),
            Some(json!({"name": "Again", "type": "unique_id"})),
        )
        .await;
    assert_eq!(
        (status, body["detail"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("type")),
        "one per database"
    );
    for prefix in ["ELEVEN_CHAR", "A-B", "ÁB"] {
        let other = fixture.create_database("Other").await;
        let (status, body) = fixture
            .call(
                "POST",
                &format!("databases/{other}/properties"),
                Some(json!({"name": "ID", "type": "unique_id", "config": {"prefix": prefix}})),
            )
            .await;
        assert_eq!(
            (status, body["detail"].as_str()),
            (StatusCode::UNPROCESSABLE_ENTITY, Some("config.prefix")),
            "{prefix}"
        );
    }
}

#[tokio::test]
async fn concurrent_row_creates_get_distinct_numbers() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("Rush").await;
    let ticket = add(&fixture, &db, "Ticket", "unique_id", json!({})).await;
    let titles: Vec<String> = (0..12).map(|n| format!("Row {n}")).collect();
    futures_util::future::join_all(titles.iter().map(|title| fixture.add_row(&db, title))).await;
    let detail = fixture.database(&db).await;
    let got: BTreeSet<i64> = numbers(&detail, &ticket)
        .iter()
        .map(|(_, number)| number.as_i64().unwrap())
        .collect();
    assert_eq!(got, (1..=12).collect());
}

/// One server of a shared Postgres database (as Orbit Cloud runs several).
struct Server {
    base: String,
    database: orbit_platform::Database,
    shutdown: CancellationToken,
    task: tokio::task::JoinHandle<()>,
    http: reqwest::Client,
    _root: tempfile::TempDir,
}

impl Server {
    async fn start(url: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut config = Config {
            environment: EnvironmentMode::Development,
            ..Config::default()
        };
        config.data.database = root.path().join("data/orbit.sqlite");
        config.data.attachments = root.path().join("data/attachments");
        config.data.backups = root.path().join("backups");
        config.data.database_url = Some(url.to_owned());
        config.app_key = Some(shared_postgres::APP_KEY.parse().unwrap());
        let app = App::build(config).await.unwrap();
        let database = app.database().clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let shutdown = CancellationToken::new();
        let token = shutdown.clone();
        let task = tokio::spawn(async move {
            app.serve(listener, token).await.unwrap();
        });
        Self {
            base,
            database,
            shutdown,
            task,
            http: reqwest::Client::new(),
            _root: root,
        }
    }

    async fn stop(self) {
        self.shutdown.cancel();
        self.task.await.unwrap();
    }

    async fn call(&self, method: &str, path: &str, session: &str, body: Value) -> Value {
        let response = self
            .http
            .request(method.parse().unwrap(), format!("{}{path}", self.base))
            .header("origin", &self.base)
            .header("cookie", format!("orbit_session_dev={session}"))
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap_or(Value::Null);
        assert!(status.is_success(), "{method} {path}: {status} {body}");
        body
    }

    /// Completes the setup; returns the owner's session and the workspace.
    async fn setup(&self) -> (String, String) {
        let identity = IdentityRepository::new(self.database.clone());
        let now = TimestampMillis::now();
        identity
            .store_setup_token(
                "fields-setup",
                TimestampMillis::from_millis(now.as_millis() + 60_000),
            )
            .await
            .unwrap();
        let setup = identity
            .complete_setup(
                SetupRequest {
                    token: "fields-setup".to_owned(),
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
        (setup.session.token, setup.workspace_id.to_string())
    }
}

#[tokio::test]
async fn two_servers_of_one_database_never_give_a_number_twice() {
    let Some(database) = shared_postgres::SharedDatabase::create().await else {
        return; // several servers need a Postgres database
    };
    let one = Server::start(&database.url).await;
    let two = Server::start(&database.url).await;
    let (session, workspace) = one.setup().await;
    let base = format!("/api/v1/workspaces/{workspace}");
    let db = one
        .call(
            "POST",
            &format!("{base}/pages"),
            &session,
            json!({"kind": "database", "title": "Shared"}),
        )
        .await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let ticket = one
        .call(
            "POST",
            &format!("{base}/databases/{db}/properties"),
            &session,
            json!({"name": "Ticket", "type": "unique_id"}),
        )
        .await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let rows = format!("{base}/databases/{db}/rows");
    let creates = (0..16).map(|n| {
        let server = if n % 2 == 0 { &one } else { &two };
        server.call(
            "POST",
            &rows,
            &session,
            json!({"title": format!("Row {n}")}),
        )
    });
    let created = futures_util::future::join_all(creates).await;
    let got: BTreeSet<i64> = created
        .iter()
        .map(|row| row["values"][&ticket].as_i64().unwrap())
        .collect();
    assert_eq!(
        got,
        (1..=16).collect(),
        "each create returns its own number"
    );
    one.stop().await;
    two.stop().await;
    database.drop().await;
}

#[tokio::test]
async fn a_files_value_names_files_of_its_own_row() {
    let fixture = Fixture::new().await;
    let db = fixture.create_database("Assets").await;
    let files = add(&fixture, &db, "Files", "files", json!({})).await;
    let row = fixture.add_row(&db, "Logo").await;
    let other = fixture.add_row(&db, "Other").await;
    let logo = fixture.upload(&row, "logo.png", PNG).await;
    let spare = fixture.upload(&row, "spare.png", PNG).await;
    let foreign = fixture.upload(&other, "foreign.png", PNG).await;

    let (status, body) = fixture
        .set_values(
            &db,
            &row,
            json!({&files: [{"file_id": spare, "name": "renamed.exe"}, {"file_id": logo, "name": "x"}]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        value(&fixture.database(&db).await, &row, &files),
        json!([{"file_id": spare, "name": "spare.png"}, {"file_id": logo, "name": "logo.png"}]),
        "names come from the stored files, in the order sent"
    );

    let too_many: Vec<Value> = (0..21)
        .map(|_| json!({"file_id": orbit_platform::Id::new_v7().to_string(), "name": "f"}))
        .collect();
    for bad in [
        json!([{"file_id": foreign, "name": "foreign.png"}]),
        json!([{"file_id": orbit_platform::Id::new_v7().to_string(), "name": "gone.png"}]),
        json!([{"file_id": logo, "name": "a"}, {"file_id": logo, "name": "b"}]),
        json!([{"file_id": logo, "name": ""}]),
        json!(too_many),
        json!("logo.png"),
    ] {
        let (status, body) = fixture.set_values(&db, &row, json!({ &files: bad })).await;
        assert_eq!(
            (status, body["detail"].as_str()),
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                Some(format!("values.{files}").as_str())
            ),
            "{bad}"
        );
    }

    // Copies point at their own copied files.
    let copy = fixture
        .ok(
            "POST",
            &format!("pages/{row}/duplicate"),
            Some(json!({"include_children": false})),
        )
        .await;
    let copy = id_of(&copy).to_owned();
    let copied = value(&fixture.database(&db).await, &copy, &files);
    let copied: Vec<(String, String)> = copied
        .as_array()
        .unwrap()
        .iter()
        .map(|file| {
            (
                file["file_id"].as_str().unwrap().to_owned(),
                file["name"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert_eq!(
        copied
            .iter()
            .map(|(_, name)| name.as_str())
            .collect::<Vec<_>>(),
        ["spare.png", "logo.png"]
    );
    for (file, _) in &copied {
        assert!(*file != spare && *file != logo, "a new file id");
        let (status, _) = fixture
            .call("GET", &format!("pages/{copy}/files/{file}"), None)
            .await;
        assert_eq!(status, StatusCode::OK, "a file of the copy");
    }

    // A file taken out of the value stays a file of the row.
    let (status, _) = fixture
        .set_values(
            &db,
            &row,
            json!({&files: [{"file_id": logo, "name": "logo.png"}]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = fixture
        .call("GET", &format!("pages/{row}/files/{spare}"), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        value(&fixture.database(&db).await, &row, &files),
        json!([{"file_id": logo, "name": "logo.png"}])
    );
}
