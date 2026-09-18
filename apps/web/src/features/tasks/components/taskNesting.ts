import type { Task } from '../api/models'

export interface NestedTask {
  root: Task
  children: Task[]
}

/**
 * One level of nesting for the list view: every task whose parent is also in
 * `tasks` becomes a child of its topmost present ancestor; everything else stays
 * a root. Deeper chains are flattened into that single child level, in input
 * order. A (corrupt) cycle cannot loop: its members all stay roots.
 */
export function nestTasks(tasks: Task[]): NestedTask[] {
  const byId = new Map(tasks.map((task) => [task.id, task]))
  const rootOf = (task: Task): Task => {
    let current = task
    const seen = new Set<string>([current.id])
    while (current.parentId) {
      const parent = byId.get(current.parentId)
      if (!parent) break
      // A cycle has no topmost ancestor: every member stays its own root.
      if (seen.has(parent.id)) return task
      seen.add(parent.id)
      current = parent
    }
    return current
  }
  const entries: NestedTask[] = []
  const index = new Map<string, NestedTask>()
  const entryFor = (root: Task) => {
    let entry = index.get(root.id)
    if (!entry) {
      entry = { root, children: [] }
      index.set(root.id, entry)
      entries.push(entry)
    }
    return entry
  }
  for (const task of tasks) {
    const root = rootOf(task)
    const entry = entryFor(root)
    if (task.id !== root.id) entry.children.push(task)
  }
  return entries
}
