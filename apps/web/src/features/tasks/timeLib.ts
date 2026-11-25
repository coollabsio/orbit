import { parseDuration } from '@orbit/api/duration'

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

/** The plain text of a paragraph block, or null for another block (a code block only shows the syntax). */
function blockText(block: unknown): string | null {
  if (typeof block !== 'object' || block === null) return null
  const { type, content, children } = block as { type?: unknown; content?: unknown; children?: unknown }
  if (type !== 'paragraph' || !Array.isArray(content) || (Array.isArray(children) && children.length > 0)) return null
  let text = ''
  for (const item of content) {
    if (typeof item !== 'object' || item === null || (item as { type?: unknown }).type !== 'text') return null
    text += String((item as { text?: unknown }).text ?? '')
  }
  return text
}

type TextItem = { type: 'text'; text: string }

/** `content` without the characters `start` to `end` of its text; each text item keeps its styles. */
function cutText(content: readonly TextItem[], start: number, end: number): TextItem[] {
  const kept: TextItem[] = []
  let offset = 0
  for (const item of content) {
    const from = offset
    offset += item.text.length
    const text = item.text.slice(0, Math.max(0, start - from)) + item.text.slice(Math.max(0, end - from))
    if (text) kept.push({ ...item, text })
  }
  return kept
}

/**
 * Takes the first `/spend …` line out of a comment's top-level paragraphs (GitLab's quick action). The line can be a
 * paragraph of its own (Enter) or one line of a paragraph (Shift+Enter). `spend` is null when the comment has no such
 * line; a `/spend` line whose duration cannot be read throws, so the draft is not sent as text.
 */
export function takeSpend(blocks: readonly unknown[]): { blocks: unknown[]; spend: Spend | null } {
  for (const [index, block] of blocks.entries()) {
    const text = blockText(block)
    if (text === null) continue
    const lines = text.split('\n')
    let start = 0
    for (const [lineIndex, line] of lines.entries()) {
      const end = start + line.length
      const match = SPEND_LINE.exec(line.trim())
      if (!match) {
        start = end + 1
        continue
      }
      const minutes = parseDuration(match[1]!)
      if (minutes === null) throw new Error(`Could not read “${match[1]}” in /spend. Use 1h 30m, 90m or 1.5h, up to 24h.`)
      const spend = { minutes, ...(match[2] ? { workedOn: match[2] } : {}) }
      const rest = lines.filter((_, other) => other !== lineIndex)
      if (rest.every((other) => !other.trim())) return { blocks: blocks.filter((_, other) => other !== index), spend }
      // the line and one line break next to it
      const [cutStart, cutEnd] = lineIndex > 0 ? [start - 1, end] : [start, end + 1]
      const paragraph = block as { content: TextItem[] }
      const blocksLeft = blocks.map((other, at) => (at === index ? { ...paragraph, content: cutText(paragraph.content, cutStart, cutEnd) } : other))
      return { blocks: blocksLeft, spend }
    }
  }
  return { blocks: [...blocks], spend: null }
}
