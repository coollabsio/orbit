import { useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'
import { apiClient } from '@/api/client'
import type { TaskRecord } from '@/api/generated/types.gen'
import { queryKeys } from '@/api/queryKeys'
import { reconcileWorkspaceTask } from '@/features/tasks/api/optimistic'
import { bulkSetTaskDuplicateOf, setTaskDuplicateOf, type VersionedTask } from '@/features/tasks/api/tasks'
import { duplicateErrorMessage, duplicateToastMessage } from '@/features/tasks/relationsLib'

export interface DuplicateTarget {
  id: string
  identifier: string
}

/**
 * Mark / unmark tasks as duplicates with an Undo toast. Plain SDK calls plus the shared QueryClient instead of
 * component-scoped mutations: Undo must still work after the row that triggered it re-mounts in the Duplicate
 * group or the detail page closes.
 */
export function useDuplicateActions(workspaceId: string) {
  const queryClient = useQueryClient()
  const settle = (records: TaskRecord[]) => {
    for (const record of records) reconcileWorkspaceTask(queryClient, workspaceId, record)
    void queryClient.invalidateQueries({ queryKey: queryKeys.tasks.all(workspaceId) })
  }

  const unmarkOne = async (task: VersionedTask): Promise<TaskRecord | undefined> => {
    try {
      const record = await setTaskDuplicateOf(apiClient, workspaceId, task, null)
      settle([record])
      return record
    } catch (error) {
      settle([])
      toast.error(duplicateErrorMessage('unmark', error))
      return undefined
    }
  }

  const unmarkMany = async (tasks: VersionedTask[]): Promise<void> => {
    try {
      settle((await bulkSetTaskDuplicateOf(apiClient, workspaceId, tasks, null)).items)
    } catch (error) {
      settle([])
      toast.error(duplicateErrorMessage('unmark', error))
    }
  }

  const markOne = async (task: VersionedTask, target: DuplicateTarget): Promise<TaskRecord | undefined> => {
    try {
      const record = await setTaskDuplicateOf(apiClient, workspaceId, task, target.id)
      settle([record])
      toast.success(duplicateToastMessage(1, target.identifier), {
        action: { label: 'Undo', onClick: () => void unmarkOne(record) },
      })
      return record
    } catch (error) {
      settle([])
      toast.error(duplicateErrorMessage('mark', error))
      return undefined
    }
  }

  const markMany = async (tasks: VersionedTask[], target: DuplicateTarget): Promise<void> => {
    try {
      const page = await bulkSetTaskDuplicateOf(apiClient, workspaceId, tasks, target.id)
      settle(page.items)
      toast.success(duplicateToastMessage(tasks.length, target.identifier), {
        action: { label: 'Undo', onClick: () => void unmarkMany(page.items) },
      })
    } catch (error) {
      settle([])
      toast.error(duplicateErrorMessage('mark', error))
    }
  }

  return { markOne, markMany, unmarkOne }
}
