import { asDocument, type RichTextDocument } from '../../../components/editor/document'
import type { AttachmentRecord, AuditEvent, CommentRecord, LabelRecord, ProjectRecord, TaskRecord } from '../../../api/generated/types.gen'

export type StatusCategory = 'unstarted' | 'started' | 'completed' | 'cancelled'
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

export interface User {
  id: string
  membershipId: string
  name: string
  handle: string
  email: string
  role: 'Owner' | 'Admin' | 'Member'
  color: string
  online: boolean
  title: string
  roleIds: string[]
  version: number
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
  /** The stored rich text document. */
  bodyJson: RichTextDocument
  /** Plain text the server derives from the document; for copying and search. */
  bodyText: string
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
  createdAt: string
  statusId?: string
}

export interface TaskReference {
  sourceType: 'task' | 'comment'
  sourceId: string
  /** Where clicking the backlink navigates: the source task, or a comment's task. */
  sourceTaskId: string
  sourceTaskIdentifier: string
  sourceTaskTitle: string
}

export interface Task {
  id: string
  identifier: string
  title: string
  /** The stored rich text document. */
  descriptionJson: RichTextDocument
  /** Plain text the server derives from the document; what list rows and search read. */
  descriptionText: string
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
  dueAt: string | null
  /** The task this one is a sub-issue of (any project, same workspace). */
  parentId: string | null
  /** Live sub-issues, and how many of them are completed or cancelled. */
  subIssueTotal: number
  subIssueDone: number
  /** Set when this task is marked as a duplicate of another. */
  duplicateOfTaskId: string | null
  /** Tasks marked as duplicates of this one. Filled on detail reads only. */
  duplicateIds: string[]
  /** Tasks and comments whose rich text mentions this task. Detail reads only. */
  referencedBy: TaskReference[]
  createdAt: string
  updatedAt: string
  comments: TaskComment[]
  activity: TaskActivity[]
  version: number
}

export interface TaskViewState {
  currentUserId: string
  users: User[]
  statuses: TaskStatusDef[]
  labels: LabelRecord[]
  tasks: Task[]
}

/** Audit actions whose resource/verb split would not read as a sentence. */
const ACTIVITY_LABELS: Record<string, string> = {
  'task.bulk_updated': 'Updated task',
  'task.duplicate_marked': 'Marked as duplicate',
  'task.duplicate_unmarked': 'Unmarked as duplicate',
}

/** `task.updated` -> "Updated task". Underscores never reach the activity feed. */
export function activityText(action: string): string {
  const label = ACTIVITY_LABELS[action]
  if (label) return label
  return action
    .split('.')
    .reverse()
    .join(' ')
    .replace(/_/g, ' ')
    .replace(/^./, (letter) => letter.toUpperCase())
}

export function taskFromRecord(
  record: TaskRecord,
  comments: CommentRecord[] = [],
  attachments: AttachmentRecord[] = [],
  activity: AuditEvent[] = [],
): Task {
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
    identifier: record.identifier,
    title: record.title,
    descriptionJson: asDocument(record.description_json),
    descriptionText: record.description_text,
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
    dueAt: record.due_at ?? null,
    // The graph fields are tolerated missing: optimistic and partial records
    // (and payloads cached before the field existed) must still map.
    parentId: record.parent_id ?? null,
    subIssueTotal: record.sub_issue_total ?? 0,
    subIssueDone: record.sub_issue_done ?? 0,
    duplicateOfTaskId: record.duplicate_of_task_id ?? null,
    duplicateIds: record.duplicate_ids ?? [],
    referencedBy: (record.referenced_by ?? []).map((reference) => ({
      sourceType: reference.source_type === 'comment' ? 'comment' : 'task',
      sourceId: reference.source_id,
      sourceTaskId: reference.source_task_id,
      sourceTaskIdentifier: reference.source_task_identifier,
      sourceTaskTitle: reference.source_task_title,
    })),
    createdAt: record.created_at,
    updatedAt: record.updated_at,
    comments: comments.map((comment) => ({
      id: comment.id,
      authorId: comment.author_id,
      bodyJson: asDocument(comment.body_json),
      bodyText: comment.body_text,
      createdAt: comment.created_at,
      parentId: comment.parent_id ?? undefined,
      editedAt: comment.updated_at === comment.created_at ? null : comment.updated_at,
      attachments: attachments
        .filter((attachment) => attachment.comment_id === comment.id)
        .map(attachmentView),
      version: comment.version,
    })),
    activity: activity.map((event) => {
      const metadata = event.metadata as Record<string, unknown>
      return {
        id: event.id,
        actorId: event.actor_id ?? '',
        actorName: typeof metadata.actor_service_account_name === 'string' ? metadata.actor_service_account_name : undefined,
        actorServiceAccountId: typeof metadata.actor_service_account_id === 'string' ? metadata.actor_service_account_id : undefined,
        text: activityText(event.action),
        createdAt: event.occurred_at,
      }
    }),
    version: record.version,
  }
}
