//! The formula cost check of spec 15.9: `GET /databases/{id}` of 5,000 rows with 20 formulas,
//! each at the 500-node limit and with `map` over a 100-item list. Target: under 300 ms.
//!
//! `cargo test --release -p orbit-server --test database_formula_bench -- --ignored --nocapture`

mod database_fixture;

use std::time::Instant;

use database_fixture::{Fixture, id_of};
use orbit_platform::{Id, TimestampMillis};
use serde_json::{Value, json};

const ROWS: i64 = 5_000;
const FORMULAS: usize = 20;
const RUNS: usize = 7;

/// Formula `n`: `map` over `[1..100]` with a body that reads two properties, plus a second list
/// of `pad` numbers so the tree reaches the node limit without nesting deeper.
fn expression(n: usize, pad: usize) -> String {
    let items: Vec<String> = (1..=100).map(|i| i.to_string()).collect();
    let padding: Vec<String> = (0..pad).map(|i| (i % 10).to_string()).collect();
    format!(
        "sum(map([{}], current * prop(\"Price\") + index % 7 - prop(\"Qty\") / 2)) + sum([{}]) + {n}",
        items.join(","),
        padding.join(",")
    )
}

async fn seeded(fixture: &Fixture, title: &str) -> (String, String, String) {
    let db = fixture.create_database(title).await;
    let price = fixture.property_id(&db, "Price", "number", None).await;
    let qty = fixture.property_id(&db, "Qty", "number", None).await;
    let teamspace: String = fixture
        .database
        .scalar(&format!("SELECT teamspace_id FROM pages WHERE id = '{db}'"))
        .await
        .unwrap();
    let now = TimestampMillis::now().as_millis();
    let owner = fixture.owner_id.to_string();
    let mut tx = fixture.database.pool().begin().await.unwrap();
    for position in 0..ROWS {
        let id = Id::new_v7().to_string();
        sqlx::query(
            "INSERT INTO pages (id, workspace_id, parent_id, teamspace_id, title, position, creator_id, updated_by, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, 'Row', $5, $6, $7, $8, $9)",
        )
        .bind(&id)
        .bind(&fixture.workspace_id)
        .bind(&db)
        .bind(&teamspace)
        .bind(position)
        .bind(&owner)
        .bind(&owner)
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await
        .unwrap();
        for (property, value) in [(&price, position % 97), (&qty, position % 13)] {
            sqlx::query(
                "INSERT INTO page_property_values (page_id, property_id, workspace_id, value_json, updated_by, updated_at) \
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(&id)
            .bind(property)
            .bind(&fixture.workspace_id)
            .bind(value.to_string())
            .bind(&owner)
            .bind(now)
            .execute(&mut *tx)
            .await
            .unwrap();
        }
    }
    tx.commit().await.unwrap();
    (db, price, qty)
}

/// The largest padding that keeps formula 0 within the node limit.
async fn widest_pad(fixture: &Fixture, db: &str) -> usize {
    for pad in (0..=400).rev() {
        let preview = fixture
            .ok(
                "POST",
                &format!("databases/{db}/formula/preview"),
                Some(json!({"expression": expression(0, pad)})),
            )
            .await;
        if preview.get("error").is_none() {
            return pad;
        }
    }
    panic!("no padding fits");
}

fn median_ms(mut times: Vec<f64>) -> (f64, f64, f64) {
    times.sort_by(f64::total_cmp);
    (times[times.len() / 2], times[0], times[times.len() - 1])
}

#[tokio::test]
#[ignore = "a benchmark; run it with --release --ignored --nocapture"]
async fn get_of_5000_rows_with_20_formulas_at_the_node_limit() {
    let fixture = Fixture::new().await;
    let (plain, _, _) = seeded(&fixture, "Plain").await;
    let (heavy, _, _) = seeded(&fixture, "Heavy").await;
    let pad = widest_pad(&fixture, &heavy).await;
    let over = fixture
        .ok(
            "POST",
            &format!("databases/{heavy}/formula/preview"),
            Some(json!({"expression": expression(0, pad + 1)})),
        )
        .await;
    assert_eq!(
        over["error"]["message"], "This formula is too long (at most 500 parts).",
        "formula 0 sits at the node limit"
    );
    let mut formulas = Vec::new();
    for n in 0..FORMULAS {
        let property = fixture
            .add_property(
                &heavy,
                json!({"name": format!("F{n}"), "type": "formula", "config": {"expression": expression(n, pad)}}),
            )
            .await;
        formulas.push(id_of(&property).to_owned());
    }

    let get = |db: String| {
        let fixture = &fixture;
        async move {
            let started = Instant::now();
            let body = fixture.database(&db).await;
            (started.elapsed().as_secs_f64() * 1_000.0, body)
        }
    };
    // Warm up both, then alternate so caches and drift favor neither.
    let (_, body) = get(heavy.clone()).await;
    let rows = body["rows"].as_array().unwrap();
    assert_eq!(rows.len(), usize::try_from(ROWS).unwrap());
    let failed: Vec<(usize, &Value)> = rows
        .iter()
        .enumerate()
        .flat_map(|(at, row)| {
            formulas
                .iter()
                .map(move |formula| (at, &row["values"][formula]))
        })
        .filter(|(_, value)| !value.is_number())
        .collect();
    get(plain.clone()).await;

    let (mut plain_times, mut heavy_times) = (Vec::new(), Vec::new());
    for _ in 0..RUNS {
        plain_times.push(get(plain.clone()).await.0);
        heavy_times.push(get(heavy.clone()).await.0);
    }
    let (plain_median, plain_min, plain_max) = median_ms(plain_times);
    let (heavy_median, heavy_min, heavy_max) = median_ms(heavy_times);
    println!(
        "formula bench ({RUNS} runs each, {ROWS} rows, {FORMULAS} formulas, padding {pad}): \
         without formulas median {plain_median:.1} ms (range {plain_min:.1}..{plain_max:.1}); \
         with formulas median {heavy_median:.1} ms (range {heavy_min:.1}..{heavy_max:.1}); \
         {} of {} cells failed{}",
        failed.len(),
        rows.len() * FORMULAS,
        failed
            .first()
            .map(|(at, value)| format!(", the first in row {at}: {value}"))
            .unwrap_or_default(),
    );
    assert!(
        failed.is_empty(),
        "every cell computes within the read budget"
    );
    assert!(heavy_median < 300.0, "the spec 15.9 target is 300 ms");
}
