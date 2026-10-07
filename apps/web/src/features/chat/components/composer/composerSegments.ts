import { splitEmoji } from '@/lib/twemoji'
import { findMentions, type MentionTarget } from '../../lib/mentionTokens'

/**
 * How the box draws a piece of formatted text. `marker` is the syntax itself (`**`, `> `, a fence line), which stays in
 * the text and is dimmed; `muted` is quoted text and subtext.
 */
export type ComposerMark = 'strong' | 'em' | 'underline' | 'strike' | 'code' | 'spoiler' | 'muted' | 'marker'

/** A piece of the text in the message box, as the box shows it. The pieces join back to the exact text. */
export type ComposerSegment = (
  | { kind: 'text' }
  /** `@Name`, `#name`, `@channel`, `@here`: the same matches that are sent as mentions. */
  | { kind: 'mention' }
  | { kind: 'emoji'; url: string }
) & {
  text: string
  /** Absent on unformatted text. */
  marks?: readonly ComposerMark[]
}

interface Run {
  start: number
  end: number
  marks: readonly ComposerMark[]
}

/** What a run of one marker character means, by its length: `*`, `**`, `***`. */
const DELIMITERS: Record<string, readonly (readonly ComposerMark[] | null)[]> = {
  '*': [['em'], ['strong'], ['strong', 'em']],
  _: [['em'], ['underline'], ['underline', 'em']],
  '~': [null, ['strike']],
  '|': [null, ['spoiler']],
}
/** A backslash before one of these makes it ordinary text. */
const PUNCTUATION = /[!-/:-@[-`{-~]/
const WORD = /[\p{L}\p{N}]/u
const LINE_PREFIX = /(?:#{1,3}|-#) /y
const FENCE_LANGUAGE = /[\w+#.-]*\n/y

function withMarks(marks: readonly ComposerMark[], ...more: ComposerMark[]): readonly ComposerMark[] {
  return [...marks, ...more.filter((mark) => !marks.includes(mark))]
}

/**
 * Cuts the text into runs of chat markdown, end to end with no gap. It is an aid for the eye, not the parser of the
 * sent message: syntax that is not closed on its line stays plain text. A mention is one opaque piece, so a `_` in a
 * name starts nothing.
 */
function markdownRuns(text: string, mentionEnds: ReadonlyMap<number, number>): Run[] {
  const runs: Run[] = []
  const emit = (start: number, end: number, marks: readonly ComposerMark[]) => {
    if (end <= start) return
    const last = runs.at(-1)
    if (last && last.end === start && last.marks.join() === marks.join()) last.end = end
    else runs.push({ start, end, marks })
  }
  const runLength = (index: number, end: number) => {
    let length = 1
    while (index + length < end && text[index + length] === text[index]) length += 1
    return length
  }
  const isEscape = (index: number, end: number) => text[index] === '\\' && index + 1 < end && PUNCTUATION.test(text[index + 1]!)
  /** Inline code that opens at `index` and closes on the same line with the same number of backticks. */
  const codeSpan = (index: number, end: number) => {
    const length = runLength(index, end)
    const close = text.indexOf('`'.repeat(length), index + length)
    return close !== -1 && close + length <= end ? { open: index + length, close, end: close + length } : null
  }
  /** Where the marker that closes a run opened just before `from` starts, or -1. */
  const closer = (char: string, length: number, from: number, end: number) => {
    let index = from
    while (index < end) {
      const mentionEnd = mentionEnds.get(index)
      if (mentionEnd) index = mentionEnd
      else if (isEscape(index, end)) index += 2
      else if (text[index] === '`') index = codeSpan(index, end)?.end ?? index + runLength(index, end)
      else if (text[index] === char) {
        const run = runLength(index, end)
        // A longer run closes with its last characters, so `**a *b***` closes both.
        const at = index + run - length
        // A pair inside single markers is bold or underline of its own, not the end of the italic.
        const pairInItalic = length === 1 && run === 2
        // `snake_case_name` is a name, not italic.
        const insideWord = char === '_' && length === 1 && WORD.test(text[at + 1] ?? '')
        if (run >= length && at > from && !pairInItalic && !insideWord) return at
        index += run
      } else index += 1
    }
    return -1
  }

  const inline = (start: number, end: number, marks: readonly ComposerMark[]) => {
    let plain = start
    let index = start
    while (index < end) {
      const mentionEnd = mentionEnds.get(index)
      const lengths = DELIMITERS[text[index]!]
      if (mentionEnd) {
        index = Math.min(mentionEnd, end)
      } else if (isEscape(index, end)) {
        emit(plain, index, marks)
        emit(index, index + 1, withMarks(marks, 'marker'))
        plain = index + 1
        index += 2
      } else if (text[index] === '`') {
        const span = codeSpan(index, end)
        if (span) {
          const code = withMarks(marks, 'code')
          emit(plain, index, marks)
          emit(index, span.open, withMarks(code, 'marker'))
          emit(span.open, span.close, code)
          emit(span.close, span.end, withMarks(code, 'marker'))
          plain = index = span.end
        } else index += runLength(index, end)
      } else if (lengths) {
        const run = runLength(index, end)
        let next = index + run
        for (let length = Math.min(run, lengths.length); length > 0; length -= 1) {
          const added = lengths[length - 1]
          const open = index + length
          // `2 * 3 * 4` is arithmetic: italic starts right at a character, and `_` not in the middle of a word.
          const notItalic = length === 1 && (/\s/.test(text[open] ?? ' ') || (text[index] === '_' && WORD.test(text[index - 1] ?? '')))
          const close = added && !notItalic ? closer(text[index]!, length, open, end) : -1
          if (!added || close === -1) continue
          const inner = withMarks(marks, ...added)
          emit(plain, index, marks)
          emit(index, open, withMarks(inner, 'marker'))
          inline(open, close, inner)
          emit(close, close + length, withMarks(inner, 'marker'))
          plain = next = close + length
          break
        }
        index = next
      } else index += 1
    }
    emit(plain, end, marks)
  }

  let position = 0
  /** After `>>> ` everything to the end of the text is quoted. */
  let quoted = false
  while (position < text.length) {
    const newline = text.indexOf('\n', position)
    let lineEnd = newline === -1 ? text.length : newline + 1
    let marks: readonly ComposerMark[] = quoted ? ['muted'] : []
    if (!quoted && (text.startsWith('> ', position) || text.startsWith('>>> ', position))) {
      quoted = text[position + 1] === '>'
      const prefixEnd = position + (quoted ? 4 : 2)
      emit(position, prefixEnd, ['marker'])
      position = prefixEnd
      marks = ['muted']
    }
    if (text.startsWith('```', position)) {
      const code = withMarks(marks, 'code')
      const close = text.indexOf('```', position + 3)
      FENCE_LANGUAGE.lastIndex = position + 3
      // The language name on the opening line belongs to the fence, not to the code.
      const openEnd = position + 3 + Math.max(0, (FENCE_LANGUAGE.exec(text)?.[0].length ?? 0) - 1)
      const codeEnd = close === -1 ? text.length : close
      emit(position, openEnd, withMarks(code, 'marker'))
      emit(openEnd, codeEnd, code)
      emit(codeEnd, Math.min(codeEnd + 3, text.length), withMarks(code, 'marker'))
      // What follows the closing fence on its line is ordinary text again.
      position = Math.min(codeEnd + 3, text.length)
      const after = text.indexOf('\n', position)
      lineEnd = after === -1 ? text.length : after + 1
    } else {
      LINE_PREFIX.lastIndex = position
      const prefix = LINE_PREFIX.exec(text)?.[0]
      if (prefix) {
        if (prefix === '-# ') marks = withMarks(marks, 'muted')
        emit(position, position + prefix.length, withMarks(marks, 'marker'))
        position += prefix.length
      }
    }
    inline(position, lineEnd, marks)
    position = lineEnd
  }
  return runs
}

/**
 * True when the caret is in a code block that is not closed yet. `Enter` there is a new line of code, as in Discord,
 * and does not send the message. A block opens with a fence at the start of a line, as in the message that is shown;
 * ``` in the middle of a sentence opens nothing.
 */
export function inOpenCodeBlock(text: string, caret: number): boolean {
  let open = false
  for (const line of text.slice(0, caret).split('\n')) {
    // ```code``` on one line is a whole block
    if (!open) open = line.startsWith('```') && !line.slice(3).includes('```')
    else if (line.startsWith('```')) open = false
  }
  return open
}

/**
 * A code fence that the person has just typed in full at the start of a line, with the caret after it at the end of
 * the line. An opening fence gets its closing fence on the next line and the caret stays between them, as in Discord.
 * When the person types the closing fence too, the one that was added goes away. `text` and `cursor` are the result,
 * or null for no change.
 */
export function completeCodeFence(text: string, caret: number): { text: string; cursor: number } | null {
  const typed = text.slice(caret - 3, caret) === '```' && (caret === 3 || text[caret - 4] === '\n')
  const lineEnd = caret === text.length || text[caret] === '\n'
  // a block is open at the end of the text: one fence has no partner
  if (!typed || !lineEnd || !inOpenCodeBlock(text, text.length)) return null
  if (inOpenCodeBlock(text, caret)) return { text: `${text.slice(0, caret)}\n\`\`\`${text.slice(caret)}`, cursor: caret }
  return text.startsWith('\n```', caret) ? { text: text.slice(0, caret) + text.slice(caret + 4), cursor: caret } : null
}

export function composerSegments(text: string, people: readonly MentionTarget[], conversations: readonly MentionTarget[]): ComposerSegment[] {
  const segments: ComposerSegment[] = []
  // Mentions are found in the whole text, not per run, so the box tints exactly what is sent as a mention.
  const mentions = findMentions(text, people, conversations)
  const runs = markdownRuns(text, new Map(mentions.map((mention) => [mention.start, mention.end])))
  for (const run of runs) {
    const marks = run.marks.length ? { marks: run.marks } : {}
    const plain = (part: string) => {
      for (const piece of splitEmoji(part)) segments.push(piece.url ? { kind: 'emoji', text: piece.text, url: piece.url, ...marks } : { kind: 'text', text: piece.text, ...marks })
    }
    let cursor = run.start
    for (const mention of mentions) {
      if (mention.end <= cursor || mention.start >= run.end) continue
      plain(text.slice(cursor, mention.start))
      const end = Math.min(mention.end, run.end)
      segments.push({ kind: 'mention', text: text.slice(Math.max(cursor, mention.start), end), ...marks })
      cursor = end
    }
    plain(text.slice(cursor, run.end))
  }
  return segments
}
