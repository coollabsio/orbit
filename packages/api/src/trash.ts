import type { ProjectRecord, TaskRecord } from './generated/types.gen'

const DAY_MILLIS = 24 * 60 * 60 * 1000
/** The server's `TRASH_RETENTION_MILLIS`: the trash keeps records for 30 days. */
export const TRASH_RETENTION_DAYS = 30

/** Whole days until the retention purge deletes a record trashed at `deletedAt`, never below 0. */
export function daysUntilPurge(deletedAt: string, now = Date.now()): number {
  return Math.max(0, Math.ceil((Date.parse(deletedAt) + TRASH_RETENTION_DAYS * DAY_MILLIS - now) / DAY_MILLIS))
}

/** "Ana moved this task to Trash Nov 6. It will be permanently deleted in 26 days." */
export function trashedNotice(kind: 'task' | 'project', deletedBy: string | undefined, deletedAt: string, now = Date.now()): string {
  const day = new Date(deletedAt).toLocaleDateString(undefined, { month: 'short', day: 'numeric' })
  const days = daysUntilPurge(deletedAt, now)
  const when = days === 0 ? 'today' : `in ${days} ${days === 1 ? 'day' : 'days'}`
  return `${deletedBy ?? 'Someone'} moved this ${kind} to Trash ${day}. It will be permanently deleted ${when}.`
}

/** `ENG-12` of a trash entry, when its project key is known. */
export function trashIdentifier(task: Pick<TaskRecord, 'number' | 'project_id'>, keyOf: (projectId: string) => string | undefined): string | undefined {
  const key = keyOf(task.project_id)
  return key && task.number > 0 ? `${key}-${task.number}` : undefined
}

/** The trash list's search: projects by name or key, tasks by title or identifier, ignoring case. */
export function filterTrash<P extends Pick<ProjectRecord, 'name' | 'key'>, T extends Pick<TaskRecord, 'title' | 'number' | 'project_id'>>(
  projects: P[],
  tasks: T[],
  search: string,
  keyOf: (projectId: string) => string | undefined,
): { projects: P[]; tasks: T[] } {
  const query = search.trim().toLowerCase()
  if (!query) return { projects, tasks }
  const matches = (value: string | undefined) => value?.toLowerCase().includes(query) ?? false
  return {
    projects: projects.filter((project) => matches(project.name) || matches(project.key)),
    tasks: tasks.filter((task) => matches(task.title || 'Untitled task') || matches(trashIdentifier(task, keyOf))),
  }
}
