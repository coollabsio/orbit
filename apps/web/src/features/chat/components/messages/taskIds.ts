import { taskPath } from '@/lib/taskLinks'

/**
 * What is skipped (code, also a fence that is still open; a markdown link or image; a URL), or a task identifier in
 * groups 2 and 3, with the backslash that escapes it in group 1.
 */
const TASK_ID =
  /```[\s\S]*?(?:```|$)|`[^`\n]+`|!?\[[^\]\n]*\]\([^)\s]*\)|https?:\/\/[^\s<>]+|(?:(?<![\w\-/.#@\\:])(\\)|(?<![\w\-/.#@\\]))([A-Za-z0-9_]{1,20})-([1-9][0-9]{0,14})(?![\w\-/]|\.\w)/g

export interface TaskIdMatch {
  /** `[start, end)` in the text. */
  start: number
  end: number
  /** A backslash at a word start is right before it (`\ENG-12`), so it is text; `start` is at the backslash. */
  escaped: boolean
  /** The identifier with the key as the project writes it: `eng-12` gives `ENG-12`. */
  identifier: string
}

/**
 * The task identifiers typed in a text (`ENG-12`), in order. Only a key of `keys` counts (the projects of the
 * workspace, in any case), so `UTF-8` and `COVID-19` are text. An identifier is a whole word: it does not come right
 * after a letter, a digit or one of `_ - / . # @`, and no letter, digit, `_`, `-` or `/` follows it, nor a `.` with a
 * letter or digit after it. So `abc-ENG-12`, `ENG-12-rc`, `ENG-12.txt`, `path/ENG-12` and `#eng-12` are text, and
 * `(ENG-12).` is an identifier. Code, URLs and markdown links (label and target) are left alone. This list also has
 * the identifiers that a backslash keeps as text (`escaped`), for the text that leaves the backslash out. The backslash
 * must start a word as an identifier does, and not come after a `:` or another backslash, so a path is no escape:
 * `C:\ENG-12`, `D:\work\ENG-12` and `x\ENG-12` are neither escaped nor an identifier, and stay as typed.
 */
export function findTypedTaskIds(text: string, keys: readonly string[]): TaskIdMatch[] {
  if (keys.length === 0) return []
  const matches: TaskIdMatch[] = []
  for (const match of text.matchAll(TASK_ID)) {
    const typed = match[2]?.toUpperCase()
    const key = typed ? keys.find((candidate) => candidate.toUpperCase() === typed) : undefined
    const start = match.index ?? 0
    if (key) matches.push({ start, end: start + match[0].length, escaped: match[1] !== undefined, identifier: `${key}-${match[3]}` })
  }
  return matches
}

/** The task identifiers typed in a text that show as their task: not the ones with a backslash right before them. */
export function findTaskIds(text: string, keys: readonly string[]): TaskIdMatch[] {
  return findTypedTaskIds(text, keys).filter((match) => !match.escaped)
}

/** The URL of the task an identifier names: an identifier shows as that URL does, as a chip and a card. */
export function taskIdUrl(identifier: string, origin: string): string {
  return `${origin}${taskPath({ id: identifier })}`
}

/**
 * A body with each task identifier as its task URL, for `extractLinkUrls` and `standaloneLinkUrls`: an identifier then
 * shares the cards and their limit with the URLs. Never shown and never stored. A space ends a URL that text follows.
 */
export function linkTaskIds(body: string, keys: readonly string[], origin: string): string {
  let out = ''
  let cursor = 0
  for (const id of findTaskIds(body, keys)) {
    out += `${body.slice(cursor, id.start)}${taskIdUrl(id.identifier, origin)}${/^\s?$/.test(body.charAt(id.end)) ? '' : ' '}`
    cursor = id.end
  }
  return out + body.slice(cursor)
}
