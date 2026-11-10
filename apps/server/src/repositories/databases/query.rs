//! Row filters and sorts on the server (spec P3 14.4, decision D-P3-2), for the API and MCP
//! tools that query a database. The rules are those of `@orbit/database` (`filter.ts`,
//! `sort.ts`); both engines answer the shared cases in `packages/database/cases/`.
//!
//! Formulas and rollups filter and sort as their result type, relations by their pages' titles.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::str::FromStr;

use jiff::civil::Date;
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan};
use orbit_platform::Id;
use serde_json::Value;

use super::formula::FormulaType;
use super::{
    CellValue, DatabaseRow, DateConfig, DatePoint, FilterGroup, FilterNode, FilterOperator,
    FilterRule, GroupOp, Property, PropertyKind, RollupFunction, SortDirection, StatusGroup,
    ViewSort, parse_value,
};

/// Who reads, and when and where: `"me"` is `user_id`, relative dates count from `now`, and
/// times count as their day in `time_zone`.
pub struct QueryContext {
    pub user_id: Id,
    pub now: Timestamp,
    pub time_zone: TimeZone,
    /// Workspace members by id: their display names, which person sorts compare.
    pub members: HashMap<Id, String>,
    /// Titles of the pages that relation values link to, by page id: text rules on a relation
    /// match them.
    pub related: HashMap<String, String>,
}

/// The rows that pass `filter`, ordered by `sorts`.
#[must_use]
pub fn query_rows(
    rows: Vec<DatabaseRow>,
    properties: &[Property],
    filter: &FilterGroup,
    sorts: &[ViewSort],
    ctx: &QueryContext,
) -> Vec<DatabaseRow> {
    let by_id: HashMap<Id, &Property> = properties
        .iter()
        .map(|property| (property.id, property))
        .collect();
    let rows = rows
        .into_iter()
        .filter(|row| matches_filter(row, filter, &by_id, ctx))
        .collect();
    sort_rows(rows, sorts, &by_id, ctx)
}

/// A day or a time of a date value.
#[derive(Clone, Copy)]
enum Point {
    Day(Date),
    Moment(Timestamp),
}

impl Point {
    fn parse(point: &DatePoint) -> Option<Self> {
        match point {
            DatePoint::Day(day) => Date::from_str(day).ok().map(Self::Day),
            DatePoint::Moment(moment) => Timestamp::from_str(moment).ok().map(Self::Moment),
        }
    }

    fn from_millis(millis: i64) -> Option<Self> {
        Timestamp::from_millisecond(millis).ok().map(Self::Moment)
    }

    /// The viewer's calendar day of the point.
    fn day(self, time_zone: &TimeZone) -> Date {
        match self {
            Self::Day(day) => day,
            Self::Moment(moment) => moment.to_zoned(time_zone.clone()).date(),
        }
    }

    /// Milliseconds since the epoch; a day counts from its local midnight.
    fn millis(self, time_zone: &TimeZone) -> Option<i64> {
        match self {
            Self::Moment(moment) => Some(moment.as_millisecond()),
            Self::Day(day) => day
                .to_zoned(time_zone.clone())
                .ok()
                .map(|zoned| zoned.timestamp().as_millisecond()),
        }
    }
}

/// How a property filters and sorts. Computed types follow their result, as `formulaBase` and
/// `rollupBase` of `@orbit/database`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Text,
    Number,
    Choice,
    /// Multi-select options or people: `contains` any of the rule's ids.
    Many,
    Checkbox,
    Date,
    /// Related pages: text rules match their titles; they do not sort.
    Pages,
    /// Files and buttons: only the empty rules filter them.
    Inert,
}

fn class_of(kind: &PropertyKind) -> Class {
    match kind {
        PropertyKind::Title
        | PropertyKind::Text
        | PropertyKind::Url
        | PropertyKind::Email
        | PropertyKind::Phone => Class::Text,
        PropertyKind::Number(_) | PropertyKind::UniqueId(_) => Class::Number,
        PropertyKind::Select(_) | PropertyKind::Status(_) => Class::Choice,
        PropertyKind::MultiSelect(_)
        | PropertyKind::Person
        | PropertyKind::CreatedBy
        | PropertyKind::LastEditedBy => Class::Many,
        PropertyKind::Checkbox => Class::Checkbox,
        PropertyKind::Date(_) | PropertyKind::CreatedTime | PropertyKind::LastEditedTime => {
            Class::Date
        }
        PropertyKind::Relation(_) => Class::Pages,
        PropertyKind::Files | PropertyKind::Button(_) => Class::Inert,
        PropertyKind::Formula(config) => formula_class(&config.result_type),
        PropertyKind::Rollup(config) => match rollup_result(config.function) {
            RollupResult::List => Class::Text,
            RollupResult::Date => Class::Date,
            RollupResult::Number => Class::Number,
        },
    }
}

fn formula_class(result: &FormulaType) -> Class {
    match result {
        FormulaType::Number => Class::Number,
        FormulaType::Boolean => Class::Checkbox,
        FormulaType::Date => Class::Date,
        FormulaType::Person => Class::Many,
        FormulaType::Page => Class::Pages,
        FormulaType::List(item) if **item == FormulaType::Person => Class::Many,
        FormulaType::List(item) if **item == FormulaType::Page => Class::Pages,
        FormulaType::Text | FormulaType::Unknown | FormulaType::List(_) => Class::Text,
    }
}

enum RollupResult {
    List,
    Date,
    Number,
}

/// `rollupResult` of `relations.ts`.
const fn rollup_result(function: RollupFunction) -> RollupResult {
    match function {
        RollupFunction::ShowOriginal | RollupFunction::ShowUnique => RollupResult::List,
        RollupFunction::EarliestDate | RollupFunction::LatestDate | RollupFunction::DateRange => {
            RollupResult::Date
        }
        _ => RollupResult::Number,
    }
}

/// The non-empty value a row shows for a property, as `cellOf` and `parseValue` in `values.ts`.
enum Cell {
    /// Title, text, URL, email, phone, and text results.
    Text(String),
    Number(f64),
    Choice(String),
    Choices(Vec<String>),
    People(Vec<Id>),
    Date(Point, Option<Point>),
    Checked,
    Pages(Vec<String>),
    Files(usize),
}

fn cell_of(row: &DatabaseRow, property: &Property) -> Option<Cell> {
    let raw = row.values.get(&property.id.to_string());
    let stored = || raw.and_then(|raw| parse_value(&property.kind, raw).ok().flatten());
    let moment = |millis: i64| Point::from_millis(millis).map(|point| Cell::Date(point, None));
    match &property.kind {
        PropertyKind::Title => (!row.title.is_empty()).then(|| Cell::Text(row.title.clone())),
        PropertyKind::CreatedTime => moment(row.created_at.as_millis()),
        PropertyKind::LastEditedTime => moment(row.updated_at.as_millis()),
        PropertyKind::CreatedBy => Some(Cell::People(vec![row.created_by])),
        PropertyKind::LastEditedBy => Some(Cell::People(vec![row.updated_by])),
        PropertyKind::Status(options) => stored().and_then(from_value).or_else(|| {
            options
                .iter()
                .find(|option| option.group == StatusGroup::Todo)
                .map(|option| Cell::Choice(option.id.clone()))
        }),
        PropertyKind::Button(_) => None,
        PropertyKind::Formula(config) => raw.and_then(|raw| formula_cell(&config.result_type, raw)),
        PropertyKind::Rollup(config) => raw.and_then(|raw| rollup_cell(config.function, raw)),
        PropertyKind::Text
        | PropertyKind::Number(_)
        | PropertyKind::Select(_)
        | PropertyKind::MultiSelect(_)
        | PropertyKind::Date(_)
        | PropertyKind::Person
        | PropertyKind::Checkbox
        | PropertyKind::Url
        | PropertyKind::Email
        | PropertyKind::Phone
        | PropertyKind::Files
        | PropertyKind::UniqueId(_)
        | PropertyKind::Relation(_) => stored().and_then(from_value),
    }
}

fn from_value(value: CellValue) -> Option<Cell> {
    Some(match value {
        CellValue::Text(text) | CellValue::Url(text) => Cell::Text(text),
        CellValue::Number(number) => Cell::Number(number),
        #[allow(clippy::cast_precision_loss, reason = "unique IDs stay far below 2^53")]
        CellValue::UniqueId(number) => Cell::Number(number as f64),
        CellValue::Choice(id) => Cell::Choice(id),
        CellValue::Choices(ids) => Cell::Choices(ids),
        CellValue::People(ids) => Cell::People(ids),
        CellValue::Checked => Cell::Checked,
        CellValue::Date(date) => Cell::Date(
            Point::parse(&date.start)?,
            date.end.as_ref().and_then(Point::parse),
        ),
        CellValue::Pages(ids) => Cell::Pages(ids.iter().map(Id::to_string).collect()),
        CellValue::Files(files) => Cell::Files(files.len()),
    })
}

/// A formula result as its base type reads it (`formulaRaw` of `formula/values.ts`): one person
/// or page is a list of one, and other lists read as their items' text. An error is empty.
fn formula_cell(result: &FormulaType, raw: &Value) -> Option<Cell> {
    let one_or_many = |raw: &Value| -> Vec<String> {
        match raw {
            Value::String(id) => vec![id.clone()],
            Value::Array(items) => items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect(),
            _ => Vec::new(),
        }
    };
    match formula_class(result) {
        Class::Number => raw
            .as_f64()
            .filter(|number| number.is_finite())
            .map(Cell::Number),
        Class::Checkbox => (raw == &Value::Bool(true)).then_some(Cell::Checked),
        Class::Date => parse_value(&PropertyKind::Date(DateConfig::default()), raw)
            .ok()
            .flatten()
            .and_then(from_value),
        Class::Many => {
            let ids: Vec<Id> = one_or_many(raw)
                .iter()
                .filter_map(|id| id.parse().ok())
                .collect();
            (!ids.is_empty()).then_some(Cell::People(ids))
        }
        Class::Pages => {
            let ids = one_or_many(raw);
            (!ids.is_empty()).then_some(Cell::Pages(ids))
        }
        _ => {
            let text = match (result, raw) {
                (FormulaType::Text, Value::String(text)) => text.clone(),
                (FormulaType::Text, _) => String::new(),
                _ => item_text(raw),
            };
            (!text.is_empty()).then_some(Cell::Text(text))
        }
    }
}

/// `itemText` of `formula/values.ts`.
fn item_text(raw: &Value) -> String {
    match raw {
        Value::Array(items) => items
            .iter()
            .map(item_text)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join(", "),
        Value::Object(object) => match (object.get("start"), object.get("end")) {
            (Some(Value::String(start)), Some(Value::String(end))) => format!("{start} → {end}"),
            (Some(Value::String(start)), _) => start.clone(),
            _ => String::new(),
        },
        Value::String(text) => text.clone(),
        Value::Number(number) => number.as_f64().map_or_else(String::new, |n| n.to_string()),
        Value::Bool(flag) => flag.to_string(),
        Value::Null => String::new(),
    }
}

/// A rollup result: a list reads as its texts joined with ", ", the others as a number or date.
fn rollup_cell(function: RollupFunction, raw: &Value) -> Option<Cell> {
    match rollup_result(function) {
        RollupResult::List => {
            let items: Vec<&str> = raw
                .as_array()?
                .iter()
                .filter_map(Value::as_str)
                .filter(|item| !item.is_empty())
                .collect();
            (!items.is_empty()).then(|| Cell::Text(items.join(", ")))
        }
        RollupResult::Date => parse_value(&PropertyKind::Date(DateConfig::default()), raw)
            .ok()
            .flatten()
            .and_then(from_value),
        RollupResult::Number => raw
            .as_f64()
            .filter(|number| number.is_finite())
            .map(Cell::Number),
    }
}

/// Whether `row` passes `group`: an `and` group needs every child, an `or` group one, and an
/// empty group matches every row. A rule on a property that is not in `properties` matches.
fn matches_filter(
    row: &DatabaseRow,
    group: &FilterGroup,
    properties: &HashMap<Id, &Property>,
    ctx: &QueryContext,
) -> bool {
    if group.children.is_empty() {
        return true;
    }
    let passes = |child: &FilterNode| match child {
        FilterNode::Group(group) => matches_filter(row, group, properties, ctx),
        FilterNode::Rule(rule) => properties
            .get(&rule.property_id)
            .is_none_or(|property| matches_rule(row, rule, property, ctx)),
    };
    match group.op {
        GroupOp::And => group.children.iter().all(passes),
        GroupOp::Or => group.children.iter().any(passes),
    }
}

/// `matches` of `filter.ts`: a rule without a value (`null`, `""`, `[]`) matches every row,
/// text compares ignore case, and dates compare the viewer's local days (a range by its start,
/// except `is`, which matches a day inside the range).
fn matches_rule(
    row: &DatabaseRow,
    rule: &FilterRule,
    property: &Property,
    ctx: &QueryContext,
) -> bool {
    use FilterOperator as Op;
    let cell = cell_of(row, property);
    match rule.operator {
        Op::IsEmpty => return cell.is_none(),
        Op::IsNotEmpty => return cell.is_some(),
        _ => {}
    }
    let value = &rule.value;
    if matches!(value, Value::Null)
        || value.as_str() == Some("")
        || value.as_array().is_some_and(Vec::is_empty)
    {
        return true;
    }
    match class_of(&property.kind) {
        Class::Text | Class::Pages => value.as_str().is_none_or(|needle| {
            let text = match &cell {
                Some(Cell::Text(text)) => text.clone(),
                Some(Cell::Pages(ids)) => ids
                    .iter()
                    .filter_map(|id| ctx.related.get(id))
                    .map(|title| if title.is_empty() { "Untitled" } else { title })
                    .collect::<Vec<_>>()
                    .join(", "),
                _ => String::new(),
            };
            match_text(&text, rule.operator, needle)
        }),
        Class::Number => value.as_f64().is_none_or(|target| {
            let number = match cell {
                Some(Cell::Number(number)) => Some(number),
                _ => None,
            };
            match_number(number, rule.operator, target)
        }),
        Class::Choice => {
            let ids = id_list(value, ctx);
            let hit = matches!(&cell, Some(Cell::Choice(id)) if ids.contains(id));
            if rule.operator == Op::IsNot {
                !hit
            } else {
                hit
            }
        }
        Class::Many => {
            let ids = id_list(value, ctx);
            let hit = match &cell {
                Some(Cell::Choices(held)) => held.iter().any(|id| ids.contains(id)),
                Some(Cell::People(held)) => held.iter().any(|id| ids.contains(&id.to_string())),
                _ => false,
            };
            if matches!(rule.operator, Op::NotContains | Op::IsNot) {
                !hit
            } else {
                hit
            }
        }
        Class::Checkbox => value
            .as_bool()
            .is_none_or(|ticked| cell.is_some() == ticked),
        Class::Date => match value.as_str().and_then(|text| rule_day(text, ctx)) {
            None => true,
            Some(target) => match_date(cell.as_ref(), rule.operator, target, ctx),
        },
        Class::Inert => true,
    }
}

fn match_text(text: &str, operator: FilterOperator, needle: &str) -> bool {
    let text = text.to_lowercase();
    let needle = needle.to_lowercase();
    match operator {
        FilterOperator::Equals => text == needle,
        FilterOperator::NotEquals => text != needle,
        FilterOperator::Contains => text.contains(&needle),
        FilterOperator::NotContains => !text.contains(&needle),
        FilterOperator::StartsWith => text.starts_with(&needle),
        FilterOperator::EndsWith => text.ends_with(&needle),
        _ => true,
    }
}

/// An empty number fails every comparison except `!=`.
#[allow(clippy::float_cmp, reason = "`=` compares as `filter.ts` does")]
fn match_number(number: Option<f64>, operator: FilterOperator, target: f64) -> bool {
    let Some(number) = number else {
        return operator == FilterOperator::Ne;
    };
    match operator {
        FilterOperator::Eq => number == target,
        FilterOperator::Ne => number != target,
        FilterOperator::Gt => number > target,
        FilterOperator::Lt => number < target,
        FilterOperator::Ge => number >= target,
        FilterOperator::Le => number <= target,
        _ => true,
    }
}

fn match_date(
    cell: Option<&Cell>,
    operator: FilterOperator,
    target: Date,
    ctx: &QueryContext,
) -> bool {
    let Some(Cell::Date(start, end)) = cell else {
        return false;
    };
    let start_day = start.day(&ctx.time_zone);
    match operator {
        FilterOperator::Is => {
            let end_day = end.map_or(start_day, |end| end.day(&ctx.time_zone));
            start_day <= target && target <= end_day
        }
        FilterOperator::Before => start_day < target,
        FilterOperator::After => start_day > target,
        FilterOperator::OnOrBefore => start_day <= target,
        FilterOperator::OnOrAfter => start_day >= target,
        _ => true,
    }
}

/// The local day a rule names: a relative name counted from `ctx.now`, or a date or a time.
fn rule_day(value: &str, ctx: &QueryContext) -> Option<Date> {
    let today = || ctx.now.to_zoned(ctx.time_zone.clone()).date();
    // `checked_add` of months clamps to the month's last day (Jan 31 + 1 month = Feb 28).
    let shifted = |span: jiff::Span| today().checked_add(span).ok();
    match value {
        "today" => Some(today()),
        "tomorrow" => shifted(1.day()),
        "yesterday" => shifted((-1).days()),
        "one_week_ago" => shifted((-7).days()),
        "one_week_from_now" => shifted(7.days()),
        "one_month_ago" => shifted((-1).months()),
        "one_month_from_now" => shifted(1.month()),
        _ => DatePoint::parse(value)
            .as_ref()
            .and_then(Point::parse)
            .map(|point| point.day(&ctx.time_zone)),
    }
}

/// Option or user ids of a rule, with `"me"` (alone or in the list) as the viewer.
fn id_list(value: &Value, ctx: &QueryContext) -> Vec<String> {
    let id = |item: &Value| {
        item.as_str().map(|id| {
            if id == "me" {
                ctx.user_id.to_string()
            } else {
                id.to_owned()
            }
        })
    };
    match value.as_array() {
        Some(items) => items.iter().filter_map(id).collect(),
        None => id(value).into_iter().collect(),
    }
}

/// A sort key; `None` (an empty value) sorts last in both directions.
enum SortKey {
    Number(f64),
    Text(String),
    /// Option positions of a multi-select, compared item by item.
    Order(Vec<usize>),
}

/// `sortRows` of `sort.ts`: empty values last in both directions, options by their order in the
/// config, people by display name, unticked checkboxes before ticked ones, dates by their start,
/// files by their count; relations do not sort.
/// Ties fall back to the manual order (`position`, then `id`). Sorts on unknown properties are
/// skipped.
fn sort_rows(
    rows: Vec<DatabaseRow>,
    sorts: &[ViewSort],
    properties: &HashMap<Id, &Property>,
    ctx: &QueryContext,
) -> Vec<DatabaseRow> {
    let active: Vec<(&Property, bool)> = sorts
        .iter()
        .filter_map(|sort| {
            let property = properties.get(&sort.property_id)?;
            Some((*property, sort.direction == SortDirection::Desc))
        })
        .collect();
    let mut keyed: Vec<(Vec<Option<SortKey>>, DatabaseRow)> = rows
        .into_iter()
        .map(|row| {
            let keys = active
                .iter()
                .map(|(property, _)| sort_key(&row, property, ctx))
                .collect();
            (keys, row)
        })
        .collect();
    keyed.sort_by(|(left_keys, left), (right_keys, right)| {
        for ((left_key, right_key), (_, descending)) in
            left_keys.iter().zip(right_keys).zip(&active)
        {
            let order = match (left_key, right_key) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => return Ordering::Greater,
                (Some(_), None) => return Ordering::Less,
                (Some(left_key), Some(right_key)) => compare_keys(left_key, right_key),
            };
            if order != Ordering::Equal {
                return if *descending { order.reverse() } else { order };
            }
        }
        left.position
            .cmp(&right.position)
            .then_with(|| left.id.to_string().cmp(&right.id.to_string()))
    });
    keyed.into_iter().map(|(_, row)| row).collect()
}

fn sort_key(row: &DatabaseRow, property: &Property, ctx: &QueryContext) -> Option<SortKey> {
    let cell = cell_of(row, property);
    match (class_of(&property.kind), cell) {
        (Class::Checkbox, cell) => Some(SortKey::Number(if cell.is_some() { 1.0 } else { 0.0 })),
        (Class::Pages | Class::Inert, Some(Cell::Files(count))) =>
        {
            #[allow(clippy::cast_precision_loss, reason = "at most 20 files")]
            Some(SortKey::Number(count as f64))
        }
        (Class::Pages | Class::Inert, _) | (_, None) => None,
        (_, Some(Cell::Text(text))) => Some(SortKey::Text(text)),
        (_, Some(Cell::Number(number))) => Some(SortKey::Number(number)),
        (_, Some(Cell::People(ids))) => {
            let names: Vec<&str> = ids
                .iter()
                .filter_map(|id| ctx.members.get(id).map(String::as_str))
                .filter(|name| !name.is_empty())
                .collect();
            (!names.is_empty()).then(|| SortKey::Text(names.join(", ")))
        }
        (_, Some(Cell::Choice(id))) => {
            option_position(&property.kind, &id).map(|at| SortKey::Number(at as f64))
        }
        (_, Some(Cell::Choices(ids))) => {
            let order: Vec<usize> = ids
                .iter()
                .filter_map(|id| option_position(&property.kind, id))
                .collect();
            (!order.is_empty()).then_some(SortKey::Order(order))
        }
        (_, Some(Cell::Date(start, _))) => start
            .millis(&ctx.time_zone)
            .map(|millis| SortKey::Number(millis as f64)),
        (_, Some(Cell::Checked | Cell::Pages(_) | Cell::Files(_))) => None,
    }
}

fn option_position(kind: &PropertyKind, id: &str) -> Option<usize> {
    kind.options().iter().position(|(option, _)| *option == id)
}

fn compare_keys(left: &SortKey, right: &SortKey) -> Ordering {
    match (left, right) {
        (SortKey::Number(left), SortKey::Number(right)) => {
            left.partial_cmp(right).unwrap_or(Ordering::Equal)
        }
        (SortKey::Text(left), SortKey::Text(right)) => compare_text(left, right),
        (SortKey::Order(left), SortKey::Order(right)) => left.cmp(right),
        _ => Ordering::Equal,
    }
}

/// Text order close to `Intl.Collator(locale, {sensitivity: 'base', numeric: true})`: letter
/// case is ignored and digit runs compare as numbers ("Item 2" before "Item 10"). Accents count
/// (the collator ignores them), so the shared cases use ASCII text.
fn compare_text(left: &str, right: &str) -> Ordering {
    let (mut left, mut right) = (left.chars().peekable(), right.chars().peekable());
    loop {
        match (left.peek().copied(), right.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(a), Some(b)) if a.is_ascii_digit() && b.is_ascii_digit() => {
                let digits = |chars: &mut std::iter::Peekable<std::str::Chars<'_>>| {
                    let mut run = String::new();
                    while let Some(digit) = chars.next_if(char::is_ascii_digit) {
                        run.push(digit);
                    }
                    run
                };
                let (a, b) = (digits(&mut left), digits(&mut right));
                let (a, b) = (a.trim_start_matches('0'), b.trim_start_matches('0'));
                let order = a.len().cmp(&b.len()).then_with(|| a.cmp(b));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(a), Some(b)) => {
                left.next();
                right.next();
                let order = a.to_lowercase().cmp(b.to_lowercase());
                if order != Ordering::Equal {
                    return order;
                }
            }
        }
    }
}
