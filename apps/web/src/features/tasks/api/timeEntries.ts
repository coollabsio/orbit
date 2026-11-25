import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { apiClient } from '@/api/client'
import { queryKeys } from '@/api/queryKeys'
import type { TaskTimeEntries, TimeEntry, TimeEntryInput } from './models'

const JSON_HEADERS = { 'Content-Type': 'application/json' }

export function useTaskTimeEntries(workspaceId: string, taskId: string | undefined) {
  return useQuery({
    queryKey: queryKeys.timeEntries(workspaceId, taskId ?? ''),
    enabled: Boolean(taskId),
    queryFn: async () => (await apiClient.get<{ 200: TaskTimeEntries }, unknown, true>({
      url: '/api/v1/workspaces/{workspace_id}/tasks/{task_id}/time-entries',
      path: { workspace_id: workspaceId, task_id: taskId! },
      throwOnError: true,
    })).data,
  })
}

/** Logs time on the task: for the caller, or for `user_id` with `time.manage`. */
export async function postTimeEntry(workspaceId: string, taskId: string, input: Partial<TimeEntryInput> & { minutes: number }) {
  return (await apiClient.post<{ 201: TimeEntry }, unknown, true>({
    url: '/api/v1/workspaces/{workspace_id}/tasks/{task_id}/time-entries',
    path: { workspace_id: workspaceId, task_id: taskId },
    body: input,
    headers: JSON_HEADERS,
    throwOnError: true,
  })).data
}

export function useCreateTimeEntry(workspaceId: string, taskId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (input: TimeEntryInput) => postTimeEntry(workspaceId, taskId, input),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.timeEntries(workspaceId, taskId) }),
  })
}

export function useUpdateTimeEntry(workspaceId: string, taskId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async ({ entryId, version, changes }: { entryId: string; version: number; changes: Partial<TimeEntryInput> }) => (await apiClient.patch<{ 200: TimeEntry }, unknown, true>({
      url: '/api/v1/workspaces/{workspace_id}/time-entries/{entry_id}',
      path: { workspace_id: workspaceId, entry_id: entryId },
      body: { ...changes, expected_version: version },
      headers: JSON_HEADERS,
      throwOnError: true,
    })).data,
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.timeEntries(workspaceId, taskId) }),
  })
}

export function useDeleteTimeEntry(workspaceId: string, taskId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (entryId: string) => {
      await apiClient.delete({
        url: '/api/v1/workspaces/{workspace_id}/time-entries/{entry_id}',
        path: { workspace_id: workspaceId, entry_id: entryId },
        throwOnError: true,
      })
    },
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.timeEntries(workspaceId, taskId) }),
  })
}
