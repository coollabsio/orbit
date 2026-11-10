import { addDays, offsetMs, pointDay } from './dates'
import type { DatabaseView, DatabaseViewConfig, DatePoint, Property, Row, ViewConfig } from './types'
import { apiValue, cellOf } from './values'

const DAY_MS = 86_400_000

export const ZOOMS = ['day', 'week', 'month', 'quarter', 'year'] as const
export type TimelineZoom = (typeof ZOOMS)[number]

export const ZOOM_LABELS: Record<TimelineZoom, string> = { day: 'Day', week: 'Week', month: 'Month', quarter: 'Quarter', year: 'Year' }

/** Pixels per day at each zoom. */
export const DAY_WIDTH: Record<TimelineZoom, number> = { day: 96, week: 36, month: 12, quarter: 4, year: 1.5 }

/** How far a timeline reaches past its bars and today, and the most it reaches from today. */
const PAD_DAYS: Record<TimelineZoom, number> = { day: 14, week: 42, month: 120, quarter: 365, year: 730 }
const MAX_REACH_DAYS = 5 * 366

/** A date property: the only type whose value a bar drag can write. */
export type TimelineDateProperty = Property & { type: 'date' }

/** A timeline view's fields with defaults filled in and properties resolved (spec P3 16.2). */
export type Timeline = {
  /** `null`: the database has no date property yet. */
  start: TimelineDateProperty | null
  end: TimelineDateProperty | null
  zoom: TimelineZoom
  showTable: boolean
  /** The table pane's properties after the title. */
  columns: Property[]
}

function dateProperty(properties: readonly Property[], id: string | null | undefined): TimelineDateProperty | null {
  const property = id == null ? undefined : properties.find((candidate) => candidate.id === id)
  return property?.type === 'date' ? (property as TimelineDateProperty) : null
}

/**
 * Like the server's `timeline::repair`: a start or end that is gone or not a date is dropped (a timeline then starts at
 * the first date property), an end equal to the start is dropped, table columns keep known non-title properties once.
 */
export function timelineFields(type: DatabaseView['type'], config: DatabaseViewConfig, properties: readonly Property[]): Partial<ViewConfig> {
  const kept = dateProperty(properties, config.start_property_id)
  const start = kept ?? (type === 'timeline' ? (properties.find((property) => property.type === 'date') ?? null) : null)
  const end = dateProperty(properties, config.end_property_id)
  const known = new Set(properties.filter((property) => property.type !== 'title').map((property) => property.id))
  const columns = [...new Set(config.table_columns ?? [])].filter((id) => known.has(id))
  return {
    ...(start ? { start_property_id: start.id } : {}),
    ...(end && end.id !== start?.id ? { end_property_id: end.id } : {}),
    ...(config.zoom && config.zoom !== 'month' ? { zoom: config.zoom } : {}),
    ...(config.show_table === false ? { show_table: false } : {}),
    ...(columns.length > 0 ? { table_columns: columns } : {}),
  }
}

/** The timeline of a repaired config. */
export function timelineOf(config: ViewConfig, properties: readonly Property[]): Timeline {
  const byId = new Map(properties.map((property) => [property.id, property]))
  return {
    start: dateProperty(properties, config.start_property_id),
    end: dateProperty(properties, config.end_property_id),
    zoom: config.zoom ?? 'month',
    showTable: config.show_table !== false,
    columns: (config.table_columns ?? []).flatMap((id) => byId.get(id) ?? []),
  }
}

/** A bar's dates: the end is inclusive and never before the start. */
export type Span = { start: DatePoint; end: DatePoint }

/** A row's bar, with the viewer's first and last day of it. */
export type Bar = { span: Span; startDay: string; endDay: string }

/**
 * A row's bar: from the start property's start to the end property's date, or to the start property's own end when the
 * view has no end property. A single date is a one-day bar; an end before the start is ignored. `null`: no start.
 */
export function barOf(row: Row, timeline: Timeline, timeZone: string): Bar | null {
  if (!timeline.start) return null
  const value = cellOf(row, timeline.start)
  if (value?.kind !== 'date') return null
  const start = value.date.start
  let end = value.date.end ?? start
  if (timeline.end) {
    const other = cellOf(row, timeline.end)
    end = other?.kind === 'date' ? other.date.start : start
  }
  const startDay = pointDay(start, timeZone)
  let endDay = pointDay(end, timeZone)
  if (endDay < startDay) {
    end = start
    endDay = startDay
  }
  return { span: { start, end }, startDay, endDay }
}

/** What a drag changes: the whole bar, or one of its edges. */
export type BarEdge = 'both' | 'start' | 'end'

/**
 * The span `days` days later (earlier when negative). A moment keeps its local time in `timeZone` across DST. Moving an
 * edge stops at the other edge, so a bar is at least one day.
 */
export function moveBar(bar: Bar, days: number, edge: BarEdge, timeZone: string): Span {
  const length = daysBetween(bar.startDay, bar.endDay)
  const span = bar.span
  switch (edge) {
    case 'both':
      return { start: shift(span.start, days, timeZone), end: shift(span.end, days, timeZone) }
    case 'start':
      return { start: shift(span.start, Math.min(days, length), timeZone), end: span.end }
    case 'end':
      return { start: span.start, end: shift(span.end, Math.max(days, -length), timeZone) }
  }
}

/** The values a moved bar writes (API shape), only for the properties that change. */
export function spanValues(row: Row, timeline: Timeline, span: Span, timeZone: string): Record<string, unknown> {
  const start = timeline.start
  if (!start) return {}
  const current = cellOf(row, start)
  const before = current?.kind === 'date' ? current.date : null
  if (!timeline.end) {
    // A single date stays one when the bar stays one day long.
    const oneDay = pointDay(span.start, timeZone) === pointDay(span.end, timeZone) && !before?.end
    return { [start.id]: apiValue({ kind: 'date', date: { start: span.start, end: oneDay ? null : span.end } }) }
  }
  const values: Record<string, unknown> = {}
  if (!before || pointKey(before.start) !== pointKey(span.start)) values[start.id] = apiValue({ kind: 'date', date: { start: span.start, end: null } })
  const endValue = cellOf(row, timeline.end)
  const endBefore = endValue?.kind === 'date' ? endValue.date.start : null
  const unchanged = endBefore ? pointKey(endBefore) === pointKey(span.end) : pointKey(span.end) === pointKey(span.start)
  if (!unchanged) values[timeline.end.id] = apiValue({ kind: 'date', date: { start: span.end, end: null } })
  return values
}

/** The value a click on an empty row's track writes: a one-day bar on `day`. */
export function dayValues(timeline: Timeline, day: string): Record<string, unknown> {
  return timeline.start ? { [timeline.start.id]: apiValue({ kind: 'date', date: { start: { kind: 'day', day }, end: null } }) } : {}
}

/** The first and last day a timeline shows: its bars and today, padded by the zoom, at most about 5 years from today. */
export function timelineRange(bars: readonly Bar[], today: string, zoom: TimelineZoom): { start: string; end: string } {
  const pad = PAD_DAYS[zoom]
  let start = addDays(today, -pad)
  let end = addDays(today, pad)
  for (const bar of bars) {
    if (bar.startDay < addDays(start, pad)) start = addDays(bar.startDay, -pad)
    if (bar.endDay > addDays(end, -pad)) end = addDays(bar.endDay, pad)
  }
  const earliest = addDays(today, -MAX_REACH_DAYS)
  const latest = addDays(today, MAX_REACH_DAYS)
  start = start < earliest ? earliest : start
  end = end > latest ? latest : end
  // Whole months (whole years at the year zoom), so the top labels start at their edge.
  start = zoom === 'year' ? `${start.slice(0, 4)}-01-01` : `${start.slice(0, 7)}-01`
  end = zoom === 'year' ? `${end.slice(0, 4)}-12-31` : lastOfMonth(end)
  return { start, end }
}

/** A labeled run of days: `start` is the index of its first day from the axis start. */
export type AxisSpan = { start: number; days: number; label: string; day: string }

export type Axis = {
  start: string
  /** Days on the axis. */
  days: number
  dayWidth: number
  /** The top labels (months, quarters or years). */
  groups: AxisSpan[]
  /** The lower labels (days, weeks or months). */
  ticks: AxisSpan[]
}

type Unit = 'day' | 'week' | 'month' | 'quarter' | 'year'

const UNITS: Record<TimelineZoom, { group: Unit; tick: Unit }> = {
  day: { group: 'month', tick: 'day' },
  week: { group: 'month', tick: 'day' },
  month: { group: 'month', tick: 'week' },
  quarter: { group: 'quarter', tick: 'month' },
  year: { group: 'year', tick: 'month' },
}

/** The axis of a timeline from `range.start` to `range.end` (local days, both included). */
export function axis(zoom: TimelineZoom, range: { start: string; end: string }, locale?: string): Axis {
  const days = daysBetween(range.start, range.end) + 1
  const units = UNITS[zoom]
  return {
    start: range.start,
    days,
    dayWidth: DAY_WIDTH[zoom],
    groups: spans(range.start, days, units.group, zoom, locale),
    ticks: spans(range.start, days, units.tick, zoom, locale),
  }
}

/** The axis index of a day (negative before the start). */
export function dayIndex(axisStart: string, day: string): number {
  return daysBetween(axisStart, day)
}

export function daysBetween(from: string, to: string): number {
  return Math.round((Date.parse(`${to}T00:00:00Z`) - Date.parse(`${from}T00:00:00Z`)) / DAY_MS)
}

function spans(start: string, days: number, unit: Unit, zoom: TimelineZoom, locale: string | undefined): AxisSpan[] {
  const result: AxisSpan[] = []
  let index = 0
  while (index < days) {
    const day = addDays(start, index)
    const next = Math.min(days, index + daysBetween(day, nextStart(day, unit)))
    result.push({ start: index, days: next - index, label: label(day, unit, zoom, locale), day })
    index = next
  }
  return result
}

/** The first day of the unit after the one `day` is in (weeks start on Monday). */
function nextStart(day: string, unit: Unit): string {
  const [year, month] = day.split('-').map(Number) as [number, number]
  switch (unit) {
    case 'day':
      return addDays(day, 1)
    case 'week': {
      const weekday = (new Date(`${day}T00:00:00Z`).getUTCDay() + 6) % 7
      return addDays(day, 7 - weekday)
    }
    case 'month':
      return utcDay(Date.UTC(year, month, 1))
    case 'quarter':
      return utcDay(Date.UTC(year, Math.floor((month - 1) / 3) * 3 + 3, 1))
    case 'year':
      return utcDay(Date.UTC(year + 1, 0, 1))
  }
}

function label(day: string, unit: Unit, zoom: TimelineZoom, locale: string | undefined): string {
  const date = new Date(`${day}T00:00:00Z`)
  const format = (options: Intl.DateTimeFormatOptions) => new Intl.DateTimeFormat(locale, { timeZone: 'UTC', ...options }).format(date)
  switch (unit) {
    case 'day':
      return zoom === 'day' ? format({ weekday: 'short', day: 'numeric' }) : format({ day: 'numeric' })
    case 'week':
      return format({ month: 'short', day: 'numeric' })
    case 'month':
      return zoom === 'quarter' || zoom === 'year' ? format({ month: 'short' }) : format({ month: 'long', year: 'numeric' })
    case 'quarter':
      return `Q${Math.floor(date.getUTCMonth() / 3) + 1} ${date.getUTCFullYear()}`
    case 'year':
      return String(date.getUTCFullYear())
  }
}

/** The same point `days` days later; a moment keeps its wall time in `timeZone`. */
function shift(point: DatePoint, days: number, timeZone: string): DatePoint {
  if (days === 0) return point
  if (point.kind === 'day') return { kind: 'day', day: addDays(point.day, days) }
  const wall = point.ms + offsetMs(point.ms, timeZone) + days * DAY_MS
  const ms = wall - offsetMs(wall - offsetMs(wall, timeZone), timeZone)
  return { kind: 'moment', iso: new Date(ms).toISOString().replace(/\.\d{3}Z$/, 'Z'), ms }
}

function pointKey(point: DatePoint): string {
  return point.kind === 'day' ? point.day : point.iso
}

function lastOfMonth(day: string): string {
  const [year, month] = day.split('-').map(Number) as [number, number]
  return utcDay(Date.UTC(year, month, 0))
}

function utcDay(ms: number): string {
  return new Date(ms).toISOString().slice(0, 10)
}
