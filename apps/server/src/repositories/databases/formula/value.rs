//! Formula values: what a property reads as inside a formula (spec 15.5) and the JSON a result
//! writes into `row.values`.

use std::cmp::Ordering;

use jiff::Timestamp;
use jiff::civil::Date;
use orbit_platform::Id;
use serde_json::{Value, json};

use std::collections::BTreeMap;

use super::super::{
    CellValue, DatabaseRow, DatePoint, PropertyKind, RelatedPage, RollupFunction, StatusGroup,
    parse_value,
};
use super::FormulaType;

/// A date point: a day without time (it has no zone) or a moment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Point {
    Day(Date),
    At(Timestamp),
}

/// A date or a date range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DateVal {
    pub start: Point,
    pub end: Option<Point>,
}

impl DateVal {
    pub const fn point(start: Point) -> Self {
        Self { start, end: None }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    /// No value: an empty number or date property, `first([])`.
    Empty,
    Number(f64),
    Text(String),
    Bool(bool),
    Date(DateVal),
    Person(Id),
    Page {
        id: Id,
        title: String,
    },
    List(Vec<Val>),
}

impl Val {
    /// `empty()`: no value, `""`, `0` and `[]` (spec 15.5).
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Empty => true,
            Self::Number(number) => *number == 0.0,
            Self::Text(text) => text.is_empty(),
            Self::List(items) => items.is_empty(),
            Self::Bool(_) | Self::Date(_) | Self::Person(_) | Self::Page { .. } => false,
        }
    }

    /// The JSON written into `row.values`, in the shape of a stored value of the type; `None`
    /// for no value (the key stays absent, as for an empty stored value).
    pub fn to_json(&self) -> Option<Value> {
        Some(match self {
            Self::Empty => return None,
            Self::Number(number) => json!(number),
            Self::Text(text) => json!(text),
            Self::Bool(flag) => json!(flag),
            Self::Date(date) => json!({
                "start": point_text(date.start),
                "end": date.end.map(point_text),
            }),
            Self::Person(id) | Self::Page { id, .. } => json!(id.to_string()),
            Self::List(items) => Value::Array(
                items
                    .iter()
                    .map(|item| item.to_json().unwrap_or(Value::Null))
                    .collect(),
            ),
        })
    }
}

/// `YYYY-MM-DD` or `YYYY-MM-DDTHH:MM:SSZ`, as stored dates are written.
pub fn point_text(point: Point) -> String {
    match point {
        Point::Day(date) => date.to_string(),
        Point::At(at) => at.strftime("%Y-%m-%dT%H:%M:%SZ").to_string(),
    }
}

fn parse_point(point: &DatePoint) -> Option<Point> {
    match point {
        DatePoint::Day(day) => day.parse::<Date>().ok().map(Point::Day),
        DatePoint::Moment(moment) => moment.parse::<Timestamp>().ok().map(Point::At),
    }
}

fn moment(millis: i64) -> Val {
    Timestamp::from_millisecond(millis)
        .map_or(Val::Empty, |at| Val::Date(DateVal::point(Point::At(at))))
}

/// How a property reads in a formula: its formula type, or why formulas cannot read it.
pub fn property_type(kind: &PropertyKind) -> Result<FormulaType, String> {
    let list = |item| FormulaType::List(Box::new(item));
    #[allow(unreachable_patterns)]
    Ok(match kind {
        PropertyKind::Title
        | PropertyKind::Text
        | PropertyKind::Url
        | PropertyKind::Email
        | PropertyKind::Phone
        | PropertyKind::Select(_)
        | PropertyKind::Status(_) => FormulaType::Text,
        PropertyKind::Number(_) | PropertyKind::UniqueId(_) => FormulaType::Number,
        PropertyKind::Checkbox => FormulaType::Boolean,
        PropertyKind::Date(_) | PropertyKind::CreatedTime | PropertyKind::LastEditedTime => {
            FormulaType::Date
        }
        PropertyKind::MultiSelect(_) => list(FormulaType::Text),
        PropertyKind::Person | PropertyKind::CreatedBy | PropertyKind::LastEditedBy => {
            list(FormulaType::Person)
        }
        PropertyKind::Formula(config) => config.result_type.clone(),
        PropertyKind::Relation(_) => list(FormulaType::Page),
        PropertyKind::Rollup(config) => match config.function {
            RollupFunction::ShowOriginal | RollupFunction::ShowUnique => list(FormulaType::Text),
            RollupFunction::EarliestDate
            | RollupFunction::LatestDate
            | RollupFunction::DateRange => FormulaType::Date,
            _ => FormulaType::Number,
        },
        other => {
            return Err(format!(
                "Formulas cannot read {} properties yet.",
                other.property_type().as_str().replace('_', " ")
            ));
        }
    })
}

/// The value of a non-formula property of `row` as a formula reads it.
pub fn read(
    kind: &PropertyKind,
    row: &DatabaseRow,
    raw: Option<&Value>,
    pages: &BTreeMap<String, RelatedPage>,
) -> Val {
    let option_name = |id: &str| {
        kind.options()
            .into_iter()
            .find(|(option, _)| *option == id)
            .map_or_else(String::new, |(_, name)| name.to_owned())
    };
    match kind {
        PropertyKind::Title => return Val::Text(row.title.clone()),
        PropertyKind::CreatedTime => return moment(row.created_at.as_millis()),
        PropertyKind::LastEditedTime => return moment(row.updated_at.as_millis()),
        PropertyKind::CreatedBy => return Val::List(vec![Val::Person(row.created_by)]),
        PropertyKind::LastEditedBy => return Val::List(vec![Val::Person(row.updated_by)]),
        PropertyKind::Relation(_) => return related(raw, pages),
        PropertyKind::Rollup(config) => return rollup(raw, config.function),
        _ => {}
    }
    let value = raw.and_then(|raw| parse_value(kind, raw).ok().flatten());
    match (kind, value) {
        (PropertyKind::Status(options), None) => Val::Text(
            options
                .iter()
                .find(|option| option.group == StatusGroup::Todo)
                .map(|option| option.name.clone())
                .unwrap_or_default(),
        ),
        (PropertyKind::Number(_) | PropertyKind::UniqueId(_) | PropertyKind::Date(_), None) => {
            Val::Empty
        }
        (PropertyKind::Checkbox, None) => Val::Bool(false),
        (PropertyKind::MultiSelect(_) | PropertyKind::Person, None) => Val::List(Vec::new()),
        (_, None) => Val::Text(String::new()),
        (_, Some(CellValue::Text(text) | CellValue::Url(text))) => Val::Text(text),
        (_, Some(CellValue::Number(number))) => Val::Number(number),
        #[allow(clippy::cast_precision_loss)]
        (_, Some(CellValue::UniqueId(number))) => Val::Number(number as f64),
        (_, Some(CellValue::Choice(id))) => Val::Text(option_name(&id)),
        (_, Some(CellValue::Choices(ids))) => {
            Val::List(ids.iter().map(|id| Val::Text(option_name(id))).collect())
        }
        (_, Some(CellValue::Date(date))) => match parse_point(&date.start) {
            Some(start) => Val::Date(DateVal {
                start,
                end: date.end.as_ref().and_then(parse_point),
            }),
            None => Val::Empty,
        },
        (_, Some(CellValue::People(ids))) => Val::List(ids.into_iter().map(Val::Person).collect()),
        (_, Some(CellValue::Checked)) => Val::Bool(true),
        #[allow(unreachable_patterns)]
        (_, Some(_)) => Val::Empty,
    }
}

/// A relation cell (page ids after the relation pass) as pages with their titles.
fn related(raw: Option<&Value>, pages: &BTreeMap<String, RelatedPage>) -> Val {
    let ids = raw.and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
    Val::List(
        ids.iter()
            .filter_map(Value::as_str)
            .filter_map(|id| {
                Some(Val::Page {
                    id: id.parse().ok()?,
                    title: pages
                        .get(id)
                        .map(|page| page.title.clone())
                        .unwrap_or_default(),
                })
            })
            .collect(),
    )
}

/// A rollup result: a number, a date `{start, end}` or texts (show original and unique).
fn rollup(raw: Option<&Value>, function: RollupFunction) -> Val {
    if raw.is_none()
        && matches!(
            function,
            RollupFunction::ShowOriginal | RollupFunction::ShowUnique
        )
    {
        return Val::List(Vec::new());
    }
    let point = |value: Option<&Value>| value.and_then(Value::as_str).and_then(DatePoint::parse);
    match raw {
        Some(Value::Number(number)) => number.as_f64().map_or(Val::Empty, Val::Number),
        Some(Value::Array(items)) => Val::List(
            items
                .iter()
                .filter_map(Value::as_str)
                .map(|text| Val::Text(text.to_owned()))
                .collect(),
        ),
        Some(date @ Value::Object(_)) => {
            match point(date.get("start")).as_ref().and_then(parse_point) {
                Some(start) => Val::Date(DateVal {
                    start,
                    end: point(date.get("end")).as_ref().and_then(parse_point),
                }),
                None => Val::Empty,
            }
        }
        _ => Val::Empty,
    }
}

/// The order of `sort()`, `min` and comparisons: numbers, texts, booleans and dates by value;
/// no value first. `None` when the values cannot be ordered.
pub fn compare(left: &Val, right: &Val, instant: impl Fn(Point) -> Timestamp) -> Option<Ordering> {
    match (left, right) {
        (Val::Empty, Val::Empty) => Some(Ordering::Equal),
        (Val::Empty, _) => Some(Ordering::Less),
        (_, Val::Empty) => Some(Ordering::Greater),
        (Val::Number(a), Val::Number(b)) => a.partial_cmp(b),
        (Val::Text(a), Val::Text(b)) => Some(a.cmp(b)),
        (Val::Bool(a), Val::Bool(b)) => Some(a.cmp(b)),
        (Val::Date(a), Val::Date(b)) => Some(instant(a.start).cmp(&instant(b.start))),
        _ => None,
    }
}
