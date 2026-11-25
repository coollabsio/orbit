//! Phase 1 gate of `docs/specs/2026-11-24-database-rows-at-scale-design.md` (section 4.6): what the
//! current full-load engine costs at `ROWS` rows. Each step prints its median, range and work count.
//!
//! `BENCH_ROWS=50000 cargo test --release -p orbit-server --test database_scale_bench -- --ignored --nocapture`
//! (add `ORBIT_TEST_POSTGRES_URL=…` for Postgres). Above `MAX_ROWS`, the row create is refused.

use crate::database_fixture;

use std::time::Instant;

use database_fixture::{Fixture, id_of, option_id, property};
use orbit_platform::{Id, TimestampMillis};
use orbit_server::repositories::databases::{AgentQuery, DatabaseRepository};
use serde_json::{Value, json};

const RUNS: usize = 5;
const CHUNK: usize = 400;

fn rows() -> usize {
    std::env::var("BENCH_ROWS")
        .ok()
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(50_000)
}

fn stats(mut times: Vec<f64>) -> String {
    times.sort_by(f64::total_cmp);
    format!(
        "median {:.0} ms (range {:.0}..{:.0}, {} runs)",
        times[times.len() / 2],
        times[0],
        times[times.len() - 1],
        times.len()
    )
}

fn status_kb(field: &str) -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    status
        .lines()
        .find_map(|line| line.strip_prefix(field))
        .and_then(|rest| rest.trim().trim_end_matches("kB").trim().parse().ok())
        .unwrap()
}

/// Peak resident memory added by `work`, in MB: resets the high-water mark first.
async fn peak_mb<T>(work: impl Future<Output = T>) -> (T, f64) {
    std::fs::write("/proc/self/clear_refs", "5").unwrap();
    let before = status_kb("VmRSS:");
    let out = work.await;
    let peak = status_kb("VmHWM:");
    (out, peak.saturating_sub(before) as f64 / 1024.0)
}

struct Seeder<'a> {
    fixture: &'a Fixture,
    teamspace: String,
    now: i64,
}

impl Seeder<'_> {
    async fn new<'b>(fixture: &'b Fixture, db: &str) -> Seeder<'b> {
        let teamspace = fixture
            .database
            .scalar(&format!("SELECT teamspace_id FROM pages WHERE id = '{db}'"))
            .await
            .unwrap();
        Seeder {
            fixture,
            teamspace,
            now: TimestampMillis::now().as_millis(),
        }
    }

    async fn insert(&self, table: &str, columns: &[&str], records: Vec<Vec<Value>>) {
        for chunk in records.chunks(CHUNK) {
            let mut sql = format!("INSERT INTO {table} ({}) VALUES ", columns.join(", "));
            let mut n = 0;
            for (at, record) in chunk.iter().enumerate() {
                if at > 0 {
                    sql.push_str(", ");
                }
                let marks: Vec<String> = record
                    .iter()
                    .map(|_| {
                        n += 1;
                        format!("${n}")
                    })
                    .collect();
                sql.push_str(&format!("({})", marks.join(", ")));
            }
            let mut query = sqlx::query(sqlx::AssertSqlSafe(sql));
            for record in chunk {
                for value in record {
                    query = match value {
                        Value::Number(number) => query.bind(number.as_i64().unwrap()),
                        Value::String(text) => query.bind(text.clone()),
                        other => query.bind(other.to_string()),
                    };
                }
            }
            query.execute(self.fixture.database.pool()).await.unwrap();
        }
    }

    async fn rows(&self, db: &str, count: usize) -> Vec<String> {
        let ids: Vec<String> = (0..count).map(|_| Id::new_v7().to_string()).collect();
        let owner = self.fixture.owner_id.to_string();
        let records = ids
            .iter()
            .enumerate()
            .map(|(at, id)| {
                vec![
                    json!(id),
                    json!(self.fixture.workspace_id),
                    json!(db),
                    json!(self.teamspace),
                    json!(format!("Row {at} {}", WORDS[at % WORDS.len()])),
                    // Gaps of `POSITION_GAP`, as the `docs.row_positions` job leaves old rows.
                    json!(at as i64 * 65_536),
                    json!(owner),
                    json!(owner),
                    json!(self.now),
                    json!(self.now),
                ]
            })
            .collect();
        self.insert(
            "pages",
            &[
                "id",
                "workspace_id",
                "parent_id",
                "teamspace_id",
                "title",
                "position",
                "creator_id",
                "updated_by",
                "created_at",
                "updated_at",
            ],
            records,
        )
        .await;
        ids
    }

    /// One stored value per row for `property`; `value` returns the JSON text or `None`.
    async fn values(
        &self,
        rows: &[String],
        property: &str,
        value: impl Fn(usize) -> Option<String>,
    ) {
        let owner = self.fixture.owner_id.to_string();
        let records = rows
            .iter()
            .enumerate()
            .filter_map(|(at, row)| {
                value(at).map(|json| {
                    vec![
                        json!(row),
                        json!(property),
                        json!(self.fixture.workspace_id),
                        Value::String(json),
                        json!(owner),
                        json!(self.now),
                    ]
                })
            })
            .collect();
        self.insert(
            "page_property_values",
            &[
                "page_id",
                "property_id",
                "workspace_id",
                "value_json",
                "updated_by",
                "updated_at",
            ],
            records,
        )
        .await;
    }

    async fn links(&self, rows: &[String], property: &str, targets: &[String]) {
        let records = rows
            .iter()
            .enumerate()
            .flat_map(|(at, row)| {
                (0..(at % 3) + 1).map(move |k| {
                    vec![
                        json!(property),
                        json!(row),
                        json!(targets[(at * 7 + k * 13) % targets.len()]),
                        json!(self.fixture.workspace_id),
                        json!(k as i64),
                        json!(self.now),
                    ]
                })
            })
            .collect();
        self.insert(
            "page_relations",
            &[
                "property_id",
                "page_id",
                "related_page_id",
                "workspace_id",
                "position",
                "created_at",
            ],
            records,
        )
        .await;
    }
}

const WORDS: [&str; 10] = [
    "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel", "india", "juliet",
];

fn options(prefix: &str, count: usize) -> Value {
    json!({"options": (0..count).map(|i| json!({"name": format!("{prefix}{i}")})).collect::<Vec<_>>()})
}

struct Seeded {
    main: String,
    target: String,
    text: String,
}

async fn seed(fixture: &Fixture, count: usize) -> Seeded {
    let target = fixture.create_database("Target").await;
    let amount = fixture.property_id(&target, "Amount", "number", None).await;
    let main = fixture.create_database("Main").await;
    let mut texts = Vec::new();
    for n in 0..5 {
        texts.push(
            fixture
                .property_id(&main, &format!("T{n}"), "text", None)
                .await,
        );
    }
    let mut numbers = Vec::new();
    for n in 0..3 {
        numbers.push(
            fixture
                .property_id(&main, &format!("N{n}"), "number", None)
                .await,
        );
    }
    let mut selects = Vec::new();
    for n in 0..2 {
        let config = options(&format!("s{n}-"), 12);
        fixture
            .property_id(&main, &format!("S{n}"), "select", Some(config))
            .await;
        let detail = fixture.database(&main).await;
        let select = property(&detail, &format!("S{n}"));
        let ids: Vec<String> = (0..12)
            .map(|i| option_id(select, &format!("s{n}-{i}")))
            .collect();
        selects.push((id_of(select).to_owned(), ids));
    }
    fixture
        .property_id(&main, "M", "multi_select", Some(options("m", 20)))
        .await;
    let detail = fixture.database(&main).await;
    let multi = property(&detail, "M");
    let multi_ids: Vec<String> = (0..20)
        .map(|i| option_id(multi, &format!("m{i}")))
        .collect();
    let multi = id_of(multi).to_owned();
    let mut dates = Vec::new();
    for n in 0..2 {
        dates.push(
            fixture
                .property_id(&main, &format!("D{n}"), "date", None)
                .await,
        );
    }
    let check = fixture.property_id(&main, "Done", "checkbox", None).await;
    let url = fixture.property_id(&main, "Link", "url", None).await;
    let relation = fixture
        .property_id(
            &main,
            "Orders",
            "relation",
            Some(json!({"database_id": target, "limit": "none", "two_way": false})),
        )
        .await;
    for (name, function) in [("Total", "sum"), ("Count", "count_all")] {
        fixture
            .property_id(
                &main,
                name,
                "rollup",
                Some(json!({"relation_property_id": relation, "target_property_id": amount, "function": function})),
            )
            .await;
    }
    for (name, expression) in [
        ("F0", "prop(\"N0\") * prop(\"N1\")"),
        ("F1", "if(prop(\"F0\") > 500, \"big\", \"small\")"),
        ("F2", "prop(\"Total\") + prop(\"N2\")"),
        ("F3", "format(prop(\"N0\"))"),
        ("F4", "if(prop(\"Done\"), 1, 0) + prop(\"Count\")"),
    ] {
        fixture
            .property_id(
                &main,
                name,
                "formula",
                Some(json!({"expression": expression})),
            )
            .await;
    }

    let seeder = Seeder::new(fixture, &main).await;
    let started = Instant::now();
    let target_rows = seeder.rows(&target, count).await;
    seeder
        .values(&target_rows, &amount, |at| {
            Some(((at * 37) % 1000).to_string())
        })
        .await;
    let rows = seeder.rows(&main, count).await;
    for (n, text) in texts.iter().enumerate() {
        seeder
            .values(&rows, text, |at| {
                (at % 7 != n).then(|| {
                    json!(format!(
                        "{} {} {}",
                        WORDS[(at + n) % 10],
                        WORDS[(at * 3 + n) % 10],
                        at
                    ))
                    .to_string()
                })
            })
            .await;
    }
    for (n, number) in numbers.iter().enumerate() {
        seeder
            .values(&rows, number, |at| Some(((at * (n + 3)) % 997).to_string()))
            .await;
    }
    for (property, ids) in &selects {
        seeder
            .values(&rows, property, |at| {
                Some(json!(ids[(at * 5) % ids.len()]).to_string())
            })
            .await;
    }
    seeder
        .values(&rows, &multi, |at| {
            Some(json!([multi_ids[at % 20], multi_ids[(at + 7) % 20]]).to_string())
        })
        .await;
    for (n, date) in dates.iter().enumerate() {
        seeder
            .values(&rows, date, |at| {
                Some(json!({"start": format!("2026-{:02}-{:02}", (at + n) % 12 + 1, at % 28 + 1), "end": null}).to_string())
            })
            .await;
    }
    seeder
        .values(&rows, &check, |at| (at % 2 == 0).then(|| "true".to_owned()))
        .await;
    seeder
        .values(&rows, &url, |at| {
            Some(json!(format!("https://example.com/{at}")).to_string())
        })
        .await;
    seeder.links(&rows, &relation, &target_rows).await;
    println!(
        "seeded {count} rows x 20 properties (+{count} target rows) in {:.1} s",
        started.elapsed().as_secs_f64()
    );
    Seeded {
        main,
        target,
        text: texts[0].clone(),
    }
}

#[tokio::test]
#[ignore = "a benchmark; run it with --release --ignored --nocapture"]
async fn full_load_engine_at_scale() {
    let count = rows();
    let fixture = Fixture::new().await;
    let backend = if orbit_platform::testing_postgres() {
        "postgres"
    } else {
        "sqlite"
    };
    println!(
        "backend {backend}, {count} rows, uptime: {:?}",
        std::fs::read_to_string("/proc/loadavg").unwrap().trim()
    );
    let s = seed(&fixture, count).await;

    // Warm-up read also checks that the work happened.
    let body = fixture.database(&s.main).await;
    let loaded = body["rows"].as_array().map(Vec::len).unwrap_or(0);
    assert_eq!(
        loaded,
        count,
        "GET returned every row: {}",
        &body.to_string()[..300.min(body.to_string().len())]
    );
    let first = &body["rows"][0]["values"];
    let computed = first.as_object().map(|values| values.len()).unwrap_or(0);
    println!("row 0 has {computed} values (stored + computed): {first}");
    let bytes = body.to_string().len();
    drop(body);

    let mut get_times = Vec::new();
    let mut get_peaks = Vec::new();
    let mut target_times = Vec::new();
    for _ in 0..RUNS {
        let started = Instant::now();
        let (_, peak) = peak_mb(fixture.database(&s.main)).await;
        get_times.push(started.elapsed().as_secs_f64() * 1000.0);
        get_peaks.push(peak);
        let started = Instant::now();
        fixture.database(&s.target).await;
        target_times.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    println!(
        "GET main (20 props, 2 rollups, 5 formulas): {}; response {:.1} MB; peak RSS added {}",
        stats(get_times),
        bytes as f64 / 1_048_576.0,
        get_peaks
            .iter()
            .map(|mb| format!("{mb:.0} MB"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!("GET target (1 number prop): {}", stats(target_times));

    let repository = DatabaseRepository::new((*fixture.database).clone());
    let workspace: Id = fixture.workspace_id.parse().unwrap();
    let database: Id = s.main.parse().unwrap();
    let mut query_times = Vec::new();
    let mut total = 0;
    for _ in 0..RUNS {
        let started = Instant::now();
        let page = repository
            .agent_query(
                workspace,
                database,
                fixture.owner_id,
                AgentQuery {
                    filter: Some(json!({"op": "and", "children": [
                        {"property_id": "N0", "operator": ">=", "value": 100},
                        {"property_id": "Done", "operator": "is", "value": true},
                    ]})),
                    sorts: Some(
                        json!([{"property_id": "N1", "direction": "desc"}, {"property_id": "T0"}]),
                    ),
                    view_id: None,
                    limit: Some(50),
                    cursor: None,
                    tz: None,
                },
                "https://orbit.test",
            )
            .await
            .unwrap();
        query_times.push(started.elapsed().as_secs_f64() * 1000.0);
        total = page.total;
    }
    assert!(total > 0, "the query matched rows");
    println!(
        "agent query (2 filters, 2 sorts, limit 50, total {total}): {}",
        stats(query_times)
    );

    let fill = async || {
        let started = Instant::now();
        let (mut batches, mut slowest, mut after) = (0, 0.0_f64, None);
        loop {
            let batch = Instant::now();
            let last = repository
                .refresh_search_text_batch(database, after)
                .await
                .unwrap();
            slowest = slowest.max(batch.elapsed().as_secs_f64() * 1000.0);
            batches += 1;
            match last {
                Some(last) => after = Some(last),
                None => break,
            }
        }
        while repository.fill_search_text().await.unwrap() > 0 {}
        let missing = fixture
            .count(
                "SELECT COUNT(*) FROM pages WHERE parent_id = $1 AND property_text IS NULL",
                &s.main,
            )
            .await;
        assert_eq!(missing, 0, "every row has its search text again");
        println!(
            "  then the search text job: {batches} transactions, {:.0} ms in all, slowest {slowest:.0} ms",
            started.elapsed().as_secs_f64() * 1000.0
        );
    };
    let timed = |label: &str, started: Instant| {
        println!(
            "{label}: {:.0} ms",
            started.elapsed().as_secs_f64() * 1000.0
        );
    };
    fill().await;

    let started = Instant::now();
    let (status, created) = fixture
        .call(
            "POST",
            &format!("databases/{}/rows", s.main),
            Some(json!({"title": "Top", "after_row_id": null})),
        )
        .await;
    if status.is_success() {
        timed("create a row at the top", started);
    } else {
        println!("create a row at the top: refused with {status} (raise MAX_ROWS to measure it)");
    }
    let detail = fixture.database(&s.main).await;
    let rows = detail["rows"].as_array().unwrap();
    let middle = rows[rows.len() / 2]["id"].as_str().unwrap().to_owned();
    let last = rows[rows.len() - 1]["id"].as_str().unwrap().to_owned();
    if status.is_success() {
        assert_eq!(rows[0]["id"], created["id"], "the new row comes first");
    }
    drop(detail);
    let started = Instant::now();
    fixture
        .ok(
            "POST",
            &format!("databases/{}/rows/{last}/move", s.main),
            Some(json!({"after_row_id": middle})),
        )
        .await;
    timed("move the last row to the middle", started);
    let detail = fixture.database(&s.main).await;
    let order: Vec<&str> = detail["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["id"].as_str().unwrap())
        .collect();
    let at = order.iter().position(|id| *id == middle).unwrap();
    assert_eq!(
        order[at + 1],
        last,
        "the moved row follows its new neighbour"
    );
    drop(detail);

    let started = Instant::now();
    fixture.property_id(&s.main, "Extra", "text", None).await;
    timed("add a text property", started);
    fill().await;

    let started = Instant::now();
    let (status, body) = fixture
        .call(
            "DELETE",
            &format!("databases/{}/properties/{}", s.main, s.text),
            None,
        )
        .await;
    assert!(status.is_success(), "{status} {body}");
    timed("delete a text property", started);
    fill().await;
}
