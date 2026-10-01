export interface TextSegment {
  text: string
  /** Part of a search match. */
  match: boolean
}

/**
 * Cuts `text` at the `[start, end)` ranges of a search hit. Ranges may come unsorted, overlap or reach past the text;
 * the segments always join back to `text`.
 */
export function splitByRanges(text: string, ranges: readonly (readonly [number, number])[]): TextSegment[] {
  const sorted = ranges
    .map(([start, end]) => [Math.max(0, start), Math.min(text.length, end)] as const)
    .filter(([start, end]) => end > start)
    .sort((a, b) => a[0] - b[0])

  const segments: TextSegment[] = []
  let at = 0
  for (const [start, end] of sorted) {
    // Overlapping or touching ranges become one match.
    if (start <= at && segments.at(-1)?.match) {
      if (end > at) {
        segments[segments.length - 1] = { text: segments[segments.length - 1].text + text.slice(at, end), match: true }
        at = end
      }
      continue
    }
    const from = Math.max(start, at)
    if (end <= from) continue
    if (from > at) segments.push({ text: text.slice(at, from), match: false })
    segments.push({ text: text.slice(from, end), match: true })
    at = end
  }
  if (at < text.length) segments.push({ text: text.slice(at), match: false })
  return segments
}

/**
 * A result shows a few lines only. When the first match is far into the text, the text before it is cut to its last
 * `lead` characters (at a word start where there is one), so the match is in view.
 */
export function leadToFirstMatch(segments: readonly TextSegment[], lead = 60): TextSegment[] {
  const [first, ...rest] = segments
  if (!first || first.match || rest.length === 0 || first.text.length <= lead) return [...segments]
  const tail = first.text.slice(-lead)
  const wordStart = tail.search(/\s/)
  return [{ text: `…${wordStart >= 0 ? tail.slice(wordStart + 1) : tail}`, match: false }, ...rest]
}
