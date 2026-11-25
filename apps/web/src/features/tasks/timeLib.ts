/** The most one time entry can hold: a whole day. */
export const MAX_ENTRY_MINUTES = 1440

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
    const match = /^(?:(\d+(?:\.\d+)?|\.\d+)\s*h)?\s*(?:(\d+)\s*m)?$/.exec(input)
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

/** Today in the local timezone as `YYYY-MM-DD` (not UTC: late in the evening, UTC is already tomorrow). */
export function todayLocalDate(now: Date = new Date()): string {
  const pad = (value: number) => String(value).padStart(2, '0')
  return `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`
}

/** A `YYYY-MM-DD` work day as `Nov 10`, with the year when it is not this year. */
export function workedOnLabel(date: string, now: Date = new Date()): string {
  const [year, month, day] = date.split('-').map(Number)
  // a local date: `new Date('2026-11-10')` would be UTC midnight, the day before west of Greenwich
  const local = new Date(year!, month! - 1, day!)
  return local.toLocaleDateString(undefined, { month: 'short', day: 'numeric', ...(year === now.getFullYear() ? {} : { year: 'numeric' }) })
}

/** A `/spend 1h 30m` line of a comment, with an optional work day: `/spend 2h 2026-11-10`. */
const SPEND_LINE = /^\/spend\s+(.+?)(?:\s+(\d{4}-\d{2}-\d{2}))?\s*$/i

export interface Spend {
  minutes: number
  /** `YYYY-MM-DD`; absent for today. */
  workedOn?: string
}

/** The plain text of a block's inline content, or null for a block that is not plain text. */
function blockText(block: unknown): string | null {
  if (typeof block !== 'object' || block === null) return null
  const { content, children } = block as { content?: unknown; children?: unknown }
  if (!Array.isArray(content) || (Array.isArray(children) && children.length > 0)) return null
  let text = ''
  for (const item of content) {
    if (typeof item !== 'object' || item === null || (item as { type?: unknown }).type !== 'text') return null
    text += String((item as { text?: unknown }).text ?? '')
  }
  return text
}

/**
 * Takes the first top-level `/spend …` line out of a comment's blocks (GitLab's quick action). `spend` is null when the
 * comment has no such line; a `/spend` line whose duration cannot be read throws, so the draft is not sent as text.
 */
export function takeSpend(blocks: readonly unknown[]): { blocks: unknown[]; spend: Spend | null } {
  for (const [index, block] of blocks.entries()) {
    const match = SPEND_LINE.exec(blockText(block)?.trim() ?? '')
    if (!match) continue
    const minutes = parseDuration(match[1]!)
    if (minutes === null) throw new Error(`Could not read “${match[1]}” in /spend. Use 1h 30m, 90m or 1.5h, up to 24h.`)
    return { blocks: blocks.filter((_, other) => other !== index), spend: { minutes, ...(match[2] ? { workedOn: match[2] } : {}) } }
  }
  return { blocks: [...blocks], spend: null }
}
