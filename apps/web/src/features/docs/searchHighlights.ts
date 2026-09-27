import type { TextRange } from '@/api/generated/types.gen'

export interface HighlightSegment {
  text: string
  match: boolean
}

/**
 * Splits `text` at the server's highlight ranges (UTF-16 offsets, the unit of JavaScript string indices) into plain
 * and matched segments, for rendering matches bold without HTML. Out-of-range, overlapping or unordered ranges are
 * clamped or skipped, so a stale or odd response never throws.
 */
export function highlightSegments(text: string, ranges: readonly TextRange[] | undefined): HighlightSegment[] {
  const segments: HighlightSegment[] = []
  let cursor = 0
  const sorted = [...(ranges ?? [])].sort((a, b) => a.start - b.start)
  for (const range of sorted) {
    const start = Math.max(cursor, Math.min(range.start, text.length))
    const end = Math.min(range.end, text.length)
    if (end <= start) continue
    if (start > cursor) segments.push({ text: text.slice(cursor, start), match: false })
    segments.push({ text: text.slice(start, end), match: true })
    cursor = end
  }
  if (cursor < text.length || segments.length === 0) segments.push({ text: text.slice(cursor), match: false })
  return segments
}
