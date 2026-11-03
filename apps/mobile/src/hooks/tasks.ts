/** The queries and writes of the task detail page and the New task dialog, over `data/task-api.ts`. */
import { useMutation, useQueries, useQuery, useQueryClient } from '@tanstack/react-query'
import { useDeferredValue, useRef } from 'react'
import { Alert } from 'react-native'

import { keys } from '../data/keys'
import * as api from '../data/orbit'
import * as taskApi from '../data/task-api'
import { isVersionConflict, optimisticTask, parentErrorMessage, replaceTask, type TaskPatch } from '../data/tasks'
import { useOrbit } from '../orbit-context'
import { errorMessage } from '../session-context'

/**
 * Query keys of these hooks. Each sits under a prefix the workspace events socket already refreshes (`task`,
 * `tasks`, `comments`), so another member's change shows here too.
 */
export const taskKeys = {
  relations: (workspaceId: string, taskId: string) => [...keys.task(workspaceId, taskId), 'relations'] as const,
  attachments: (workspaceId: string, taskId: string) => [...keys.task(workspaceId, taskId), 'attachments'] as const,
  commentAttachments: (workspaceId: string, taskId: string, commentId: string) => [...keys.comments(workspaceId, taskId), commentId, 'attachments'] as const,
  subIssues: (workspaceId: string, parentId: string) => [...keys.tasks(workspaceId), 'sub-issues', parentId] as const,
  search: (workspaceId: string, query: string) => [...keys.tasks(workspaceId), 'search', query] as const,
}

export function useTaskRelations(taskId: string) {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: taskKeys.relations(workspaceId, taskId), queryFn: () => taskApi.relations(client, workspaceId, taskId) })
}

export function useSubIssues(taskId: string) {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: taskKeys.subIssues(workspaceId, taskId), queryFn: () => taskApi.subIssues(client, workspaceId, taskId) })
}

/** Files attached to the task itself, from before rich text. */
export function useTaskAttachments(taskId: string) {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: taskKeys.attachments(workspaceId, taskId), queryFn: () => taskApi.taskAttachments(client, workspaceId, taskId) })
}

/** Files attached to each comment, from before rich text, by comment id. */
export function useCommentAttachments(taskId: string, comments: readonly api.CommentRecord[]): ReadonlyMap<string, taskApi.AttachmentRecord[]> {
  const { client, workspaceId } = useOrbit()
  return useQueries({
    queries: comments.map((comment) => ({
      queryKey: taskKeys.commentAttachments(workspaceId, taskId, comment.id),
      queryFn: () => taskApi.commentAttachments(client, workspaceId, taskId, comment.id),
    })),
    combine: (results) => new Map(comments.map((comment, index) => [comment.id, results[index]?.data ?? []])),
  })
}

/** Recently updated tasks, or the server search for `query`: the rows of the task picker. */
export function useTaskSearch(query: string, enabled: boolean) {
  const { client, workspaceId } = useOrbit()
  const search = useDeferredValue(query.trim())
  return useQuery({
    queryKey: taskKeys.search(workspaceId, search),
    queryFn: () => taskApi.searchTasks(client, workspaceId, search),
    enabled,
    placeholderData: (previous) => previous,
  })
}

/**
 * One write for every property of a task (status, priority, assignees, labels, due date, source, parent,
 * duplicate). Each tap shows at once; the requests go one after another and each sends the version the previous one
 * returned, so quick taps (three assignees in a row) neither conflict nor lose a change. A change by someone else is
 * a version conflict, which shows the latest task.
 */
export function useTaskWrite(taskId: string) {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  const taskKey = keys.task(workspaceId, taskId)
  const queue = useRef<Promise<unknown>>(Promise.resolve())
  const pending = useRef(0)
  const setTask = (next: api.TaskRecord) => {
    queryClient.setQueryData(taskKey, next)
    queryClient.setQueryData<api.TaskRecord[]>(keys.tasks(workspaceId), (list) => list && replaceTask(list, next))
  }
  return useMutation({
    mutationFn: (patch: TaskPatch) => {
      pending.current += 1
      const send = queue.current
        .catch(() => undefined)
        .then(async () => {
          const latest = queryClient.getQueryData<api.TaskRecord>(taskKey) ?? (await api.task(client, workspaceId, taskId))
          return api.changeTask(client, workspaceId, taskId, { ...patch, expected_version: latest.version })
        })
        .finally(() => {
          pending.current -= 1
        })
      queue.current = send
      return send
    },
    onMutate: async (patch) => {
      await queryClient.cancelQueries({ queryKey: taskKey, exact: true })
      const previous = queryClient.getQueryData<api.TaskRecord>(taskKey)
      if (previous) setTask(optimisticTask(previous, patch))
      return previous
    },
    onError: (error, patch, previous) => {
      if (previous) setTask(previous)
      void queryClient.invalidateQueries({ queryKey: taskKey })
      if (isVersionConflict(error)) Alert.alert('Task changed', 'Someone else changed this task. Showing the latest version; try again.')
      else if ('parent_task_id' in patch) Alert.alert('Could not change the parent', parentErrorMessage(error))
      else if ('duplicate_of_id' in patch) Alert.alert(patch.duplicate_of_id ? 'Couldn’t mark as duplicate' : 'Couldn’t unmark as duplicate', errorMessage(error))
      else Alert.alert('Task update failed', errorMessage(error))
    },
    onSuccess: (updated) => {
      // while later taps are on their way, keep what they show and take only the new version
      const shown = queryClient.getQueryData<api.TaskRecord>(taskKey)
      setTask(pending.current > 0 && shown ? { ...shown, version: updated.version } : updated)
      // closing a task or changing its parent can change its parent, its sub-issues and their lists
      void queryClient.invalidateQueries({ queryKey: keys.tasks(workspaceId) })
      void queryClient.invalidateQueries({ queryKey: keys.taskActivity(workspaceId, taskId) })
    },
  })
}

export function useCreateLabel() {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ projectId, name, color }: { projectId: string; name: string; color: string }) => taskApi.newLabel(client, workspaceId, projectId, { name, color }),
    onSuccess: (label) => {
      queryClient.setQueryData<api.LabelRecord[]>(keys.labels(workspaceId), (list) => (list ? [...list, label] : [label]))
      void queryClient.invalidateQueries({ queryKey: keys.labels(workspaceId) })
    },
  })
}

/** Adds a blocks / blocked by / related relation; both tasks' relations and `blocked` flags change. */
export function useRelationWrites(taskId: string) {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  const refresh = () => {
    void queryClient.invalidateQueries({ queryKey: ['task', workspaceId] })
    void queryClient.invalidateQueries({ queryKey: keys.tasks(workspaceId) })
  }
  const add = useMutation({
    mutationFn: (body: { type: taskApi.NewTaskRelationType; task_id: string }) => taskApi.addRelation(client, workspaceId, taskId, body),
    onSuccess: refresh,
    onError: (error) => Alert.alert('Couldn’t add the relation', errorMessage(error)),
  })
  const remove = useMutation({
    mutationFn: (relationId: string) => taskApi.removeRelation(client, workspaceId, taskId, relationId),
    onSuccess: refresh,
    onError: (error) => Alert.alert('Couldn’t remove the relation', errorMessage(error)),
  })
  return { add, remove }
}

/** Creates a task from a full body (a sub-issue, the New task dialog). */
export function useCreateTask() {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body: taskApi.CreateTaskBody) => taskApi.createTaskWith(client, workspaceId, body),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: keys.tasks(workspaceId) }),
  })
}

/** "Add existing" of the Sub-issues section: makes another task a sub-issue of `parentId`. */
export function useAdoptSubIssue(parentId: string) {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (child: Pick<api.TaskRecord, 'id' | 'version'>) => api.changeTask(client, workspaceId, child.id, { parent_task_id: parentId, expected_version: child.version }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: keys.tasks(workspaceId) })
      void queryClient.invalidateQueries({ queryKey: keys.task(workspaceId, parentId) })
    },
    onError: (error) =>
      isVersionConflict(error) ? Alert.alert('Task changed', 'Someone else changed that task. Try again.') : Alert.alert('Could not change the parent', parentErrorMessage(error)),
  })
}
