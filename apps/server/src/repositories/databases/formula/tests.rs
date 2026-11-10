use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

use jiff::Timestamp;
use jiff::tz::TimeZone;
use orbit_platform::{Id, TimestampMillis};
use serde_json::{Value, json};

use super::super::{DatabaseError, DatabaseRow, Property, PropertyKind, PropertyType};
use super::eval::{Context, Machine, TOO_SLOW};
use super::{FillScope, FormulaProblem, Member, checked, fill, preview, retyped, syntax};

const OWNER: &str = "0190a0a0-0000-7000-8000-000000000001";

fn kind(property_type: &str, config: Option<Value>) -> PropertyKind {
    let property_type: PropertyType = serde_json::from_value(json!(property_type)).unwrap();
    PropertyKind::parse(property_type, config.as_ref()).unwrap()
}

fn property(name: &str, kind: PropertyKind, position: i64) -> Property {
    Property {
        id: Id::new_v7(),
        name: name.to_owned(),
        kind,
        position,
    }
}

fn formula(name: &str, expression: &str) -> Property {
    property(
        name,
        kind("formula", Some(json!({ "expression": expression }))),
        100,
    )
}

/// The properties the cases read, by name.
fn fixture_properties() -> Vec<Property> {
    let options = |pairs: &[(&str, &str)]| json!({ "options": pairs.iter().map(|(id, name)| json!({"id": id, "name": name, "color": "blue"})).collect::<Vec<_>>() });
    [
        ("Name", kind("title", None)),
        ("Price", kind("number", None)),
        ("Qty", kind("number", None)),
        ("Empty", kind("number", None)),
        ("Note", kind("text", None)),
        ("Done", kind("checkbox", None)),
        ("Due", kind("date", None)),
        ("Meeting", kind("date", None)),
        (
            "Tags",
            kind(
                "multi_select",
                Some(options(&[("t-a", "alpha"), ("t-b", "beta")])),
            ),
        ),
        ("Stage", kind("select", Some(options(&[("s-1", "Open")])))),
        ("Owner", kind("person", None)),
    ]
    .into_iter()
    .enumerate()
    .map(|(at, (name, kind))| property(name, kind, i64::try_from(at).unwrap()))
    .collect()
}

fn row(properties: &[Property], values: &Value) -> DatabaseRow {
    let owner: Id = OWNER.parse().unwrap();
    let mut stored = BTreeMap::new();
    let mut title = String::new();
    for (name, value) in values.as_object().unwrap() {
        let property = properties.iter().find(|p| p.name == *name).unwrap();
        let value = if value == "OWNER" {
            json!([OWNER])
        } else {
            value.clone()
        };
        if property.kind == PropertyKind::Title {
            title = value.as_str().unwrap().to_owned();
        } else {
            stored.insert(property.id.to_string(), value);
        }
    }
    DatabaseRow {
        id: Id::new_v7(),
        title,
        icon: None,
        position: 0,
        version: 0,
        created_at: TimestampMillis::from_millis(0),
        created_by: owner,
        updated_at: TimestampMillis::from_millis(0),
        updated_by: owner,
        has_content: false,
        values: stored,
        hidden_links: BTreeMap::new(),
        preview: None,
    }
}

fn people() -> HashMap<Id, Member> {
    HashMap::from([(
        OWNER.parse().unwrap(),
        Member {
            name: "Owner Person".to_owned(),
            email: "owner@example.com".to_owned(),
        },
    )])
}

/// Fills the formulas of `rows` in `tz`.
fn fill_in(properties: &[Property], rows: &mut [DatabaseRow], tz: &str) {
    let people = people();
    let pages = BTreeMap::new();
    fill(
        properties,
        rows,
        &FillScope {
            tz,
            people: &people,
            pages: &pages,
        },
    );
}

/// `others` plus `candidate` checked as a client sends it: the stored property or the 422 body.
fn save(others: &[Property], candidate: Property) -> Result<Property, FormulaProblem> {
    match checked(others, candidate) {
        Ok(property) => Ok(property),
        Err(DatabaseError::FormulaInvalid(problem)) => Err(problem),
        Err(other) => panic!("unexpected error {other:?}"),
    }
}

fn result_type(property: &Property) -> String {
    match &property.kind {
        PropertyKind::Formula(config) => config.result_type.name(),
        other => panic!("not a formula: {other:?}"),
    }
}

fn cell(row: &DatabaseRow, key: &str) -> Value {
    row.values.get(key).cloned().unwrap_or(Value::Null)
}

/// JSON equality with numbers compared as `f64` (results are `f64`, the fixture writes `7`).
fn same(got: &Value, expected: &Value) -> bool {
    match (got, expected) {
        (Value::Number(a), Value::Number(b)) => {
            (a.as_f64().unwrap() - b.as_f64().unwrap()).abs() < 1e-9
        }
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same(a, b))
        }
        _ => got == expected,
    }
}

#[test]
fn formula_cases_give_their_type_value_or_error() {
    let cases: Value = serde_json::from_str(include_str!("formula_cases.json")).unwrap();
    let properties = fixture_properties();
    let base = row(&properties, &cases["row"]);
    let mut failures = Vec::new();
    for case in cases["cases"].as_array().unwrap() {
        let source = case["expression"].as_str().unwrap();
        let saved = save(&properties, formula("Result", source));
        let saved = match (saved, case.get("invalid")) {
            (Err(problem), Some(expected)) => {
                let got =
                    json!({"message": problem.message, "start": problem.start, "end": problem.end});
                if got != *expected {
                    failures.push(format!(
                        "{source}\n  expected 422 {expected}\n  got      {got}"
                    ));
                }
                continue;
            }
            (Err(problem), None) => {
                failures.push(format!("{source}\n  unexpected 422 {problem:?}"));
                continue;
            }
            (Ok(saved), Some(expected)) => {
                failures.push(format!(
                    "{source}\n  expected 422 {expected}\n  saved as {}",
                    result_type(&saved)
                ));
                continue;
            }
            (Ok(saved), None) => saved,
        };
        if let Some(expected) = case.get("type")
            && result_type(&saved) != *expected
        {
            failures.push(format!(
                "{source}\n  expected type {expected}\n  got           {}",
                result_type(&saved)
            ));
        }
        let mut all = properties.clone();
        let key = saved.id.to_string();
        all.push(saved);
        let mut rows = [base.clone()];
        fill_in(&all, &mut rows, "UTC");
        let got = cell(&rows[0], &key);
        let expected = match (case.get("value"), case.get("error")) {
            (_, Some(message)) => json!({ "error": message }),
            (Some(value), None) => serde_json::from_str(
                &value
                    .to_string()
                    .replace("\"OWNER\"", &format!("\"{OWNER}\"")),
            )
            .unwrap(),
            (None, None) => panic!("case without a value: {source}"),
        };
        if !same(&got, &expected) {
            failures.push(format!("{source}\n  expected {expected}\n  got      {got}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} cases failed:\n{}",
        failures.len(),
        cases["cases"].as_array().unwrap().len(),
        failures.join("\n")
    );
}

#[test]
fn a_saved_formula_stores_property_ids_and_names_still_resolve() {
    let properties = fixture_properties();
    let price = properties[1].id;
    let saved = save(
        &properties,
        formula("Total", "prop(\"Price\") * prop(\"Qty\")"),
    )
    .unwrap();
    let PropertyKind::Formula(config) = &saved.kind else {
        unreachable!()
    };
    assert_eq!(
        config.expression,
        format!("prop(\"{price}\") * prop(\"{}\")", properties[2].id)
    );
    assert_eq!(config.to_json()["result_type"], "number");
    // Saving the stored form again keeps it.
    let again = save(
        &properties,
        Property {
            kind: saved.kind.clone(),
            ..saved.clone()
        },
    )
    .unwrap();
    assert_eq!(again.kind, saved.kind);
}

#[test]
fn the_syntax_limits_name_the_limit() {
    let properties = fixture_properties();
    let long = format!("\"{}\"", "x".repeat(1_999));
    let problem = save(&properties, formula("Long", &long)).unwrap_err();
    assert_eq!(
        (problem.message.as_str(), problem.start, problem.end),
        ("A formula has at most 2000 characters.", 2_000, 2_001)
    );
    assert!(
        save(
            &properties,
            formula("Fits", &format!("\"{}\"", "x".repeat(1_998)))
        )
        .is_ok()
    );

    // 32 levels of parentheses parse; 33 do not.
    let nested = |depth: usize| format!("{}1{}", "(".repeat(depth), ")".repeat(depth));
    let deepest_ok = (1..=40)
        .take_while(|depth| syntax::parse(&nested(*depth)).is_ok())
        .last()
        .unwrap();
    let problem = save(&properties, formula("Deep", &nested(deepest_ok + 1))).unwrap_err();
    assert_eq!(
        problem.message,
        "This formula is nested too deeply (at most 32 levels)."
    );
    assert!(deepest_ok >= 30, "only {deepest_ok} levels parse");

    // A list of 499 numbers has 500 nodes; one more is too many.
    let list = |items: usize| format!("[{}].length()", vec!["1"; items].join(","));
    let problem = save(&properties, formula("Wide", &list(500))).unwrap_err();
    assert_eq!(
        problem.message,
        "This formula is too long (at most 500 parts)."
    );
    let fits = save(&properties, formula("Wide", &list(498))).unwrap();
    let mut rows = [row(&properties, &json!({}))];
    let mut all = properties.clone();
    let key = fits.id.to_string();
    all.push(fits);
    fill_in(&all, &mut rows, "UTC");
    assert_eq!(rows[0].values[&key], json!(498.0));
}

#[test]
fn a_cycle_names_the_formula_it_runs_through() {
    let mut properties = fixture_properties();
    let a = formula("A", "1");
    let b = formula("B", &format!("prop(\"{}\") + 1", a.id));
    properties.push(a.clone());
    properties.push(b.clone());
    let others: Vec<Property> = properties
        .iter()
        .filter(|p| p.id != a.id)
        .cloned()
        .collect();
    let problem = save(
        &others,
        Property {
            kind: kind("formula", Some(json!({ "expression": "prop(\"B\") * 2" }))),
            ..a.clone()
        },
    )
    .unwrap_err();
    assert_eq!(
        (problem.message.as_str(), problem.start, problem.end),
        ("This formula uses itself through \"B\".", 0, 13)
    );
    let problem = save(
        &properties
            .iter()
            .filter(|p| p.id != a.id)
            .cloned()
            .collect::<Vec<_>>(),
        Property {
            kind: kind("formula", Some(json!({ "expression": "prop(\"A\")" }))),
            ..a
        },
    )
    .unwrap_err();
    assert_eq!(problem.message, "This formula uses itself through \"A\".");
}

#[test]
fn formulas_read_formulas_in_order_and_errors_pass_down_the_chain() {
    let mut properties = fixture_properties();
    // Declared out of order: C reads B reads A.
    let a_id = Id::new_v7();
    let b_id = Id::new_v7();
    let c = formula("C", &format!("prop(\"{b_id}\") * 10"));
    let b = Property {
        id: b_id,
        ..formula("B", &format!("prop(\"{a_id}\") + 1"))
    };
    let a = Property {
        id: a_id,
        ..formula("A", "prop(\"Qty\") / prop(\"Empty\")")
    };
    properties.extend([c.clone(), b.clone(), a.clone()]);
    for at in properties.len() - 3..properties.len() {
        let others: Vec<Property> = properties
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != at)
            .map(|(_, p)| p.clone())
            .collect();
        properties[at] = save(&others, properties[at].clone()).unwrap();
    }
    for (at, config) in retyped(&properties) {
        properties[at].kind = PropertyKind::Formula(config);
    }
    let mut rows = [
        row(&properties, &json!({"Qty": 4, "Empty": 2})),
        row(&properties, &json!({"Qty": 4, "Empty": 0})),
    ];
    fill_in(&properties, &mut rows, "UTC");
    assert_eq!(
        [&a, &b, &c].map(|p| cell(&rows[0], &p.id.to_string())),
        [json!(2.0), json!(3.0), json!(30.0)]
    );
    assert_eq!(
        [&a, &b, &c].map(|p| cell(&rows[1], &p.id.to_string())),
        [
            json!({"error": "Division by zero."}),
            json!({"error": "\"A\" has an error."}),
            json!({"error": "\"B\" has an error."}),
        ]
    );
}

#[test]
fn a_chain_of_more_than_15_formulas_fails_at_the_16th() {
    let mut properties = fixture_properties();
    let mut previous: Option<Id> = None;
    let mut chain = Vec::new();
    for layer in 1..=16 {
        let expression =
            previous.map_or_else(|| "1".to_owned(), |id| format!("prop(\"{id}\") + 1"));
        let property = formula(&format!("L{layer}"), &expression);
        previous = Some(property.id);
        chain.push(property.id);
        properties.push(property);
    }
    for (at, config) in retyped(&properties) {
        properties[at].kind = PropertyKind::Formula(config);
    }
    let mut rows = [row(&properties, &json!({}))];
    fill_in(&properties, &mut rows, "UTC");
    assert_eq!(cell(&rows[0], &chain[14].to_string()), json!(15.0));
    assert_eq!(
        cell(&rows[0], &chain[15].to_string()),
        json!({"error": "This formula reads more than 15 formulas in a chain."})
    );
}

#[test]
fn a_database_holds_at_most_20_formulas() {
    let mut properties = fixture_properties();
    for at in 0..20 {
        properties.push(formula(&format!("F{at}"), "1"));
    }
    let problem = save(&properties, formula("F20", "1")).unwrap_err();
    assert_eq!(problem.message, "A database holds at most 20 formulas.");
}

#[test]
fn a_deleted_property_turns_cells_into_an_error_and_retypes_the_formula() {
    let mut properties = fixture_properties();
    let total = save(&properties, formula("Total", "prop(\"Price\") * 2")).unwrap();
    let label = save(&properties, formula("Label", "prop(\"Note\")")).unwrap();
    properties.push(total.clone());
    properties.push(label.clone());
    properties.retain(|p| p.name != "Price");
    let mut rows = [row(&properties, &json!({"Note": "kept"}))];
    fill_in(&properties, &mut rows, "UTC");
    assert_eq!(
        cell(&rows[0], &total.id.to_string()),
        json!({"error": "A property was deleted."})
    );
    assert_eq!(cell(&rows[0], &label.id.to_string()), json!("kept"));
    // The stored expression keeps the dangling id; saving it again names the deletion.
    let problem = save(
        &properties
            .iter()
            .filter(|p| p.id != total.id)
            .cloned()
            .collect::<Vec<_>>(),
        total.clone(),
    )
    .unwrap_err();
    assert_eq!(
        (problem.message.as_str(), problem.start),
        ("A property was deleted.", 5)
    );

    // A type change of a read property retypes the formula.
    let mut properties = fixture_properties();
    let note = save(&properties, formula("Echo", "prop(\"Qty\")")).unwrap();
    properties.push(note.clone());
    let qty = properties.iter_mut().find(|p| p.name == "Qty").unwrap();
    qty.kind = kind("text", None);
    let retyped = retyped(&properties);
    assert_eq!(retyped.len(), 1);
    assert_eq!(retyped[0].1.result_type.name(), "text");
}

#[test]
fn the_step_limit_is_the_same_on_every_run() {
    let properties = fixture_properties();
    let source = "map(split(repeat(\"x\", 999)), map(split(\"abc\"), current))";
    let saved = save(&properties, formula("Heavy", source)).unwrap();
    let key = saved.id.to_string();
    let mut all = properties.clone();
    all.push(saved);
    let mut rows: Vec<DatabaseRow> = (0..5).map(|_| row(&properties, &json!({}))).collect();
    fill_in(&all, &mut rows, "UTC");
    for row in &rows {
        assert_eq!(
            cell(row, &key),
            json!({"error": "This formula needs more than 2,000 steps for this row."})
        );
    }
}

#[test]
fn a_read_past_its_time_budget_shows_the_too_slow_error() {
    let properties = fixture_properties();
    let saved = save(
        &properties,
        formula("Slow", "map(split(repeat(\"x\", 300)), current).length()"),
    )
    .unwrap();
    let mut all = properties.clone();
    all.push(saved);
    let compiled = super::compile_all(&all);
    let (ir, _) = compiled.last().unwrap().program.as_ref().unwrap();
    let people = people();
    let names: Vec<String> = all.iter().map(|p| p.name.clone()).collect();
    let inputs: Vec<Result<super::value::Val, ()>> =
        all.iter().map(|_| Ok(super::value::Val::Empty)).collect();
    let mut context = Context {
        tz: TimeZone::UTC,
        now: Timestamp::now(),
        people: &people,
        deadline: Instant::now() + Duration::from_secs(60),
        names: &names,
    };
    let id = Id::new_v7();
    assert_eq!(
        Machine::new(&context, &inputs, id).run(ir),
        Ok(super::value::Val::Number(300.0))
    );
    context.deadline = Instant::now()
        .checked_sub(Duration::from_millis(1))
        .unwrap();
    assert_eq!(
        Machine::new(&context, &inputs, id).run(ir),
        Err(TOO_SLOW.to_owned())
    );
}

#[test]
fn today_follows_the_readers_time_zone() {
    let properties = fixture_properties();
    let saved = save(&properties, formula("Today", "today()")).unwrap();
    let key = saved.id.to_string();
    let mut all = properties.clone();
    all.push(saved);
    for zone in ["Pacific/Kiritimati", "Pacific/Pago_Pago", "UTC"] {
        let mut rows = [row(&properties, &json!({}))];
        fill_in(&all, &mut rows, zone);
        let expected = Timestamp::now()
            .to_zoned(TimeZone::get(zone).unwrap())
            .date()
            .to_string();
        assert_eq!(
            cell(&rows[0], &key),
            json!({"start": expected, "end": null}),
            "{zone}"
        );
    }
    // An unknown zone reads as UTC.
    let mut rows = [row(&properties, &json!({}))];
    fill_in(&all, &mut rows, "Mars/Olympus");
    let utc = Timestamp::now().to_zoned(TimeZone::UTC).date().to_string();
    assert_eq!(cell(&rows[0], &key), json!({"start": utc, "end": null}));
}

#[test]
fn a_moment_reads_in_the_readers_time_zone() {
    let properties = fixture_properties();
    let saved = save(
        &properties,
        formula(
            "Local",
            "formatDate(prop(\"Meeting\"), \"YYYY-MM-DD HH:mm\")",
        ),
    )
    .unwrap();
    let key = saved.id.to_string();
    let mut all = properties.clone();
    all.push(saved);
    let mut rows = [row(
        &properties,
        &json!({"Meeting": {"start": "2026-03-28T23:30:00Z", "end": null}}),
    )];
    fill_in(&all, &mut rows, "Europe/Budapest");
    assert_eq!(cell(&rows[0], &key), json!("2026-03-29 00:30"));
}

#[test]
fn a_preview_returns_the_type_or_the_error_and_writes_samples() {
    let properties = fixture_properties();
    let people = people();
    let pages = BTreeMap::new();
    let scope = FillScope {
        tz: "UTC",
        people: &people,
        pages: &pages,
    };
    let mut rows = vec![
        row(&properties, &json!({"Price": 2, "Qty": 3})),
        row(&properties, &json!({"Price": 5})),
    ];
    let result = preview(
        &properties,
        None,
        "prop(\"Price\") * prop(\"Qty\")",
        &mut rows,
        &scope,
    );
    let result = serde_json::to_value(&result).unwrap();
    assert_eq!(
        result,
        json!({
            "result_type": "number",
            "samples": [
                {"row_id": rows[0].id.to_string(), "value": 6.0},
                {"row_id": rows[1].id.to_string()},
            ],
        })
    );
    let result = preview(&properties, None, "prop(\"Price\") +", &mut rows, &scope);
    assert_eq!(
        serde_json::to_value(&result).unwrap(),
        json!({
            "error": {"message": "Expected a value, found the end of the formula.", "start": 15, "end": 15},
            "samples": [],
        })
    );
}

/// The evaluator part of the spec 15.9 cost check, without the database read and the JSON:
/// `cargo test --release -p orbit-server --lib formula::tests::fill_cost -- --ignored --nocapture`
#[test]
#[ignore = "a benchmark; run it with --release --ignored --nocapture"]
fn fill_cost_of_20_formulas_at_the_node_limit() {
    let items: Vec<String> = (1..=100).map(|i| i.to_string()).collect();
    let variants = [
        (
            "map body only",
            format!(
                "sum(map([{}], current * prop(\"Price\") + index % 7 - prop(\"Qty\") / 2))",
                items.join(",")
            ),
        ),
        (
            "map identity",
            format!("sum(map([{}], current))", items.join(",")),
        ),
        ("list literal only", format!("sum([{}])", items.join(","))),
        (
            "typical",
            "if(prop(\"Qty\") > 0, prop(\"Price\") * prop(\"Qty\"), 0)".to_owned(),
        ),
    ];
    let properties = fixture_properties();
    let full = (0..=400)
        .rev()
        .map(|pad| {
            let padding: Vec<String> = (0..pad).map(|i| (i % 10).to_string()).collect();
            format!(
                "sum(map([{}], current * prop(\"Price\") + index % 7 - prop(\"Qty\") / 2)) + sum([{}]) + 1",
                items.join(","),
                padding.join(",")
            )
        })
        .find(|source| save(&properties, formula("F", source)).is_ok())
        .unwrap();
    let rows_per_run = 1_000;
    for (label, source) in std::iter::once(("at the node limit", full)).chain(variants) {
        let mut all = properties.clone();
        let mut keys = Vec::new();
        for n in 0..20 {
            let saved = save(&all, formula(&format!("F{n}"), &source)).unwrap();
            keys.push(saved.id.to_string());
            all.push(saved);
        }
        let base: Vec<DatabaseRow> = (0..rows_per_run)
            .map(|at| row(&properties, &json!({"Price": at % 97, "Qty": at % 13})))
            .collect();
        let mut times = Vec::new();
        for _ in 0..7 {
            let mut rows = base.clone();
            let started = Instant::now();
            fill_in(&all, &mut rows, "UTC");
            times.push(started.elapsed().as_secs_f64() * 1_000.0);
            let failed = rows
                .iter()
                .flat_map(|row| keys.iter().map(|key| &row.values[key]))
                .filter(|value| !value.is_number())
                .count();
            assert_eq!(failed, 0, "{label}: every cell computed");
        }
        times.sort_by(f64::total_cmp);
        let cells = f64::from(rows_per_run * 20);
        println!(
            "fill {label}: {rows_per_run} rows x 20 formulas, median {:.1} ms (range {:.1}..{:.1}), {:.2} us per cell, {:.0} ms for 5,000 rows",
            times[3],
            times[0],
            times[6],
            times[3] * 1_000.0 / cells,
            times[3] * 5.0
        );
    }
}
