import { useState, type DragEvent } from 'react'
import { toast } from 'sonner'
import type { TaskRecord } from '@/api/generated/types.gen'
import type { Task } from '@/features/tasks/api/models'
import { useBulkTasks, useUpdateTask } from '@/features/tasks/api/tasks'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import type { GroupContext } from './grouping'
import { acceptsDrop, placementUpdates, planDrop, zoneIdOf, type GroupValues, type TaskPatch } from './layoutGroups'
import { reportMoveError } from './moveErrors'

type GroupDrag = { taskId: string; from: GroupValues }
/**
 * `index` = insertion slot among the zone's other items; null when the order is not manual. `held`: an item under the
 * pointer took the hover over (`holdDrop`); the slot keeps its place but the zone shows no target.
 */
type GroupDrop = { zone: string; index: number | null; held?: boolean }
/**
 * Where a drop lands: a list group section, a board column or cell, or a collapsed group / swim lane.
 * A collapsed zone carries its own values only (a hidden value must never be written). `itemsShown` is
 * false when the zone's items are not on screen: the task then goes to the zone's end.
 */
type DropZone = { id: string; values: GroupValues; tasks: Task[]; itemsShown: boolean }

/** Slot among the zone's items (not counting the dragged one) at the pointer, by item midpoints. */
function indexAt(zone: HTMLElement, itemSelector: string, clientY: number) {
  const items = Array.from(zone.querySelectorAll<HTMLElement>(`${itemSelector}:not([data-dragging])`))
  const below = items.findIndex((item) => {
    const rect = item.getBoundingClientRect()
    return clientY < rect.top + rect.height / 2
  })
  return below === -1 ? items.length : below
}

/**
 * Drag and drop between group zones, shared by the list and the board: the drag state, the props for each
 * zone, and the drop → write mapping. A drop plans every changed group field at once (`planDrop`), so a
 * board drop into another column and lane is one write. Every move error goes through `reportMoveError`.
 */
export function useGroupDrop({ tasks, manual, groupContext, itemSelector, onDuplicate, detach, onDetached }: {
  tasks: Task[]
  manual: boolean
  groupContext: GroupContext
  /** The draggable items inside a zone (list rows, board cards), for the insertion slot. */
  itemSelector: string
  /** A drop on the Duplicate status: the caller asks for the canonical task. */
  onDuplicate: (task: Task) => void
  /** Nested list: a drop at root level (a group zone) detaches a nested row. */
  detach?: (task: Task) => boolean
  /** After a write that detached `task` (as it was before the drop); `records` are the write's results, `response` its raw body. */
  onDetached?: (task: Task, records: TaskRecord[], response: unknown) => void
}) {
  const { workspace } = useWorkspace()
  const updateTask = useUpdateTask(workspace.id)
  const moveTasks = useBulkTasks(workspace.id)
  const [drag, setDrag] = useState<GroupDrag | null>(null)
  const [drop, setDrop] = useState<GroupDrop | null>(null)

  const startDrag = (taskId: string, from: GroupValues) => setDrag({ taskId, from })
  const endDrag = () => {
    setDrag(null)
    setDrop(null)
  }

  const detaches = (taskId: string) => {
    const task = tasks.find((item) => item.id === taskId)
    return Boolean(task && detach?.(task))
  }

  const dropInto = (current: GroupDrag, zone: DropZone, index: number | null) => {
    const task = tasks.find((item) => item.id === current.taskId)
    if (!task) return
    // merged into every write: a nested row dropped at root level leaves its parent
    const extra: TaskPatch = detach?.(task) ? { parent_task_id: null } : {}
    const hasExtra = Object.keys(extra).length > 0
    // a detach gets the same toast and Undo as a nest drop
    const announce = (records: TaskRecord[], response: unknown) => { if (hasExtra) onDetached?.(task, records, response) }
    const single = { onError: reportMoveError, onSuccess: (record: TaskRecord) => announce([record], record) }
    const bulk = { onError: reportMoveError, onSuccess: (page: { items: TaskRecord[] }) => announce(page.items, page) }
    const patchOnly = () => updateTask.mutate({ taskId: task.id, body: { expected_version: task.version, ...extra } }, single)
    if (zoneIdOf(current.from) === zone.id) {
      if (index === null) {
        if (hasExtra) patchOnly()
        return
      }
      const updates = placementUpdates(task, zone.tasks, index, extra)
      if (updates.length > 0) moveTasks.mutate(updates, bulk)
      return
    }
    const plan = planDrop(task, current.from, zone.values, groupContext)
    if (plan.kind === 'duplicate') onDuplicate(task)
    else if (plan.kind === 'error') toast.error(plan.message)
    else if (plan.kind === 'update') {
      const patch = { ...plan.patch, ...extra }
      if (index !== null) moveTasks.mutate(placementUpdates(task, zone.tasks, index, patch), bulk)
      else updateTask.mutate({ taskId: task.id, body: { expected_version: task.version, ...patch } }, single)
    } else if (hasExtra) {
      if (index !== null) moveTasks.mutate(placementUpdates(task, zone.tasks, index, extra), bulk)
      else patchOnly()
    }
  }

  const zoneProps = (zone: DropZone) => {
    const indexFor = (element: HTMLElement, clientY: number) => {
      if (!manual) return null
      // no items to aim at: the task goes to the zone's end
      if (!zone.itemsShown) return zone.tasks.filter((task) => task.id !== drag?.taskId).length
      return indexAt(element, itemSelector, clientY)
    }
    return {
      'data-drop-over': (drop?.zone === zone.id && !drop.held) || undefined,
      onDragOver: (event: DragEvent<HTMLElement>) => {
        if (!drag || !acceptsDrop(drag.from, zone.values)) return
        // same zone without manual order: nothing would change (unless a nested row detaches), so no drop target
        if (!manual && zoneIdOf(drag.from) === zone.id && !detaches(drag.taskId)) return
        event.preventDefault()
        event.stopPropagation()
        event.dataTransfer.dropEffect = 'move'
        const index = indexFor(event.currentTarget, event.clientY)
        if (drop?.zone !== zone.id || drop.index !== index || drop.held) setDrop({ zone: zone.id, index })
      },
      onDragLeave: (event: DragEvent<HTMLElement>) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null) && drop?.zone === zone.id) setDrop(null)
      },
      onDrop: (event: DragEvent<HTMLElement>) => {
        event.preventDefault()
        event.stopPropagation()
        if (drag) dropInto(drag, zone, indexFor(event.currentTarget, event.clientY))
        endDrag()
      },
    }
  }

  /** A row took the hover over (`useNestDrop`): hide the zone's insertion line. */
  const clearDrop = () => setDrop(null)
  /**
   * A card took the hover over: keep the slot (its placeholder keeps its space, hidden) so nothing below shifts
   * under the pointer. The zone's next dragover releases it.
   */
  const holdDrop = () => setDrop((current) => (current && !current.held ? { ...current, held: true } : current))

  return { drag, drop, startDrag, endDrag, clearDrop, holdDrop, zoneProps, saving: moveTasks.isPending || updateTask.isPending }
}
