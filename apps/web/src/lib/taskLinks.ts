/**
 * Task URLs name a task by its identifier (`/tasks/ENG-12`, `/views/:viewId/ENG-12`); a task UUID still works and is
 * what a link falls back to while the number or project key is not known.
 */

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i
/** Project keys are letters, digits and `_` (at most 20); the number follows the only `-`. */
const IDENTIFIER = /^([A-Za-z0-9_]{1,20})-([1-9][0-9]{0,14})$/

/** A task as a link needs it. `number` and `projectKey` make the identifier; without both the link uses the id. */
export interface TaskLinkTarget {
  id: string
  number?: number | null
  projectKey?: string | null
}

export function isTaskUuid(value: string): boolean {
  return UUID.test(value)
}

/** `eng-12` → `{ key: 'ENG', number: 12 }`; `null` for anything that is not an identifier. */
export function parseTaskIdentifier(value: string): { key: string; number: number } | null {
  const match = IDENTIFIER.exec(value.trim())
  return match ? { key: match[1]!.toUpperCase(), number: Number(match[2]) } : null
}

export function formatTaskIdentifier(projectKey: string, number: number): string {
  return `${projectKey}-${number}`
}

/** The route segment of a task: its identifier when known, else its id. */
export function taskSlug(task: TaskLinkTarget): string {
  return task.projectKey && task.number && task.number > 0 ? formatTaskIdentifier(task.projectKey, task.number) : task.id
}

/** `/tasks/ENG-12`, or `${basePath}/ENG-12` for a task opened inside a saved view (`/views/:viewId`). */
export function taskPath(task: TaskLinkTarget, basePath = '/tasks'): string {
  return `${basePath}/${encodeURIComponent(taskSlug(task))}`
}

/** Whether a route segment names this task: its id, or its identifier in any case. */
export function taskParamMatches(param: string, task: TaskLinkTarget): boolean {
  if (param === task.id) return true
  const parsed = parseTaskIdentifier(param)
  return parsed !== null && parsed.number === task.number && parsed.key === task.projectKey?.toUpperCase()
}

/** The canonical form of a route segment, to compare two segments: identifiers upper-cased, ids lower-cased. */
export function normalizeTaskParam(param: string): string {
  const parsed = parseTaskIdentifier(param)
  return parsed ? formatTaskIdentifier(parsed.key, parsed.number) : param.toLowerCase()
}
