import { FIELD_META, MAX_FILTER_CONDITIONS, isCompleteCondition } from './filterFields'
import { countConditions, isGroup, type Condition, type FilterGroup, type FilterNode } from './viewState'

export { MAX_FILTER_CONDITIONS }

/** The user's root group is depth 1; root + two nested levels stays within the server's 3 even when a scope wraps an OR root. */
export const MAX_FILTER_DEPTH = 3

/** The server allows groups this many levels below the root of the effective tree (the root is level 0). */
const SERVER_MAX_NESTING = 3

export type NodePath = number[]
export type FilterIssue = { path: string; message: string }

export function toIssuePath(path: NodePath): string {
  return ['filter', ...path.map((index) => `children[${index}]`)].join('.')
}

/** `filter.children[2].children[0].value` → [2, 0]; paths outside `filter` → null. */
export function parseIssuePath(issuePath: string): NodePath | null {
  if (issuePath !== 'filter' && !issuePath.startsWith('filter.')) return null
  return [...issuePath.matchAll(/children\[(\d+)\]/g)].map((match) => Number(match[1]))
}

export function samePath(a: NodePath | null, b: NodePath): boolean {
  return a !== null && a.length === b.length && a.every((value, index) => value === b[index])
}

/** Replaces the node at `path` with `update(node)`; returning null removes it. The root cannot be removed. */
export function updateNode(root: FilterGroup, path: NodePath, update: (node: FilterNode) => FilterNode | null): FilterGroup {
  if (path.length === 0) {
    const next = update(root)
    return next && isGroup(next) ? next : root
  }
  const [head, ...rest] = path
  return {
    ...root,
    children: root.children.flatMap((child, index): FilterNode[] => {
      if (index !== head) return [child]
      if (rest.length === 0) {
        const next = update(child)
        return next ? [next] : []
      }
      return isGroup(child) ? [updateNode(child, rest, update)] : [child]
    }),
  }
}

export function appendChild(root: FilterGroup, groupPath: NodePath, child: FilterNode): FilterGroup {
  return updateNode(root, groupPath, (node) => (isGroup(node) ? { ...node, children: [...node.children, child] } : node))
}

/**
 * ANDs a condition onto the tree (the chip bar's add): appended to an AND root, otherwise the root is wrapped. An OR
 * root with at most one child means the same as an AND root, so it takes the condition directly and gains no level.
 */
export function appendCondition(filter: FilterGroup, condition: Condition): FilterGroup {
  if (filter.op === 'and') return { ...filter, children: [...filter.children, condition] }
  if (filter.children.length <= 1) return { op: 'and', children: [...filter.children, condition] }
  return { op: 'and', children: [filter, condition] }
}

export function blankCondition(): Condition {
  return { field: 'status', operator: 'is', value: [] }
}

/**
 * Client-side checks run before the server dry-run, using the server's path format. Depth and size are measured
 * on the tree the server will get on any page (`effectiveFilter`): unscoped, the tree goes as-is; scoped, an AND
 * root (including the wrapper `appendCondition` adds) flattens into the scope, while a non-empty OR root nests
 * under it one level down. So a tree accepted here never gets a 422 for depth or condition count.
 */
export function firstLocalIssue(root: FilterGroup): FilterIssue | null {
  if (countConditions(root) > MAX_FILTER_CONDITIONS) {
    return { path: 'filter', message: `Filters can have at most ${MAX_FILTER_CONDITIONS} conditions.` }
  }
  const nestsUnderScope = root.op === 'or' && root.children.length > 0
  const maxNesting = SERVER_MAX_NESTING - (nestsUnderScope ? 1 : 0)
  const depthMessage = `Groups can nest up to ${maxNesting} levels below ${nestsUnderScope ? 'an Or' : 'the'} root.`
  const visit = (group: FilterGroup, path: NodePath): FilterIssue | null => {
    for (const [index, child] of group.children.entries()) {
      const childPath = [...path, index]
      if (isGroup(child)) {
        if (childPath.length > maxNesting) return { path: toIssuePath(childPath), message: depthMessage }
        const nested = visit(child, childPath)
        if (nested) return nested
      } else if (!isCompleteCondition(child)) {
        return {
          path: `${toIssuePath(childPath)}.value`,
          message: FIELD_META[child.field].kind === 'text' ? 'Enter text to search for.' : 'Choose a value.',
        }
      }
    }
    return null
  }
  return visit(root, [])
}
