const MINUTE = 60_000
const HOUR = 60 * MINUTE
const DAY = 24 * HOUR

export function relativeTime(iso: string): string {
  const diff = Date.now() - new Date(iso).getTime()
  if (diff < MINUTE) return 'now'
  if (diff < HOUR) return `${Math.floor(diff / MINUTE)}m`
  if (diff < DAY) return `${Math.floor(diff / HOUR)}h`
  if (diff < 7 * DAY) return `${Math.floor(diff / DAY)}d`
  return shortDate(iso)
}

export function shortDate(iso: string): string {
  return new Date(iso).toLocaleDateString(undefined, { month: 'short', day: 'numeric' })
}

export function fullDate(iso: string): string {
  return new Date(iso).toLocaleDateString(undefined, {
    weekday: 'short',
    month: 'short',
    day: 'numeric',
    year: 'numeric',
  })
}

export function timeOfDay(iso: string): string {
  return new Date(iso).toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' })
}

/** "Today", "Yesterday", or a short date — for message group separators. */
export function dayLabel(iso: string): string {
  const date = new Date(iso)
  const today = new Date()
  const yesterday = new Date(today.getTime() - DAY)
  if (date.toDateString() === today.toDateString()) return 'Today'
  if (date.toDateString() === yesterday.toDateString()) return 'Yesterday'
  return fullDate(iso)
}

/** ISO timestamp n minutes/hours/days in the past — for mock data. */
export function ago(amount: number, unit: 'm' | 'h' | 'd'): string {
  const ms = unit === 'm' ? amount * MINUTE : unit === 'h' ? amount * HOUR : amount * DAY
  return new Date(Date.now() - ms).toISOString()
}

/** ISO timestamp n days in the future — for mock due dates. */
export function inDays(days: number): string {
  return new Date(Date.now() + days * DAY).toISOString()
}
