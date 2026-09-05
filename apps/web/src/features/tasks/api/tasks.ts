import { useMutation, useInfiniteQuery, useQueries, useQuery, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import { apiClient } from '../../../api/client'
import type { createApiClient } from '../../../api/client'
import {
  bulkTasks,
  createAttachmentComment,
  createComment,
  createTask,
  deleteComment,
  deleteTask,
  deleteTaskAttachment,
  getTask,
  listComments,
  listCommentAttachments,
  listTaskAttachments,
  listTaskTrash,
  listTasks,
  reorderTasks,
  restoreTask,
  updateComment,
  updateTask,
  uploadCommentAttachments,
  uploadTaskAttachments,
} from '../../../api/generated/sdk.gen'
import type {
  BulkItem,
  CreateTaskBody,
  ListTasksData,
  PageTaskRecord,
  ReorderItem,
  TaskRecord,
  TaskUpdateBody,
  CommentRecord,
} from '../../../api/generated/types.gen'
import { queryKeys } from '../../../api/queryKeys'
import { isTaskVersionConflict } from './conflicts'
import { commentUploadMode } from './commentUpload'
import { patchWorkspaceTask, reconcileWorkspaceTask, restoreWorkspaceTasks, type WorkspaceTaskSnapshot } from './optimistic'

type ApiClient = ReturnType<typeof createApiClient>
export type TaskFilters = NonNullable<ListTasksData['query']>

export async function taskListPage(
  client: ApiClient,
  workspaceId: string,
  filters: TaskFilters,
  cursor?: string,
): Promise<PageTaskRecord> {
  const { data } = await listTasks({
    client,
    path: { workspace_id: workspaceId },
    query: { ...filters, cursor },
    throwOnError: true,
  })
  if (!data) throw new Error('Tasks response was empty.')
  return data
}

export function nextTaskCursor(page: PageTaskRecord): string | undefined {
  return page.next_cursor ?? undefined
}

function required<T>(data: T | undefined, message: string): T {
  if (data === undefined) throw new Error(message)
  return data
}

export function useTasks(workspaceId: string, filters: TaskFilters = {}) {
  return useInfiniteQuery({
    queryKey: queryKeys.tasks.list(workspaceId, filters),
    initialPageParam: undefined as string | undefined,
    queryFn: ({ pageParam }) => taskListPage(apiClient, workspaceId, filters, pageParam),
    getNextPageParam: nextTaskCursor,
  })
}

export function useTask(workspaceId: string, taskId: string | undefined) {
  return useQuery({
    queryKey: queryKeys.tasks.detail(workspaceId, taskId ?? ''),
    enabled: Boolean(taskId),
    queryFn: async () => {
      const { data } = await getTask({ client: apiClient, path: { workspace_id: workspaceId, task_id: taskId! }, throwOnError: true })
      return required(data, 'Task response was empty.')
    },
  })
}

export function useTaskComments(workspaceId: string, taskId: string | undefined) {
  return useQuery({
    queryKey: queryKeys.comments(workspaceId, taskId ?? ''),
    enabled: Boolean(taskId),
    queryFn: async () => {
      const { data } = await listComments({ client: apiClient, path: { workspace_id: workspaceId, task_id: taskId! }, query: { limit: 100 }, throwOnError: true })
      return required(data, 'Comments response was empty.').items
    },
  })
}

export function useTaskAttachments(workspaceId: string, taskId: string | undefined) {
  return useQuery({
    queryKey: queryKeys.attachments(workspaceId, taskId ?? ''),
    enabled: Boolean(taskId),
    queryFn: async () => {
      const { data } = await listTaskAttachments({ client: apiClient, path: { workspace_id: workspaceId, task_id: taskId! }, query: { limit: 100 }, throwOnError: true })
      return required(data, 'Attachments response was empty.').items
    },
  })
}

export function useCommentAttachments(workspaceId: string, taskId: string | undefined, comments: CommentRecord[]) {
  const results = useQueries({
    queries: taskId ? comments.map((comment) => ({
      queryKey: queryKeys.commentAttachments(workspaceId, taskId, comment.id),
      queryFn: async () => {
        const { data } = await listCommentAttachments({ client: apiClient, path: { workspace_id: workspaceId, task_id: taskId, comment_id: comment.id }, query: { limit: 100 }, throwOnError: true })
        return required(data, 'Comment attachments response was empty.').items
      },
    })) : [],
  })
  return {
    data: results.flatMap((result) => result.data ?? []),
    isPending: results.some((result) => result.isPending),
    isError: results.some((result) => result.isError),
  }
}

export function useTaskTrash(workspaceId: string) {
  return useQuery({
    queryKey: queryKeys.taskTrash(workspaceId),
    queryFn: async () => {
      const { data } = await listTaskTrash({ client: apiClient, path: { workspace_id: workspaceId }, query: { limit: 100 }, throwOnError: true })
      return required(data, 'Task trash response was empty.').items
    },
  })
}

function promptForConflict(error: Error, refresh: () => void) {
  if (isTaskVersionConflict(error) && window.confirm('This task changed on the server. Refresh it now?')) refresh()
}

function optimisticTaskPatch(body: Omit<TaskUpdateBody, 'expected_version'>): Partial<TaskRecord> {
  return Object.fromEntries(Object.entries(body).filter(([, value]) => value != null)) as Partial<TaskRecord>
}

export function useCreateTask(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (body: CreateTaskBody) => {
      const { data } = await createTask({ client: apiClient, path: { workspace_id: workspaceId }, body, throwOnError: true })
      return required(data, 'Create task response was empty.')
    },
    onSuccess: (record) => {
      queryClient.setQueryData(queryKeys.tasks.detail(workspaceId, record.id), record)
      void queryClient.invalidateQueries({ queryKey: queryKeys.tasks.all(workspaceId) })
    },
  })
}

export function useUpdateTask(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation<TaskRecord, Error, { taskId: string; body: TaskUpdateBody }, WorkspaceTaskSnapshot>({
    mutationFn: async ({ taskId, body }) => {
      const { data } = await updateTask({ client: apiClient, path: { workspace_id: workspaceId, task_id: taskId }, body, throwOnError: true })
      return required(data, 'Update task response was empty.')
    },
    onMutate: async ({ taskId, body }) => {
      await queryClient.cancelQueries({ queryKey: queryKeys.tasks.all(workspaceId) })
      const { expected_version: _version, ...patch } = body
      return patchWorkspaceTask(queryClient, workspaceId, taskId, optimisticTaskPatch(patch))
    },
    onError: (error, _input, snapshot) => {
      if (snapshot) restoreWorkspaceTasks(queryClient, snapshot)
      promptForConflict(error, () => void queryClient.invalidateQueries({ queryKey: queryKeys.tasks.all(workspaceId) }))
    },
    onSuccess: (record) => reconcileWorkspaceTask(queryClient, workspaceId, record),
    onSettled: () => void queryClient.invalidateQueries({ queryKey: queryKeys.tasks.all(workspaceId) }),
  })
}

function snapshotBulk(queryClient: ReturnType<typeof useQueryClient>, workspaceId: string, updates: BulkItem[]): WorkspaceTaskSnapshot {
  const [first, ...rest] = updates
  if (!first) return { entries: [] }
  const { expected_version: _version, id, ...patch } = first
  const snapshot = patchWorkspaceTask(queryClient, workspaceId, id, optimisticTaskPatch(patch))
  for (const update of rest) {
    const { expected_version: _otherVersion, id: taskId, ...otherPatch } = update
    patchWorkspaceTask(queryClient, workspaceId, taskId, optimisticTaskPatch(otherPatch))
  }
  return snapshot
}

export function useBulkTasks(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation<PageTaskRecord, Error, BulkItem[], WorkspaceTaskSnapshot>({
    mutationFn: async (updates) => {
      const { data } = await bulkTasks({ client: apiClient, path: { workspace_id: workspaceId }, body: { updates }, throwOnError: true })
      return required(data, 'Bulk task response was empty.')
    },
    onMutate: async (updates) => {
      await queryClient.cancelQueries({ queryKey: queryKeys.tasks.all(workspaceId) })
      return snapshotBulk(queryClient, workspaceId, updates)
    },
    onError: (error, _input, snapshot) => {
      if (snapshot) restoreWorkspaceTasks(queryClient, snapshot)
      promptForConflict(error, () => void queryClient.invalidateQueries({ queryKey: queryKeys.tasks.all(workspaceId) }))
    },
    onSuccess: (page) => page.items.forEach((record) => reconcileWorkspaceTask(queryClient, workspaceId, record)),
    onSettled: () => void queryClient.invalidateQueries({ queryKey: queryKeys.tasks.all(workspaceId) }),
  })
}

export function useReorderTasks(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation<PageTaskRecord, Error, ReorderItem[], WorkspaceTaskSnapshot>({
    mutationFn: async (items) => {
      const { data } = await reorderTasks({ client: apiClient, path: { workspace_id: workspaceId }, body: { items }, throwOnError: true })
      return required(data, 'Reorder response was empty.')
    },
    onMutate: async (items) => {
      await queryClient.cancelQueries({ queryKey: queryKeys.tasks.all(workspaceId) })
      return snapshotBulk(queryClient, workspaceId, items.map((item) => ({ ...item })))
    },
    onError: (error, _input, snapshot) => {
      if (snapshot) restoreWorkspaceTasks(queryClient, snapshot)
      promptForConflict(error, () => void queryClient.invalidateQueries({ queryKey: queryKeys.tasks.all(workspaceId) }))
    },
    onSuccess: (page) => page.items.forEach((record) => reconcileWorkspaceTask(queryClient, workspaceId, record)),
    onSettled: () => void queryClient.invalidateQueries({ queryKey: queryKeys.tasks.all(workspaceId) }),
  })
}

export function useDeleteTask(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async ({ taskId, version }: { taskId: string; version: number }) => {
      await deleteTask({ client: apiClient, path: { workspace_id: workspaceId, task_id: taskId }, query: { expected_version: version }, throwOnError: true })
      return taskId
    },
    onSuccess: (taskId) => {
      queryClient.removeQueries({ queryKey: queryKeys.tasks.detail(workspaceId, taskId) })
      void queryClient.invalidateQueries({ queryKey: queryKeys.tasks.all(workspaceId) })
      void queryClient.invalidateQueries({ queryKey: queryKeys.taskTrash(workspaceId) })
    },
  })
}

export function useRestoreTask(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async ({ taskId, version }: { taskId: string; version: number }) => {
      const { data } = await restoreTask({ client: apiClient, path: { workspace_id: workspaceId, task_id: taskId }, body: { expected_version: version }, throwOnError: true })
      return required(data, 'Restore task response was empty.')
    },
    onError: (error) => promptForConflict(error, () => void queryClient.invalidateQueries({ queryKey: queryKeys.taskTrash(workspaceId) })),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.tasks.all(workspaceId) })
      void queryClient.invalidateQueries({ queryKey: queryKeys.taskTrash(workspaceId) })
    },
  })
}

export function useCreateTaskComment(workspaceId: string, taskId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async ({ body, parentId, files }: { body: string; parentId?: string; files: File[] }) => {
      let commentId: string
      const mode = commentUploadMode(body, files.length)
      if (mode === 'text') {
        const response = await createComment({ client: apiClient, path: { workspace_id: workspaceId, task_id: taskId }, body: { body, parent_id: parentId }, throwOnError: true })
        commentId = required(response.data, 'Create comment response was empty.').id
        for (const file of files) await uploadCommentAttachments({ client: apiClient, path: { workspace_id: workspaceId, task_id: taskId, comment_id: commentId }, body: { file }, throwOnError: true })
      } else if (mode === 'attachment-only') {
        const [first, ...rest] = files
        if (!first) throw new Error('A comment attachment was missing.')
        const response = await createAttachmentComment({ client: apiClient, path: { workspace_id: workspaceId, task_id: taskId }, body: { file: first }, throwOnError: true })
        commentId = required(response.data, 'Attachment comment response was empty.').comment.id
        for (const file of rest) await uploadCommentAttachments({ client: apiClient, path: { workspace_id: workspaceId, task_id: taskId, comment_id: commentId }, body: { file }, throwOnError: true })
      } else throw new Error('A comment needs text or an attachment.')
      return commentId
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.comments(workspaceId, taskId) })
      void queryClient.invalidateQueries({ queryKey: queryKeys.attachments(workspaceId, taskId) })
    },
  })
}

export function useUpdateTaskComment(workspaceId: string, taskId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async ({ commentId, body, version }: { commentId: string; body: string; version: number }) => {
      const { data } = await updateComment({ client: apiClient, path: { workspace_id: workspaceId, task_id: taskId, comment_id: commentId }, body: { body, expected_version: version }, throwOnError: true })
      return required(data, 'Update comment response was empty.')
    },
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.comments(workspaceId, taskId) }),
  })
}

export function useDeleteTaskComment(workspaceId: string, taskId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async ({ commentId, version }: { commentId: string; version: number }) => {
      await deleteComment({ client: apiClient, path: { workspace_id: workspaceId, task_id: taskId, comment_id: commentId }, query: { expected_version: version }, throwOnError: true })
    },
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.comments(workspaceId, taskId) }),
  })
}

export function useUploadTaskAttachments(workspaceId: string, taskId: string) {
  const queryClient = useQueryClient()
  const [progress, setProgress] = useState(0)
  const mutation = useMutation({
    mutationFn: async (files: File[]) => {
      setProgress(0)
      for (const [index, file] of files.entries()) {
        await uploadTaskAttachments({ client: apiClient, path: { workspace_id: workspaceId, task_id: taskId }, body: { file }, throwOnError: true })
        setProgress(Math.round(((index + 1) / files.length) * 100))
      }
    },
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.attachments(workspaceId, taskId) }),
  })
  return { ...mutation, progress }
}

export function useDeleteTaskAttachment(workspaceId: string, taskId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (attachmentId: string) => {
      await deleteTaskAttachment({ client: apiClient, path: { workspace_id: workspaceId, task_id: taskId, attachment_id: attachmentId }, throwOnError: true })
    },
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.attachments(workspaceId, taskId) }),
  })
}
