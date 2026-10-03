/** Due-date maths for the date picker: local days, half-hour times, and the `{ start, end }` value a task stores. */

/** A calendar selection (react-day-picker's `DateRange`). */
export interface DaySpan {
  from: Date | undefined
  to?: Date | undefined
}

export interface DueRange {
  /** Local midnight of the first day for a range, null for a single due date. */
  start: string | null
  /** The single due date, or the range's last day, at the chosen time. */
  end: string
}

export function timeOf(date: Date) {
  return `${String(date.getHours()).padStart(2, '0')}:${String(date.getMinutes()).padStart(2, '0')}`
}

function withTime(day: Date, time: string) {
  const [h, m] = time.split(':').map(Number)
  const next = new Date(day)
  next.setHours(h, m, 0, 0)
  return next
}

function startOfLocalDay(day: Date) {
  const next = new Date(day)
  next.setHours(0, 0, 0, 0)
  return next
}

/** Monday to Sunday of this week (0) or a later one. */
export function weekFromNow(weeksAhead: number): { from: Date; to: Date } {
  const today = new Date()
  const mondayOffset = (today.getDay() + 6) % 7
  const from = startOfLocalDay(today)
  from.setDate(from.getDate() - mondayOffset + weeksAhead * 7)
  const to = new Date(from)
  to.setDate(to.getDate() + 6)
  return { from, to }
}

export function isSameDay(a: Date, b: Date) {
  return a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate()
}

/** The due value a calendar selection stands for: one day (a range of one day too) has no start. */
export function dueRangeOf(range: DaySpan | undefined, time: string): DueRange | null {
  if (!range?.from) return null
  const single = !range.to || isSameDay(range.from, range.to)
  return {
    start: single ? null : startOfLocalDay(range.from).toISOString(),
    end: withTime(single ? range.from : range.to!, time).toISOString(),
  }
}

/** Same instants, whatever the ISO formatting (the server may drop milliseconds). */
export function sameDue(a: DueRange | null, b: DueRange | null) {
  const time = (iso: string | null) => (iso ? new Date(iso).getTime() : null)
  return time(a?.start ?? null) === time(b?.start ?? null) && time(a?.end ?? null) === time(b?.end ?? null)
}
