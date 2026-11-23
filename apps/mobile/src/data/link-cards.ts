/**
 * Which URLs of a chat message show a card under it, and what each one names: the web's `messageText.ts`
 * (`extractLinkUrls`), `taskIds.ts` (`linkTaskIds`) and `chatPageLib.ts` (`orbitLinkTarget`). No React.
 */

/** Fenced and inline code: a URL inside it is text. */
const CODE = /```[\s\S]*?```|`[^`\n]+`/g
const URL_PATTERN = /https?:\/\/[^\s<>]+/g
/** A URL in angle brackets (`<https://…>`) is a link without a card, as in Discord. After a backslash the bracket is text. */
const WRAPPED_URL = /(?<!\\)<https?:\/\/[^\s<>]+>/g

/**
 * What is skipped (code, also a fence that is still open; a markdown link or image; a URL), or a task identifier in
 * groups 2 and 3, with the backslash that escapes it in group 1.
 */
const TASK_ID =
  /```[\s\S]*?(?:```|$)|`[^`\n]+`|!?\[[^\]\n]*\]\([^)\s]*\)|https?:\/\/[^\s<>]+|(?:(?<![\w\-/.#@\\:])(\\)|(?<![\w\-/.#@\\]))([A-Za-z0-9_]{1,20})-([1-9][0-9]{0,14})(?![\w\-/]|\.\w)/g

/** URLs in a stored body that can become link cards: outside code and angle brackets, without trailing punctuation, each once. */
export function extractLinkUrls(body: string, limit = 3): string[] {
  const urls: string[] = []
  for (const match of body.replace(CODE, ' ').replace(WRAPPED_URL, ' ').matchAll(URL_PATTERN)) {
    const url = match[0].replace(/[.,!?;:)}\]'"]+$/, '')
    if (url && !urls.includes(url)) urls.push(url)
    if (urls.length === limit) break
  }
  return urls
}

/**
 * A body with each typed task identifier (`ENG-12`, a key of `keys` in any case, not after a backslash) as the URL of
 * its task, so that it shares the cards and their limit with the URLs. Never shown and never stored.
 */
export function linkTaskIds(body: string, keys: readonly string[], origin: string): string {
  if (keys.length === 0) return body
  let out = ''
  let cursor = 0
  for (const match of body.matchAll(TASK_ID)) {
    const typed = match[2]?.toUpperCase()
    const key = typed ? keys.find((candidate) => candidate.toUpperCase() === typed) : undefined
    if (!key || match[1] !== undefined) continue
    const start = match.index ?? 0
    const end = start + match[0].length
    out += `${body.slice(cursor, start)}${origin}/tasks/${encodeURIComponent(`${key}-${match[3]}`)}${/^\s?$/.test(body.charAt(end)) ? '' : ' '}`
    cursor = end
  }
  return out + body.slice(cursor)
}

export type OrbitLinkTarget = { kind: 'task' | 'page' | 'view'; id: string }

/** The Orbit task (by id or identifier), page or saved view a URL of this server names; `null` for any other URL. */
export function orbitLinkTarget(url: string, origin: string): OrbitLinkTarget | null {
  let parsed: URL
  try {
    parsed = new URL(url, origin)
  } catch {
    return null
  }
  if (parsed.origin !== new URL(origin).origin) return null
  const [, root, id, rest, more] = parsed.pathname.split('/')
  const segment = (value: string) => {
    try {
      return decodeURIComponent(value)
    } catch {
      return value
    }
  }
  if (root === 'views' && id && rest && !more) return { kind: 'task', id: segment(rest) }
  if (!id || rest) return null
  if (root === 'tasks') return id === 'projects' ? null : { kind: 'task', id: segment(id) }
  if (root === 'docs') return id === 'trash' || id === 'import' ? null : { kind: 'page', id }
  if (root === 'views') return { kind: 'view', id }
  return null
}
