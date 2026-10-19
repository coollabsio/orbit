import type { LabelRecord } from '@/api/generated/types.gen'
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

export const PROJECT_COLORS = [
  '#8b5cf6', '#6366f1', '#0ea5e9', '#06b6d4', '#10b981', '#22c55e', '#84cc16', '#eab308', '#f59e0b', '#f97316', '#ef4444',
  '#ec4899', '#d946ef', '#64748b', '#78716c',
]

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

/**
 * Labels belong to one project. Same-named labels (case-insensitive, ASCII-only like SQLite `lower()` in the server's
 * label filter) of different projects share a key, so cross-project views can merge them.
 */
export function labelKeyOf(label: Pick<LabelRecord, 'name'>): string {
  return label.name.trim().replace(/[A-Z]+/g, (letters) => letters.toLowerCase())
}

/** The labels of one project. */
export function projectLabels(labels: LabelRecord[], projectId: string): LabelRecord[] {
  return labels.filter((label) => label.project_id === projectId)
}

/**
 * `labelIds` for a task in `projectId`: its own labels stay, and another project's label becomes the project's label
 * with the same key, as when a task moves. A project can hold both "Bug" and "bug": the exact name wins.
 */
export function matchingLabelIds(labels: LabelRecord[], labelIds: string[], projectId: string): string[] {
  const own = projectLabels(labels, projectId)
  const kept = labelIds.filter((id) => own.some((label) => label.id === id))
  const mapped = labels
    .filter((label) => label.project_id !== projectId && labelIds.includes(label.id))
    .flatMap((foreign) => {
      const same = own.filter((label) => labelKeyOf(label) === labelKeyOf(foreign))
      const match = same.find((label) => label.name === foreign.name) ?? same[0]
      return match ? [match.id] : []
    })
  return [...new Set([...kept, ...mapped])]
}

/**
 * One label per key, sorted by name, and each label id → its key's representative. The representative is the oldest
 * label (UUIDv7 ids sort by creation): the label that saved views stored before labels were copied into every project.
 */
export function labelRepresentatives(labels: LabelRecord[]): { labels: LabelRecord[]; representativeOf: Map<string, string> } {
  const byKey = new Map<string, LabelRecord>()
  for (const label of labels) {
    const key = labelKeyOf(label)
    const current = byKey.get(key)
    if (!current || label.id < current.id) byKey.set(key, label)
  }
  const representativeOf = new Map(labels.map((label) => [label.id, byKey.get(labelKeyOf(label))!.id]))
  return { labels: [...byKey.values()].sort((a, b) => a.name.localeCompare(b.name)), representativeOf }
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
