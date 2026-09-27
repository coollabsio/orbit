// Pure tree + selection logic for the Notion import "Choose pages" step. No React or API imports.
//
// Selection model: the server imports a selected page together with all its sub-pages
// (`selection.notion_ids`), so a checked page always means "this page and everything under it".
// The selected set is therefore kept closed under descendants: checking a node adds its subtree,
// unchecking a node removes its subtree and its ancestors (they no longer include everything).
import type { NotionImportNode, NotionImportSelectionBody } from '@/api/generated/types.gen'

export type ImportNode = Pick<NotionImportNode, 'notion_id' | 'parent_id' | 'kind' | 'title' | 'icon' | 'child_count'>

export interface ImportTree {
  /** Nodes in tree order (parents before children, siblings in Notion order). */
  nodes: ImportNode[]
  byId: Map<string, ImportNode>
  /** Child ids per parent id; `null` holds the roots (no parent, or a parent missing from the list). */
  children: Map<string | null, string[]>
  /** The parent of each node inside this tree (`null` for roots). */
  parents: Map<string, string | null>
}

export type CheckState = 'checked' | 'unchecked' | 'indeterminate'

export function buildImportTree(nodes: readonly ImportNode[]): ImportTree {
  const byId = new Map<string, ImportNode>()
  for (const node of nodes) if (!byId.has(node.notion_id)) byId.set(node.notion_id, node)
  const children = new Map<string | null, string[]>()
  const parents = new Map<string, string | null>()
  for (const node of byId.values()) {
    const parent = node.parent_id !== null && node.parent_id !== node.notion_id && byId.has(node.parent_id) ? node.parent_id : null
    parents.set(node.notion_id, parent)
    const list = children.get(parent)
    if (list) list.push(node.notion_id)
    else children.set(parent, [node.notion_id])
  }
  return { nodes: [...byId.values()], byId, children, parents }
}

export function childIds(tree: ImportTree, id: string | null): string[] {
  return tree.children.get(id) ?? []
}

/** `id` and every node below it (depth-first). Guards against cycles in malformed input. */
export function subtreeIds(tree: ImportTree, id: string): string[] {
  const out: string[] = []
  const seen = new Set<string>()
  const stack = [id]
  while (stack.length > 0) {
    const current = stack.pop()!
    if (seen.has(current) || !tree.byId.has(current)) continue
    seen.add(current)
    out.push(current)
    const kids = childIds(tree, current)
    for (let index = kids.length - 1; index >= 0; index -= 1) stack.push(kids[index])
  }
  return out
}

export function ancestorIds(tree: ImportTree, id: string): string[] {
  const out: string[] = []
  const seen = new Set<string>([id])
  let parent = tree.parents.get(id) ?? null
  while (parent !== null && !seen.has(parent)) {
    seen.add(parent)
    out.push(parent)
    parent = tree.parents.get(parent) ?? null
  }
  return out
}

export function selectAll(tree: ImportTree): Set<string> {
  return new Set(tree.byId.keys())
}

/** Checks `id` with its subtree, or (when it is already checked) unchecks its subtree and its ancestors. */
export function toggleNode(tree: ImportTree, selected: ReadonlySet<string>, id: string): Set<string> {
  const next = new Set(selected)
  if (!tree.byId.has(id)) return next
  if (selected.has(id)) {
    for (const item of subtreeIds(tree, id)) next.delete(item)
    for (const item of ancestorIds(tree, id)) next.delete(item)
  } else {
    for (const item of subtreeIds(tree, id)) next.add(item)
  }
  return next
}

/**
 * Tri-state of every node: checked (the node, and so its subtree, is selected), indeterminate (not selected, but
 * something below it is), unchecked. One bottom-up pass.
 */
export function checkStates(tree: ImportTree, selected: ReadonlySet<string>): Map<string, CheckState> {
  const states = new Map<string, CheckState>()
  const anyBelow = new Map<string, boolean>()
  // Tree order has parents first, so walking it backwards visits children before parents.
  const order = tree.nodes
  for (let index = order.length - 1; index >= 0; index -= 1) {
    const id = order[index].notion_id
    const below = childIds(tree, id).some((child) => selected.has(child) || anyBelow.get(child) === true)
    anyBelow.set(id, below)
    states.set(id, selected.has(id) ? 'checked' : below ? 'indeterminate' : 'unchecked')
  }
  return states
}

/** Selected nodes whose parent is not selected: the smallest `notion_ids` list that covers the selection. */
export function minimalRoots(tree: ImportTree, selected: ReadonlySet<string>): string[] {
  return tree.nodes
    .map((node) => node.notion_id)
    .filter((id) => {
      if (!selected.has(id)) return false
      const parent = tree.parents.get(id) ?? null
      return parent === null || !selected.has(parent)
    })
}

export function selectedCount(tree: ImportTree, selected: ReadonlySet<string>): number {
  let count = 0
  for (const id of tree.byId.keys()) if (selected.has(id)) count += 1
  return count
}

/** The `selection` body for `start`: `{ all: true }` when every node is checked, else the minimal roots; null when empty. */
export function selectionBody(tree: ImportTree, selected: ReadonlySet<string>): NotionImportSelectionBody | null {
  const count = selectedCount(tree, selected)
  if (count === 0) return null
  if (count === tree.byId.size) return { all: true }
  return { notion_ids: minimalRoots(tree, selected) }
}

export interface VisibleRow {
  node: ImportNode
  depth: number
  hasChildren: boolean
  expanded: boolean
  /** The row's title matches the search query. */
  match: boolean
}

export function nodeTitle(node: Pick<ImportNode, 'title'>): string {
  return node.title.trim() ? node.title : 'Untitled'
}

/**
 * The rows to render, in tree order. Without a query: roots plus the children of expanded nodes. With a query:
 * the matching nodes (case-insensitive title match) and their ancestors, ancestors shown expanded.
 */
export function visibleRows(tree: ImportTree, expanded: ReadonlySet<string>, query = ''): VisibleRow[] {
  const needle = query.trim().toLowerCase()
  const rows: VisibleRow[] = []
  if (!needle) {
    const walk = (parent: string | null, depth: number, seen: Set<string>) => {
      for (const id of childIds(tree, parent)) {
        if (seen.has(id)) continue
        seen.add(id)
        const node = tree.byId.get(id)!
        const hasChildren = childIds(tree, id).length > 0
        const open = hasChildren && expanded.has(id)
        rows.push({ node, depth, hasChildren, expanded: open, match: false })
        if (open) walk(id, depth + 1, seen)
      }
    }
    walk(null, 0, new Set())
    return rows
  }
  const matches = new Set<string>()
  const shown = new Set<string>()
  for (const node of tree.nodes) {
    if (!nodeTitle(node).toLowerCase().includes(needle)) continue
    matches.add(node.notion_id)
    shown.add(node.notion_id)
    for (const ancestor of ancestorIds(tree, node.notion_id)) shown.add(ancestor)
  }
  const walk = (parent: string | null, depth: number) => {
    for (const id of childIds(tree, parent)) {
      if (!shown.has(id)) continue
      shown.delete(id)
      const node = tree.byId.get(id)!
      const hasChildren = childIds(tree, id).length > 0
      const open = childIds(tree, id).some((child) => shown.has(child))
      rows.push({ node, depth, hasChildren, expanded: open, match: matches.has(id) })
      walk(id, depth + 1)
    }
  }
  walk(null, 0)
  return rows
}
