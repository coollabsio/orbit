import type { Task } from '@/features/tasks/api/models'

/** Deepest indentation step (spec §7.2); deeper rows keep this indent. */
export const MAX_TREE_INDENT = 6
/** Indentation per level of the status + title area, in px. */
export const TREE_INDENT_PX = 16

export type TreeTask = Pick<Task, 'id' | 'parentTaskId'>

export interface TaskTree<T extends TreeTask = Task> {
  /** Tasks whose parent is not in the result, in input order. */
  roots: T[]
  /** Direct children that are in the result, in input (view) order. */
  childrenOf: ReadonlyMap<string, T[]>
  /** Ids rendered under a parent (their parent is in the result). */
  nested: ReadonlySet<string>
}

export type TreeRow<T = Task> = { task: T; depth: number; indent: number; hasChildren: boolean }

/** Nests a flat, already ordered result. Pure: the list and the detail build their rows from it. */
export function buildTaskTree<T extends TreeTask>(tasks: readonly T[]): TaskTree<T> {
  const byId = new Map(tasks.map((task) => [task.id, task]))
  const childrenOf = new Map<string, T[]>()
  const nested = new Set<string>()
  for (const task of tasks) {
    const parentId = task.parentTaskId
    if (!parentId || parentId === task.id || !byId.has(parentId)) continue
    const siblings = childrenOf.get(parentId)
    if (siblings) siblings.push(task)
    else childrenOf.set(parentId, [task])
    nested.add(task.id)
  }
  const reached = new Set<string>()
  const reach = (start: T) => {
    const stack = [start]
    while (stack.length > 0) {
      const task = stack.pop()!
      if (reached.has(task.id)) continue
      reached.add(task.id)
      stack.push(...(childrenOf.get(task.id) ?? []))
    }
  }
  for (const task of tasks) if (!nested.has(task.id)) reach(task)
  // a cycle in stale cache data is unreachable from any root: its first task becomes a root, so no row disappears
  for (const task of tasks) {
    if (reached.has(task.id)) continue
    nested.delete(task.id)
    const parentId = task.parentTaskId!
    childrenOf.set(parentId, (childrenOf.get(parentId) ?? []).filter((item) => item.id !== task.id))
    reach(task)
  }
  return { roots: tasks.filter((task) => !nested.has(task.id)), childrenOf, nested }
}

/**
 * Depth-first rows from `roots` (a group's roots, in group order); skips the subtree of a collapsed id.
 * `seen` guards against any residual cycle so no id is ever emitted twice.
 */
export function flattenTree<T extends TreeTask>(tree: TaskTree<T>, roots: readonly T[], collapsed: ReadonlySet<string>): TreeRow<T>[] {
  const rows: TreeRow<T>[] = []
  const seen = new Set<string>()
  const visit = (task: T, depth: number) => {
    if (seen.has(task.id)) return
    seen.add(task.id)
    const children = tree.childrenOf.get(task.id) ?? []
    rows.push({ task, depth, indent: Math.min(depth, MAX_TREE_INDENT), hasChildren: children.length > 0 })
    if (collapsed.has(task.id)) return
    for (const child of children) visit(child, depth + 1)
  }
  for (const root of roots) visit(root, 0)
  return rows
}

/** Every id nested under `id` (any depth), ignoring collapse state. */
export function descendantIds(tree: TaskTree<TreeTask>, id: string): string[] {
  const ids: string[] = []
  const seen = new Set<string>([id])
  const stack = [...(tree.childrenOf.get(id) ?? [])]
  while (stack.length > 0) {
    const task = stack.pop()!
    if (seen.has(task.id)) continue
    seen.add(task.id)
    ids.push(task.id)
    stack.push(...(tree.childrenOf.get(task.id) ?? []))
  }
  return ids
}

/** Count of every task nested under `id` (any depth). */
export function subtreeSize(tree: TaskTree<TreeTask>, id: string): number {
  return descendantIds(tree, id).length
}
