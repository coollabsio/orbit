/**
 * The web's date and time wording (`lib/format.ts`, `features/chat/lib/time.ts`). `locale` is for tests; the app passes
 * none, so the device locale decides, as the browser's does on the web.
 */
const MINUTE = 60_000
const HOUR = 60 * MINUTE
const DAY = 24 * HOUR

/** `Oct 15`. */
export const shortDate = (iso: string, locale?: string) => new Date(iso).toLocaleDateString(locale, { month: 'short', day: 'numeric' })

/** `Thu, Oct 15, 2026`. */
export const fullDate = (iso: string, locale?: string) => new Date(iso).toLocaleDateString(locale, { weekday: 'short', month: 'short', day: 'numeric', year: 'numeric' })

/** `09:52 PM` (or the locale's 24-hour form): the time beside an author. */
export const timeOfDay = (iso: string, locale?: string) => new Date(iso).toLocaleTimeString(locale, { hour: '2-digit', minute: '2-digit' })

/** `now`, `5m`, `3h`, `2d`, then the short date. */
export function relativeTime(iso: string, now: Date = new Date(), locale?: string): string {
  const diff = now.getTime() - new Date(iso).getTime()
  if (diff < MINUTE) return 'now'
  if (diff < HOUR) return `${Math.floor(diff / MINUTE)}m`
  if (diff < DAY) return `${Math.floor(diff / HOUR)}h`
  if (diff < 7 * DAY) return `${Math.floor(diff / DAY)}d`
  return shortDate(iso, locale)
}

/** `just now`, `5m ago` … then the short date: a thread's last reply. */
export function relativeAgo(iso: string, now: Date = new Date(), locale?: string): string {
  const short = relativeTime(iso, now, locale)
  if (now.getTime() - new Date(iso).getTime() >= 7 * DAY) return short
  return short === 'now' ? 'just now' : `${short} ago`
}

/** `Today`, `Yesterday` or the full date: the chat day pill. */
export function dayLabel(iso: string, now: Date = new Date(), locale?: string): string {
  const date = new Date(iso)
  if (date.toDateString() === now.toDateString()) return 'Today'
  if (date.toDateString() === new Date(now.getTime() - DAY).toDateString()) return 'Yesterday'
  return fullDate(iso, locale)
}
