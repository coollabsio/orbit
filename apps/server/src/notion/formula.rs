//! Notion formulas as live Orbit formulas (spec P2/P3 15.10). Notion's API returns a formula's
//! expression with each property as `{{notion:block_property:<property id>:…}}` (older
//! responses use `prop("Name")`). An expression that parses and type checks with the Orbit
//! parser stays a live formula; any other keeps the static values of its result type.

use orbit_platform::Id;
use serde_json::Value;

use super::database::{DatabasePlan, Source, property_id};
use crate::repositories::databases::{
    FormulaConfig, Property, PropertyKind, PropertyType, check_formula,
};

const TOKEN_START: &str = "{{notion:block_property:";

/// Turns each planned formula property of `plan` whose expression Orbit can run into a formula.
pub fn keep_live(database: Id, source: &Source, plan: &mut DatabasePlan) {
    let mut properties: Vec<Property> = plan
        .properties
        .iter()
        .zip(0..)
        .map(|(planned, position)| Property {
            id: planned.id,
            name: planned.name.clone(),
            kind: PropertyKind::parse(planned.property_type, Some(&planned.config))
                .unwrap_or_else(|_| PropertyKind::default_for(planned.property_type)),
            position,
        })
        .collect();
    for (name, schema) in &source.data_source.properties {
        if schema.get("type").and_then(Value::as_str) != Some("formula") {
            continue;
        }
        let notion_id = schema.get("id").and_then(Value::as_str).unwrap_or(name);
        let id = property_id(database, notion_id);
        let Some(at) = properties.iter().position(|property| property.id == id) else {
            continue;
        };
        let Some(expression) = schema
            .pointer("/formula/expression")
            .and_then(Value::as_str)
            .and_then(|expression| orbit_expression(database, expression))
        else {
            continue;
        };
        let candidate = Property {
            kind: PropertyKind::Formula(FormulaConfig {
                expression,
                ..FormulaConfig::default()
            }),
            ..properties[at].clone()
        };
        let others: Vec<Property> = properties
            .iter()
            .filter(|property| property.id != id)
            .cloned()
            .collect();
        let Ok(live) = check_formula(&others, candidate) else {
            continue;
        };
        plan.properties[at].property_type = PropertyType::Formula;
        plan.properties[at].config = live.kind.config_json();
        properties[at] = live;
        if let Some(count) = plan.report.lossy.get_mut("formula_values") {
            *count -= 1;
            if *count == 0 {
                plan.report.lossy.remove("formula_values");
            }
        }
    }
}

/// The expression with each Notion property token as `prop("<Orbit property id>")`; `None` for
/// a token that does not close.
fn orbit_expression(database: Id, expression: &str) -> Option<String> {
    let mut out = String::with_capacity(expression.len());
    let mut rest = expression;
    while let Some(start) = rest.find(TOKEN_START) {
        out.push_str(&rest[..start]);
        let token = &rest[start + TOKEN_START.len()..];
        let end = token.find("}}")?;
        let notion_id = token[..end].split(':').next()?;
        out.push_str(&format!("prop(\"{}\")", property_id(database, notion_id)));
        rest = &token[end + 2..];
    }
    out.push_str(rest);
    Some(out)
}
