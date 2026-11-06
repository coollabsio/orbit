// Pure tree + selection logic for the "Choose pages" step of the Docs imports (Notion, Obsidian). No React or API
// imports.
//
// Selection model: the server imports a selected node together with everything below it (`selection.notion_ids`,
// `selection.paths`), so a checked node always means "this node and everything under it". The selected set is
// therefore kept closed under descendants: checking a node adds its subtree, unchecking a node removes its subtree
// and its ancestors (they no longer include everything).

/** What the selection logic reads from a scanned node; `identify` gives its id and parent id. */
export interface ImportNode {
  title: string
}

export type IdentifyNode<T> = (node: T) => readonly [id: string, parentId: string | null]

export interface ImportTree<T extends ImportNode = ImportNode> {
  /** Node ids in tree order (parents before children, siblings in source order). */
  ids: string[]
  byId: Map<string, T>
  /** Child ids per parent id; `null` holds the roots (no parent, or a parent missing from the list). */
  children: Map<string | null, string[]>
  /** The parent of each node inside this tree (`null` for roots). */
  parents: Map<string, string | null>
}

export type CheckState = 'checked' | 'unchecked' | 'indeterminate'

export function buildImportTree<T extends ImportNode>(nodes: readonly T[], identify: IdentifyNode<T>): ImportTree<T> {
  const byId = new Map<string, T>()
  const declaredParents = new Map<string, string | null>()
  for (const node of nodes) {
    const [id, parentId] = identify(node)
    if (byId.has(id)) continue
    byId.set(id, node)
    declaredParents.set(id, parentId)
  }
  const children = new Map<string | null, string[]>()
  const parents = new Map<string, string | null>()
  for (const [id, declared] of declaredParents) {
    const parent = declared !== null && declared !== id && byId.has(declared) ? declared : null
    parents.set(id, parent)
    const list = children.get(parent)
    if (list) list.push(id)
    else children.set(parent, [id])
  }
  return { ids: [...byId.keys()], byId, children, parents }
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
  const order = tree.ids
  for (let index = order.length - 1; index >= 0; index -= 1) {
    const id = order[index]
    const below = childIds(tree, id).some((child) => selected.has(child) || anyBelow.get(child) === true)
    anyBelow.set(id, below)
    states.set(id, selected.has(id) ? 'checked' : below ? 'indeterminate' : 'unchecked')
  }
  return states
}

/** Selected nodes whose parent is not selected: the smallest list of roots that covers the selection. */
export function minimalRoots(tree: ImportTree, selected: ReadonlySet<string>): string[] {
  return tree.ids.filter((id) => {
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

export type ImportSelection = { all: true } | { roots: string[] }

/** What `start` imports: `{ all: true }` when every node is checked, else the minimal roots; null when empty. */
export function importSelection(tree: ImportTree, selected: ReadonlySet<string>): ImportSelection | null {
  const count = selectedCount(tree, selected)
  if (count === 0) return null
  if (count === tree.byId.size) return { all: true }
  return { roots: minimalRoots(tree, selected) }
}

export interface VisibleRow<T extends ImportNode = ImportNode> {
  id: string
  node: T
  depth: number
  hasChildren: boolean
  expanded: boolean
  /** The row's title matches the search query. */
  match: boolean
}

export function nodeTitle(node: ImportNode): string {
  return node.title.trim() ? node.title : 'Untitled'
}

/**
 * The rows to render, in tree order. Without a query: roots plus the children of expanded nodes. With a query:
 * the matching nodes (case-insensitive title match) and their ancestors, ancestors shown expanded.
 */
export function visibleRows<T extends ImportNode>(tree: ImportTree<T>, expanded: ReadonlySet<string>, query = ''): VisibleRow<T>[] {
  const needle = query.trim().toLowerCase()
  const rows: VisibleRow<T>[] = []
  if (!needle) {
    const walk = (parent: string | null, depth: number, seen: Set<string>) => {
      for (const id of childIds(tree, parent)) {
        if (seen.has(id)) continue
        seen.add(id)
        const node = tree.byId.get(id)!
        const hasChildren = childIds(tree, id).length > 0
        const open = hasChildren && expanded.has(id)
        rows.push({ id, node, depth, hasChildren, expanded: open, match: false })
        if (open) walk(id, depth + 1, seen)
      }
    }
    walk(null, 0, new Set())
    return rows
  }
  const matches = new Set<string>()
  const shown = new Set<string>()
  for (const [id, node] of tree.byId) {
    if (!nodeTitle(node).toLowerCase().includes(needle)) continue
    matches.add(id)
    shown.add(id)
    for (const ancestor of ancestorIds(tree, id)) shown.add(ancestor)
  }
  const walk = (parent: string | null, depth: number) => {
    for (const id of childIds(tree, parent)) {
      if (!shown.has(id)) continue
      shown.delete(id)
      const node = tree.byId.get(id)!
      const hasChildren = childIds(tree, id).length > 0
      const open = childIds(tree, id).some((child) => shown.has(child))
      rows.push({ id, node, depth, hasChildren, expanded: open, match: matches.has(id) })
      walk(id, depth + 1)
    }
  }
  walk(null, 0)
  return rows
}
