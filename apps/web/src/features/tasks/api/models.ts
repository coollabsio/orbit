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
  createdAt: string
  parentId?: string
  attachments?: Attachment[]
  editedAt?: string | null
  version: number
}

export interface TaskActivity {
  id: string
  actorId: string
  actorName?: string
  actorServiceAccountId?: string
  text: string
  /** The other task of a relation event; the feed links `identifier` inside `text`. */
  related?: { taskId: string; identifier: string }
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
  'task.marked_duplicate': 'Marked as duplicate',
  'task.unmarked_duplicate': 'Unmarked as duplicate',
  'task.relation_added': 'Added relation',
  'task.relation_removed': 'Removed relation',
}

function genericActivityText(action: string): string {
  return action.split('.').reverse().join(' ').replace(/^./, (letter) => letter.toUpperCase())
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
      return { related, text: incoming ? `Marked ${identifier} as duplicate` : `Marked as duplicate of ${identifier}${title ? ` · ${title}` : ''}` }
    case 'task.unmarked_duplicate':
      return incoming ? { related, text: `Unmarked ${identifier} as duplicate` } : { text: fallback }
    case 'task.relation_added':
      return { related, text: blocks ? (incoming ? `Added blocker ${identifier}` : `Blocks ${identifier}`) : `Added related ${identifier}` }
    default:
      return { related, text: blocks ? (incoming ? `Removed blocker ${identifier}` : `No longer blocks ${identifier}`) : `Removed related ${identifier}` }
  }
}

/** Human task id: project key + last four id characters, e.g. ORB-91C0. */
export function taskIdentifier(taskId: string, project: Pick<ProjectRecord, 'key'> | undefined): string {
  return `${project?.key ?? 'TASK'}-${taskId.slice(-4).toUpperCase()}`
}

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
      createdAt: comment.created_at,
      parentId: comment.parent_id ?? undefined,
      editedAt: comment.updated_at === comment.created_at ? null : comment.updated_at,
      attachments: attachments
        .filter((attachment) => attachment.comment_id === comment.id)
        .map(attachmentView),
      version: comment.version,
    })),
    activity: activity.map((event) => {
      const metadata = (event.metadata ?? {}) as Record<string, unknown>
      const relation = relationActivity(event.action, metadata, (taskId, projectId) => taskIdentifier(taskId, projectFor(projectId)))
      return {
        id: event.id,
        actorId: event.actor_id ?? '',
        actorName: typeof metadata.actor_service_account_name === 'string' ? metadata.actor_service_account_name : undefined,
        actorServiceAccountId: typeof metadata.actor_service_account_id === 'string' ? metadata.actor_service_account_id : undefined,
        text: relation?.text ?? genericActivityText(event.action),
        related: relation?.related,
        createdAt: event.occurred_at,
      }
    }),
    duplicateOf: record.duplicate_of
      ? { id: record.duplicate_of.id, projectId: record.duplicate_of.project_id, title: record.duplicate_of.title }
      : null,
    blocked: record.blocked ?? false,
    version: record.version,
  }
}
