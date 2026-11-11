/**
 * Task identifiers typed in plain text (`ENG-12`), as the web finds them (`features/chat/components/messages/taskIds.ts`
 * there): only a key of the workspace's projects counts, as a whole word, and code, URLs and markdown links are left
 * alone. Mobile comment fields send each one as a link to the task, as the web editors insert it.
 */
/** What is skipped (code, a markdown link or image, a URL), or an identifier in groups 1 and 2. */
const TASK_ID =
  /```[\s\S]*?(?:```|$)|`[^`\n]+`|!?\[[^\]\n]*\]\([^)\s]*\)|https?:\/\/[^\s<>]+|(?<![\w\-/.#@\\])([A-Za-z0-9_]{1,20})-([1-9][0-9]{0,14})(?![\w\-/]|\.\w)/g

export interface TaskIdMatch {
  start: number
  end: number
  /** With the key as the project writes it: `eng-12` gives `ENG-12`. */
  identifier: string
}

export function findTaskIds(text: string, keys: readonly string[]): TaskIdMatch[] {
  if (keys.length === 0) return []
  const matches: TaskIdMatch[] = []
  for (const match of text.matchAll(TASK_ID)) {
    const typed = match[1]?.toUpperCase()
    const key = typed ? keys.find((candidate) => candidate.toUpperCase() === typed) : undefined
    const start = match.index ?? 0
    if (key) matches.push({ start, end: start + match[0].length, identifier: `${key}-${match[2]}` })
  }
  return matches
}

/** The web address of a task on the server. */
export function taskUrl(server: string, identifier: string): string {
  return `${server.replace(/\/+$/, '')}/tasks/${encodeURIComponent(identifier)}`
}

/** Markdown with each typed identifier as a link to its task: `ENG-12` → `[ENG-12](https://…/tasks/ENG-12)`. */
export function linkTaskIdsInMarkdown(text: string, keys: readonly string[], server: string): string {
  let out = ''
  let cursor = 0
  for (const id of findTaskIds(text, keys)) {
    out += `${text.slice(cursor, id.start)}[${text.slice(id.start, id.end)}](${taskUrl(server, id.identifier)})`
    cursor = id.end
  }
  return out + text.slice(cursor)
}
