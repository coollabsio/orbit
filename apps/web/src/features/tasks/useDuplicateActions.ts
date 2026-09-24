import { useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'
import { apiClient } from '@/api/client'
import type { TaskRecord } from '@/api/generated/types.gen'
import { queryKeys } from '@/api/queryKeys'
import { reconcileWorkspaceTask } from '@/features/tasks/api/optimistic'
import { bulkSetTaskDuplicateOf, bulkTaskDuplicateUpdates, setTaskDuplicateOf, type VersionedTask } from '@/features/tasks/api/tasks'
import { duplicateErrorMessage, duplicateToastMessage } from '@/features/tasks/relationsLib'

export interface DuplicateTarget {
  id: string
  identifier: string
}

/** A task about to be marked; `duplicateOf` is its current target, which Undo puts back. */
export type DuplicateSource = VersionedTask & { duplicateOf?: { id: string } | null }

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

  /** Undo: every task goes back to its target before the mark (`null` = it was not a duplicate). */
  const restore = async (records: TaskRecord[], previous: ReadonlyMap<string, string | null>): Promise<void> => {
    try {
      // unmarks go first: a task can only return to a canonical that is no longer a duplicate itself
      const updates = records
        .map((record) => ({ id: record.id, expected_version: record.version, duplicate_of_id: previous.get(record.id) ?? null }))
        .sort((a, b) => Number(a.duplicate_of_id !== null) - Number(b.duplicate_of_id !== null))
      settle(updates.length === 1
        ? [await setTaskDuplicateOf(apiClient, workspaceId, records[0]!, updates[0]!.duplicate_of_id)]
        : (await bulkTaskDuplicateUpdates(apiClient, workspaceId, updates)).items)
    } catch (error) {
      settle([])
      toast.error(duplicateErrorMessage('unmark', error))
    }
  }
  const previousTargets = (tasks: DuplicateSource[]) => new Map(tasks.map((task) => [task.id, task.duplicateOf?.id ?? null]))

  const markOne = async (task: DuplicateSource, target: DuplicateTarget): Promise<TaskRecord | undefined> => {
    try {
      const record = await setTaskDuplicateOf(apiClient, workspaceId, task, target.id)
      settle([record])
      toast.success(duplicateToastMessage(1, target.identifier), {
        action: { label: 'Undo', onClick: () => void restore([record], previousTargets([task])) },
      })
      return record
    } catch (error) {
      settle([])
      toast.error(duplicateErrorMessage('mark', error))
      return undefined
    }
  }

  const markMany = async (tasks: DuplicateSource[], target: DuplicateTarget): Promise<void> => {
    try {
      const page = await bulkSetTaskDuplicateOf(apiClient, workspaceId, tasks, target.id)
      settle(page.items)
      toast.success(duplicateToastMessage(tasks.length, target.identifier), {
        action: { label: 'Undo', onClick: () => void restore(page.items, previousTargets(tasks)) },
      })
    } catch (error) {
      settle([])
      toast.error(duplicateErrorMessage('mark', error))
    }
  }

  return { markOne, markMany, unmarkOne }
}
