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
  text: string
  createdAt: string
  statusId?: string
}

export interface Task {
  id: string
  identifier: string
  title: string
  description: string
  statusId: string
  position: number
  priority: TaskPriority
  assigneeIds: string[]
  creatorId: string
  projectId: string
  labels: string[]
  attachments: Attachment[]
  dueAt: string | null
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
    description: record.description,
    statusId: record.status_id,
    position: record.position,
    priority,
    assigneeIds: record.assignee_ids,
    creatorId: record.creator_id,
    projectId: record.project_id,
    labels: record.label_ids,
    attachments: attachments.filter((attachment) => !attachment.comment_id).map(attachmentView),
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
    activity: activity.map((event) => ({
      id: event.id,
      actorId: event.actor_id ?? '',
      text: event.action.split('.').map((part, index) => index === 0 ? part : part).reverse().join(' ').replace(/^./, (letter) => letter.toUpperCase()),
      createdAt: event.occurred_at,
    })),
    version: record.version,
  }
}
