/**
 * The task calls the detail page and the New task dialog make beyond `orbit.ts`: relations, sub-issues, task search,
 * labels, attachments, and a create that takes every property. Plain functions over the generated SDK, so the
 * screens and `scripts/api-check-tasks.ts` share them.
 */
import {
  createLabel,
  createTask,
  createTaskRelation,
  deleteLabel,
  deleteTaskAttachment,
  deleteTaskRelation,
  emptyTaskTrash,
  getTrashedProject,
  getTrashedTask,
  listCommentAttachments,
  listProjectTrash,
  listTaskAttachments,
  listTaskRelations,
  listTasks,
  listTaskTrash,
  listTrashedProjectTasks,
  purgeProject,
  purgeTask,
  restoreProject,
  restoreTask,
  uploadTaskAttachments,
} from '@orbit/api/generated/sdk.gen'
import type { AttachmentRecord, CreateTaskBody, NewTaskRelationType, TaskRelationRecord, TrashedProject, TrashedTask } from '@orbit/api/generated/types.gen'

import type { LabelRecord, OrbitClient, ProjectRecord, TaskRecord } from './orbit'
import type { PickedFile } from './tasks'

export type { AttachmentRecord, CreateTaskBody, NewTaskRelationType, TaskRelationRecord, TrashedProject, TrashedTask }

const task = (client: OrbitClient, workspaceId: string, taskId: string) => ({ client, path: { workspace_id: workspaceId, task_id: taskId }, throwOnError: true as const })

/** Follows `next_cursor` until the server has no more. */
async function allPages<T>(page: (cursor: string | undefined) => Promise<{ items: T[]; next_cursor?: string | null }>): Promise<T[]> {
  const items: T[] = []
  let cursor: string | undefined
  do {
    const next = await page(cursor)
    items.push(...next.items)
    cursor = next.next_cursor ?? undefined
  } while (cursor)
  return items
}

export async function createTaskWith(client: OrbitClient, workspaceId: string, body: CreateTaskBody): Promise<TaskRecord> {
  return (await createTask({ client, path: { workspace_id: workspaceId }, body, throwOnError: true })).data
}

/** The draft of the New task dialog as the create body: trimmed text, and no description when it is empty. */
export function newTaskBody(draft: {
  title: string
  description: string
  projectId: string
  statusId: string
  priority: string
  assigneeIds: readonly string[]
  labelIds: readonly string[]
  dueAt: string | null
  dueStartAt: string | null
  parentId: string | null
}): CreateTaskBody {
  const description = draft.description.trim()
  return {
    title: draft.title.trim(),
    ...(description ? { description } : {}),
    project_id: draft.projectId,
    status_id: draft.statusId,
    priority: draft.priority,
    assignee_ids: [...draft.assigneeIds],
    label_ids: [...draft.labelIds],
    due_at: draft.dueAt,
    due_start_at: draft.dueStartAt,
    parent_task_id: draft.parentId,
  }
}

/** Recently updated tasks, or the server's search over titles, descriptions and identifiers (`ENG-12`). */
export async function searchTasks(client: OrbitClient, workspaceId: string, search: string): Promise<TaskRecord[]> {
  const query = { sort: 'updated_at', order: 'desc', limit: 100, search: search.trim() || undefined }
  return (await listTasks({ client, path: { workspace_id: workspaceId }, query, throwOnError: true })).data.items
}

/** Direct sub-issues, oldest first. */
export function subIssues(client: OrbitClient, workspaceId: string, parentId: string): Promise<TaskRecord[]> {
  return allPages(
    async (cursor) =>
      (await listTasks({ client, path: { workspace_id: workspaceId }, query: { parent_task_id: parentId, sort: 'created_at', order: 'asc', limit: 100, cursor }, throwOnError: true }))
        .data,
  )
}

export async function relations(client: OrbitClient, workspaceId: string, taskId: string): Promise<TaskRelationRecord[]> {
  return (await listTaskRelations(task(client, workspaceId, taskId))).data
}

export async function addRelation(client: OrbitClient, workspaceId: string, taskId: string, body: { type: NewTaskRelationType; task_id: string }): Promise<TaskRelationRecord> {
  return (await createTaskRelation({ ...task(client, workspaceId, taskId), body })).data
}

export async function removeRelation(client: OrbitClient, workspaceId: string, taskId: string, relationId: string): Promise<void> {
  await deleteTaskRelation({ client, path: { workspace_id: workspaceId, task_id: taskId, relation_id: relationId }, throwOnError: true })
}

export async function newLabel(client: OrbitClient, workspaceId: string, projectId: string, body: { name: string; color: string }): Promise<LabelRecord> {
  return (await createLabel({ client, path: { workspace_id: workspaceId, project_id: projectId }, body, throwOnError: true })).data
}

export async function removeLabel(client: OrbitClient, workspaceId: string, label: Pick<LabelRecord, 'id' | 'project_id' | 'version'>): Promise<void> {
  await deleteLabel({ client, path: { workspace_id: workspaceId, project_id: label.project_id, label_id: label.id }, query: { expected_version: label.version }, throwOnError: true })
}

/** Files attached to the task itself (before rich text). Inline files of the description and comments are not in it. */
export function taskAttachments(client: OrbitClient, workspaceId: string, taskId: string): Promise<AttachmentRecord[]> {
  return allPages(async (cursor) => (await listTaskAttachments({ ...task(client, workspaceId, taskId), query: { limit: 100, cursor } })).data)
}

/** Files attached to a comment (before rich text). */
export function commentAttachments(client: OrbitClient, workspaceId: string, taskId: string, commentId: string): Promise<AttachmentRecord[]> {
  return allPages(
    async (cursor) =>
      (await listCommentAttachments({ client, path: { workspace_id: workspaceId, task_id: taskId, comment_id: commentId }, query: { limit: 100, cursor }, throwOnError: true })).data,
  )
}

/**
 * Uploads a file for a comment's or the description's text (`inline`), as the web's paperclip does; the text then
 * links it by its download path. The app's part is an expo-file-system `File`, which the SDK's serializer would turn
 * into JSON, so this one builds the form itself.
 */
export async function uploadInline(client: OrbitClient, workspaceId: string, taskId: string, file: PickedFile): Promise<AttachmentRecord> {
  const { data } = await uploadTaskAttachments({
    ...task(client, workspaceId, taskId),
    query: { inline: '1' },
    body: { file: file.blob },
    bodySerializer: () => {
      const form = new FormData()
      if (file.blob instanceof Blob) form.append('file', file.blob, file.name)
      else form.append('file', file.blob)
      return form
    },
  })
  return data
}

export async function removeAttachment(client: OrbitClient, workspaceId: string, taskId: string, attachmentId: string): Promise<void> {
  await deleteTaskAttachment({ client, path: { workspace_id: workspaceId, task_id: taskId, attachment_id: attachmentId }, throwOnError: true })
}

/** Projects in the trash, newest first. Their tasks are hidden with them. */
export function projectTrash(client: OrbitClient, workspaceId: string): Promise<ProjectRecord[]> {
  return allPages(async (cursor) => (await listProjectTrash({ client, path: { workspace_id: workspaceId }, query: { limit: 100, cursor }, throwOnError: true })).data)
}

/** Tasks in the trash of live projects, newest first. */
export function taskTrash(client: OrbitClient, workspaceId: string): Promise<TaskRecord[]> {
  return allPages(async (cursor) => (await listTaskTrash({ client, path: { workspace_id: workspaceId }, query: { limit: 100, cursor }, throwOnError: true })).data)
}

export async function restoreTrashedProject(client: OrbitClient, workspaceId: string, project: Pick<ProjectRecord, 'id' | 'version'>): Promise<ProjectRecord> {
  return (await restoreProject({ client, path: { workspace_id: workspaceId, project_id: project.id }, body: { expected_version: project.version }, throwOnError: true })).data
}

export async function restoreTrashedTask(client: OrbitClient, workspaceId: string, item: Pick<TaskRecord, 'id' | 'version'>): Promise<TaskRecord> {
  return (await restoreTask({ ...task(client, workspaceId, item.id), body: { expected_version: item.version } })).data
}

/** Deletes a trashed project forever with all its tasks. */
export async function purgeTrashedProject(client: OrbitClient, workspaceId: string, projectId: string): Promise<void> {
  await purgeProject({ client, path: { workspace_id: workspaceId, project_id: projectId }, throwOnError: true })
}

/** Deletes forever every trashed task and every trashed project with its tasks. */
export async function emptyTrash(client: OrbitClient, workspaceId: string): Promise<{ purged_tasks: number; purged_projects: number }> {
  return (await emptyTaskTrash({ client, path: { workspace_id: workspaceId }, throwOnError: true })).data
}

/** A task the trash shows (trashed, or in a trashed project) with its status, labels and who trashed it. */
export async function trashedTask(client: OrbitClient, workspaceId: string, taskId: string): Promise<TrashedTask> {
  return (await getTrashedTask(task(client, workspaceId, taskId))).data
}

/** A trashed project with its statuses and who trashed it. */
export async function trashedProject(client: OrbitClient, workspaceId: string, projectId: string): Promise<TrashedProject> {
  return (await getTrashedProject({ client, path: { workspace_id: workspaceId, project_id: projectId }, throwOnError: true })).data
}

/** Every task of a trashed project, by number. */
export function trashedProjectTasks(client: OrbitClient, workspaceId: string, projectId: string): Promise<TaskRecord[]> {
  return allPages(async (cursor) => (await listTrashedProjectTasks({ client, path: { workspace_id: workspaceId, project_id: projectId }, query: { limit: 100, cursor }, throwOnError: true })).data)
}

/** Deletes a trashed task forever with the sub-issues trashed with it. */
export async function purgeTrashedTask(client: OrbitClient, workspaceId: string, taskId: string): Promise<void> {
  await purgeTask(task(client, workspaceId, taskId))
}
