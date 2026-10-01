import type { User } from '@/features/workspaces/models'
import type { AttachmentRecord, AuditEvent, CommentRecord, LabelRecord, ProjectRecord, TaskRecord } from '@/api/generated/types.gen'

export type StatusCategory = 'unstarted' | 'started' | 'completed' | 'cancelled' | 'duplicate'
export type TaskPriority = 'none' | 'low' | 'medium' | 'high' | 'urgent'

export type Project = ProjectRecord

export interface TaskStatusDef {
  id: string
  projectId: string
  name: string
  description: string
  color: string
  category: StatusCategory
  position: number
  version: number
}

/** Another task as a relation needs it; the identifier is built from its project's key (`taskIdentifier`). */
export interface TaskRef {
  id: string
  projectId: string
  title: string
}


export interface Attachment {
  id: string
  fileName: string
  mimeType: string
  fileSize: number
  url: string
}

export interface TaskComment {
  id: string
  authorId: string
  body: string
  /** Decided by the server for the current user. */
  canEdit: boolean
  canDelete: boolean
  createdAt: string
  parentId?: string
  attachments?: Attachment[]
  editedAt?: string | null
  version: number
}

/** One field an update changed. Ids stay ids: the feed names them (`activityChangeText`); projects are already names. */
export type TaskChange =
  | { field: 'status'; from: string; to: string }
  | { field: 'priority'; from: string; to: string }
  | { field: 'assignees' | 'labels'; added: string[]; removed: string[] }
  | { field: 'due'; start: string | null; end: string | null }
  | { field: 'title' | 'project'; from: string; to: string }
  | { field: 'description' | 'source_url' }

export interface TaskActivity {
  id: string
  actorId: string
  actorName?: string
  actorServiceAccountId?: string
  text: string
  /** The other task of a relation event; the feed links `identifier` inside `text`. */
  related?: { taskId: string; identifier: string }
  /** The changed field of a `task.updated` event; the feed shows its sentence instead of `text`. */
  change?: TaskChange
  createdAt: string
  statusId?: string
}

export interface Task {
  id: string
  identifier: string
  title: string
  description: string
  sourceUrl?: string | null
  statusId: string
  position: number
  priority: TaskPriority
  assigneeIds: string[]
  creatorId?: string
  creatorServiceAccountId?: string
  creatorServiceAccountName?: string
  projectId: string
  labels: string[]
  attachments: Attachment[]
  dueStartAt?: string | null
  dueAt: string | null
  createdAt: string
  updatedAt: string
  comments: TaskComment[]
  activity: TaskActivity[]
  /** The canonical task while this task is marked as a duplicate. */
  duplicateOf?: TaskRef | null
  /** At least one open task (not completed, cancelled or duplicate) blocks this one. */
  blocked?: boolean
  /** Parent task id; null at the top level. */
  parentTaskId?: string | null
  /** The parent, for the "Parent title ›" label. */
  parent?: TaskKeyRef | null
  /** Direct live sub-issues. */
  subIssueCount?: number
  /** Direct sub-issues that are completed or cancelled. */
  subIssueClosedCount?: number
  /** Root first; only the detail endpoint sends it. */
  ancestors?: TaskKeyRef[]
  version: number
}

export interface TaskViewState {
  currentUserId: string
  users: User[]
  statuses: TaskStatusDef[]
  labels: LabelRecord[]
  tasks: Task[]
}

const RELATION_FALLBACK: Record<string, string> = {
  'task.marked_duplicate': 'marked as duplicate',
  'task.unmarked_duplicate': 'unmarked as duplicate',
  'task.relation_added': 'added relation',
  'task.relation_removed': 'removed relation',
}

function genericActivityText(action: string): string {
  return action.split('.').reverse().join(' ')
}

/**
 * Sentence for a relation audit event. `direction` is relative to the task the event is stored on:
 * outgoing = this task is the blocker / the duplicate; incoming = the other task is.
 */
function relationActivity(
  action: string,
  metadata: Record<string, unknown>,
  identifierOf: (taskId: string, projectId: string | undefined) => string,
): Pick<TaskActivity, 'text' | 'related'> | null {
  const fallback = RELATION_FALLBACK[action]
  if (!fallback) return null
  const taskId = typeof metadata.related_task_id === 'string' ? metadata.related_task_id : undefined
  if (!taskId) return { text: fallback }
  const projectId = typeof metadata.related_task_project_id === 'string' ? metadata.related_task_project_id : undefined
  const identifier = identifierOf(taskId, projectId)
  const title = typeof metadata.related_task_title === 'string' ? metadata.related_task_title : ''
  const incoming = metadata.direction === 'incoming'
  const blocks = metadata.type === 'blocks'
  const related = { taskId, identifier }
  switch (action) {
    case 'task.marked_duplicate':
      return { related, text: incoming ? `marked ${identifier} as duplicate` : `marked as duplicate of ${identifier}${title ? ` · ${title}` : ''}` }
    case 'task.unmarked_duplicate':
      return incoming ? { related, text: `unmarked ${identifier} as duplicate` } : { text: fallback }
    case 'task.relation_added':
      return { related, text: blocks ? (incoming ? `added blocker ${identifier}` : `blocks ${identifier}`) : `added related ${identifier}` }
    default:
      return { related, text: blocks ? (incoming ? `removed blocker ${identifier}` : `no longer blocks ${identifier}`) : `removed related ${identifier}` }
  }
}

/**
 * Parent and automation events (spec §6). The other task's key comes from the record's parent/ancestors, else from
 * the `*_project_id` metadata the server sends, else the task's own project.
 */
function subIssueActivity(
  action: string,
  metadata: Record<string, unknown>,
  identifierOf: (taskId: string, projectId: string | undefined) => string,
): Pick<TaskActivity, 'text' | 'related' | 'statusId'> | null {
  const text = (key: string) => (typeof metadata[key] === 'string' ? metadata[key] as string : undefined)
  // `to` → `to_project_id`, `source_task_id` → `source_project_id`
  const link = (key: string) => {
    const taskId = text(key)
    return taskId ? { taskId, identifier: identifierOf(taskId, text(`${key.replace(/_task_id$/, '')}_project_id`)) } : undefined
  }
  if (action === 'task.parent_changed') {
    const to = link('to')
    if (to) return { text: `set parent to ${to.identifier}`, related: to }
    const from = link('from')
    return from ? { text: `removed parent ${from.identifier}`, related: from } : { text: 'changed parent' }
  }
  if (action === 'task.auto_closed') {
    const statusId = text('to_status_id')
    const source = link('source_task_id')
    if (metadata.reason === 'parent_closed' && source) return { text: `closed automatically because ${source.identifier} was closed`, related: source, statusId }
    return { text: 'closed automatically because all sub-issues were done', statusId }
  }
  return null
}

/**
 * The fields a `task.updated` event changed, one entry each. `null` for an event without the `changes`
 * metadata (recorded before the server sent it); an empty list when only a field with its own event changed.
 */
function updateChanges(metadata: Record<string, unknown>, projectName: (projectId: string) => string): TaskChange[] | null {
  const changes = metadata.changes
  if (typeof changes !== 'object' || changes === null) return null
  const text = (value: unknown) => (typeof value === 'string' ? value : '')
  const ids = (value: unknown) => (Array.isArray(value) ? value.filter((id): id is string => typeof id === 'string') : [])
  return Object.entries(changes as Record<string, Record<string, unknown> | true>).flatMap(([field, value]): TaskChange[] => {
    if (field === 'description' || field === 'source_url') return [{ field }]
    if (typeof value !== 'object' || value === null) return []
    if (field === 'status' || field === 'priority' || field === 'title') return [{ field, from: text(value.from), to: text(value.to) }]
    if (field === 'project') return [{ field, from: projectName(text(value.from)), to: projectName(text(value.to)) }]
    if (field === 'assignees' || field === 'labels') return [{ field, added: ids(value.added), removed: ids(value.removed) }]
    if (field === 'due') return [{ field, start: text(value.start) || null, end: text(value.end) || null }]
    return []
  })
}

/** Human task id: project key + last four id characters, e.g. ORB-91C0. */
export function taskIdentifier(taskId: string, project: Pick<ProjectRecord, 'key'> | undefined): string {
  return `${project?.key ?? 'TASK'}-${taskId.slice(-4).toUpperCase()}`
}

/** A task named by id, title and project key: the "Parent title ›" label and the detail breadcrumb. */
export interface TaskKeyRef {
  id: string
  title: string
  projectKey: string
}

/** Identifier of a `TaskKeyRef`, same rule as `taskIdentifier`: ORB-91C0. */
export function refIdentifier(ref: Pick<TaskKeyRef, 'id' | 'projectKey'>): string {
  return `${ref.projectKey}-${ref.id.slice(-4).toUpperCase()}`
}

type WireRef = { id: string; title: string; project_key: string }
/** Sub-issue fields as the API sends them (`ancestors`: GET /tasks/{id} only). Read structurally. */
export type SubIssueWire = {
  parent_task_id?: string | null
  parent?: WireRef | null
  sub_issue_count?: number
  sub_issue_closed_count?: number
  ancestors?: WireRef[]
}
const keyRef = (ref: WireRef): TaskKeyRef => ({ id: ref.id, title: ref.title, projectKey: ref.project_key })

export function taskFromRecord(
  record: TaskRecord,
  project: ProjectRecord | undefined,
  comments: CommentRecord[] = [],
  attachments: AttachmentRecord[] = [],
  activity: AuditEvent[] = [],
  /** Every workspace project, so relation events can name tasks of other projects. */
  projects: ProjectRecord[] = [],
): Task {
  const projectFor = (projectId: string | undefined) => projectId
    ? projects.find((item) => item.id === projectId) ?? (project?.id === projectId ? project : undefined)
    : project
  const attachmentView = (attachment: AttachmentRecord): Attachment => ({
    id: attachment.id,
    fileName: attachment.display_name,
    mimeType: attachment.media_type,
    fileSize: attachment.byte_size,
    url: attachment.comment_id
      ? `/api/v1/workspaces/${attachment.workspace_id}/tasks/${attachment.task_id}/comments/${attachment.comment_id}/attachments/${attachment.id}/download`
      : `/api/v1/workspaces/${attachment.workspace_id}/tasks/${attachment.task_id}/attachments/${attachment.id}/download`,
  })
  const priority: TaskPriority = ['none', 'low', 'medium', 'high', 'urgent'].includes(record.priority)
    ? record.priority as TaskPriority
    : 'none'
  const wire = record as TaskRecord & SubIssueWire
  // tasks named in activity: the parent and ancestors carry their own project key
  const knownRefs = [...(wire.parent ? [wire.parent] : []), ...(wire.ancestors ?? [])]
  const identifierOf = (taskId: string, projectId: string | undefined) => {
    const ref = knownRefs.find((item) => item.id === taskId)
    return ref ? refIdentifier(keyRef(ref)) : taskIdentifier(taskId, projectFor(projectId))
  }
  return {
    id: record.id,
    identifier: taskIdentifier(record.id, project),
    title: record.title,
    description: record.description,
    sourceUrl: record.source_url ?? null,
    statusId: record.status_id,
    position: record.position,
    priority,
    assigneeIds: record.assignee_ids,
    creatorId: record.creator_id ?? undefined,
    creatorServiceAccountId: record.creator_service_account_id ?? undefined,
    creatorServiceAccountName: record.creator_service_account_name ?? undefined,
    projectId: record.project_id,
    labels: record.label_ids,
    attachments: attachments.filter((attachment) => !attachment.comment_id).map(attachmentView),
    dueStartAt: record.due_start_at ?? null,
    dueAt: record.due_at ?? null,
    createdAt: record.created_at,
    updatedAt: record.updated_at,
    comments: comments.map((comment) => ({
      id: comment.id,
      authorId: comment.author_id,
      body: comment.body,
      canEdit: comment.can_edit,
      canDelete: comment.can_delete,
      createdAt: comment.created_at,
      parentId: comment.parent_id ?? undefined,
      editedAt: comment.updated_at === comment.created_at ? null : comment.updated_at,
      attachments: attachments
        .filter((attachment) => attachment.comment_id === comment.id)
        .map(attachmentView),
      version: comment.version,
    })),
    activity: activity.flatMap((event): TaskActivity[] => {
      const metadata = (event.metadata ?? {}) as Record<string, unknown>
      const sentence: Pick<TaskActivity, 'text' | 'related' | 'statusId'> | null =
        relationActivity(event.action, metadata, identifierOf) ?? subIssueActivity(event.action, metadata, identifierOf)
      const base: TaskActivity = {
        id: event.id,
        actorId: event.actor_id ?? '',
        actorName: typeof metadata.actor_service_account_name === 'string' ? metadata.actor_service_account_name : undefined,
        actorServiceAccountId: typeof metadata.actor_service_account_id === 'string' ? metadata.actor_service_account_id : undefined,
        text: sentence?.text ?? genericActivityText(event.action),
        related: sentence?.related,
        statusId: sentence?.statusId,
        createdAt: event.occurred_at,
      }
      const changes = event.action === 'task.updated' ? updateChanges(metadata, (id) => projectFor(id)?.name ?? 'another project') : null
      // one row per changed field; a status change shows the new status glyph
      return changes
        ? changes.map((change) => ({ ...base, id: `${event.id}:${change.field}`, change, statusId: change.field === 'status' ? change.to : undefined }))
        : [base]
    }),
    duplicateOf: record.duplicate_of
      ? { id: record.duplicate_of.id, projectId: record.duplicate_of.project_id, title: record.duplicate_of.title }
      : null,
    blocked: record.blocked ?? false,
    parentTaskId: wire.parent_task_id ?? null,
    parent: wire.parent ? keyRef(wire.parent) : null,
    subIssueCount: wire.sub_issue_count ?? 0,
    subIssueClosedCount: wire.sub_issue_closed_count ?? 0,
    ancestors: (wire.ancestors ?? []).map(keyRef),
    version: record.version,
  }
}
