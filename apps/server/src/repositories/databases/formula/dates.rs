//! Date functions in the reader's time zone. A day without time has no zone: its parts are the
//! day itself; it meets a moment at midnight of the reader's zone.

use jiff::civil::{Date, DateTime, Time};
use jiff::tz::TimeZone;
use jiff::{Span, Timestamp, Unit, Zoned};

use super::check::Func;
use super::value::{DateVal, Point};

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const WEEKDAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];
/// `dateAdd` moves a date by at most this many units.
const MAX_SHIFT: f64 = 10_000_000.0;

pub fn zoned(tz: &TimeZone, point: Point) -> Result<Zoned, String> {
    match point {
        Point::Day(day) => day.to_zoned(tz.clone()).map_err(|_| out_of_range()),
        Point::At(at) => Ok(at.to_zoned(tz.clone())),
    }
}

pub fn instant(tz: &TimeZone, point: Point) -> Timestamp {
    zoned(tz, point).map_or(Timestamp::MIN, |zoned| zoned.timestamp())
}

fn civil(tz: &TimeZone, point: Point) -> DateTime {
    match point {
        Point::Day(day) => day.to_datetime(Time::midnight()),
        Point::At(at) => at.to_zoned(tz.clone()).datetime(),
    }
}

fn out_of_range() -> String {
    "The date is out of range.".to_owned()
}

pub fn today(tz: &TimeZone, now: Timestamp) -> Point {
    Point::Day(now.to_zoned(tz.clone()).date())
}

/// `dateAdd(date, amount, unit)`; `dateSubtract` passes `-amount`.
pub fn add(tz: &TimeZone, date: DateVal, amount: f64, unit: &str) -> Result<DateVal, String> {
    if amount.abs() > MAX_SHIFT {
        return Err(out_of_range());
    }
    #[allow(clippy::cast_possible_truncation)]
    let amount = amount.trunc() as i64;
    let (unit, amount) = match unit {
        "quarters" => ("months", amount * 3),
        other => (other, amount),
    };
    let span = match unit {
        "years" => Span::new().try_years(amount),
        "months" => Span::new().try_months(amount),
        "weeks" => Span::new().try_weeks(amount),
        "days" => Span::new().try_days(amount),
        "hours" => Span::new().try_hours(amount),
        "minutes" => Span::new().try_minutes(amount),
        _ => return Err(unknown_unit(unit)),
    }
    .map_err(|_| out_of_range())?;
    let timed = matches!(unit, "hours" | "minutes");
    let shift = |point: Point| -> Result<Point, String> {
        match point {
            Point::Day(day) if !timed => day.checked_add(span).map(Point::Day),
            other => zoned(tz, other)?
                .checked_add(span)
                .map(|zoned| Point::At(zoned.timestamp())),
        }
        .map_err(|_| out_of_range())
    };
    Ok(DateVal {
        start: shift(date.start)?,
        end: date.end.map(shift).transpose()?,
    })
}

fn unknown_unit(unit: &str) -> String {
    format!(
        "Unknown unit \"{unit}\". Use one of: {}.",
        super::check::UNITS.join(", ")
    )
}

/// `dateBetween(a, b, unit)`: `a - b` in whole units, toward zero.
pub fn between(tz: &TimeZone, a: Point, b: Point, unit: &str) -> Result<f64, String> {
    let (jiff_unit, divide) = match unit {
        "years" => (Unit::Year, 1),
        "quarters" => (Unit::Month, 3),
        "months" => (Unit::Month, 1),
        "weeks" => (Unit::Week, 1),
        "days" => (Unit::Day, 1),
        "hours" => (Unit::Hour, 1),
        "minutes" => (Unit::Minute, 1),
        _ => return Err(unknown_unit(unit)),
    };
    let (a, b) = (zoned(tz, a)?, zoned(tz, b)?);
    let span = a.since((jiff_unit, &b)).map_err(|_| out_of_range())?;
    let whole = match jiff_unit {
        Unit::Year => i64::from(span.get_years()),
        Unit::Month => i64::from(span.get_months()),
        Unit::Week => i64::from(span.get_weeks()),
        Unit::Day => i64::from(span.get_days()),
        Unit::Hour => i64::from(span.get_hours()),
        _ => span.get_minutes(),
    };
    #[allow(clippy::cast_precision_loss)]
    Ok((whole / divide) as f64)
}

/// `dateRange(a, b)`: from the start of `a` to the start of `b`, in order. A day meets a
/// moment as the moment at its midnight.
pub fn range(tz: &TimeZone, a: Point, b: Point) -> Result<DateVal, String> {
    let (a, b) = match (a, b) {
        (Point::Day(_), Point::Day(_)) | (Point::At(_), Point::At(_)) => (a, b),
        _ => (
            Point::At(zoned(tz, a)?.timestamp()),
            Point::At(zoned(tz, b)?.timestamp()),
        ),
    };
    let ordered = instant(tz, a) <= instant(tz, b);
    let (start, end) = if ordered { (a, b) } else { (b, a) };
    Ok(DateVal {
        start,
        end: Some(end),
    })
}

/// `year()`, `month()` (1 to 12), `date()` (day of the month), `day()` (1 Monday to 7 Sunday),
/// `hour()`, `minute()`, `week()` (ISO week) and `timestamp()` (milliseconds).
pub fn part(tz: &TimeZone, point: Point, func: Func) -> f64 {
    let time = civil(tz, point);
    let value = match func {
        Func::Year => i64::from(time.year()),
        Func::Month => i64::from(time.month()),
        Func::Date => i64::from(time.day()),
        Func::Day => i64::from(time.date().weekday().to_monday_one_offset()),
        Func::Hour => i64::from(time.hour()),
        Func::Minute => i64::from(time.minute()),
        Func::Week => i64::from(time.date().iso_week_date().week()),
        _ => instant(tz, point).as_millisecond(),
    };
    #[allow(clippy::cast_precision_loss)]
    let value = value as f64;
    value
}

pub fn from_timestamp(millis: f64) -> Result<Point, String> {
    if !millis.is_finite() {
        return Err(out_of_range());
    }
    #[allow(clippy::cast_possible_truncation)]
    Timestamp::from_millisecond(millis.trunc() as i64)
        .map(Point::At)
        .map_err(|_| out_of_range())
}

/// `parseDate`: `2026-01-31`, `2026-01-31T14:30[:00]` (in the reader's zone), or a moment with
/// `Z` or an offset.
pub fn parse(tz: &TimeZone, text: &str) -> Result<Point, String> {
    let text = text.trim();
    if let Ok(day) = text.parse::<Date>()
        && !text.contains(['T', ' '])
    {
        return Ok(Point::Day(day));
    }
    if let Ok(at) = text.parse::<Timestamp>() {
        return Ok(Point::At(at));
    }
    if let Ok(time) = text.parse::<DateTime>()
        && let Ok(zoned) = time.to_zoned(tz.clone())
    {
        return Ok(Point::At(zoned.timestamp()));
    }
    Err(format!(
        "parseDate() cannot read \"{}\". Use a date like 2026-01-31 or 2026-01-31T14:30.",
        text.chars().take(40).collect::<String>()
    ))
}

/// `formatDate(date, pattern)` with the tokens of spec 15.6; text in `[…]` stays as is.
pub fn format(tz: &TimeZone, point: Point, pattern: &str) -> String {
    const TOKENS: [&str; 16] = [
        "YYYY", "YY", "MMMM", "MMM", "MM", "M", "DD", "D", "dddd", "ddd", "HH", "H", "hh", "h",
        "mm", "A",
    ];
    let time = civil(tz, point);
    let month = MONTHS[usize::try_from(time.month() - 1).unwrap_or(0)];
    let weekday =
        WEEKDAYS[usize::try_from(time.date().weekday().to_monday_zero_offset()).unwrap_or(0)];
    let twelve = match time.hour() % 12 {
        0 => 12,
        hour => hour,
    };
    let mut out = String::new();
    let mut rest = pattern;
    while !rest.is_empty() {
        if let Some(literal) = rest.strip_prefix('[') {
            let (inside, after) = literal.split_once(']').unwrap_or((literal, ""));
            out.push_str(inside);
            rest = after;
            continue;
        }
        let Some(token) = TOKENS.iter().find(|token| rest.starts_with(**token)) else {
            let next = rest.chars().next().unwrap_or_default();
            out.push(next);
            rest = &rest[next.len_utf8()..];
            continue;
        };
        let text = match *token {
            "YYYY" => format!("{:04}", time.year()),
            "YY" => format!("{:02}", time.year().rem_euclid(100)),
            "MMMM" => month.to_owned(),
            "MMM" => month[..3].to_owned(),
            "MM" => format!("{:02}", time.month()),
            "M" => time.month().to_string(),
            "DD" => format!("{:02}", time.day()),
            "D" => time.day().to_string(),
            "dddd" => weekday.to_owned(),
            "ddd" => weekday[..3].to_owned(),
            "HH" => format!("{:02}", time.hour()),
            "H" => time.hour().to_string(),
            "hh" => format!("{twelve:02}"),
            "h" => twelve.to_string(),
            "mm" => format!("{:02}", time.minute()),
            _ => if time.hour() < 12 { "AM" } else { "PM" }.to_owned(),
        };
        out.push_str(&text);
        rest = &rest[token.len()..];
    }
    out
}
