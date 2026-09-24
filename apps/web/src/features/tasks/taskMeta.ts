import type { CreateTaskBody } from '@/api/generated'
import type { StatusCategory, TaskPriority, TaskStatusDef } from '@/features/tasks/api/models'

export type TaskView = 'mine' | 'overdue' | 'due_soon' | 'current_week' | 'my_week'

/** Defaults that keep a newly created task in the personal view it came from. */
export function taskViewCreateDefaults(view: TaskView | undefined, currentUserId: string, now = new Date()): Partial<CreateTaskBody> {
  if (view === 'mine') return currentUserId ? { assignee_ids: [currentUserId] } : {}
  if (!view) return {}

  const startOfToday = Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate())
  if (view === 'current_week' || view === 'my_week') {
    const start = new Date(now)
    start.setHours(0, 0, 0, 0)
    start.setDate(start.getDate() - (start.getDay() + 6) % 7)
    const end = new Date(start)
    end.setDate(end.getDate() + 6)
    end.setHours(12, 0, 0, 0)
    return {
      due_start_at: start.toISOString(),
      due_at: end.toISOString(),
      ...(view === 'my_week' && currentUserId ? { assignee_ids: [currentUserId] } : {}),
    }
  }
  const dueAt = view === 'overdue'
    ? new Date(startOfToday - 1).toISOString()
    : new Date(startOfToday + 12 * 60 * 60 * 1_000).toISOString()
  return { due_at: dueAt }
}

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

/** Same-named statuses of different projects share a key, so cross-project views can merge them. */
export function statusKeyOf(status: TaskStatusDef): string {
  return `${status.category}:${status.name.trim().toLowerCase()}`
}

export const PRIORITY_LABEL: Record<TaskPriority, string> = {
  none: 'No priority',
  low: 'Low',
  medium: 'Medium',
  high: 'High',
  urgent: 'Urgent',
}

export const PRIORITY_ORDER: TaskPriority[] = ['urgent', 'high', 'medium', 'low', 'none']
