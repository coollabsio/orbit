//! Formula properties (spec P2/P3 item 13, decisions D-P3-1 and D-P3-3): an expression over the
//! row's other properties, run only here, on the server, each time a database is read. Nothing
//! is stored but the expression (property ids in `prop("…")`) and its inferred result type.
//!
//! - [`syntax`] lexes and parses (spec 15.4) within the length, node and depth limits (15.8);
//! - [`check`] resolves `prop("…")` (an id or a name), checks types and builds [`check::Ir`];
//! - [`eval`] runs the IR per row with a step counter and a deadline per read;
//! - [`compile_all`] orders a database's formulas by their references, so a formula reads the
//!   results of the formulas it uses, and rejects cycles and chains deeper than 15.
//!
//! Every recursive walk here runs over a tree that the parser capped at [`MAX_DEPTH`] levels.

mod check;
mod dates;
mod eval;
mod syntax;
#[cfg(test)]
mod tests;
mod value;

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::time::{Duration, Instant};

use jiff::Timestamp;
use jiff::tz::TimeZone;
use orbit_platform::Id;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use utoipa::ToSchema;

use orbit_platform::Row;
use sqlx::{Any, Transaction};

use super::{
    ComputeScope, DatabaseError, DatabaseRow, Property, PropertyKind, PropertyType, RelatedPage,
};
use check::{Checker, Ir, PropInfo};
pub use eval::Member;
use eval::{Context, Machine, TOO_SLOW};
use syntax::{Node, NodeKind};
use value::Val;

pub const MAX_EXPRESSION_CHARS: usize = 2_000;
pub const MAX_NODES: usize = 500;
pub const MAX_DEPTH: usize = 32;
pub const MAX_STEPS: u32 = 2_000;
pub const MAX_LIST_ITEMS: usize = 1_000;
pub const MAX_RESULT_CHARS: usize = 10_000;
pub const MAX_FORMULAS: usize = 20;
/// Formulas that read formulas: at most this many in a chain (Notion's layer limit).
pub const MAX_LAYERS: usize = 15;
/// All formulas of one database read share this much time; later cells show an error.
const READ_BUDGET: Duration = Duration::from_secs(2);
/// Rows a preview computes.
pub const PREVIEW_ROWS: usize = 10;

/// Byte offsets into the expression.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub const fn to(self, other: Self) -> Self {
        Self {
            start: self.start,
            end: other.end,
        }
    }
}

/// A parse or type error of a formula, at a place in its expression.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormulaError {
    pub message: String,
    pub span: Span,
}

impl FormulaError {
    pub fn new(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
        }
    }

    /// The error as the API returns it, with UTF-16 offsets into `source` (JavaScript string
    /// indexes) so the editor can underline it.
    #[must_use]
    pub fn problem(&self, source: &str) -> FormulaProblem {
        let utf16 = |byte: usize| {
            let byte = byte.min(source.len());
            let byte = (0..=byte)
                .rev()
                .find(|at| source.is_char_boundary(*at))
                .unwrap_or(0);
            source[..byte].encode_utf16().count()
        };
        FormulaProblem {
            message: self.message.clone(),
            start: utf16(self.span.start),
            end: utf16(self.span.end),
        }
    }
}

/// A formula error in an API response: `start`..`end` are UTF-16 offsets into the expression.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[schema(as = DatabaseFormulaProblem)]
pub struct FormulaProblem {
    pub message: String,
    pub start: usize,
    pub end: usize,
}

/// The type of a formula's result (spec 15.3), written `number`, `text`, `boolean`, `date`,
/// `person`, `page` or `list<…>` of these.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormulaType {
    Number,
    Text,
    Boolean,
    Date,
    Person,
    Page,
    List(Box<FormulaType>),
    /// The items of `[]`: fits any type.
    Unknown,
}

impl FormulaType {
    /// The type both fit, if any (`Unknown` fits everything).
    fn unify(&self, other: &Self) -> Option<Self> {
        match (self, other) {
            (Self::Unknown, other) | (other, Self::Unknown) => Some(other.clone()),
            (Self::List(a), Self::List(b)) => a.unify(b).map(|item| Self::List(Box::new(item))),
            (a, b) if a == b => Some(a.clone()),
            _ => None,
        }
    }

    fn name(&self) -> String {
        match self {
            Self::Number => "number".to_owned(),
            Self::Text | Self::Unknown => "text".to_owned(),
            Self::Boolean => "boolean".to_owned(),
            Self::Date => "date".to_owned(),
            Self::Person => "person".to_owned(),
            Self::Page => "page".to_owned(),
            Self::List(item) => format!("list<{}>", item.name()),
        }
    }

    /// For messages: "a number", "a list of texts".
    fn article(&self) -> String {
        match self {
            Self::Number => "a number".to_owned(),
            Self::Text | Self::Unknown => "a text".to_owned(),
            Self::Boolean => "true/false".to_owned(),
            Self::Date => "a date".to_owned(),
            Self::Person => "a person".to_owned(),
            Self::Page => "a page".to_owned(),
            Self::List(item) => match **item {
                Self::Boolean => "a list of true/false values".to_owned(),
                Self::Person => "a list of people".to_owned(),
                _ => format!("a list of {}s", item.name()),
            },
        }
    }

    /// Reads [`Self::name`] back without recursion.
    fn parse(text: &str) -> Option<Self> {
        let mut rest = text;
        let mut lists = 0;
        while let Some(inner) = rest.strip_prefix("list<") {
            rest = inner.strip_suffix('>')?;
            lists += 1;
        }
        let mut ty = match rest {
            "number" => Self::Number,
            "text" => Self::Text,
            "boolean" => Self::Boolean,
            "date" => Self::Date,
            "person" => Self::Person,
            "page" => Self::Page,
            _ => return None,
        };
        for _ in 0..lists {
            ty = Self::List(Box::new(ty));
        }
        Some(ty)
    }
}

impl fmt::Display for FormulaType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.name())
    }
}

/// The config of a formula property. Clients send `{expression}` (names or ids in
/// `prop("…")`); the server stores ids and the inferred `result_type`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormulaConfig {
    pub expression: String,
    pub result_type: FormulaType,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigInput {
    #[serde(default)]
    expression: String,
    #[serde(default)]
    result_type: Option<String>,
}

impl Default for FormulaConfig {
    fn default() -> Self {
        Self {
            expression: String::new(),
            result_type: FormulaType::Text,
        }
    }
}

impl FormulaConfig {
    /// Parses a sent or stored config; errors name the field path. The expression is checked
    /// against the other properties by [`prepare`].
    pub fn parse(config: &Value) -> Result<Self, String> {
        let input: ConfigInput =
            serde_json::from_value(config.clone()).map_err(|_| "config".to_owned())?;
        let result_type = match input.result_type {
            Some(text) => FormulaType::parse(&text).ok_or("config.result_type")?,
            None => FormulaType::Text,
        };
        Ok(Self {
            expression: input.expression,
            result_type,
        })
    }

    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({ "expression": self.expression, "result_type": self.result_type.name() })
    }
}

/// One compiled formula of a database.
struct Compiled {
    /// The formula's index in the database's properties.
    index: usize,
    program: Result<(Ir, FormulaType), FormulaError>,
}

fn expression(property: &Property) -> Option<&str> {
    match &property.kind {
        PropertyKind::Formula(config) => Some(&config.expression),
        _ => None,
    }
}

/// Finds the property that `prop("…")` names: an id first, then a name.
fn resolve(properties: &[Property], reference: &str) -> Result<usize, String> {
    properties
        .iter()
        .position(|property| property.id.to_string() == reference)
        .or_else(|| {
            properties
                .iter()
                .position(|property| property.name == reference)
        })
        .ok_or_else(|| {
            if reference.parse::<Id>().is_ok() {
                "A property was deleted.".to_owned()
            } else {
                format!("No property named \"{reference}\".")
            }
        })
}

/// The `prop("…")` texts of a tree with the span of each quoted text.
fn references(node: &Node, out: &mut Vec<(Span, String)>) {
    match &node.kind {
        NodeKind::Call { name, args, .. } => {
            if name == "prop"
                && let [arg] = args.as_slice()
                && let NodeKind::Text(reference) = &arg.kind
            {
                out.push((arg.span, reference.clone()));
            }
            args.iter().for_each(|arg| references(arg, out));
        }
        NodeKind::List(items) => items.iter().for_each(|item| references(item, out)),
        NodeKind::Negate(inner) | NodeKind::Not(inner) => references(inner, out),
        NodeKind::Binary(_, left, right) => {
            references(left, out);
            references(right, out);
        }
        NodeKind::Chain(first, rest) => {
            references(first, out);
            rest.iter().for_each(|(_, node)| references(node, out));
        }
        NodeKind::Ternary(test, then, other) => {
            references(test, out);
            references(then, out);
            references(other, out);
        }
        NodeKind::Number(_) | NodeKind::Text(_) | NodeKind::Bool(_) | NodeKind::Name(_) => {}
    }
}

/// Every formula of `properties`, compiled in an order where each comes after the formulas it
/// reads. A formula in a cycle, past [`MAX_FORMULAS`] or more than [`MAX_LAYERS`] deep fails, and
/// so does every formula that reads a failed one.
fn compile_all(properties: &[Property]) -> Vec<Compiled> {
    let formulas: Vec<usize> = (0..properties.len())
        .filter(|at| expression(&properties[*at]).is_some())
        .collect();
    let whole = |source: &str| Span::new(0, source.len());
    // `None`: an empty formula, which is valid and gives no value.
    let mut parsed: HashMap<usize, Option<Result<Node, FormulaError>>> = HashMap::new();
    let mut deps: HashMap<usize, Vec<usize>> = HashMap::new();
    for (count, at) in formulas.iter().enumerate() {
        let source = expression(&properties[*at]).unwrap_or_default();
        let tree = if count >= MAX_FORMULAS {
            Some(Err(FormulaError::new(
                format!("A database holds at most {MAX_FORMULAS} formulas."),
                whole(source),
            )))
        } else if source.trim().is_empty() {
            None
        } else {
            Some(syntax::parse(source))
        };
        let mut refs = Vec::new();
        if let Some(Ok(tree)) = &tree {
            references(tree, &mut refs);
        }
        let mut uses: Vec<usize> = refs
            .iter()
            .filter_map(|(_, reference)| resolve(properties, reference).ok())
            .filter(|index| formulas.contains(index))
            .collect();
        uses.sort_unstable();
        uses.dedup();
        deps.insert(*at, uses);
        parsed.insert(*at, tree);
    }
    let reaches = |from: usize, target: usize| {
        let mut seen = vec![from];
        let mut stack = deps[&from].clone();
        while let Some(next) = stack.pop() {
            if next == target {
                return true;
            }
            if !seen.contains(&next) {
                seen.push(next);
                stack.extend(deps[&next].iter().copied());
            }
        }
        false
    };
    let mut props: Vec<PropInfo> = properties
        .iter()
        .map(|property| PropInfo {
            ty: match property.kind {
                PropertyKind::Formula(_) => Err(format!("\"{}\" has an error.", property.name)),
                _ => value::property_type(&property.kind),
            },
        })
        .collect();
    let mut layers: HashMap<usize, usize> = HashMap::new();
    let mut done: Vec<Compiled> = Vec::new();
    let mut pending = formulas.clone();
    while !pending.is_empty() {
        let ready: Vec<usize> = pending
            .iter()
            .copied()
            .filter(|at| {
                deps[at]
                    .iter()
                    .all(|dep| !pending.contains(dep) || reaches(*dep, *dep))
            })
            .collect();
        // Only formulas in a cycle (and those that read them) are left.
        let next = if ready.is_empty() {
            pending.clone()
        } else {
            ready
        };
        for at in next {
            pending.retain(|other| *other != at);
            let source = expression(&properties[at]).unwrap_or_default();
            let cycle = deps[&at]
                .iter()
                .find(|dep| **dep == at || reaches(**dep, at))
                .copied();
            let layer = 1 + deps[&at]
                .iter()
                .filter_map(|dep| layers.get(dep))
                .max()
                .copied()
                .unwrap_or(0);
            layers.insert(at, layer);
            let program = match (parsed.remove(&at).flatten(), cycle) {
                (Some(Err(error)), _) => Err(error),
                (_, Some(dep)) => Err(FormulaError::new(
                    format!(
                        "This formula uses itself through \"{}\".",
                        properties[dep].name
                    ),
                    whole(source),
                )),
                _ if layer > MAX_LAYERS => Err(FormulaError::new(
                    format!("This formula reads more than {MAX_LAYERS} formulas in a chain."),
                    whole(source),
                )),
                (Some(Ok(tree)), None) => {
                    let find = |reference: &str| resolve(properties, reference);
                    Checker::new(&props, &find).check(&tree)
                }
                (None, _) => Ok((Ir::Const(Val::Empty), FormulaType::Text)),
            };
            if let Ok((_, ty)) = &program {
                props[at].ty = Ok(ty.clone());
            }
            done.push(Compiled { index: at, program });
        }
    }
    done
}

/// Checks the formula of `properties[index]` as a client sent it (names or ids) against the
/// database; returns its config to store, with ids in `prop("…")` and the result type.
pub fn prepare(properties: &[Property], index: usize) -> Result<FormulaConfig, FormulaError> {
    let source = expression(&properties[index]).unwrap_or_default();
    if source.chars().count() > MAX_EXPRESSION_CHARS {
        let start = source
            .char_indices()
            .nth(MAX_EXPRESSION_CHARS)
            .map_or(0, |(at, _)| at);
        return Err(FormulaError::new(
            format!("A formula has at most {MAX_EXPRESSION_CHARS} characters."),
            Span::new(start, source.len()),
        ));
    }
    let compiled = compile_all(properties);
    let program = compiled
        .into_iter()
        .find(|compiled| compiled.index == index)
        .map(|compiled| compiled.program)
        .unwrap_or_else(|| Ok((Ir::Const(Val::Empty), FormulaType::Text)))?;
    let mut refs = Vec::new();
    if !source.trim().is_empty() {
        references(&syntax::parse(source)?, &mut refs);
    }
    let mut stored = source.to_owned();
    refs.sort_by_key(|(span, _)| std::cmp::Reverse(span.start));
    for (span, reference) in refs {
        if let Ok(target) = resolve(properties, &reference) {
            stored.replace_range(
                span.start..span.end,
                &format!("\"{}\"", properties[target].id),
            );
        }
    }
    Ok(FormulaConfig {
        expression: stored,
        result_type: program.1,
    })
}

/// Formulas whose result type changed with the other properties (a referenced property changed
/// type or was deleted): their index and new config.
pub fn retyped(properties: &[Property]) -> Vec<(usize, FormulaConfig)> {
    compile_all(properties)
        .into_iter()
        .filter_map(|compiled| {
            let PropertyKind::Formula(config) = &properties[compiled.index].kind else {
                return None;
            };
            let (_, ty) = compiled.program.ok()?;
            (ty != config.result_type).then(|| {
                (
                    compiled.index,
                    FormulaConfig {
                        expression: config.expression.clone(),
                        result_type: ty,
                    },
                )
            })
        })
        .collect()
}

/// Whether any property is a formula.
pub fn has_formulas(properties: &[Property]) -> bool {
    properties
        .iter()
        .any(|property| expression(property).is_some())
}

/// The error shape of a formula cell: `{"error": message}`.
fn error_value(message: &str) -> Value {
    json!({ "error": message })
}

fn mark_used(ir: &Ir, used: &mut [bool]) {
    match ir {
        Ir::Prop(index) => used[*index] = true,
        Ir::Const(_) | Ir::Local(_) => {}
        Ir::List(items) | Ir::Call(_, items) => items.iter().for_each(|item| mark_used(item, used)),
        Ir::Lambda(_, a, b) | Ir::Let(a, b) | Ir::Binary(_, a, b) => {
            mark_used(a, used);
            mark_used(b, used);
        }
        Ir::If(a, b, c) => {
            mark_used(a, used);
            mark_used(b, used);
            mark_used(c, used);
        }
        Ir::Negate(inner) | Ir::Not(inner) => mark_used(inner, used),
        Ir::Chain(first, rest) => {
            mark_used(first, used);
            rest.iter().for_each(|(_, ir)| mark_used(ir, used));
        }
    }
}

/// What a formula pass reads besides the rows.
pub struct FillScope<'a> {
    /// The reader's IANA zone; an unknown zone reads as UTC.
    pub tz: &'a str,
    pub people: &'a HashMap<Id, Member>,
    /// The related pages of the read's relation cells, for page titles.
    pub pages: &'a BTreeMap<String, RelatedPage>,
}

/// Writes every formula value of `rows` into `values`: the result in its type's JSON shape,
/// nothing for no value, `{"error": message}` for a failed cell. Never fails the read.
pub fn fill(properties: &[Property], rows: &mut [DatabaseRow], scope: &FillScope<'_>) {
    let compiled = compile_all(properties);
    if compiled.is_empty() {
        return;
    }
    let mut used = vec![false; properties.len()];
    for compiled in &compiled {
        if let Ok((ir, _)) = &compiled.program {
            mark_used(ir, &mut used);
        }
    }
    let names: Vec<String> = properties
        .iter()
        .map(|property| property.name.clone())
        .collect();
    let ctx = Context {
        tz: TimeZone::get(scope.tz).unwrap_or(TimeZone::UTC),
        now: Timestamp::now(),
        people: scope.people,
        deadline: Instant::now() + READ_BUDGET,
        names: &names,
    };
    for row in rows.iter_mut() {
        let mut inputs: Vec<Result<Val, ()>> = properties
            .iter()
            .enumerate()
            .map(|(at, property)| {
                Ok(if used[at] && expression(property).is_none() {
                    value::read(
                        &property.kind,
                        row,
                        row.values.get(&property.id.to_string()),
                        scope.pages,
                    )
                } else {
                    Val::Empty
                })
            })
            .collect();
        for compiled in &compiled {
            let key = properties[compiled.index].id.to_string();
            let result = match &compiled.program {
                Err(error) => Err(error.message.clone()),
                Ok(_) if Instant::now() > ctx.deadline => Err(TOO_SLOW.to_owned()),
                Ok((ir, _)) => Machine::new(&ctx, &inputs, row.id).run(ir),
            };
            match result {
                Ok(value) => {
                    match value.to_json() {
                        Some(json) => row.values.insert(key, json),
                        None => row.values.remove(&key),
                    };
                    inputs[compiled.index] = Ok(value);
                }
                Err(message) => {
                    row.values.insert(key, error_value(&message));
                    inputs[compiled.index] = Err(());
                }
            }
        }
    }
}

/// `POST …/formula/preview`: the result type of `expression` as the formula of `property_id`
/// (a new property when `None`), or its error, and its values for `rows`.
#[derive(Clone, Debug, Serialize, ToSchema)]
#[schema(as = DatabaseFormulaPreview)]
pub struct FormulaPreview {
    /// `number`, `text`, `boolean`, `date`, `person`, `page` or `list<…>`; absent with `error`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<FormulaProblem>,
    pub samples: Vec<FormulaSample>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[schema(as = DatabaseFormulaSample)]
pub struct FormulaSample {
    #[schema(value_type = String)]
    pub row_id: Id,
    /// The cell value as `GET` returns it; absent when empty.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<Object>)]
    pub value: Option<Value>,
}

/// Previews `expression` on `rows` (rows with their computed values filled).
pub fn preview(
    properties: &[Property],
    property_id: Option<Id>,
    expression: &str,
    rows: &mut [DatabaseRow],
    scope: &FillScope<'_>,
) -> FormulaPreview {
    let mut properties = properties.to_vec();
    let kind = PropertyKind::Formula(FormulaConfig {
        expression: expression.to_owned(),
        result_type: FormulaType::Unknown,
    });
    let index = match property_id.and_then(|id| properties.iter().position(|p| p.id == id)) {
        Some(at) => {
            properties[at].kind = kind;
            at
        }
        None => {
            properties.push(Property {
                id: Id::new_v7(),
                name: String::new(),
                kind,
                position: i64::MAX,
            });
            properties.len() - 1
        }
    };
    let config = match prepare(&properties, index) {
        Ok(config) => config,
        Err(error) => {
            return FormulaPreview {
                result_type: None,
                error: Some(error.problem(expression)),
                samples: Vec::new(),
            };
        }
    };
    let result_type = config.result_type.name();
    properties[index].kind = PropertyKind::Formula(config);
    fill(&properties, rows, scope);
    let key = properties[index].id.to_string();
    FormulaPreview {
        result_type: Some(result_type),
        error: None,
        samples: rows
            .iter()
            .map(|row| FormulaSample {
                row_id: row.id,
                value: row.values.get(&key).cloned(),
            })
            .collect(),
    }
}

/// `candidate` (a formula property, its expression as the client sent it) checked against the
/// database's `others`: stored with ids in `prop("…")` and its result type.
pub fn checked(others: &[Property], candidate: Property) -> Result<Property, DatabaseError> {
    let PropertyKind::Formula(config) = &candidate.kind else {
        return Ok(candidate);
    };
    let source = config.expression.clone();
    let mut properties = others.to_vec();
    properties.push(candidate);
    let index = properties.len() - 1;
    let formulas = properties
        .iter()
        .filter(|property| expression(property).is_some())
        .count();
    let prepared = if formulas > MAX_FORMULAS {
        Err(FormulaError::new(
            format!("A database holds at most {MAX_FORMULAS} formulas."),
            Span::new(0, 0),
        ))
    } else {
        prepare(&properties, index)
    };
    let config = prepared.map_err(|error| DatabaseError::FormulaInvalid(error.problem(&source)))?;
    let mut property = properties.swap_remove(index);
    property.kind = PropertyKind::Formula(config);
    Ok(property)
}

/// Workspace members by user id, for `name()` and `email()`.
async fn members(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
) -> Result<HashMap<Id, Member>, DatabaseError> {
    let rows = sqlx::query(
        "SELECT users.id AS id, users.display_name AS name, users.email AS email FROM memberships \
         JOIN users ON users.id = memberships.user_id WHERE memberships.workspace_id = $1",
    )
    .bind(workspace_id.to_string())
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let id = row.get::<String, _>("id").parse::<Id>().ok()?;
            Some((
                id,
                Member {
                    name: row.get("name"),
                    email: row.get("email"),
                },
            ))
        })
        .collect())
}

/// The formula pass of the computed-value layer: every formula value of `rows`, after the
/// relation and rollup passes.
pub(super) async fn fill_rows(
    tx: &mut Transaction<'_, Any>,
    scope: &ComputeScope,
    properties: &[Property],
    rows: &mut [DatabaseRow],
    pages: &BTreeMap<String, RelatedPage>,
) -> Result<(), DatabaseError> {
    if rows.is_empty() || !has_formulas(properties) {
        return Ok(());
    }
    let people = members(tx, scope.workspace_id).await?;
    fill(
        properties,
        rows,
        &FillScope {
            tz: &scope.tz,
            people: &people,
            pages,
        },
    );
    Ok(())
}

/// [`preview`] for a database read in `tx`: `rows` are its first rows with computed values.
pub(super) async fn preview_rows(
    tx: &mut Transaction<'_, Any>,
    scope: &ComputeScope,
    properties: &[Property],
    property_id: Option<Id>,
    expression: &str,
    rows: &mut [DatabaseRow],
    pages: &BTreeMap<String, RelatedPage>,
) -> Result<FormulaPreview, DatabaseError> {
    let people = members(tx, scope.workspace_id).await?;
    Ok(preview(
        properties,
        property_id,
        expression,
        rows,
        &FillScope {
            tz: &scope.tz,
            people: &people,
            pages,
        },
    ))
}

/// Stores the new result type of each formula whose type followed a change of the properties
/// it reads (a type change or a deletion).
pub(super) async fn refresh_types(
    tx: &mut Transaction<'_, Any>,
    properties: &mut [Property],
    now: orbit_platform::TimestampMillis,
) -> Result<(), DatabaseError> {
    for (at, config) in retyped(properties) {
        properties[at].kind = PropertyKind::Formula(config);
        sqlx::query(
            "UPDATE database_properties SET config_json = $1, updated_at = $2 WHERE id = $3",
        )
        .bind(properties[at].kind.config_json().to_string())
        .bind(now.as_millis())
        .bind(properties[at].id.to_string())
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

impl FormulaType {
    /// The stored type whose filters, sorts and calculations fit this result: a list, person or
    /// page result only filters on empty.
    const fn filter_type(&self) -> PropertyType {
        match self {
            Self::Number => PropertyType::Number,
            Self::Text | Self::Unknown => PropertyType::Text,
            Self::Boolean => PropertyType::Checkbox,
            Self::Date => PropertyType::Date,
            Self::Person | Self::Page | Self::List(_) => PropertyType::Formula,
        }
    }
}

impl PropertyKind {
    /// The type that decides a property's filter operators and calculations: a formula's
    /// follows its result type.
    #[must_use]
    pub fn filter_type(&self) -> PropertyType {
        match self {
            Self::Formula(config) => config.result_type.filter_type(),
            other => other.property_type(),
        }
    }
}

impl super::DatabaseRepository {
    /// `expression` as the formula of `property_id` (a new property when `None`): its result
    /// type or error, and its values on the first [`PREVIEW_ROWS`] rows. Writes nothing.
    pub async fn preview_formula(
        &self,
        scope: &ComputeScope,
        database_id: Id,
        property_id: Option<Id>,
        expression: &str,
    ) -> Result<FormulaPreview, DatabaseError> {
        let mut tx = self.database.transaction().await?;
        super::open(&mut tx, scope.workspace_id, database_id, scope.actor_id).await?;
        let properties = super::properties_in_tx(&mut tx, database_id).await?;
        if property_id.is_some_and(|id| properties.iter().all(|property| property.id != id)) {
            return Err(DatabaseError::PropertyNotFound);
        }
        let mut rows = super::rows_in_tx(&mut tx, database_id, None).await?;
        rows.truncate(PREVIEW_ROWS);
        let computed =
            super::computed::fill(&mut tx, scope, database_id, &properties, &mut rows).await?;
        let preview = preview_rows(
            &mut tx,
            scope,
            &properties,
            property_id,
            expression,
            &mut rows,
            &computed.related_pages,
        )
        .await?;
        tx.rollback().await?;
        Ok(preview)
    }
}
