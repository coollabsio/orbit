import { isGroup, MAX_FILTER_LEVELS } from './filter'
import type { DatabaseFilterGroup, DatabaseFilterNode } from './types'

/** Child indexes from the root group to a node; `[]` is the root. */
export type NodePath = readonly number[]

export function nodeAt(root: DatabaseFilterGroup, path: NodePath): DatabaseFilterNode | undefined {
  let node: DatabaseFilterNode | undefined = root
  for (const index of path) node = node && isGroup(node) ? node.children[index] : undefined
  return node
}

/** The filter with the node at `path` replaced by `change(node)`; `null` removes it. The root cannot be removed. */
export function updateNode(root: DatabaseFilterGroup, path: NodePath, change: (node: DatabaseFilterNode) => DatabaseFilterNode | null): DatabaseFilterGroup {
  if (path.length === 0) {
    const next = change(root)
    return next && isGroup(next) ? next : root
  }
  const [index, ...rest] = path
  const child = root.children[index]
  if (!child) return root
  const next = rest.length === 0 ? change(child) : isGroup(child) ? updateNode(child, rest, change) : child
  const children = [...root.children]
  if (next === null) children.splice(index, 1)
  else children[index] = next
  return { ...root, children }
}

/** The filter with `node` added at the end of the group at `path`. */
export function appendChild(root: DatabaseFilterGroup, path: NodePath, node: DatabaseFilterNode): DatabaseFilterGroup {
  return updateNode(root, path, (group) => (isGroup(group) ? { ...group, children: [...group.children, node] } : group))
}

/** The filter with a copy of the node at `path` right after it. */
export function duplicateNode(root: DatabaseFilterGroup, path: NodePath): DatabaseFilterGroup {
  const node = nodeAt(root, path)
  if (!node || path.length === 0) return root
  return updateNode(root, path.slice(0, -1), (group) => {
    if (!isGroup(group)) return group
    const children = [...group.children]
    children.splice(path[path.length - 1] + 1, 0, structuredClone(node))
    return { ...group, children }
  })
}

/** The filter with the rule at `path` moved into a new group of its own (Notion's "Turn into group"). */
export function wrapInGroup(root: DatabaseFilterGroup, path: NodePath): DatabaseFilterGroup {
  return updateNode(root, path, (node) => ({ op: 'and', children: [node] }))
}

/** Whether the group at `path` may hold another group: groups nest 3 levels, the root being level 1. */
export function canNestGroup(path: NodePath): boolean {
  return path.length + 1 < MAX_FILTER_LEVELS
}
