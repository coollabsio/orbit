//! When the next occurrence of a recurring task is due. Pure: the rule is parsed already, and all
//! calendar math happens in the rule's time zone.

use jiff::civil::{Date, Weekday};
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = TaskRecurrenceFrequency)]
pub enum Frequency {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = TaskRecurrenceBase)]
pub enum RecurrenceBase {
    /// A fixed calendar counted from the anchor; missed dates are skipped.
    Due,
    /// `interval` units after the day the occurrence was completed.
    Completion,
}

/// The weekdays of a weekly rule: Monday is bit 0, Sunday bit 6. Empty means the anchor's weekday.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WeekdaySet(u8);

impl WeekdaySet {
    #[must_use]
    pub fn from_bits(bits: i64) -> Self {
        Self((bits & 0x7f) as u8)
    }

    #[must_use]
    pub fn bits(self) -> i64 {
        i64::from(self.0)
    }

    #[must_use]
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    #[must_use]
    pub fn contains(self, day: Weekday) -> bool {
        self.0 & Self::bit(day) != 0
    }

    #[must_use]
    pub fn with(self, day: Weekday) -> Self {
        Self(self.0 | Self::bit(day))
    }

    fn bit(day: Weekday) -> u8 {
        1 << day.to_monday_zero_offset()
    }
}

#[derive(Clone, Debug)]
pub struct Schedule {
    pub frequency: Frequency,
    /// 1..=365, checked when the rule is set.
    pub interval: u16,
    /// Weekly rules on a fixed calendar only.
    pub weekdays: WeekdaySet,
    pub base: RecurrenceBase,
    /// The due date the rule was set from: day 0 of the fixed calendar, and the time of day when
    /// the occurrence has no due date.
    pub anchor: Timestamp,
    pub timezone: TimeZone,
}

/// The due date of the occurrence after one due at `prev_due` and completed at `completed_at`.
/// Keeps the local time of day of `prev_due` (or of the anchor). On a fixed calendar the result
/// is the first date after both the previous due date and the completion, so a late completion
/// never creates an overdue task. `None` only past the end of jiff's calendar (year 9999).
#[must_use]
pub fn next_due(
    rule: &Schedule,
    prev_due: Option<Timestamp>,
    completed_at: Timestamp,
) -> Option<Timestamp> {
    let local = |at: Timestamp| at.to_zoned(rule.timezone.clone());
    let time = local(prev_due.unwrap_or(rule.anchor)).time();
    let completed = local(completed_at).date();
    let interval = i64::from(rule.interval.max(1));
    let date = match rule.base {
        RecurrenceBase::Completion => match rule.frequency {
            Frequency::Daily => completed.checked_add(interval.days()).ok()?,
            Frequency::Weekly => completed.checked_add((interval * 7).days()).ok()?,
            Frequency::Monthly => completed.checked_add(interval.months()).ok()?,
            Frequency::Yearly => completed.checked_add(interval.years()).ok()?,
        },
        RecurrenceBase::Due => {
            let previous = local(prev_due.unwrap_or(completed_at)).date();
            let after = previous.max(completed);
            let anchor = local(rule.anchor).date();
            match rule.frequency {
                Frequency::Daily => every_days(anchor, interval, after)?,
                Frequency::Weekly if rule.weekdays.is_empty() => {
                    every_days(anchor, interval * 7, after)?
                }
                Frequency::Weekly => on_weekdays(anchor, interval, rule.weekdays, after)?,
                Frequency::Monthly => every_months(anchor, interval, after)?,
                Frequency::Yearly => every_months(anchor, interval * 12, after)?,
            }
        }
    };
    date.to_datetime(time)
        .to_zoned(rule.timezone.clone())
        .ok()
        .map(|zoned| zoned.timestamp())
}

fn days_between(from: Date, to: Date) -> i64 {
    i64::from(to.since(from).map_or(0, |span| span.get_days()))
}

/// The first `anchor + k * step` days (k >= 0) after `after`.
fn every_days(anchor: Date, step: i64, after: Date) -> Option<Date> {
    let elapsed = days_between(anchor, after);
    if elapsed < 0 {
        return Some(anchor);
    }
    anchor
        .checked_add((((elapsed / step) + 1) * step).days())
        .ok()
}

/// The first date after `after` (and not before the anchor) that falls on one of `weekdays` in
/// a week whose distance from the anchor's week is a multiple of `interval`.
fn on_weekdays(anchor: Date, interval: i64, weekdays: WeekdaySet, after: Date) -> Option<Date> {
    let anchor_monday = anchor
        .checked_sub(i64::from(anchor.weekday().to_monday_zero_offset()).days())
        .ok()?;
    let mut day = after.tomorrow().ok()?.max(anchor);
    for _ in 0..(7 * (interval + 1)) {
        let week = days_between(anchor_monday, day).div_euclid(7);
        if week % interval == 0 && weekdays.contains(day.weekday()) {
            return Some(day);
        }
        day = day.tomorrow().ok()?;
    }
    None
}

/// The first `anchor + k * step` months (k >= 0) after `after`, each on the anchor's day of the
/// month or the month's last day, whichever comes first (the 31st never drifts to the 28th).
fn every_months(anchor: Date, step: i64, after: Date) -> Option<Date> {
    let month_index = |date: Date| i64::from(date.year()) * 12 + i64::from(date.month()) - 1;
    let occurrence = |k: i64| -> Option<Date> {
        let index = month_index(anchor) + k * step;
        let year = i16::try_from(index.div_euclid(12)).ok()?;
        let month = i8::try_from(index.rem_euclid(12) + 1).ok()?;
        let first = Date::new(year, month, 1).ok()?;
        first
            .with()
            .day(anchor.day().min(first.days_in_month()))
            .build()
            .ok()
    };
    let start = ((month_index(after) - month_index(anchor)) / step - 1).max(0);
    (start..start + 3)
        .filter_map(occurrence)
        .find(|date| *date > after)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(value: &str) -> Timestamp {
        value.parse().unwrap()
    }

    fn rule(frequency: Frequency, interval: u16, base: RecurrenceBase, anchor: &str) -> Schedule {
        Schedule {
            frequency,
            interval,
            weekdays: WeekdaySet::default(),
            base,
            anchor: at(anchor),
            timezone: TimeZone::UTC,
        }
    }

    fn next(rule: &Schedule, prev_due: Option<&str>, completed: &str) -> String {
        next_due(rule, prev_due.map(at), at(completed))
            .unwrap()
            .to_string()
    }

    #[test]
    fn daily_completed_on_time() {
        let daily = rule(
            Frequency::Daily,
            1,
            RecurrenceBase::Due,
            "2026-11-02T09:00:00Z",
        );
        assert_eq!(
            next(&daily, Some("2026-11-02T09:00:00Z"), "2026-11-02T08:00:00Z"),
            "2026-11-03T09:00:00Z"
        );
    }

    #[test]
    fn late_completion_skips_missed_dates() {
        let every_two_days = rule(
            Frequency::Daily,
            2,
            RecurrenceBase::Due,
            "2026-11-02T09:00:00Z",
        );
        // due Nov 2, done Nov 7: Nov 4 and 6 are skipped, Nov 8 is the next date of the calendar
        assert_eq!(
            next(
                &every_two_days,
                Some("2026-11-02T09:00:00Z"),
                "2026-11-07T18:00:00Z"
            ),
            "2026-11-08T09:00:00Z"
        );
    }

    #[test]
    fn early_completion_moves_past_the_due_date() {
        let weekly = rule(
            Frequency::Weekly,
            1,
            RecurrenceBase::Due,
            "2026-11-06T17:00:00Z",
        );
        assert_eq!(
            next(
                &weekly,
                Some("2026-11-06T17:00:00Z"),
                "2026-11-04T10:00:00Z"
            ),
            "2026-11-13T17:00:00Z"
        );
    }

    #[test]
    fn weekly_on_monday_and_wednesday() {
        let mut weekly = rule(
            Frequency::Weekly,
            1,
            RecurrenceBase::Due,
            "2026-11-02T09:00:00Z",
        );
        weekly.weekdays = WeekdaySet::default()
            .with(Weekday::Monday)
            .with(Weekday::Wednesday);
        assert_eq!(
            next(
                &weekly,
                Some("2026-11-02T09:00:00Z"),
                "2026-11-02T10:00:00Z"
            ),
            "2026-11-04T09:00:00Z"
        );
        assert_eq!(
            next(
                &weekly,
                Some("2026-11-04T09:00:00Z"),
                "2026-11-04T10:00:00Z"
            ),
            "2026-11-09T09:00:00Z"
        );
    }

    #[test]
    fn every_two_weeks_on_monday_and_wednesday() {
        let mut fortnightly = rule(
            Frequency::Weekly,
            2,
            RecurrenceBase::Due,
            "2026-11-02T09:00:00Z",
        );
        fortnightly.weekdays = WeekdaySet::default()
            .with(Weekday::Monday)
            .with(Weekday::Wednesday);
        assert_eq!(
            next(
                &fortnightly,
                Some("2026-11-04T09:00:00Z"),
                "2026-11-04T10:00:00Z"
            ),
            "2026-11-16T09:00:00Z"
        );
    }

    #[test]
    fn monthly_on_the_31st_clamps_without_drifting() {
        let monthly = rule(
            Frequency::Monthly,
            1,
            RecurrenceBase::Due,
            "2026-01-31T12:00:00Z",
        );
        assert_eq!(
            next(
                &monthly,
                Some("2026-01-31T12:00:00Z"),
                "2026-01-31T13:00:00Z"
            ),
            "2026-02-28T12:00:00Z"
        );
        assert_eq!(
            next(
                &monthly,
                Some("2026-02-28T12:00:00Z"),
                "2026-02-28T13:00:00Z"
            ),
            "2026-03-31T12:00:00Z"
        );
    }

    #[test]
    fn every_two_months_and_leap_day_yearly() {
        let bimonthly = rule(
            Frequency::Monthly,
            2,
            RecurrenceBase::Due,
            "2026-01-15T12:00:00Z",
        );
        assert_eq!(
            next(
                &bimonthly,
                Some("2026-01-15T12:00:00Z"),
                "2026-04-20T12:00:00Z"
            ),
            "2026-05-15T12:00:00Z"
        );
        let yearly = rule(
            Frequency::Yearly,
            1,
            RecurrenceBase::Due,
            "2028-02-29T12:00:00Z",
        );
        assert_eq!(
            next(
                &yearly,
                Some("2028-02-29T12:00:00Z"),
                "2028-02-29T13:00:00Z"
            ),
            "2029-02-28T12:00:00Z"
        );
    }

    #[test]
    fn completion_base_counts_from_the_completion_day() {
        let mut after_done = rule(
            Frequency::Daily,
            3,
            RecurrenceBase::Completion,
            "2026-11-02T09:00:00Z",
        );
        after_done.weekdays = WeekdaySet::default().with(Weekday::Monday);
        assert_eq!(
            next(
                &after_done,
                Some("2026-11-02T09:00:00Z"),
                "2026-11-10T20:00:00Z"
            ),
            "2026-11-13T09:00:00Z"
        );
        let monthly = rule(
            Frequency::Monthly,
            1,
            RecurrenceBase::Completion,
            "2026-01-01T08:00:00Z",
        );
        assert_eq!(
            next(&monthly, None, "2026-01-31T20:00:00Z"),
            "2026-02-28T08:00:00Z"
        );
    }

    #[test]
    fn without_a_due_date_the_completion_is_the_previous_date() {
        let daily = rule(
            Frequency::Daily,
            1,
            RecurrenceBase::Due,
            "2026-11-02T07:30:00Z",
        );
        assert_eq!(
            next(&daily, None, "2026-11-05T15:00:00Z"),
            "2026-11-06T07:30:00Z"
        );
    }

    #[test]
    fn keeps_the_local_time_across_daylight_saving() {
        let mut weekly = rule(
            Frequency::Weekly,
            1,
            RecurrenceBase::Due,
            "2026-10-19T07:00:00Z",
        );
        weekly.timezone = TimeZone::get("Europe/Budapest").unwrap();
        // 09:00 CEST on Oct 19; Budapest leaves summer time on Oct 25, so 09:00 CET is 08:00 UTC
        assert_eq!(
            next(
                &weekly,
                Some("2026-10-19T07:00:00Z"),
                "2026-10-19T10:00:00Z"
            ),
            "2026-10-26T08:00:00Z"
        );
    }
}
