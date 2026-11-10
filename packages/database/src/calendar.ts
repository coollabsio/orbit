import { addDays, offsetMs, pointDay } from './dates'
import type { DatePoint, DateProperty, DateValue, Property, Row, ViewContext } from './types'
import { cellOf } from './values'

const DAY_MS = 86_400_000

/** Cards a month cell shows before "+N more" (spec P3 13.2). */
export const MAX_DAY_CARDS = 3

/** A day of the week: 0 is Sunday, 6 is Saturday. */
export type Weekday = 0 | 1 | 2 | 3 | 4 | 5 | 6

/** As the server's `calendar::datable`: a `date`, `created_time` or `last_edited_time` property. */
export function datable(property: Property | undefined): property is DateProperty {
  return property?.type === 'date' || property?.type === 'created_time' || property?.type === 'last_edited_time'
}

/** The property a calendar without one places its rows by: the first `date` property. */
export function defaultDateProperty(properties: readonly Property[]): string | null {
  return properties.find((property) => property.type === 'date')?.id ?? null
}

/** The first day of the week in `locale` (the runtime's when absent); Monday when the runtime cannot tell. */
export function localeWeekStart(locale?: string): Weekday {
  try {
    const resolved = new Intl.Locale(locale ?? new Intl.DateTimeFormat().resolvedOptions().locale) as Intl.Locale & {
      getWeekInfo?: () => { firstDay: number }
      weekInfo?: { firstDay: number }
    }
    const firstDay = (resolved.getWeekInfo?.() ?? resolved.weekInfo)?.firstDay
    if (firstDay !== undefined && firstDay >= 1 && firstDay <= 7) return (firstDay % 7) as Weekday
  } catch {
    // an unknown locale
  }
  return 1
}

/** The weekday of a `YYYY-MM-DD` day. */
export function weekdayOf(day: string): Weekday {
  return new Date(`${day}T00:00:00Z`).getUTCDay() as Weekday
}

/** Days from `from` to `to` (negative when `to` is earlier). */
export function dayDiff(from: string, to: string): number {
  return Math.round((Date.parse(`${to}T00:00:00Z`) - Date.parse(`${from}T00:00:00Z`)) / DAY_MS)
}

/** The 7 days of the week that holds `day`. */
export function weekOf(day: string, weekStart: Weekday): string[] {
  const first = addDays(day, -((weekdayOf(day) - weekStart + 7) % 7))
  return Array.from({ length: 7 }, (_, index) => addDays(first, index))
}

/** The 6 weeks a month grid shows: the week that holds the 1st of `day`'s month and the 5 after it. */
export function calendarWeeks(day: string, weekStart: Weekday): string[][] {
  const first = weekOf(`${day.slice(0, 7)}-01`, weekStart)[0]!
  return Array.from({ length: 6 }, (_, week) => weekOf(addDays(first, week * 7), weekStart))
}

/** The local days a row covers on the calendar, both included; `null` when it has no date. */
export type DaySpan = { start: string; end: string }

export function rowSpan(row: Row, property: DateProperty, timeZone: string): DaySpan | null {
  const value = cellOf(row, property)
  if (value?.kind !== 'date') return null
  const start = pointDay(value.date.start, timeZone)
  const end = value.date.end ? pointDay(value.date.end, timeZone) : start
  return { start, end: end < start ? start : end }
}

/** The rows a calendar cannot place: those with no date ("No date (N)"). */
export function undatedRows(rows: readonly Row[], property: DateProperty, timeZone: string): Row[] {
  return rows.filter((row) => rowSpan(row, property, timeZone) === null)
}

/**
 * A row in one week: from `column` (0 to 6) for `length` days, in `lane` (0 is the top). A range that starts before
 * or ends after the week continues there.
 */
export type CalendarBar = {
  row: Row
  span: DaySpan
  column: number
  length: number
  lane: number
  continuesBefore: boolean
  continuesAfter: boolean
}

/**
 * The rows that cover a day of `week`, as bars with lanes: earlier starts first, then longer ranges, then the view's
 * order; each bar takes the first lane free on all its days.
 */
export function placeRows(rows: readonly Row[], property: DateProperty, week: readonly string[], timeZone: string): CalendarBar[] {
  const first = week[0]!
  const last = week[week.length - 1]!
  const items = rows.flatMap((row, order) => {
    const span = rowSpan(row, property, timeZone)
    return span && span.start <= last && span.end >= first ? [{ row, span, order }] : []
  })
  items.sort((a, b) => (a.span.start < b.span.start ? -1 : a.span.start > b.span.start ? 1 : 0) || dayDiff(a.span.end, b.span.end) || a.order - b.order)
  /** The last column each lane holds. */
  const laneEnds: number[] = []
  return items.map(({ row, span }) => {
    const column = span.start < first ? 0 : dayDiff(first, span.start)
    const end = span.end > last ? week.length - 1 : dayDiff(first, span.end)
    let lane = laneEnds.findIndex((taken) => taken < column)
    if (lane === -1) lane = laneEnds.length
    laneEnds[lane] = end
    return { row, span, column, length: end - column + 1, lane, continuesBefore: span.start < first, continuesAfter: span.end > last }
  })
}

/** The bars that cover `column`, in lane order. */
export function barsOn(bars: readonly CalendarBar[], column: number): CalendarBar[] {
  return bars.filter((bar) => bar.column <= column && bar.column + bar.length > column).sort((a, b) => a.lane - b.lane)
}

/** A date on `day`, as "+" on a calendar day gives a new row. */
export function dayDate(day: string): DateValue {
  return { start: { kind: 'day', day }, end: null }
}

/** `value` moved by `days`: start and end move together, and a time keeps its local hour in `timeZone`. */
export function moveDate(value: DateValue, days: number, timeZone: string): DateValue {
  return { start: movePoint(value.start, days, timeZone), end: value.end && movePoint(value.end, days, timeZone) }
}

/**
 * `value` ending on `day` (dragging a range's right edge); a time keeps the end's (or the start's) local hour. `null`
 * when the end would come before the start.
 */
export function withEndDay(value: DateValue, day: string, timeZone: string): DateValue | null {
  const { start } = value
  if (start.kind === 'day') {
    if (day < start.day) return null
    return { start, end: day === start.day ? null : { kind: 'day', day } }
  }
  const from = value.end ?? start
  if (from.kind !== 'moment') return null
  const end = movePoint(from, dayDiff(pointDay(from, timeZone), day), timeZone)
  if (end.kind !== 'moment' || end.ms < start.ms) return null
  return { start, end: end.ms === start.ms ? null : end }
}

function movePoint(point: DatePoint, days: number, timeZone: string): DatePoint {
  if (point.kind === 'day') return { kind: 'day', day: addDays(point.day, days) }
  const local = point.ms + offsetMs(point.ms, timeZone) + days * DAY_MS
  const ms = local - offsetMs(local - offsetMs(local, timeZone), timeZone)
  return { kind: 'moment', iso: new Date(ms).toISOString().replace(/\.\d{3}Z$/, 'Z'), ms }
}

/** The local start time a card shows ("2:30 PM"), by the property's time format; `null` for a date without a time. */
export function startTime(row: Row, property: DateProperty, ctx: ViewContext): string | null {
  const value = cellOf(row, property)
  if (value?.kind !== 'date' || value.date.start.kind !== 'moment') return null
  const hour12 = property.type === 'date' ? property.config.time_format === '12h' : undefined
  return new Intl.DateTimeFormat(ctx.locale, { timeZone: ctx.timeZone, hour: 'numeric', minute: '2-digit', hour12 }).format(value.date.start.ms)
}

/** "November 2026": the month of `day`. */
export function monthTitle(day: string, locale?: string): string {
  return new Intl.DateTimeFormat(locale, { timeZone: 'UTC', month: 'long', year: 'numeric' }).format(Date.parse(`${day}T00:00:00Z`))
}

/** "Mon" (or "Monday"): the name of a day's weekday. */
export function weekdayName(day: string, locale?: string, width: 'short' | 'long' = 'short'): string {
  return new Intl.DateTimeFormat(locale, { timeZone: 'UTC', weekday: width }).format(Date.parse(`${day}T00:00:00Z`))
}
