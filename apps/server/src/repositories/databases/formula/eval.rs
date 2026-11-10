//! The evaluator. Every node costs a step, and list work costs steps by size, so one cell stops
//! after `MAX_STEPS` with the same result each time; a deadline per database read stops a slow
//! read as a whole.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::time::Instant;

use jiff::Timestamp;
use jiff::tz::TimeZone;
use orbit_platform::Id;

use super::check::{Func, Ir};
use super::syntax::BinaryOp;
use super::value::{DateVal, Point, Val, compare, point_text};
use super::{MAX_LIST_ITEMS, MAX_RESULT_CHARS, MAX_STEPS, dates};

/// A workspace member as `name()` and `email()` read them.
#[derive(Clone, Debug)]
pub struct Member {
    pub name: String,
    pub email: String,
}

/// What every cell of one read shares.
pub struct Context<'a> {
    pub tz: TimeZone,
    pub now: Timestamp,
    pub people: &'a HashMap<Id, Member>,
    pub deadline: Instant,
    /// Property names by index, for the message of a reference to a failed formula.
    pub names: &'a [String],
}

pub const TOO_SLOW: &str = "Formulas took too long for this database. Simplify them.";

/// One cell's run: the row's inputs (`Err` for a formula of this row that failed).
pub struct Machine<'a> {
    ctx: &'a Context<'a>,
    inputs: &'a [Result<Val, ()>],
    row_id: Id,
    locals: Vec<Val>,
    steps: u32,
}

type R = Result<Val, String>;

fn number(value: f64) -> R {
    if value.is_finite() {
        Ok(Val::Number(value))
    } else {
        Err("The result is not a finite number.".to_owned())
    }
}

fn text(value: String) -> R {
    if value.len() > MAX_RESULT_CHARS && value.chars().count() > MAX_RESULT_CHARS {
        return Err(too_long());
    }
    Ok(Val::Text(value))
}

fn too_long() -> String {
    "The text is longer than 10,000 characters.".to_owned()
}

fn list(items: Vec<Val>) -> R {
    if items.len() > MAX_LIST_ITEMS {
        return Err("A list in this formula has more than 1,000 items.".to_owned());
    }
    Ok(Val::List(items))
}

fn as_text(value: &Val) -> &str {
    match value {
        Val::Text(text) => text,
        _ => "",
    }
}

fn as_list(value: Val) -> Vec<Val> {
    match value {
        Val::List(items) => items,
        _ => Vec::new(),
    }
}

fn truthy(value: &Val) -> bool {
    matches!(value, Val::Bool(true))
}

/// An index argument as a whole number (toward zero).
#[allow(clippy::cast_possible_truncation)]
fn whole(value: &Val) -> Option<i64> {
    match value {
        Val::Number(number) if number.is_finite() => Some(number.trunc() as i64),
        _ => None,
    }
}

/// JavaScript `slice` bounds: a negative index counts from the end.
fn bounds(len: usize, start: Option<i64>, end: Option<i64>) -> (usize, usize) {
    let len = i64::try_from(len).unwrap_or(i64::MAX);
    let clamp = |at: i64| {
        let at = if at < 0 {
            (len + at).max(0)
        } else {
            at.min(len)
        };
        usize::try_from(at).unwrap_or(0)
    };
    let (start, end) = (clamp(start.unwrap_or(0)), clamp(end.unwrap_or(len)));
    (start, end.max(start))
}

impl<'a> Machine<'a> {
    pub fn new(ctx: &'a Context<'a>, inputs: &'a [Result<Val, ()>], row_id: Id) -> Self {
        Self {
            ctx,
            inputs,
            row_id,
            locals: Vec::new(),
            steps: 0,
        }
    }

    pub fn run(&mut self, ir: &Ir) -> R {
        self.eval(ir)
    }

    fn step(&mut self, cost: usize) -> Result<(), String> {
        let before = self.steps;
        self.steps = self
            .steps
            .saturating_add(u32::try_from(cost).unwrap_or(u32::MAX));
        if self.steps > MAX_STEPS {
            return Err("This formula needs more than 2,000 steps for this row.".to_owned());
        }
        if before / 64 != self.steps / 64 && Instant::now() > self.ctx.deadline {
            return Err(TOO_SLOW.to_owned());
        }
        Ok(())
    }

    fn instant(&self, point: Point) -> Timestamp {
        dates::instant(&self.ctx.tz, point)
    }

    fn same(&self, left: &Val, right: &Val) -> bool {
        match (left, right) {
            (Val::Date(a), Val::Date(b)) => {
                self.instant(a.start) == self.instant(b.start)
                    && a.end.map(|end| self.instant(end)) == b.end.map(|end| self.instant(end))
            }
            (Val::Page { id: a, .. }, Val::Page { id: b, .. }) => a == b,
            (Val::List(a), Val::List(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(a, b)| self.same(a, b))
            }
            _ => left == right,
        }
    }

    fn order(&self, left: &Val, right: &Val) -> Option<Ordering> {
        compare(left, right, |point| self.instant(point))
    }

    fn format(&self, value: &Val) -> String {
        match value {
            Val::Empty => String::new(),
            Val::Number(number) => number.to_string(),
            Val::Text(text) => text.clone(),
            Val::Bool(flag) => flag.to_string(),
            Val::Date(date) => match date.end {
                Some(end) => format!("{} → {}", point_text(date.start), point_text(end)),
                None => point_text(date.start),
            },
            Val::Person(id) => self
                .ctx
                .people
                .get(id)
                .map_or_else(String::new, |member| member.name.clone()),
            Val::Page { title, .. } => title.clone(),
            Val::List(items) => {
                let mut out = String::new();
                for (at, item) in items.iter().enumerate() {
                    if at > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(&self.format(item));
                    if out.len() > MAX_RESULT_CHARS * 4 {
                        break;
                    }
                }
                out
            }
        }
    }

    fn eval(&mut self, ir: &Ir) -> R {
        self.step(1)?;
        match ir {
            Ir::Const(value) => Ok(value.clone()),
            Ir::List(items) => {
                let mut values = Vec::with_capacity(items.len());
                for item in items {
                    values.push(self.eval(item)?);
                }
                list(values)
            }
            Ir::Prop(index) => self.inputs[*index].clone().map_err(|()| {
                format!(
                    "\"{}\" has an error.",
                    self.ctx.names.get(*index).map_or("", String::as_str)
                )
            }),
            Ir::Local(slot) => Ok(self.locals[*slot].clone()),
            Ir::Let(value, body) => {
                let value = self.eval(value)?;
                self.locals.push(value);
                let result = self.eval(body);
                self.locals.pop();
                result
            }
            Ir::If(test, then, other) => {
                if truthy(&self.eval(test)?) {
                    self.eval(then)
                } else {
                    self.eval(other)
                }
            }
            Ir::Negate(inner) => match self.eval(inner)? {
                Val::Number(value) => number(-value),
                _ => Ok(Val::Empty),
            },
            Ir::Not(inner) => Ok(Val::Bool(!truthy(&self.eval(inner)?))),
            Ir::Binary(BinaryOp::And, left, right) => Ok(Val::Bool(
                truthy(&self.eval(left)?) && truthy(&self.eval(right)?),
            )),
            Ir::Binary(BinaryOp::Or, left, right) => Ok(Val::Bool(
                truthy(&self.eval(left)?) || truthy(&self.eval(right)?),
            )),
            Ir::Binary(op, left, right) => {
                let left = self.eval(left)?;
                let right = self.eval(right)?;
                self.binary(*op, left, right)
            }
            Ir::Lambda(func, items, body) => {
                let items = as_list(self.eval(items)?);
                self.lambda(*func, items, body)
            }
            Ir::Call(func, args) => {
                let mut values = Vec::with_capacity(args.len());
                for arg in args {
                    values.push(self.eval(arg)?);
                }
                self.call(*func, values)
            }
        }
    }

    fn binary(&self, op: BinaryOp, left: Val, right: Val) -> R {
        let numbers = match (&left, &right) {
            (Val::Number(a), Val::Number(b)) => Some((*a, *b)),
            _ => None,
        };
        match op {
            BinaryOp::Equal => return Ok(Val::Bool(self.same(&left, &right))),
            BinaryOp::NotEqual => return Ok(Val::Bool(!self.same(&left, &right))),
            BinaryOp::Less | BinaryOp::LessEqual | BinaryOp::Greater | BinaryOp::GreaterEqual => {
                if left == Val::Empty || right == Val::Empty {
                    return Ok(Val::Bool(false));
                }
                let ordering = self.order(&left, &right);
                return Ok(Val::Bool(ordering.is_some_and(|ordering| match op {
                    BinaryOp::Less => ordering.is_lt(),
                    BinaryOp::LessEqual => ordering.is_le(),
                    BinaryOp::Greater => ordering.is_gt(),
                    _ => ordering.is_ge(),
                })));
            }
            BinaryOp::Add => {
                if let (Val::Text(_), _) | (_, Val::Text(_)) = (&left, &right) {
                    let mut joined = as_text(&left).to_owned();
                    joined.push_str(as_text(&right));
                    return text(joined);
                }
            }
            _ => {}
        }
        let Some((a, b)) = numbers else {
            return Ok(Val::Empty);
        };
        match op {
            BinaryOp::Add => number(a + b),
            BinaryOp::Subtract => number(a - b),
            BinaryOp::Multiply => number(a * b),
            BinaryOp::Divide | BinaryOp::Remainder if b == 0.0 => {
                Err("Division by zero.".to_owned())
            }
            BinaryOp::Divide => number(a / b),
            BinaryOp::Remainder => number(a % b),
            _ => number(a.powf(b)),
        }
    }

    fn lambda(&mut self, func: Func, items: Vec<Val>, body: &Ir) -> R {
        let mut kept = Vec::new();
        for (at, item) in items.into_iter().enumerate() {
            self.locals.push(item.clone());
            #[allow(clippy::cast_precision_loss)]
            self.locals.push(Val::Number(at as f64));
            let result = self.eval(body);
            self.locals.truncate(self.locals.len() - 2);
            let result = result?;
            match func {
                Func::Map => kept.push(result),
                _ if !truthy(&result) => {
                    if func == Func::Every {
                        return Ok(Val::Bool(false));
                    }
                }
                Func::Filter => kept.push(item),
                Func::Find => return Ok(item),
                #[allow(clippy::cast_precision_loss)]
                Func::FindIndex => return Ok(Val::Number(at as f64)),
                Func::Some => return Ok(Val::Bool(true)),
                _ => {}
            }
        }
        match func {
            Func::Map | Func::Filter => list(kept),
            Func::Find => Ok(Val::Empty),
            Func::FindIndex => Ok(Val::Number(-1.0)),
            Func::Some => Ok(Val::Bool(false)),
            _ => Ok(Val::Bool(true)),
        }
    }

    /// The numbers of `min`, `max`, `sum`, `mean` and `median`: number arguments and the items
    /// of number lists, without empty values.
    fn numbers(args: Vec<Val>) -> Vec<f64> {
        let mut out = Vec::new();
        for arg in args {
            match arg {
                Val::Number(number) => out.push(number),
                Val::List(items) => out.extend(items.into_iter().filter_map(|item| match item {
                    Val::Number(number) => Some(number),
                    _ => None,
                })),
                _ => {}
            }
        }
        out
    }

    fn date_arg(value: &Val) -> Option<DateVal> {
        match value {
            Val::Date(date) => Some(*date),
            _ => None,
        }
    }

    #[allow(clippy::too_many_lines)]
    fn call(&mut self, func: Func, mut args: Vec<Val>) -> R {
        let size: usize = args
            .iter()
            .map(|arg| match arg {
                Val::List(items) => items.len(),
                Val::Text(text) => text.len() / 64,
                _ => 0,
            })
            .sum();
        self.step(size / 4)?;
        let num = |at: usize| match args.get(at) {
            Some(Val::Number(number)) => Some(*number),
            _ => None,
        };
        let tz = self.ctx.tz.clone();
        match func {
            Func::Empty => Ok(Val::Bool(args[0].is_empty())),
            Func::Equal => Ok(Val::Bool(self.same(&args[0], &args[1]))),
            Func::Unequal => Ok(Val::Bool(!self.same(&args[0], &args[1]))),
            Func::Length => {
                #[allow(clippy::cast_precision_loss)]
                let length = match &args[0] {
                    Val::List(items) => items.len(),
                    other => as_text(other).chars().count(),
                } as f64;
                Ok(Val::Number(length))
            }
            Func::Substring => {
                let chars: Vec<char> = as_text(&args[0]).chars().collect();
                let start = whole(&args[1]).map(|at| at.max(0));
                let end = args.get(2).and_then(whole).map(|at| at.max(0));
                let (start, end) = bounds(chars.len(), start, end);
                Ok(Val::Text(chars[start..end].iter().collect()))
            }
            Func::Contains => Ok(Val::Bool(as_text(&args[0]).contains(as_text(&args[1])))),
            Func::Lower => Ok(Val::Text(as_text(&args[0]).to_lowercase())),
            Func::Upper => text(as_text(&args[0]).to_uppercase()),
            Func::Trim => Ok(Val::Text(as_text(&args[0]).trim().to_owned())),
            Func::Replace | Func::ReplaceAll => {
                let (source, find, with) =
                    (as_text(&args[0]), as_text(&args[1]), as_text(&args[2]));
                if find.is_empty() {
                    return Ok(args.swap_remove(0));
                }
                let count = if func == Func::Replace {
                    usize::from(source.contains(find))
                } else {
                    source.matches(find).count()
                };
                if source.len() + count * with.len().saturating_sub(find.len())
                    > MAX_RESULT_CHARS * 4
                {
                    return Err(too_long());
                }
                text(source.replacen(find, with, count))
            }
            Func::Repeat => {
                let source = as_text(&args[0]);
                let times = whole(&args[1]).unwrap_or(0).max(0);
                let times = usize::try_from(times).unwrap_or(usize::MAX);
                if source.chars().count().saturating_mul(times) > MAX_RESULT_CHARS {
                    return Err(too_long());
                }
                Ok(Val::Text(source.repeat(times)))
            }
            Func::Format => text(self.format(&args[0])),
            Func::Split => {
                let source = as_text(&args[0]);
                let separator = args.get(1).map_or("", as_text);
                let parts: Vec<Val> = if separator.is_empty() {
                    source
                        .chars()
                        .take(MAX_LIST_ITEMS + 1)
                        .map(|char| Val::Text(char.to_string()))
                        .collect()
                } else {
                    source
                        .split(separator)
                        .take(MAX_LIST_ITEMS + 1)
                        .map(|part| Val::Text(part.to_owned()))
                        .collect()
                };
                list(parts)
            }
            Func::Join => {
                let separator = args.get(1).map_or("", as_text).to_owned();
                let mut out = String::new();
                for (at, item) in as_list(args.swap_remove(0)).iter().enumerate() {
                    if at > 0 {
                        out.push_str(&separator);
                    }
                    out.push_str(&self.format(item));
                    if out.len() > MAX_RESULT_CHARS * 4 {
                        return Err(too_long());
                    }
                }
                text(out)
            }
            Func::Link => Ok(args.swap_remove(0)),
            Func::Add => self.binary(BinaryOp::Add, args[0].clone(), args[1].clone()),
            Func::Subtract => self.binary(BinaryOp::Subtract, args[0].clone(), args[1].clone()),
            Func::Multiply => self.binary(BinaryOp::Multiply, args[0].clone(), args[1].clone()),
            Func::Divide => self.binary(BinaryOp::Divide, args[0].clone(), args[1].clone()),
            Func::Mod => self.binary(BinaryOp::Remainder, args[0].clone(), args[1].clone()),
            Func::Pow => self.binary(BinaryOp::Power, args[0].clone(), args[1].clone()),
            Func::Abs | Func::Ceil | Func::Floor | Func::Sqrt | Func::Sign | Func::Round => {
                let Some(value) = num(0) else {
                    return Ok(Val::Empty);
                };
                number(match func {
                    Func::Abs => value.abs(),
                    Func::Ceil => value.ceil(),
                    Func::Floor => value.floor(),
                    Func::Sqrt => value.sqrt(),
                    Func::Sign if value == 0.0 => 0.0,
                    Func::Sign => value.signum(),
                    _ => {
                        #[allow(clippy::cast_possible_truncation)]
                        let places = num(1).unwrap_or(0.0).trunc().clamp(-15.0, 15.0) as i32;
                        let scale = 10f64.powi(places);
                        (value * scale).round() / scale
                    }
                })
            }
            Func::Min | Func::Max | Func::Sum | Func::Mean | Func::Median => {
                let mut values = Self::numbers(args);
                #[allow(clippy::cast_precision_loss)]
                let count = values.len() as f64;
                if values.is_empty() {
                    return Ok(if func == Func::Sum {
                        Val::Number(0.0)
                    } else {
                        Val::Empty
                    });
                }
                number(match func {
                    Func::Min => values.iter().copied().fold(f64::INFINITY, f64::min),
                    Func::Max => values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                    Func::Sum => values.iter().sum(),
                    Func::Mean => values.iter().sum::<f64>() / count,
                    _ => {
                        values.sort_by(f64::total_cmp);
                        let middle = values.len() / 2;
                        if values.len() % 2 == 0 {
                            f64::midpoint(values[middle - 1], values[middle])
                        } else {
                            values[middle]
                        }
                    }
                })
            }
            Func::ToNumber => Ok(match &args[0] {
                Val::Number(value) => Val::Number(*value),
                Val::Bool(flag) => Val::Number(if *flag { 1.0 } else { 0.0 }),
                Val::Text(value) => value
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .filter(|value| value.is_finite())
                    .map_or(Val::Empty, Val::Number),
                Val::Date(date) => Val::Number(dates::part(&tz, date.start, Func::Timestamp)),
                _ => Val::Empty,
            }),
            Func::Pi => Ok(Val::Number(std::f64::consts::PI)),
            Func::E => Ok(Val::Number(std::f64::consts::E)),
            Func::Now => Ok(Val::Date(DateVal::point(Point::At(self.ctx.now)))),
            Func::Today => Ok(Val::Date(DateVal::point(dates::today(&tz, self.ctx.now)))),
            Func::DateAdd | Func::DateSubtract => {
                let (Some(date), Some(amount)) = (Self::date_arg(&args[0]), num(1)) else {
                    return Ok(Val::Empty);
                };
                let amount = if func == Func::DateSubtract {
                    -amount
                } else {
                    amount
                };
                dates::add(&tz, date, amount, as_text(&args[2])).map(Val::Date)
            }
            Func::DateBetween => {
                let (Some(a), Some(b)) = (Self::date_arg(&args[0]), Self::date_arg(&args[1]))
                else {
                    return Ok(Val::Empty);
                };
                dates::between(&tz, a.start, b.start, as_text(&args[2])).map(Val::Number)
            }
            Func::DateStart | Func::DateEnd => {
                Ok(Self::date_arg(&args[0]).map_or(Val::Empty, |date| {
                    let point = match func {
                        Func::DateEnd => date.end.unwrap_or(date.start),
                        _ => date.start,
                    };
                    Val::Date(DateVal::point(point))
                }))
            }
            Func::DateRange => {
                let (Some(a), Some(b)) = (Self::date_arg(&args[0]), Self::date_arg(&args[1]))
                else {
                    return Ok(Val::Empty);
                };
                dates::range(&tz, a.start, b.start).map(Val::Date)
            }
            Func::FormatDate => match Self::date_arg(&args[0]) {
                Some(date) => text(dates::format(&tz, date.start, as_text(&args[1]))),
                None => Ok(Val::Text(String::new())),
            },
            Func::ParseDate => {
                dates::parse(&tz, as_text(&args[0])).map(|point| Val::Date(DateVal::point(point)))
            }
            Func::Year
            | Func::Month
            | Func::Date
            | Func::Day
            | Func::Hour
            | Func::Minute
            | Func::Week
            | Func::Timestamp => Ok(Self::date_arg(&args[0]).map_or(Val::Empty, |date| {
                Val::Number(dates::part(&tz, date.start, func))
            })),
            Func::FromTimestamp => match num(0) {
                Some(millis) => {
                    dates::from_timestamp(millis).map(|point| Val::Date(DateVal::point(point)))
                }
                None => Ok(Val::Empty),
            },
            Func::Name | Func::Email => {
                let read = |value: &Val| match value {
                    Val::Person(id) => {
                        Val::Text(self.ctx.people.get(id).map_or_else(String::new, |member| {
                            if func == Func::Name {
                                member.name.clone()
                            } else {
                                member.email.clone()
                            }
                        }))
                    }
                    _ => Val::Text(String::new()),
                };
                Ok(match &args[0] {
                    Val::List(items) => Val::List(items.iter().map(read).collect()),
                    other => read(other),
                })
            }
            Func::Id => Ok(Val::Text(match args.first() {
                Some(Val::Page { id, .. }) => id.to_string(),
                Some(_) => String::new(),
                None => self.row_id.to_string(),
            })),
            Func::At => {
                let items = as_list(args.swap_remove(0));
                let len = items.len();
                let at = whole(&args[0]).unwrap_or(0);
                let index = if at < 0 {
                    i64::try_from(len).unwrap_or(i64::MAX) + at
                } else {
                    at
                };
                usize::try_from(index)
                    .ok()
                    .and_then(|index| items.into_iter().nth(index))
                    .ok_or_else(|| {
                        format!("at(): index {at} is out of range (the list has {len} items).")
                    })
            }
            Func::First => Ok(as_list(args.swap_remove(0))
                .into_iter()
                .next()
                .unwrap_or(Val::Empty)),
            Func::Last => Ok(as_list(args.swap_remove(0)).pop().unwrap_or(Val::Empty)),
            Func::Slice => {
                let start = args.get(1).and_then(whole);
                let end = args.get(2).and_then(whole);
                let mut items = as_list(args.swap_remove(0));
                let (start, end) = bounds(items.len(), start, end);
                items.truncate(end);
                Ok(Val::List(items.split_off(start)))
            }
            Func::Concat => {
                let mut out = Vec::new();
                for arg in args {
                    out.extend(as_list(arg));
                    if out.len() > MAX_LIST_ITEMS {
                        return list(out);
                    }
                }
                list(out)
            }
            Func::Sort => {
                let mut items = as_list(args.swap_remove(0));
                items.sort_by(|a, b| self.order(a, b).unwrap_or(Ordering::Equal));
                Ok(Val::List(items))
            }
            Func::Reverse => {
                let mut items = as_list(args.swap_remove(0));
                items.reverse();
                Ok(Val::List(items))
            }
            Func::Unique => {
                let mut kept: Vec<Val> = Vec::new();
                for item in as_list(args.swap_remove(0)) {
                    if !kept.iter().any(|seen| self.same(seen, &item)) {
                        kept.push(item);
                    }
                }
                Ok(Val::List(kept))
            }
            Func::Includes => Ok(Val::Bool(
                as_list(args[0].clone())
                    .iter()
                    .any(|item| self.same(item, &args[1])),
            )),
            Func::Flat => {
                let mut out = Vec::new();
                for item in as_list(args.swap_remove(0)) {
                    match item {
                        Val::List(inner) => out.extend(inner),
                        other => out.push(other),
                    }
                    if out.len() > MAX_LIST_ITEMS {
                        return list(out);
                    }
                }
                list(out)
            }
            Func::Find | Func::FindIndex | Func::Filter | Func::Some | Func::Every | Func::Map => {
                Err("This function needs a test.".to_owned())
            }
        }
    }
}
