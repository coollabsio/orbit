import type { Task } from '@/features/tasks/api/models'
import type { TaskTree } from './taskTree'

/** Where a drag hovers a row or card: its top quarter, its middle half, or its bottom quarter. */
export type NestZone = 'before' | 'inside' | 'after'
/** Manual order only: the new sibling list (view order) and the slot among it, not counting the moved task. */
export type NestPlacement = { siblings: Task[]; index: number }
/**
 * `pass`: the row does not take the drop, the group zone around it does. `invalid`: the row takes the drop but it
 * would change nothing or form a cycle (no highlight, no write). `parent`: the task gets `parentId`.
 */
export type NestPlan = { kind: 'pass' } | { kind: 'invalid' } | { kind: 'parent'; parentId: string; placement: NestPlacement | null }

/** 25% before, 50% inside, 25% after (the docs tree split, `DocTreeItem` `zoneAt`). */
export function nestZoneAt(rect: { top: number; height: number }, clientY: number): NestZone {
  const y = clientY - rect.top
  if (y < rect.height * 0.25) return 'before'
  if (y > rect.height * 0.75) return 'after'
  return 'inside'
}

/** False when the target is the task itself or one of its sub-issues: walks up from the target through the known parents. */
export function canNestOn(tasks: ReadonlyArray<Pick<Task, 'id' | 'parentTaskId'>>, dragId: string, targetId: string): boolean {
  const parentOf = new Map(tasks.map((task) => [task.id, task.parentTaskId ?? null]))
  const seen = new Set<string>()
  for (let id: string | null = targetId; id !== null && !seen.has(id); id = parentOf.get(id) ?? null) {
    if (id === dragId) return false
    seen.add(id)
  }
  return true
}

/**
 * Spec §7.4. Inside: the target becomes the parent. The edges of a nested row (tree given, target nested in it): the task
 * becomes the target's sibling, placed beside it when the order is manual and every sibling shares its status and
 * project. Every other edge passes to the group zone (flat list, board, root rows). Drops that change nothing are
 * invalid, so they show no highlight.
 */
export function planNestDrop({ tasks, tree, dragId, target, zone, manual }: {
  tasks: Task[]
  tree: TaskTree | null
  dragId: string
  target: Task
  zone: NestZone
  manual: boolean
}): NestPlan {
  const dragged = tasks.find((task) => task.id === dragId)
  if (!dragged) return { kind: 'invalid' }
  if (zone === 'inside') {
    if (!canNestOn(tasks, dragId, target.id) || dragged.parentTaskId === target.id) return { kind: 'invalid' }
    return { kind: 'parent', parentId: target.id, placement: null }
  }
  if (!tree || !tree.nested.has(target.id) || !target.parentTaskId) return { kind: 'pass' }
  const parentId = target.parentTaskId
  if (target.id === dragId || !canNestOn(tasks, dragId, parentId)) return { kind: 'invalid' }
  const siblings = tree.childrenOf.get(parentId) ?? []
  // positions count within one project and status (a board column): placing among sub-issues in other columns would
  // renumber them there, so such a drop only changes the parent
  const oneColumn = siblings.every((task) => task.projectId === dragged.projectId && task.statusId === dragged.statusId)
  if (!manual || !oneColumn) return dragged.parentTaskId === parentId ? { kind: 'invalid' } : { kind: 'parent', parentId, placement: null }
  const others = siblings.filter((task) => task.id !== dragId)
  return { kind: 'parent', parentId, placement: { siblings, index: others.indexOf(target) + (zone === 'after' ? 1 : 0) } }
}
