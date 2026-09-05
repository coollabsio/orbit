import { keepPreviousData, useMutation, useInfiniteQuery, useQueries, useQuery, useQueryClient } from '@tanstack/react-query'
import { useRef, useState } from 'react'
import { apiClient } from '../../../api/client'
import type { createApiClient } from '../../../api/client'
import { fetchAllPages } from '../../../api/pagination'
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
  listTaskActivity,
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
import { PartialUploadError, uploadFiles } from './uploadQueue'

type ApiClient = ReturnType<typeof createApiClient>
export type TaskFilters = NonNullable<ListTasksData['query']>
export const MAX_BULK_TASK_UPDATES = 100

export class BulkTaskLimitError extends Error {
  count: number

  constructor(count: number) {
    super(`Bulk task operations accept at most ${MAX_BULK_TASK_UPDATES} updates; received ${count}.`)
    this.count = count
  }
}

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

export async function taskListAllPages(
  client: ApiClient,
  workspaceId: string,
  filters: TaskFilters,
): Promise<PageTaskRecord> {
  const items: TaskRecord[] = []
  let cursor: string | undefined
  do {
    const page = await taskListPage(client, workspaceId, filters, cursor)
    items.push(...page.items)
    cursor = nextTaskCursor(page)
  } while (cursor)
  return { items, next_cursor: null }
}

export function nextTaskCursor(page: PageTaskRecord): string | undefined {
  return page.next_cursor ?? undefined
}

function required<T>(data: T | undefined, message: string): T {
  if (data === undefined) throw new Error(message)
  return data
}

export function useTasks(workspaceId: string, filters: TaskFilters = {}, exhaustive = false) {
  return useInfiniteQuery({
    queryKey: queryKeys.tasks.list(workspaceId, { ...filters, exhaustive }),
    initialPageParam: undefined as string | undefined,
    queryFn: ({ pageParam }) => exhaustive
      ? taskListAllPages(apiClient, workspaceId, filters)
      : taskListPage(apiClient, workspaceId, filters, pageParam),
    getNextPageParam: nextTaskCursor,
    placeholderData: keepPreviousData,
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
      const page = await fetchAllPages(async (cursor) => {
        const { data } = await listComments({ client: apiClient, path: { workspace_id: workspaceId, task_id: taskId! }, query: { limit: 100, cursor }, throwOnError: true })
        return required(data, 'Comments response was empty.')
      })
      return page.items
    },
  })
}

export function useTaskActivity(workspaceId: string, taskId: string | undefined) {
  return useQuery({
    queryKey: queryKeys.taskActivity(workspaceId, taskId ?? ''),
    enabled: Boolean(taskId),
    queryFn: async () => {
      const page = await fetchAllPages(async (cursor) => {
        const { data } = await listTaskActivity({ client: apiClient, path: { workspace_id: workspaceId, task_id: taskId! }, query: { limit: 100, cursor }, throwOnError: true })
        return required(data, 'Task activity response was empty.')
      })
      return page.items
    },
  })
}

export function useTaskAttachments(workspaceId: string, taskId: string | undefined) {
  return useQuery({
    queryKey: queryKeys.attachments(workspaceId, taskId ?? ''),
    enabled: Boolean(taskId),
    queryFn: async () => {
      const page = await fetchAllPages(async (cursor) => {
        const { data } = await listTaskAttachments({ client: apiClient, path: { workspace_id: workspaceId, task_id: taskId! }, query: { limit: 100, cursor }, throwOnError: true })
        return required(data, 'Attachments response was empty.')
      })
      return page.items
    },
  })
}

export function useCommentAttachments(workspaceId: string, taskId: string | undefined, comments: CommentRecord[]) {
  const results = useQueries({
    queries: taskId ? comments.map((comment) => ({
      queryKey: queryKeys.commentAttachments(workspaceId, taskId, comment.id),
      queryFn: async () => {
        const page = await fetchAllPages(async (cursor) => {
          const { data } = await listCommentAttachments({ client: apiClient, path: { workspace_id: workspaceId, task_id: taskId, comment_id: comment.id }, query: { limit: 100, cursor }, throwOnError: true })
          return required(data, 'Comment attachments response was empty.')
        })
        return page.items
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
      const page = await fetchAllPages(async (cursor) => {
        const { data } = await listTaskTrash({ client: apiClient, path: { workspace_id: workspaceId }, query: { limit: 100, cursor }, throwOnError: true })
        return required(data, 'Task trash response was empty.')
      })
      return page.items
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
  const mutation = useMutation<PageTaskRecord, Error, BulkItem[], WorkspaceTaskSnapshot>({
    mutationFn: async (updates) => {
      if (updates.length > MAX_BULK_TASK_UPDATES) throw new BulkTaskLimitError(updates.length)
      const { data } = await bulkTasks({ client: apiClient, path: { workspace_id: workspaceId }, body: { updates }, throwOnError: true })
      return required(data, 'Bulk task response was empty.')
    },
    onMutate: async (updates) => {
      if (updates.length > MAX_BULK_TASK_UPDATES) return { entries: [] }
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
  return {
    ...mutation,
    retry: () => {
      if (mutation.variables && !(mutation.error instanceof BulkTaskLimitError)) mutation.mutate(mutation.variables)
    },
  }
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
  const [progress, setProgress] = useState<number>()
  const [remainingCount, setRemainingCount] = useState(0)
  const resume = useRef<{ signature: string; commentId: string; version: number; remaining: File[]; total: number } | null>(null)
  const mutation = useMutation({
    mutationFn: async (input: { body: string; parentId?: string; files: File[] }) => {
      const { body, parentId, files } = input
      const signature = JSON.stringify([body, parentId ?? null, files.map((file) => [file.name, file.size, file.type, file.lastModified])])
      if (resume.current && resume.current.signature !== signature) {
        await deleteComment({
          client: apiClient,
          path: { workspace_id: workspaceId, task_id: taskId, comment_id: resume.current.commentId },
          query: { expected_version: resume.current.version },
          throwOnError: true,
        })
        resume.current = null
      }

      let state = resume.current
      if (!state) {
        let comment: CommentRecord
        let remaining = files
        setProgress(files.length > 0 ? 0 : undefined)
        setRemainingCount(files.length)
        const mode = commentUploadMode(body, files.length)
        if (mode === 'text') {
          const response = await createComment({ client: apiClient, path: { workspace_id: workspaceId, task_id: taskId }, body: { body, parent_id: parentId }, throwOnError: true })
          comment = required(response.data, 'Create comment response was empty.')
        } else if (mode === 'attachment-only') {
          const [first, ...rest] = files
          if (!first) throw new Error('A comment attachment was missing.')
          const response = await createAttachmentComment({ client: apiClient, path: { workspace_id: workspaceId, task_id: taskId }, body: { file: first }, throwOnError: true })
          comment = required(response.data, 'Attachment comment response was empty.').comment
          remaining = rest
          setProgress(Math.round(100 / files.length))
        } else {
          throw new Error('A comment needs text or an attachment.')
        }
        state = { signature, commentId: comment.id, version: comment.version, remaining, total: files.length }
        resume.current = state
      }

      const completedBefore = state.total - state.remaining.length
      try {
        await uploadFiles(
          state.remaining,
          (file) => uploadCommentAttachments({ client: apiClient, path: { workspace_id: workspaceId, task_id: taskId, comment_id: state.commentId }, body: { file }, throwOnError: true }),
          (remainingProgress) => setProgress(Math.round(((completedBefore + remainingProgress * state.remaining.length / 100) / state.total) * 100)),
        )
      } catch (error) {
        if (error instanceof PartialUploadError) {
          state.remaining = error.remaining
          setRemainingCount(error.remaining.length)
        }
        throw error
      }
      const commentId = state.commentId
      resume.current = null
      setRemainingCount(0)
      if (state.total > 0) setProgress(100)
      return commentId
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.comments(workspaceId, taskId) })
      void queryClient.invalidateQueries({ queryKey: queryKeys.attachments(workspaceId, taskId) })
    },
  })
  return { ...mutation, progress, remainingCount }
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
  const [remainingCount, setRemainingCount] = useState(0)
  const remaining = useRef<File[]>([])
  const mutation = useMutation({
    mutationFn: async (files: File[]) => {
      setProgress(0)
      setRemainingCount(files.length)
      try {
        await uploadFiles(
          files,
          (file) => uploadTaskAttachments({ client: apiClient, path: { workspace_id: workspaceId, task_id: taskId }, body: { file }, throwOnError: true }),
          setProgress,
        )
      } catch (error) {
        if (error instanceof PartialUploadError) {
          remaining.current = error.remaining
          setRemainingCount(error.remaining.length)
        }
        throw error
      }
      remaining.current = []
      setRemainingCount(0)
    },
    onSettled: () => void queryClient.invalidateQueries({ queryKey: queryKeys.attachments(workspaceId, taskId) }),
  })
  return { ...mutation, progress, remainingCount, retry: () => mutation.mutate(remaining.current) }
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
