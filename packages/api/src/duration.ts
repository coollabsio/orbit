/** Durations of time tracking, shared by the web and mobile apps. */

/** The most one time entry can hold: a whole day. */
export const MAX_ENTRY_MINUTES = 1440

const HOURS_MINUTES = /^(?:(\d+(?:\.\d+)?|\.\d+)\s*h)?\s*(?:(\d+)\s*m)?$/

/**
 * Minutes in a duration the person typed: `1h 30m`, `1h30m`, `90m`, `1.5h`, `2h`, or a bare `45` (minutes).
 * Case and spaces do not matter. Null for anything else, for zero, and for more than a day (days and weeks are no unit:
 * one entry is the work of one day).
 */
export function parseDuration(text: string): number | null {
  const input = text.trim().toLowerCase()
  let minutes: number
  if (/^\d+$/.test(input)) {
    minutes = Number(input)
  } else {
    const match = HOURS_MINUTES.exec(input)
    if (!match || (match[1] === undefined && match[2] === undefined)) return null
    minutes = Math.round(Number(match[1] ?? 0) * 60) + Number(match[2] ?? 0)
  }
  return minutes >= 1 && minutes <= MAX_ENTRY_MINUTES ? minutes : null
}

/** `1h 30m`, `45m`, `2h`. */
export function formatDuration(minutes: number): string {
  const hours = Math.floor(minutes / 60)
  const rest = minutes % 60
  if (hours === 0) return `${rest}m`
  return rest === 0 ? `${hours}h` : `${hours}h ${rest}m`
}
