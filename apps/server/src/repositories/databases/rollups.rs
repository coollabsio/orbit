//! Rollup properties (spec P3 11.5): a value computed on read from one property of the pages a
//! relation of the same row links to. Only related pages the reader can read count (the relation
//! pass already left the others out).
//!
//! Result shapes in `values`: a number for the count, percent (0 to 1), number and checkbox
//! functions; a date value `{start, end}` for the date functions; a list of display texts for
//! `show_original` and `show_unique`. An empty result is left out, as an empty stored value.

use std::collections::{HashMap, HashSet};

use orbit_platform::Row;
use orbit_platform::{Id, TimestampMillis};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Any, Transaction};

use super::computed::{ComputeScope, Computed, fill_nested};
use super::{
    CellValue, DatabaseError, DatabaseRow, Property, PropertyKind, display_text, parse_value,
    properties_in_tx, rows_in_tx,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RollupFunction {
    ShowOriginal,
    ShowUnique,
    CountAll,
    CountValues,
    CountUnique,
    CountEmpty,
    CountNotEmpty,
    PercentEmpty,
    PercentNotEmpty,
    Sum,
    Average,
    Median,
    Min,
    Max,
    Range,
    EarliestDate,
    LatestDate,
    DateRange,
    Checked,
    Unchecked,
    PercentChecked,
    PercentUnchecked,
}

/// What a property's values are, for the functions that need numbers, dates or checkboxes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Class {
    Number,
    Date,
    Checkbox,
    Other,
    /// A formula: its result type is only known when it runs.
    Any,
}

impl RollupFunction {
    fn fits(self, class: Class) -> bool {
        let needs = match self {
            Self::Sum | Self::Average | Self::Median | Self::Min | Self::Max | Self::Range => {
                Class::Number
            }
            Self::EarliestDate | Self::LatestDate | Self::DateRange => Class::Date,
            Self::Checked | Self::Unchecked | Self::PercentChecked | Self::PercentUnchecked => {
                Class::Checkbox
            }
            _ => return true,
        };
        class == needs || class == Class::Any
    }

    /// The class of this function's result.
    fn result(self) -> Class {
        match self {
            Self::ShowOriginal | Self::ShowUnique => Class::Other,
            Self::EarliestDate | Self::LatestDate | Self::DateRange => Class::Date,
            _ => Class::Number,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RollupConfig {
    /// A relation property of the same database.
    pub relation_property_id: Id,
    /// A property of the relation's target database.
    pub target_property_id: Id,
    pub function: RollupFunction,
}

fn class_of(kind: &PropertyKind) -> Class {
    match kind {
        PropertyKind::Number(_) | PropertyKind::UniqueId(_) => Class::Number,
        PropertyKind::Date(_) | PropertyKind::CreatedTime | PropertyKind::LastEditedTime => {
            Class::Date
        }
        PropertyKind::Checkbox => Class::Checkbox,
        PropertyKind::Rollup(config) => config.function.result(),
        other if other.property_type().as_str() == "formula" => Class::Any,
        _ => Class::Other,
    }
}

/// Checks a rollup config of a property of `database_id`: the relation is a relation property of
/// the database, the target property exists in its target and fits the function, and a rollup
/// over a rollup goes one level deep only (spec 11.5).
pub(super) async fn check(
    tx: &mut Transaction<'_, Any>,
    database_id: Id,
    properties: &[Property],
    config: &RollupConfig,
) -> Result<(), DatabaseError> {
    let Some(PropertyKind::Relation(relation)) = properties
        .iter()
        .find(|property| property.id == config.relation_property_id)
        .map(|property| &property.kind)
    else {
        return Err(DatabaseError::invalid("config.relation_property_id"));
    };
    let targets = if relation.database_id == database_id {
        properties.to_vec()
    } else {
        properties_in_tx(tx, relation.database_id).await?
    };
    let target = targets
        .iter()
        .find(|property| property.id == config.target_property_id)
        .ok_or_else(|| DatabaseError::invalid("config.target_property_id"))?;
    if !config.function.fits(class_of(&target.kind)) {
        return Err(DatabaseError::invalid("config.function"));
    }
    if let PropertyKind::Rollup(inner) = &target.kind {
        let Some(PropertyKind::Relation(inner_relation)) = targets
            .iter()
            .find(|property| property.id == inner.relation_property_id)
            .map(|property| &property.kind)
        else {
            return Ok(());
        };
        let deeper = properties_in_tx(tx, inner_relation.database_id).await?;
        if deeper.iter().any(|property| {
            property.id == inner.target_property_id
                && matches!(property.kind, PropertyKind::Rollup(_))
        }) {
            return Err(DatabaseError::invalid("config.target_property_id"));
        }
    }
    Ok(())
}

/// One value of a related page, as the functions see it.
#[derive(Clone, Debug)]
enum Item {
    Text(String),
    Number(f64),
    /// ISO start and end, as stored.
    Date(String, Option<String>),
    Checked,
}

impl Item {
    fn text(&self) -> String {
        match self {
            Self::Text(text) => text.clone(),
            Self::Number(number) => number_text(*number),
            Self::Date(start, Some(end)) => format!("{start}/{end}"),
            Self::Date(start, None) => start.clone(),
            Self::Checked => "true".to_owned(),
        }
    }
}

fn number_text(number: f64) -> String {
    if number.fract() == 0.0 && number.abs() < 1e15 {
        format!("{number:.0}")
    } else {
        number.to_string()
    }
}

/// The target database of a rollup: its properties and rows (with their computed values).
struct Target {
    properties: Vec<Property>,
    rows: HashMap<String, DatabaseRow>,
    computed: Computed,
}

/// Puts the rollup values of `rows` into their `values`. The relation values are already there.
/// Target rows are loaded once per target database and computed one level deeper.
pub(super) async fn fill(
    tx: &mut Transaction<'_, Any>,
    scope: &ComputeScope,
    properties: &[Property],
    rows: &mut [DatabaseRow],
    depth: u8,
) -> Result<(), DatabaseError> {
    let mut targets: HashMap<Id, Target> = HashMap::new();
    let mut members: Option<HashMap<String, String>> = None;
    for property in properties {
        let PropertyKind::Rollup(config) = &property.kind else {
            continue;
        };
        let Some(PropertyKind::Relation(relation)) = properties
            .iter()
            .find(|candidate| candidate.id == config.relation_property_id)
            .map(|candidate| &candidate.kind)
        else {
            continue;
        };
        if !targets.contains_key(&relation.database_id) {
            let target_properties = properties_in_tx(tx, relation.database_id).await?;
            let mut target_rows = rows_in_tx(tx, relation.database_id, None).await?;
            let computed = fill_nested(
                tx,
                scope,
                relation.database_id,
                &target_properties,
                &mut target_rows,
                depth,
            )
            .await?;
            targets.insert(
                relation.database_id,
                Target {
                    properties: target_properties,
                    rows: target_rows
                        .into_iter()
                        .map(|row| (row.id.to_string(), row))
                        .collect(),
                    computed,
                },
            );
        }
        let target = &targets[&relation.database_id];
        let Some(target_property) = target
            .properties
            .iter()
            .find(|candidate| candidate.id == config.target_property_id)
        else {
            continue;
        };
        if members.is_none() && people_values(&target_property.kind) {
            members = Some(member_names(tx, scope.workspace_id).await?);
        }
        let names = members.as_ref();
        let relation_key = config.relation_property_id.to_string();
        let key = property.id.to_string();
        for row in rows.iter_mut() {
            let pages: Vec<Vec<Item>> = row
                .values
                .get(&relation_key)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .filter_map(|id| target.rows.get(id))
                .map(|page| items(target_property, page, &target.computed, names))
                .collect();
            match compute(config.function, &pages) {
                Some(value) => row.values.insert(key.clone(), value),
                None => row.values.remove(&key),
            };
        }
    }
    Ok(())
}

fn people_values(kind: &PropertyKind) -> bool {
    matches!(
        kind,
        PropertyKind::Person | PropertyKind::CreatedBy | PropertyKind::LastEditedBy
    )
}

async fn member_names(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
) -> Result<HashMap<String, String>, DatabaseError> {
    Ok(sqlx::query(
        "SELECT users.id AS id, users.display_name AS name FROM memberships \
         JOIN users ON users.id = memberships.user_id WHERE memberships.workspace_id = $1",
    )
    .bind(workspace_id.to_string())
    .fetch_all(&mut **tx)
    .await?
    .into_iter()
    .map(|row| (row.get("id"), row.get("name")))
    .collect())
}

fn moment(time: TimestampMillis) -> Item {
    Item::Date(time.to_string(), None)
}

/// The values of `property` on one related page.
fn items(
    property: &Property,
    page: &DatabaseRow,
    computed: &Computed,
    members: Option<&HashMap<String, String>>,
) -> Vec<Item> {
    let name = |id: &Id| {
        members
            .and_then(|names| names.get(&id.to_string()))
            .cloned()
            .unwrap_or_default()
    };
    let text = |text: String| (!text.is_empty()).then_some(Item::Text(text));
    let raw = page.values.get(&property.id.to_string());
    match &property.kind {
        PropertyKind::Title => text(page.title.clone()).into_iter().collect(),
        PropertyKind::CreatedTime => vec![moment(page.created_at)],
        PropertyKind::LastEditedTime => vec![moment(page.updated_at)],
        PropertyKind::CreatedBy => text(name(&page.created_by)).into_iter().collect(),
        PropertyKind::LastEditedBy => text(name(&page.updated_by)).into_iter().collect(),
        PropertyKind::Relation(_) => raw
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter_map(|id| computed.related_pages.get(id))
            .filter_map(|related| text(related.title.clone()))
            .collect(),
        kind => {
            let Some(raw) = raw else {
                return Vec::new();
            };
            match parse_value(kind, raw) {
                Ok(Some(CellValue::Text(value) | CellValue::Url(value))) => {
                    text(value).into_iter().collect()
                }
                Ok(Some(CellValue::Number(number))) => vec![Item::Number(number)],
                Ok(Some(CellValue::Choices(ids))) => ids
                    .iter()
                    .filter_map(|id| text(display_text(kind, &CellValue::Choice(id.clone()))))
                    .collect(),
                Ok(Some(CellValue::People(ids))) => {
                    ids.iter().filter_map(|id| text(name(id))).collect()
                }
                Ok(Some(CellValue::Date(date))) => vec![Item::Date(
                    date.start.as_str().to_owned(),
                    date.end.as_ref().map(|end| end.as_str().to_owned()),
                )],
                Ok(Some(CellValue::Checked)) => vec![Item::Checked],
                Ok(Some(value)) => text(display_text(kind, &value)).into_iter().collect(),
                Ok(None) => Vec::new(),
                // Computed values (rollups, formulas) have their own shapes.
                Err(()) => json_items(raw),
            }
        }
    }
}

/// Items of a computed value: a number, a text, a boolean, a date value or a list of these.
fn json_items(raw: &Value) -> Vec<Item> {
    match raw {
        Value::Number(number) => number.as_f64().map(Item::Number).into_iter().collect(),
        Value::String(text) if !text.is_empty() => vec![Item::Text(text.clone())],
        Value::Bool(true) => vec![Item::Checked],
        Value::Array(list) => list.iter().flat_map(json_items).collect(),
        Value::Object(object) => match object.get("start").and_then(Value::as_str) {
            Some(start) => vec![Item::Date(
                start.to_owned(),
                object.get("end").and_then(Value::as_str).map(str::to_owned),
            )],
            None => Vec::new(),
        },
        _ => Vec::new(),
    }
}

fn date_json(start: &str, end: Option<&str>) -> Value {
    // A range of a day and a moment is shown as days.
    let (start, end) = match end {
        Some(end) if start.len() != end.len() => (
            &start[..10.min(start.len())],
            Some(&end[..10.min(end.len())]),
        ),
        _ => (start, end),
    };
    json!({ "start": start, "end": end })
}

/// The result of `function` over the items of each related page; `None` is empty.
fn compute(function: RollupFunction, pages: &[Vec<Item>]) -> Option<Value> {
    use RollupFunction as F;
    let all = || pages.iter().flatten();
    let count = |n: usize| Some(json!(n));
    let ratio = |n: usize| {
        Some(json!(if pages.is_empty() {
            0.0
        } else {
            n as f64 / pages.len() as f64
        }))
    };
    let empty = pages.iter().filter(|items| items.is_empty()).count();
    let checked = pages
        .iter()
        .filter(|items| items.iter().any(|item| matches!(item, Item::Checked)))
        .count();
    let mut numbers: Vec<f64> = all()
        .filter_map(|item| match item {
            Item::Number(number) if number.is_finite() => Some(*number),
            _ => None,
        })
        .collect();
    numbers.sort_by(f64::total_cmp);
    let starts = || {
        all().filter_map(|item| match item {
            Item::Date(start, _) => Some(start.as_str()),
            _ => None,
        })
    };
    let ends = || {
        all().filter_map(|item| match item {
            Item::Date(start, end) => Some(end.as_deref().unwrap_or(start)),
            _ => None,
        })
    };
    match function {
        F::ShowOriginal | F::ShowUnique => {
            let mut seen = HashSet::new();
            let texts: Vec<String> = all()
                .map(Item::text)
                .filter(|text| function == F::ShowOriginal || seen.insert(text.clone()))
                .collect();
            (!texts.is_empty()).then(|| json!(texts))
        }
        F::CountAll => count(pages.len()),
        F::CountValues => count(all().count()),
        F::CountUnique => count(all().map(Item::text).collect::<HashSet<_>>().len()),
        F::CountEmpty => count(empty),
        F::CountNotEmpty => count(pages.len() - empty),
        F::PercentEmpty => ratio(empty),
        F::PercentNotEmpty => ratio(pages.len() - empty),
        // `Iterator::sum` of no floats is -0.0.
        F::Sum => Some(json!(
            numbers.iter().fold(0.0, |total, number| total + number)
        )),
        F::Average => {
            (!numbers.is_empty()).then(|| json!(numbers.iter().sum::<f64>() / numbers.len() as f64))
        }
        F::Median => (!numbers.is_empty()).then(|| {
            let middle = numbers.len() / 2;
            json!(if numbers.len() % 2 == 0 {
                (numbers[middle - 1] + numbers[middle]) / 2.0
            } else {
                numbers[middle]
            })
        }),
        F::Min => numbers.first().map(|number| json!(number)),
        F::Max => numbers.last().map(|number| json!(number)),
        F::Range => numbers
            .first()
            .zip(numbers.last())
            .map(|(min, max)| json!(max - min)),
        F::EarliestDate => starts().min().map(|start| date_json(start, None)),
        F::LatestDate => ends().max().map(|end| date_json(end, None)),
        F::DateRange => starts()
            .min()
            .zip(ends().max())
            .map(|(start, end)| date_json(start, (end != start).then_some(end))),
        F::Checked => count(checked),
        F::Unchecked => count(pages.len() - checked),
        F::PercentChecked => ratio(checked),
        F::PercentUnchecked => ratio(pages.len() - checked),
    }
}

/// A rollup result as export text: numbers as written (percent functions as `50%`), dates as
/// ISO 8601 (`start/end` for a range), lists joined with ", ".
pub(super) fn text(config: &RollupConfig, value: &Value) -> String {
    use RollupFunction as F;
    let percent = matches!(
        config.function,
        F::PercentEmpty | F::PercentNotEmpty | F::PercentChecked | F::PercentUnchecked
    );
    match value {
        Value::Number(number) => {
            let number = number.as_f64().unwrap_or_default();
            if percent {
                format!("{}%", number_text((number * 1000.0).round() / 10.0))
            } else {
                number_text(number)
            }
        }
        other => json_items(other)
            .iter()
            .map(Item::text)
            .collect::<Vec<_>>()
            .join(", "),
    }
}
