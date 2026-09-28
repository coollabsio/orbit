import { useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'
import { apiClient } from '@/api/client'
import { getTask, updateTask } from '@/api/generated/sdk.gen'
import type { BulkItem, TaskRecord } from '@/api/generated/types.gen'
import { queryKeys } from '@/api/queryKeys'
import type { Task } from '@/features/tasks/api/models'
import { announceAutoClosed, autoClosedOf, type AutoClosedTask } from '@/features/tasks/api/autoClosed'
import { findCachedTask, reconcileWorkspaceTask } from '@/features/tasks/api/optimistic'
import { bulkTaskDuplicateUpdates } from '@/features/tasks/api/tasks'
import { parentErrorMessage, parentToastMessage } from '@/features/tasks/subIssuesLib'
import { UNDO_TOAST_DURATION, keepIdentifiersTogether } from '@/lib/toast'
import { placementUpdates } from '@/features/views/layoutGroups'
import type { NestPlacement } from '@/features/views/nestDrop'

export type ParentTarget = { id: string; identifier: string }
/** A task the write auto-closed (`status_id`, its new status) and the status it had before (`previous`). */
type ClosedByWrite = AutoClosedTask & { previous: string }

/**
 * Set or remove the parent of tasks with an Undo toast. Plain SDK calls plus the shared QueryClient (like
 * `useDuplicateActions`): Undo must still work after the row that triggered it re-mounts under its new parent.
 */
export function useParentActions(workspaceId: string) {
  const queryClient = useQueryClient()
  /** `response` (raw PATCH/bulk body) names tasks the server auto-closed: a detach can close the old parent (rule A). */
  const settle = (records: TaskRecord[], response?: unknown) => {
    if (autoClosedOf(response).length > 0) {
      // before reconciling: the cache still has the old parents, so a closed former parent reads as a parent
      const before = records.flatMap((record) => findCachedTask(queryClient, workspaceId, record.id) ?? [])
      announceAutoClosed(queryClient, workspaceId, [...records, ...before], response)
    }
    for (const record of records) reconcileWorkspaceTask(queryClient, workspaceId, record)
    void queryClient.invalidateQueries({ queryKey: queryKeys.tasks.all(workspaceId) })
  }
  /** One PATCH for a single update, one atomic bulk call otherwise. Returns the records and the raw response. */
  const write = async (updates: BulkItem[]): Promise<{ records: TaskRecord[]; response: unknown }> => {
    if (updates.length === 1) {
      const { id, ...body } = updates[0]!
      const { data } = await updateTask({ client: apiClient, path: { workspace_id: workspaceId, task_id: id }, body, throwOnError: true })
      if (!data) throw new Error('Update task response was empty.')
      return { records: [data], response: data }
    }
    const page = await bulkTaskDuplicateUpdates(apiClient, workspaceId, updates)
    return { records: page.items, response: page }
  }
  /**
   * Undo: every task goes back to its parent before the change (`null` = it was top-level), and a parent the change
   * auto-closed (rule A) reopens, unless someone changed its status since. Fresh reads give the reopen its version.
   */
  const restore = async (records: TaskRecord[], previous: ReadonlyMap<string, string | null>, closed: ClosedByWrite[]) => {
    try {
      const reopen = await Promise.all(closed.map(async (item): Promise<BulkItem[]> => {
        const { data } = await getTask({ client: apiClient, path: { workspace_id: workspaceId, task_id: item.id }, throwOnError: true })
        return data && data.status_id === item.status_id ? [{ id: item.id, expected_version: data.version, status_id: item.previous }] : []
      }))
      const updates = [
        ...records.map((record) => ({ id: record.id, expected_version: record.version, parent_task_id: previous.get(record.id) ?? null })),
        ...reopen.flat(),
      ]
      const { records: restored, response } = await write(updates)
      settle(restored, response)
    } catch (error) {
      settle([])
      toast.error(parentErrorMessage(error))
    }
  }
  /**
   * Success toast for tasks whose parent changed (`changed`, as they were before the write) with an Undo that puts
   * them back under their previous parent. `records` are the write's results (their versions feed the Undo); other
   * records (siblings whose positions moved) are ignored. `response` (the raw write body) names parents the write
   * auto-closed, which the Undo reopens; call this before the cache refetches (it reads their old status).
   * Also used by writes made elsewhere, like a nested list drop at root level that detaches a row together with its
   * group change.
   */
  const announce = (changed: Task[], parent: ParentTarget | null, records: TaskRecord[], response?: unknown) => {
    if (changed.length === 0) return
    const previous = new Map(changed.map((task) => [task.id, task.parentTaskId ?? null]))
    const closed = autoClosedOf(response).flatMap((item): ClosedByWrite[] => {
      const before = findCachedTask(queryClient, workspaceId, item.id)?.status_id
      return before && before !== item.status_id ? [{ ...item, previous: before }] : []
    })
    toast.success(keepIdentifiersTogether(parentToastMessage(changed.map((task) => task.identifier), parent?.identifier ?? null)), {
      duration: UNDO_TOAST_DURATION,
      action: { label: 'Undo', onClick: () => void restore(records.filter((record) => previous.has(record.id)), previous, closed) },
    })
  }
  /**
   * `parent` null detaches (only the tasks that have a parent are written). `placement` (one task, manual order) also
   * moves it among its new siblings; a reorder among the same siblings writes positions only, with no toast.
   */
  const setParent = async (tasks: Task[], parent: ParentTarget | null, options: { placement?: NestPlacement | null } = {}) => {
    const moving = parent ? tasks : tasks.filter((task) => task.parentTaskId)
    if (moving.length === 0) return
    const parentId = parent?.id ?? null
    const changed = moving.filter((task) => (task.parentTaskId ?? null) !== parentId)
    const patch = { parent_task_id: parentId }
    const updates = options.placement && moving.length === 1
      ? placementUpdates(moving[0]!, options.placement.siblings, options.placement.index, changed.length > 0 ? patch : {})
      : changed.map((task) => ({ id: task.id, expected_version: task.version, ...patch }))
    if (updates.length === 0) return
    try {
      const { records, response } = await write(updates)
      settle(records, response)
      announce(changed, parent, records, response)
    } catch (error) {
      settle([])
      toast.error(parentErrorMessage(error))
    }
  }
  return { setParent, announce }
}
