import { dayLabel, fullDate, relativeTime, timeOfDay } from '@/lib/format'

const iso = (ms: number) => new Date(ms).toISOString()

/** "14:05" (or the locale's 12-hour form): the time next to the author name. */
export function messageTime(ms: number): string {
  return timeOfDay(iso(ms))
}

/** The time in the gutter of a continuation row: hours and minutes only, so it fits the avatar column. */
export function gutterTime(ms: number): string {
  const date = new Date(ms)
  return `${String(date.getHours()).padStart(2, '0')}:${String(date.getMinutes()).padStart(2, '0')}`
}

/** "Today", "Yesterday" or a date: the sticky day chip. */
export function dayChip(ms: number): string {
  return dayLabel(iso(ms))
}

/** "just now", "2m ago", "3h ago", "2d ago", then a short date: last reply and last activity. */
export function relativeAgo(ms: number): string {
  if (Date.now() - ms >= 7 * 24 * 60 * 60_000) return relativeTime(iso(ms))
  const short = relativeTime(iso(ms))
  return short === 'now' ? 'just now' : `${short} ago`
}

/** Date and time, for tooltips and the accessible name of a time. */
export function fullTimestamp(ms: number): string {
  return `${fullDate(iso(ms))}, ${timeOfDay(iso(ms))}`
}

/** Two times fall on the same local calendar day. */
export function isSameDay(a: number, b: number): boolean {
  return new Date(a).toDateString() === new Date(b).toDateString()
}
