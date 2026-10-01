import type { StatusCategory, TaskPriority, TaskStatusDef } from '@/features/tasks/api/models'

/** Status categories (workflow stages). Every status belongs to one; the glyph shape comes from it. */
export const CATEGORY_ORDER: StatusCategory[] = ['unstarted', 'started', 'completed', 'cancelled', 'duplicate']

export const CATEGORY_LABEL: Record<StatusCategory, string> = {
  unstarted: 'Unstarted',
  started: 'Started',
  completed: 'Completed',
  cancelled: 'Cancelled',
  duplicate: 'Duplicate',
}

/** Closed work: never overdue, dimmed on the timeline, left out of open totals. */
export function isClosedCategory(category: StatusCategory | undefined): boolean {
  return category === 'completed' || category === 'cancelled' || category === 'duplicate'
}

/** Default statuses every new project starts with. */
export const DEFAULT_STATUS_TEMPLATES: { key: string; name: string; category: StatusCategory; color: string }[] = [
  { key: 'todo', name: 'Todo', category: 'unstarted', color: '#8b8f98' },
  { key: 'in_progress', name: 'In Progress', category: 'started', color: '#f2c94c' },
  { key: 'done', name: 'Done', category: 'completed', color: '#4cb782' },
  { key: 'cancelled', name: 'Cancelled', category: 'cancelled', color: '#8b8f98' },
]

/** Preset colors offered in the status editor. */
export const STATUS_COLORS = ['#8b8f98', '#5e6ad2', '#26b5ce', '#4cb782', '#f2c94c', '#f2994a', '#f7c8c1', '#eb5757', '#a78bfa']

/** Category order first, then the position inside the category. */
export function sortStatuses(statuses: TaskStatusDef[]): TaskStatusDef[] {
  return [...statuses].sort(
    (a, b) => CATEGORY_ORDER.indexOf(a.category) - CATEGORY_ORDER.indexOf(b.category) || a.position - b.position,
  )
}

/** Statuses of one project, in workflow order. */
export function projectStatuses(statuses: TaskStatusDef[], projectId: string): TaskStatusDef[] {
  return sortStatuses(statuses.filter((s) => s.projectId === projectId))
}

/** Where new tasks land: the first unstarted status, else the first status of the project. */
export function defaultStatusOf(statuses: TaskStatusDef[], projectId: string): TaskStatusDef | undefined {
  const own = projectStatuses(statuses, projectId)
  return own.find((s) => s.category === 'unstarted') ?? own[0]
}

/**
 * Same-named statuses of different projects share a key, so cross-project views can merge them.
 * ASCII-only lowercase, like SQLite `lower()` in the server's status filter: "Überprüfung" keeps its "Ü".
 */
export function statusKeyOf(status: TaskStatusDef): string {
  return `${status.category}:${status.name.trim().replace(/[A-Z]+/g, (letters) => letters.toLowerCase())}`
}

export const PRIORITY_LABEL: Record<TaskPriority, string> = {
  none: 'No priority',
  low: 'Low',
  medium: 'Medium',
  high: 'High',
  urgent: 'Urgent',
}

export const PRIORITY_ORDER: TaskPriority[] = ['urgent', 'high', 'medium', 'low', 'none']

/** Text of a due-date button: a range, one date with its time, or the prompt to set one. */
export function dueDateLabel(value: string | null, startValue?: string | null) {
  if (!value) return 'Set due date'
  if (startValue) {
    const format = new Intl.DateTimeFormat(undefined, { year: 'numeric', month: 'short', day: 'numeric' })
    return `${format.format(new Date(startValue))} – ${format.format(new Date(value))}`
  }
  return new Date(value).toLocaleString(undefined, {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  })
}
