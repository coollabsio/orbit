import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { apiClient } from '@/api/client'
import { queryKeys } from '@/api/queryKeys'
import { createTaskField, deleteTaskField, listProjectTaskFields, listTaskFields, listTaskFieldValues, setProjectTaskFields, setTaskFieldValue, updateTaskField } from '@orbit/api/generated/sdk.gen'
import type { TaskField, TaskFieldBody, TaskFieldUpdateBody, TaskFieldValue } from '@orbit/api/generated/types.gen'
import { timeTrackingField } from '@orbit/api/taskFields'

function required<T>(data: T | undefined, message: string): T {
  if (data === undefined) throw new Error(message)
  return data
}

/** The project's time tracking field: while the project shows it, its tasks take new time. */
export function useTimeTrackingField(workspaceId: string, projectId: string | undefined) {
  return timeTrackingField(useProjectTaskFields(workspaceId, projectId).data ?? [])
}

/** The workspace's field gallery, by name. */
export function useTaskFields(workspaceId: string) {
  return useQuery({
    queryKey: queryKeys.taskFields(workspaceId),
    queryFn: async (): Promise<TaskField[]> => {
      const { data } = await listTaskFields({ client: apiClient, path: { workspace_id: workspaceId }, throwOnError: true })
      return required(data, 'Task fields response was empty.').items
    },
  })
}

/** The fields a project's tasks show, in order. */
export function useProjectTaskFields(workspaceId: string, projectId: string | undefined) {
  return useQuery({
    queryKey: queryKeys.projectTaskFields(workspaceId, projectId ?? ''),
    enabled: Boolean(projectId),
    queryFn: async (): Promise<TaskField[]> => {
      const { data } = await listProjectTaskFields({ client: apiClient, path: { workspace_id: workspaceId, project_id: projectId! }, throwOnError: true })
      return required(data, 'Project fields response was empty.').items
    },
  })
}

/** A task's values, by field id. */
export function useTaskFieldValues(workspaceId: string, taskId: string | undefined) {
  return useQuery({
    queryKey: queryKeys.taskFieldValues(workspaceId, taskId ?? ''),
    enabled: Boolean(taskId),
    queryFn: async (): Promise<TaskFieldValue[]> => {
      const { data } = await listTaskFieldValues({ client: apiClient, path: { workspace_id: workspaceId, task_id: taskId! }, throwOnError: true })
      return required(data, 'Task field values response was empty.').items
    },
  })
}

/** A gallery change shows in every project's fields and task's values too: every `…/fields` key of the workspace refetches. */
function useInvalidateFields(workspaceId: string) {
  const queryClient = useQueryClient()
  return () => {
    void queryClient.invalidateQueries({ queryKey: queryKeys.taskFields(workspaceId) })
    void queryClient.invalidateQueries({ predicate: (query) => query.queryKey[0] === 'workspace' && query.queryKey[1] === workspaceId && query.queryKey.at(-1) === 'fields' })
    // a deleted field or option drops from the values that list rows show
    void queryClient.invalidateQueries({ queryKey: [...queryKeys.tasks.all(workspaceId), 'query'] })
  }
}

export function useCreateTaskField(workspaceId: string) {
  const invalidate = useInvalidateFields(workspaceId)
  return useMutation({
    mutationFn: async (body: TaskFieldBody) => {
      const { data } = await createTaskField({ client: apiClient, path: { workspace_id: workspaceId }, body, throwOnError: true })
      return required(data, 'Create field response was empty.')
    },
    onSettled: invalidate,
  })
}

export function useUpdateTaskField(workspaceId: string) {
  const invalidate = useInvalidateFields(workspaceId)
  return useMutation({
    mutationFn: async ({ fieldId, body }: { fieldId: string; body: TaskFieldUpdateBody }) => {
      const { data } = await updateTaskField({ client: apiClient, path: { workspace_id: workspaceId, field_id: fieldId }, body, throwOnError: true })
      return required(data, 'Update field response was empty.')
    },
    onSettled: invalidate,
  })
}

export function useDeleteTaskField(workspaceId: string) {
  const invalidate = useInvalidateFields(workspaceId)
  return useMutation({
    mutationFn: async (field: Pick<TaskField, 'id' | 'version'>) => {
      await deleteTaskField({ client: apiClient, path: { workspace_id: workspaceId, field_id: field.id }, query: { expected_version: field.version }, throwOnError: true })
    },
    onSettled: invalidate,
  })
}

/** Replaces the fields the project shows, in order. */
export function useSetProjectTaskFields(workspaceId: string, projectId: string) {
  const invalidate = useInvalidateFields(workspaceId)
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (fieldIds: string[]) => {
      const { data } = await setProjectTaskFields({ client: apiClient, path: { workspace_id: workspaceId, project_id: projectId }, body: { field_ids: fieldIds }, throwOnError: true })
      return required(data, 'Project fields response was empty.').items
    },
    onSuccess: (fields) => queryClient.setQueryData(queryKeys.projectTaskFields(workspaceId, projectId), fields),
    onSettled: invalidate,
  })
}

/** Sets or clears (`null`) one value; the list shows the new value at once. */
export function useSetTaskFieldValue(workspaceId: string, taskId: string) {
  const queryClient = useQueryClient()
  const key = queryKeys.taskFieldValues(workspaceId, taskId)
  return useMutation({
    mutationFn: async ({ fieldId, value }: { fieldId: string; value: unknown }) => {
      const { data } = await setTaskFieldValue({ client: apiClient, path: { workspace_id: workspaceId, task_id: taskId, field_id: fieldId }, body: { value }, throwOnError: true })
      return required(data, 'Field value response was empty.').items
    },
    onMutate: async ({ fieldId, value }) => {
      await queryClient.cancelQueries({ queryKey: key })
      const before = queryClient.getQueryData<TaskFieldValue[]>(key)
      const others = (before ?? []).filter((item) => item.field_id !== fieldId)
      queryClient.setQueryData<TaskFieldValue[]>(key, value === null ? others : [...others, { field_id: fieldId, value }])
      return { before }
    },
    onError: (_error, _vars, context) => queryClient.setQueryData(key, context?.before),
    onSuccess: (values) => queryClient.setQueryData(key, values),
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.taskActivity(workspaceId, taskId) })
      // list rows and board cards show the values too
      void queryClient.invalidateQueries({ queryKey: [...queryKeys.tasks.all(workspaceId), 'query'] })
    },
  })
}
