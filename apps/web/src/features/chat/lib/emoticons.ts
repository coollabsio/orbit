/** The emoticons that become emoji, as in Discord. Case matters: `:P` and `:p` are both here, `XD` and `xD` too. */
export const EMOTICONS: ReadonlyMap<string, string> = new Map([
  [':)', '🙂'],
  [':-)', '🙂'],
  [':D', '😄'],
  [':-D', '😄'],
  [':(', '🙁'],
  [':-(', '🙁'],
  [';)', '😉'],
  [';-)', '😉'],
  [':P', '😛'],
  [':-P', '😛'],
  [':p', '😛'],
  [':-p', '😛'],
  [':O', '😮'],
  [':-O', '😮'],
  [':o', '😮'],
  [':-o', '😮'],
  [':/', '😕'],
  [':-/', '😕'],
  [':|', '😐'],
  [':-|', '😐'],
  [":'(", '😢'],
  [":')", '🥲'],
  [':*', '😘'],
  [':-*', '😘'],
  [':3', '😺'],
  ['<3', '❤️'],
  ['</3', '💔'],
  ['xD', '😆'],
  ['XD', '😆'],
  ['B)', '😎'],
  ['B-)', '😎'],
  ['>:(', '😠'],
  ['>:)', '😈'],
  ['O:)', '😇'],
  ['^_^', '😊'],
  ['-_-', '😑'],
])

/** Fenced and inline code, also a fence that is still open: an emoticon inside it is text. */
const CODE = /(```[\s\S]*?(?:```|$)|`[^`\n]+`)/

/**
 * Every emoticon in typed text as its emoji. An emoticon counts only as a whole word, with white space or an end of the
 * text on both sides: `http://x`, `10:30` and `\:D` (the backslash is the way to keep an emoticon) stay as typed. Code
 * is left alone.
 */
export function replaceEmoticons(text: string): string {
  // `split` with a capturing group puts the code segments at the odd indexes.
  return text
    .split(CODE)
    .map((part, index) => (index % 2 === 1 ? part : part.replace(/\S+/g, (word) => EMOTICONS.get(word) ?? word)))
    .join('')
}

/** Whether the caret is in code: after an open fence, or after an open backtick on its line. */
function inCode(before: string): boolean {
  if (before.split('```').length % 2 === 0) return true
  const line = before.slice(before.lastIndexOf('\n') + 1).replaceAll('```', '')
  return line.split('`').length % 2 === 0
}

/**
 * After the user typed white space: turns the emoticon that ends just before it into its emoji. Only the word before
 * the caret is read. Returns the new text and where the caret goes (after the white space), or `null`.
 */
export function completeEmoticon(value: string, cursor: number): { text: string; cursor: number } | null {
  const end = cursor - 1
  if (end < 1 || !/\s/.test(value[end]!)) return null
  const start = value.slice(0, end).search(/\S+$/)
  const emoji = start === -1 ? undefined : EMOTICONS.get(value.slice(start, end))
  if (!emoji || inCode(value.slice(0, start))) return null
  return { text: `${value.slice(0, start)}${emoji}${value.slice(end)}`, cursor: start + emoji.length + 1 }
}
