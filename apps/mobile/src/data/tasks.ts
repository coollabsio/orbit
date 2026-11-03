import { ApiProblem } from '@orbit/api/problem'

import type { AttachmentRecord, CreateTaskBody, TaskRef, TaskRelationRecord } from '@orbit/api/generated/types.gen'

import type { LabelRecord, ProjectRecord, StatusRecord, TaskRecord } from './orbit'

export type StatusCategory = 'unstarted' | 'started' | 'completed' | 'cancelled' | 'duplicate'
export type Priority = 'none' | 'low' | 'medium' | 'high' | 'urgent'

/** The workflow order of the web: a status sorts by its category, then by its position. */
const CATEGORY_ORDER: readonly string[] = ['unstarted', 'started', 'completed', 'cancelled', 'duplicate']
const CLOSED: readonly string[] = ['completed', 'cancelled', 'duplicate']

export const PRIORITY_LABEL: Record<Priority, string> = { none: 'No priority', low: 'Low', medium: 'Medium', high: 'High', urgent: 'Urgent' }
const PRIORITY_ORDER: readonly string[] = ['urgent', 'high', 'medium', 'low', 'none']

export const priorityOf = (task: Pick<TaskRecord, 'priority'>): Priority =>
  PRIORITY_ORDER.includes(task.priority) ? (task.priority as Priority) : 'none'

export function sortStatuses(statuses: readonly StatusRecord[]): StatusRecord[] {
  return [...statuses].sort((a, b) => CATEGORY_ORDER.indexOf(a.category) - CATEGORY_ORDER.indexOf(b.category) || a.position - b.position)
}

/** Same-named statuses of different projects share a key, so a list across projects shows one "Todo" group. */
export const statusKeyOf = (status: Pick<StatusRecord, 'category' | 'name'>) =>
  `${status.category}:${status.name.trim().replace(/[A-Z]+/g, (letters) => letters.toLowerCase())}`

export const isClosed = (status: Pick<StatusRecord, 'category'> | undefined) => status !== undefined && CLOSED.includes(status.category)

/** `GEN-12`. Just the number while the projects load. */
export function identifier(task: Pick<TaskRecord, 'project_id' | 'number'>, projects: readonly ProjectRecord[]): string {
  const project = projects.find((item) => item.id === task.project_id)
  return project ? `${project.key}-${task.number}` : `#${task.number}`
}

/** The statuses of one project in workflow order. */
export const projectStatuses = (statuses: readonly StatusRecord[], projectId: string) => sortStatuses(statuses.filter((status) => status.project_id === projectId))

/** Where a new task lands: the first unstarted status of the project, else its first status. */
export function defaultStatusOf(statuses: readonly StatusRecord[], projectId: string): StatusRecord | undefined {
  const own = projectStatuses(statuses, projectId)
  return own.find((status) => status.category === 'unstarted') ?? own[0]
}

export interface StatusGroup {
  key: string
  name: string
  /** The first status of the group: its category and color draw the icon. `undefined` for tasks with an unknown status. */
  status: StatusRecord | undefined
  tasks: TaskRecord[]
}

const byPriorityThenRecent = (a: TaskRecord, b: TaskRecord) =>
  PRIORITY_ORDER.indexOf(priorityOf(a)) - PRIORITY_ORDER.indexOf(priorityOf(b)) || b.updated_at.localeCompare(a.updated_at)

/**
 * Tasks in status groups, in workflow order (Backlog, Todo, In Progress, Done …). Empty groups are left out. A task
 * whose status is not loaded goes to a last "No status" group.
 */
export function groupTasksByStatus(tasks: readonly TaskRecord[], statuses: readonly StatusRecord[]): StatusGroup[] {
  const keyById = new Map(statuses.map((status) => [status.id, statusKeyOf(status)]))
  const groups: StatusGroup[] = []
  for (const status of sortStatuses(statuses)) {
    const key = statusKeyOf(status)
    if (!groups.some((group) => group.key === key)) groups.push({ key, name: status.name, status, tasks: [] })
  }
  const unknown: StatusGroup = { key: 'none', name: 'No status', status: undefined, tasks: [] }
  for (const task of tasks) {
    const key = keyById.get(task.status_id)
    ;(groups.find((group) => group.key === key) ?? unknown).tasks.push(task)
  }
  return [...groups, unknown].filter((group) => group.tasks.length > 0).map((group) => ({ ...group, tasks: group.tasks.sort(byPriorityThenRecent) }))
}

/** Open tasks assigned to the user: due soonest first (no due date last), then by priority. */
export function myOpenTasks(tasks: readonly TaskRecord[], statuses: readonly StatusRecord[], userId: string, limit: number): TaskRecord[] {
  const statusById = new Map(statuses.map((status) => [status.id, status]))
  const due = (task: TaskRecord) => (task.due_at ? Date.parse(task.due_at) : Number.POSITIVE_INFINITY)
  return tasks
    .filter((task) => task.assignee_ids.includes(userId) && !isClosed(statusById.get(task.status_id)))
    .sort((a, b) => due(a) - due(b) || byPriorityThenRecent(a, b))
    .slice(0, limit)
}

/** `Overdue`, `Today`, `Tomorrow`, or the weekday and date (`Fri, Oct 30`), as the web Home card. */
export function dueLabel(dueAt: string, now: Date, locale?: string): string {
  const day = (date: Date) => new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime()
  const days = Math.round((day(new Date(dueAt)) - day(now)) / 86_400_000)
  if (days < 0) return 'Overdue'
  if (days === 0) return 'Today'
  if (days === 1) return 'Tomorrow'
  return new Date(dueAt).toLocaleDateString(locale, { weekday: 'short', month: 'short', day: 'numeric' })
}

/** The list with `task` in place of its old copy. */
export const replaceTask = (list: readonly TaskRecord[], task: TaskRecord) => list.map((item) => (item.id === task.id ? task : item))

/** The task changed since it was loaded: `expected_version` no longer matches. */
export const isVersionConflict = (error: unknown) => error instanceof ApiProblem && error.status === 409 && ['task_conflict', 'conflict'].includes(error.code)

// ---- properties

/** The order of the priority menus (task detail and New task), as the web's. */
export const PRIORITY_CHOICES: readonly Priority[] = ['urgent', 'high', 'medium', 'low', 'none']

/** `list` with `id` added, or removed when it is there: a checkbox row of the assignee and label pickers. */
export const toggleId = (list: readonly string[], id: string): string[] => (list.includes(id) ? list.filter((item) => item !== id) : [...list, id])

/**
 * The task fields a property write changes. Status, priority, assignees, labels, due date and source show at once
 * (optimistic, as on the web); a parent or duplicate change waits for the server, which checks cycles.
 */
export type TaskPatch = Partial<Pick<TaskRecord, 'status_id' | 'priority' | 'assignee_ids' | 'label_ids' | 'due_at' | 'due_start_at' | 'source_url'>> & {
  parent_task_id?: string | null
  duplicate_of_id?: string | null
}

/** The task as it shows while `patch` is on its way: parent and duplicate stay until the server answers. */
export function optimisticTask(task: TaskRecord, patch: TaskPatch): TaskRecord {
  const { parent_task_id: _parent, duplicate_of_id: _duplicate, ...shown } = patch
  return { ...task, ...shown }
}

/** `github.com` for the Source chip. */
export const sourceHost = (url: string) => url.replace(/^https?:\/\//, '').split('/')[0] ?? url

/** The Source field's value as the API takes it: trimmed, and `null` (no source) when empty. */
export const sourceValue = (input: string): string | null => input.trim() || null

// ---- due date

/** What a task stores: `start` (local midnight of the first day) only for a range, `end` the day at its time. */
export interface DueRange {
  start: string | null
  end: string
}

/** `09:00`: a pick keeps the time the task had, else nine in the morning, as the web picker. */
export const timeOf = (date: Date) => `${String(date.getHours()).padStart(2, '0')}:${String(date.getMinutes()).padStart(2, '0')}`
export const DEFAULT_DUE_TIME = '09:00'

const atTime = (day: Date, time: string) => {
  const [hours = 9, minutes = 0] = time.split(':').map(Number)
  const next = new Date(day)
  next.setHours(hours, minutes, 0, 0)
  return next
}

const startOfDay = (day: Date) => {
  const next = new Date(day)
  next.setHours(0, 0, 0, 0)
  return next
}

/** One day at `time`. */
export const dueOn = (day: Date, time: string): DueRange => ({ start: null, end: atTime(day, time).toISOString() })

/** Monday to Sunday of this week (`0`) or a later one, ending at `time` on Sunday: the web's week shortcuts. */
export function dueWeek(weeksAhead: number, time: string, now: Date): DueRange {
  const monday = startOfDay(now)
  monday.setDate(monday.getDate() - ((now.getDay() + 6) % 7) + weeksAhead * 7)
  const sunday = new Date(monday)
  sunday.setDate(sunday.getDate() + 6)
  return { start: monday.toISOString(), end: atTime(sunday, time).toISOString() }
}

/** The day `days` after today, for the Today and Tomorrow rows. */
export function dayAfter(now: Date, days: number): Date {
  const day = startOfDay(now)
  day.setDate(day.getDate() + days)
  return day
}

/** Text of a due-date button: a range, one date with its time, or the prompt to set one (the web's `dueDateLabel`). */
export function dueDateLabel(dueAt: string | null | undefined, dueStartAt?: string | null, locale?: string): string {
  if (!dueAt) return 'Set due date'
  if (dueStartAt) {
    const format = new Intl.DateTimeFormat(locale, { year: 'numeric', month: 'short', day: 'numeric' })
    return `${format.format(new Date(dueStartAt))} – ${format.format(new Date(dueAt))}`
  }
  return new Date(dueAt).toLocaleString(locale, { year: 'numeric', month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' })
}

// ---- labels

/** Same-named labels of different projects share this key (`Bug` and `bug`). */
export const labelKeyOf = (label: Pick<LabelRecord, 'name'>) => label.name.trim().replace(/[A-Z]+/g, (letters) => letters.toLowerCase())

export const projectLabels = (labels: readonly LabelRecord[], projectId: string) => labels.filter((label) => label.project_id === projectId)

/** The web's label palette, with a name each for screen readers. */
export const LABEL_PALETTE: readonly { name: string; color: string }[] = [
  { name: 'Indigo', color: '#5e6ad2' },
  { name: 'Teal', color: '#26b5ce' },
  { name: 'Green', color: '#4cb782' },
  { name: 'Yellow', color: '#f2c94c' },
  { name: 'Orange', color: '#f2994a' },
  { name: 'Red', color: '#eb5757' },
  { name: 'Pink', color: '#e17bbd' },
  { name: 'Violet', color: '#a78bfa' },
  { name: 'Grey', color: '#8b8f98' },
]

/** The palette colour the project uses least, so a new label stands apart without a choice. */
export function nextLabelColor(labels: readonly Pick<LabelRecord, 'color'>[]): string {
  const uses = (color: string) => labels.filter((label) => label.color === color).length
  return LABEL_PALETTE.map((swatch) => swatch.color).reduce((best, color) => (uses(color) < uses(best) ? color : best))
}

/**
 * The label picker's rows: the project's labels matching `query`, the ones that were on the task when the picker
 * opened (`pinned`) first so rows do not jump while toggling, then by name. `create` is the name a "Create" row
 * offers: the query when no label of the project has its key.
 */
export function labelChoices(labels: readonly LabelRecord[], projectId: string, query: string, pinned: readonly string[]): { rows: LabelRecord[]; create: string | null } {
  const own = projectLabels(labels, projectId)
  const name = query.trim()
  const rows = own
    .filter((label) => !name || label.name.toLowerCase().includes(name.toLowerCase()))
    .sort((a, b) => Number(pinned.includes(b.id)) - Number(pinned.includes(a.id)) || a.name.localeCompare(b.name))
  const create = name && !own.some((label) => labelKeyOf(label) === labelKeyOf({ name })) ? name : null
  return { rows, create }
}

/**
 * `labelIds` for a task in `projectId`: its own labels stay, and another project's label becomes the project's label
 * with the same key, as when the New task dialog switches project.
 */
export function matchingLabelIds(labels: readonly LabelRecord[], labelIds: readonly string[], projectId: string): string[] {
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

// ---- relations, parent, sub-issues

/** What the task picker chooses a task for. `blocked_by` is stored as "the other task blocks this one". */
export type RelationKind = 'blocks' | 'blocked_by' | 'related' | 'duplicate'
type RelationGroupKey = 'duplicate_of' | 'blocked_by' | 'blocks' | 'related' | 'duplicated_by'

/** The "Add relation" menu, in the web's order. */
export const RELATION_OPTIONS: readonly { kind: RelationKind; label: string }[] = [
  { kind: 'blocks', label: 'Blocks…' },
  { kind: 'blocked_by', label: 'Blocked by…' },
  { kind: 'related', label: 'Related to…' },
  { kind: 'duplicate', label: 'Mark as duplicate of…' },
]

const RELATION_GROUPS: readonly { key: RelationGroupKey; label: string }[] = [
  { key: 'duplicate_of', label: 'Duplicate of' },
  { key: 'blocked_by', label: 'Blocked by' },
  { key: 'blocks', label: 'Blocks' },
  { key: 'related', label: 'Related' },
  { key: 'duplicated_by', label: 'Duplicated by' },
]

/** The section of a relation seen from the viewed task; `null` for a type this app does not know. */
function relationGroupOf({ type, direction }: Pick<TaskRelationRecord, 'type' | 'direction'>): RelationGroupKey | null {
  if (type === 'duplicate') return direction === 'outgoing' ? 'duplicate_of' : 'duplicated_by'
  if (type === 'blocks') return direction === 'outgoing' ? 'blocks' : 'blocked_by'
  if (type === 'related') return 'related'
  return null
}

/** Non-empty groups in the web's order, oldest relation first. */
export function groupRelations(relations: readonly TaskRelationRecord[]): { label: string; relations: TaskRelationRecord[] }[] {
  return RELATION_GROUPS.flatMap(({ key, label }) => {
    const own = relations.filter((relation) => relationGroupOf(relation) === key).sort((a, b) => a.created_at.localeCompare(b.created_at))
    return own.length > 0 ? [{ label, relations: own }] : []
  })
}

/** Other-side task ids of every relation: the Blocks/Related picker leaves them out. */
export const relatedTaskIds = (relations: readonly TaskRelationRecord[]) => relations.filter((relation) => relationGroupOf(relation) !== null).map((relation) => relation.task.id)

/** The task picker's title. */
export function relationPickerTitle(kind: RelationKind, subject: string): string {
  switch (kind) {
    case 'duplicate':
      return `Mark ${subject} as duplicate of…`
    case 'blocks':
      return `${subject} blocks…`
    case 'blocked_by':
      return `${subject} is blocked by…`
    case 'related':
      return `Relate ${subject} to…`
  }
}

/**
 * The task picker's rows: tasks whose identifier, title or description contain `query`, without `excludeIds`, each
 * task once, at most `limit`. `duplicateStatusIds` drops tasks that are themselves duplicates (a duplicate target).
 */
export function taskCandidates(
  tasks: readonly TaskRecord[],
  projects: readonly ProjectRecord[],
  { query, excludeIds, duplicateStatusIds, limit = 30 }: { query: string; excludeIds: readonly string[]; duplicateStatusIds?: readonly string[]; limit?: number },
): TaskRecord[] {
  const needle = query.trim().toLowerCase()
  const skip = new Set(excludeIds)
  const result: TaskRecord[] = []
  for (const task of tasks) {
    if (skip.has(task.id)) continue
    skip.add(task.id)
    if (duplicateStatusIds && (task.duplicate_of || duplicateStatusIds.includes(task.status_id))) continue
    if (needle && !`${identifier(task, projects)} ${task.title} ${task.description}`.toLowerCase().includes(needle)) continue
    result.push(task)
    if (result.length === limit) break
  }
  return result
}

/** `ENG-12` of an embedded task reference. */
export const refIdentifier = (ref: Pick<TaskRef, 'project_key' | 'number'>) => `${ref.project_key}-${ref.number}`

/** The parent write's error, in the web's words. */
export function parentErrorMessage(error: unknown): string {
  if (error instanceof ApiProblem && error.code === 'parent_cycle') return 'A task can’t be a sub-issue of itself or of its own sub-issues.'
  if (error instanceof ApiProblem && error.code === 'parent_invalid') return 'That parent task isn’t available anymore.'
  return 'Couldn’t change the parent.'
}

const sameSet = (a: readonly string[], b: readonly string[]) => a.length === b.length && a.every((id) => b.includes(id))

/**
 * A new sub-issue of `parent`: its project, the project's default status, its priority, and its assignees when the
 * user is one of them or every sibling has the same ones (the web's rule). `null` while the statuses load.
 */
export function subIssueBody(
  parent: Pick<TaskRecord, 'id' | 'project_id' | 'priority' | 'assignee_ids'>,
  title: string,
  { statuses, userId, siblings }: { statuses: readonly StatusRecord[]; userId: string; siblings: readonly Pick<TaskRecord, 'assignee_ids'>[] },
): CreateTaskBody | null {
  const status = defaultStatusOf(statuses, parent.project_id)
  if (!status) return null
  const inherit =
    parent.assignee_ids.length > 0 &&
    (parent.assignee_ids.includes(userId) || (siblings.length > 0 && siblings.every((sibling) => sameSet(sibling.assignee_ids, parent.assignee_ids))))
  return { title, project_id: parent.project_id, status_id: status.id, priority: parent.priority, assignee_ids: inherit ? [...parent.assignee_ids] : [], parent_task_id: parent.id }
}

// ---- attachments

/** A file the user picked, before upload: `uri` for previews, `blob` the part to send (`src/upload-part.ts`). */
export interface PickedFile {
  name: string
  mimeType: string
  uri: string
  blob: Blob
}

/** The download path of a task attachment, as the web builds it (`taskAttachmentUrl`). */
export function attachmentPath(attachment: Pick<AttachmentRecord, 'workspace_id' | 'task_id' | 'id' | 'comment_id'>): string {
  const base = `/api/v1/workspaces/${attachment.workspace_id}/tasks/${attachment.task_id}`
  return attachment.comment_id ? `${base}/comments/${attachment.comment_id}/attachments/${attachment.id}/download` : `${base}/attachments/${attachment.id}/download`
}

export const isImageType = (mimeType: string) => /^image\/(png|jpe?g|gif|webp|avif|heic|heif)$/i.test(mimeType)

/** An uploaded inline file in markdown: an image block for an image, else a file link (what the server writes for blocks). */
export const fileMarkdown = (file: { name: string; path: string; image: boolean }) => `${file.image ? '!' : ''}[${file.name.replace(/([\\[\]])/g, '\\$1')}](${file.path})`

/** A comment's text and its files after them, as the web composer puts inline files after the text. */
export function bodyWithFiles(text: string, files: readonly { name: string; path: string; image: boolean }[]): string {
  return [text.trim(), ...files.map(fileMarkdown)].filter(Boolean).join('\n\n')
}

export interface InlineFile {
  name: string
  /** The server path, `/api/v1/…/download`. */
  path: string
  image: boolean
}

const INLINE_FILE = /(!?)\[((?:\\.|[^\]\\])*)\]\(<?(\/api\/v1\/workspaces\/[0-9a-f-]+\/tasks\/[0-9a-f-]+\/(?:comments\/[0-9a-f-]+\/)?attachments\/[0-9a-f-]+\/download)>?\)/gi

/**
 * The files a markdown body (a comment, a description) links from this Orbit, and the text without them. The app
 * shows them as attachments instead of their markdown.
 */
export function splitInlineFiles(markdown: string): { text: string; files: InlineFile[] } {
  const files: InlineFile[] = []
  const text = markdown
    .replace(INLINE_FILE, (_match, bang: string, name: string, path: string) => {
      files.push({ name: name.replace(/\\(.)/g, '$1') || 'File', path, image: bang === '!' })
      return ''
    })
    .replace(/\n{3,}/g, '\n\n')
    .trim()
  return { text, files }
}

/** `1.2 MB`. */
export function fileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  const units = ['KB', 'MB', 'GB']
  let value = bytes / 1024
  let unit = 0
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024
    unit += 1
  }
  return `${value < 10 ? value.toFixed(1) : Math.round(value)} ${units[unit]}`
}
