import type { DatePoint, DateValue } from './types'

const DAY_MS = 86_400_000
const OFFSET_BUCKET_MS = 15 * 60_000

/** Same rules as the server's `DatePoint::parse`: `YYYY-MM-DD` or `YYYY-MM-DDTHH:MM[:SS[.fraction]]Z`. */
export function parseDatePoint(value: string): DatePoint | null {
  const split = value.indexOf('T')
  const day = split === -1 ? value : value.slice(0, split)
  if (!validDay(day)) return null
  if (split === -1) return { kind: 'day', day }
  let time = value.slice(split + 1)
  if (!time.endsWith('Z')) return null
  time = time.slice(0, -1)
  const dot = time.indexOf('.')
  const fraction = dot === -1 ? '0' : time.slice(dot + 1)
  if (dot !== -1) time = time.slice(0, dot)
  if (!/^\d+$/.test(fraction)) return null
  const parts = time.split(':')
  if (parts.length !== 2 && parts.length !== 3) return null
  const [hour, minute, second] = [
    twoDigits(parts[0], 23),
    twoDigits(parts[1], 59),
    parts.length === 3 ? twoDigits(parts[2], 59) : 0,
  ]
  if (hour === null || minute === null || second === null) return null
  const iso = `${day}T${pad(hour)}:${pad(minute)}:${pad(second)}Z`
  return { kind: 'moment', iso, ms: Date.parse(iso) }
}

/** Same rules as the server's `DateValue::new`: both ends of one kind, the end not before the start. */
export function dateValue(start: DatePoint, end: DatePoint | null): DateValue | null {
  if (end && (end.kind !== start.kind || pointText(end) < pointText(start))) return null
  return { start, end }
}

/** `start` or the ISO 8601 interval `start/end`, as the server's `DateValue::text`. */
export function dateText(date: DateValue): string {
  return date.end ? `${pointText(date.start)}/${pointText(date.end)}` : pointText(date.start)
}

export function parseDateText(text: string): DateValue | null {
  const slash = text.indexOf('/')
  if (slash === -1) {
    const start = parseDatePoint(text)
    return start && dateValue(start, null)
  }
  const start = parseDatePoint(text.slice(0, slash).trim())
  const end = parseDatePoint(text.slice(slash + 1).trim())
  return start && end && dateValue(start, end)
}

export function pointText(point: DatePoint): string {
  return point.kind === 'day' ? point.day : point.iso
}

/** The viewer's calendar day of a point: a day is itself, a moment is its day in `timeZone`. */
export function pointDay(point: DatePoint, timeZone: string): string {
  return point.kind === 'day' ? point.day : localDay(point.ms, timeZone)
}

export function localDay(ms: number, timeZone: string): string {
  return utcDay(ms + offsetMs(ms, timeZone))
}

/** Epoch ms of the local midnight that starts `day`, used to order days among moments. */
export function dayStartMs(day: string, timeZone: string): number {
  const utc = Date.parse(`${day}T00:00:00Z`)
  return utc - offsetMs(utc - offsetMs(utc, timeZone), timeZone)
}

export function addDays(day: string, days: number): string {
  return utcDay(Date.parse(`${day}T00:00:00Z`) + days * DAY_MS)
}

/** The same day `months` later, clamped to the month's last day (Jan 31 + 1 month = Feb 28). */
export function addMonths(day: string, months: number): string {
  const [year, month, date] = day.split('-').map(Number) as [number, number, number]
  const first = new Date(Date.UTC(year, month - 1 + months, 1))
  const last = new Date(Date.UTC(first.getUTCFullYear(), first.getUTCMonth() + 1, 0)).getUTCDate()
  first.setUTCDate(Math.min(date, last))
  return utcDay(first.getTime())
}

/** The zone's UTC offset at `ms`. Cached per 15 minutes: zone changes fall on quarter hours. */
export function offsetMs(ms: number, timeZone: string): number {
  let cache = offsets.get(timeZone)
  if (!cache) {
    cache = new Map()
    offsets.set(timeZone, cache)
  }
  const bucket = Math.floor(ms / OFFSET_BUCKET_MS)
  let offset = cache.get(bucket)
  if (offset === undefined) {
    const parts = partsFormatter(timeZone).formatToParts(new Date(bucket * OFFSET_BUCKET_MS))
    const get = (type: Intl.DateTimeFormatPartTypes) =>
      Number(parts.find((part) => part.type === type)?.value)
    const local = Date.UTC(get('year'), get('month') - 1, get('day'), get('hour'), get('minute'))
    offset = local - bucket * OFFSET_BUCKET_MS
    cache.set(bucket, offset)
  }
  return offset
}

const offsets = new Map<string, Map<number, number>>()
const partsFormatters = new Map<string, Intl.DateTimeFormat>()

function partsFormatter(timeZone: string): Intl.DateTimeFormat {
  let formatter = partsFormatters.get(timeZone)
  if (!formatter) {
    formatter = new Intl.DateTimeFormat('en-US', {
      timeZone,
      hourCycle: 'h23',
      year: 'numeric',
      month: 'numeric',
      day: 'numeric',
      hour: 'numeric',
      minute: 'numeric',
    })
    partsFormatters.set(timeZone, formatter)
  }
  return formatter
}

function utcDay(ms: number): string {
  return new Date(ms).toISOString().slice(0, 10)
}

function validDay(day: string): boolean {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(day)) return false
  const [year, month, date] = day.split('-').map(Number) as [number, number, number]
  if (month < 1 || month > 12) return false
  const leap = year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0)
  const days = [31, leap ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31][month - 1]!
  return date >= 1 && date <= days
}

function twoDigits(part: string | undefined, max: number): number | null {
  if (part === undefined || !/^\d{2}$/.test(part)) return null
  const value = Number(part)
  return value <= max ? value : null
}

function pad(value: number): string {
  return String(value).padStart(2, '0')
}

/** The day of a `Date` in the runtime's zone, as `YYYY-MM-DD`. */
export function dayOf(date: Date): string {
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`
}

/** Midnight of a `YYYY-MM-DD` day in the runtime's zone. */
export function localDate(day: string): Date {
  const [year, month, date] = day.split('-').map(Number)
  return new Date(year!, month! - 1, date)
}

/** A point as a date editor shows it: the viewer's local day and `HH:MM` (09:00 for a date without a time). */
export function localParts(point: DatePoint): { day: string; time: string } {
  if (point.kind === 'day') return { day: point.day, time: '09:00' }
  const date = new Date(point.ms)
  return { day: dayOf(date), time: `${pad(date.getHours())}:${pad(date.getMinutes())}` }
}

/** A local day, with a local `HH:MM` time or none, as the point the server stores. */
export function localPoint(day: string, time: string | null): DatePoint {
  if (time === null) return { kind: 'day', day }
  const date = new Date(`${day}T${time}:00`)
  const iso = date.toISOString().replace(/\.\d{3}Z$/, 'Z')
  return { kind: 'moment', iso, ms: date.getTime() }
}

/** What a date editor saves: an end before the start is dropped. */
export function editedDate(edit: { start: string; end: string | null; withTime: boolean; startTime: string; endTime: string }): DateValue {
  const start = localPoint(edit.start, edit.withTime ? edit.startTime : null)
  const end = edit.end ? localPoint(edit.end, edit.withTime ? edit.endTime : null) : null
  return { start, end: end && dateValue(start, end) ? end : null }
}
