//! Runs the cases that `@orbit/database` also runs (`packages/database/cases/`, spec P3 14.4)
//! against `query_rows`, so the two engines cannot drift apart.

use std::collections::{BTreeMap, HashMap};
use std::str::FromStr;

use jiff::Timestamp;
use jiff::tz::TimeZone;
use orbit_platform::{Id, TimestampMillis};
use serde_json::Value;

use super::super::{DatabaseRow, FilterGroup, Property, ViewSort};
use super::{QueryContext, query_rows};

struct Fixture {
    properties: Vec<Property>,
    rows: Vec<DatabaseRow>,
    ctx: QueryContext,
    cases: Vec<Value>,
}

fn load(name: &str) -> Fixture {
    let path = format!(
        "{}/../../packages/database/cases/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let file: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let text = |value: &Value| value.as_str().unwrap().to_owned();
    let id = |value: &Value| Id::from_str(value.as_str().unwrap()).unwrap();
    let millis = |value: &Value| {
        TimestampMillis::from_millis(
            Timestamp::from_str(value.as_str().unwrap())
                .unwrap()
                .as_millisecond(),
        )
    };
    let properties = file["properties"]
        .as_array()
        .unwrap()
        .iter()
        .map(|property| {
            Property::from_stored(
                property["id"].as_str().unwrap(),
                text(&property["name"]),
                property["type"].as_str().unwrap(),
                &property["config"].to_string(),
                property["position"].as_i64().unwrap(),
            )
            .unwrap_or_else(|_| panic!("property {property}"))
        })
        .collect();
    let rows = file["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| DatabaseRow {
            id: id(&row["id"]),
            title: text(&row["title"]),
            icon: row["icon"].as_str().map(str::to_owned),
            position: row["position"].as_i64().unwrap(),
            version: row["version"].as_u64().unwrap(),
            created_at: millis(&row["created_at"]),
            created_by: id(&row["created_by"]),
            updated_at: millis(&row["updated_at"]),
            updated_by: id(&row["updated_by"]),
            has_content: row["has_content"].as_bool().unwrap(),
            values: serde_json::from_value(row["values"].clone()).unwrap(),
            hidden_links: BTreeMap::new(),
            preview: None,
        })
        .collect();
    let ctx = &file["ctx"];
    let ctx = QueryContext {
        user_id: id(&ctx["user_id"]),
        now: Timestamp::from_str(ctx["now"].as_str().unwrap()).unwrap(),
        time_zone: TimeZone::get(ctx["time_zone"].as_str().unwrap()).unwrap(),
        members: ctx["members"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(member, name)| (Id::from_str(member).unwrap(), text(name)))
            .collect(),
        related: file["related_pages"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(page, related)| (page.clone(), text(&related["title"])))
            .collect::<HashMap<_, _>>(),
    };
    Fixture {
        properties,
        rows,
        ctx,
        cases: file["cases"].as_array().unwrap().clone(),
    }
}

/// Every case whose rows come out other than `expected_ids`, as `name: got [...]`.
fn failures(
    fixture: &Fixture,
    query: impl Fn(&Value) -> (FilterGroup, Vec<ViewSort>),
) -> Vec<String> {
    fixture
        .cases
        .iter()
        .filter_map(|case| {
            let (filter, sorts) = query(case);
            let got: Vec<String> = query_rows(
                fixture.rows.clone(),
                &fixture.properties,
                &filter,
                &sorts,
                &fixture.ctx,
            )
            .iter()
            .map(|row| row.id.to_string())
            .collect();
            let expected: Vec<String> =
                serde_json::from_value(case["expected_ids"].clone()).unwrap();
            (got != expected).then(|| format!("{}: got {got:?}", case["name"]))
        })
        .collect()
}

#[test]
fn the_filter_cases_pass() {
    let fixture = load("filter");
    assert!(fixture.cases.len() > 40, "the cases load");
    let failed = failures(&fixture, |case| {
        (
            serde_json::from_value(case["filter"].clone()).unwrap(),
            Vec::new(),
        )
    });
    assert!(failed.is_empty(), "{failed:#?}");
}

#[test]
fn the_sort_cases_pass() {
    let fixture = load("sort");
    assert!(fixture.cases.len() > 10, "the cases load");
    let failed = failures(&fixture, |case| {
        (
            FilterGroup::default(),
            serde_json::from_value(case["sorts"].clone()).unwrap(),
        )
    });
    assert!(failed.is_empty(), "{failed:#?}");
}
