// Pure helpers over the flat page tree (`GET /pages` → PageSummary[]). No React or API imports.
import type { PageSummary, Teamspace } from '@/api/generated/types.gen'

export type DropZone = 'before' | 'after' | 'inside'

/**
 * What every page needs in the tree; `PageSummary` and `Page` both satisfy it. The space fields are optional so
 * bare fixtures work: a page without them counts as private.
 */
export type TreePage = Pick<PageSummary, 'id' | 'parent_id' | 'position'> & Partial<Pick<PageSummary, 'teamspace_id' | 'private'>>

/** A page's space: one teamspace, or the caller's private space. Sub-pages always share their root's space. */
export type SpaceKey = 'private' | `teamspace:${string}`

export const PRIVATE_SPACE: SpaceKey = 'private'

export function teamspaceSpace(teamspaceId: string): SpaceKey {
  return `teamspace:${teamspaceId}`
}

export function spaceKey(page: { teamspace_id?: string | null; private?: boolean }): SpaceKey {
  return page.private || !page.teamspace_id ? PRIVATE_SPACE : teamspaceSpace(page.teamspace_id)
}

/** The teamspace id of a space key, or null for the private space. */
export function spaceTeamspaceId(space: SpaceKey): string | null {
  return space === PRIVATE_SPACE ? null : space.slice('teamspace:'.length)
}

/** The page fields a page gets in `space`. */
export function spaceFields(space: SpaceKey): { teamspace_id: string | null; private: boolean } {
  const teamspaceId = spaceTeamspaceId(space)
  return { teamspace_id: teamspaceId, private: teamspaceId === null }
}

/** Space fields for the create/move API (`parent_id: null` only). */
export function spaceRequest(space: SpaceKey): { teamspace_id: string } | { private: true } {
  const teamspaceId = spaceTeamspaceId(space)
  return teamspaceId === null ? { private: true } : { teamspace_id: teamspaceId }
}

/** "Private", the teamspace's name, or "Teamspace" while the list is unknown. */
export function spaceLabel(space: SpaceKey, teamspaces: readonly Pick<Teamspace, 'id' | 'name'>[] | undefined): string {
  const teamspaceId = spaceTeamspaceId(space)
  if (teamspaceId === null) return 'Private'
  return teamspaces?.find((teamspace) => teamspace.id === teamspaceId)?.name ?? 'Teamspace'
}

/** Where new root pages go by default: the teamspace flagged `is_default`, else the first by position. */
export function defaultTeamspace<T extends Pick<Teamspace, 'id' | 'is_default' | 'position'>>(teamspaces: readonly T[] | undefined): T | null {
  if (!teamspaces?.length) return null
  return teamspaces.find((teamspace) => teamspace.is_default) ?? [...teamspaces].sort((a, b) => a.position - b.position)[0]
}

/** Empty titles read as "Untitled" everywhere (tree, breadcrumbs, links, search). */
export function pageTitle(page: { title: string } | null | undefined): string {
  return page?.title.trim() ? page.title : 'Untitled'
}

function byPosition(a: TreePage, b: TreePage): number {
  return a.position - b.position || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0)
}

/** Direct children of `parentId`, ordered by position then id (the server's order). `null` = roots of every space. */
export function childrenOf<T extends TreePage>(pages: readonly T[], parentId: string | null): T[] {
  return pages.filter((page) => page.parent_id === parentId).sort(byPosition)
}

/** Root pages of one space, in tree order. */
export function rootsOf<T extends TreePage>(pages: readonly T[], space: SpaceKey): T[] {
  return pages.filter((page) => page.parent_id === null && spaceKey(page) === space).sort(byPosition)
}

/** Root pages grouped by space (each list in tree order). Spaces without pages are absent. */
export function rootsBySpace<T extends TreePage>(pages: readonly T[]): Map<SpaceKey, T[]> {
  const groups = new Map<SpaceKey, T[]>()
  for (const page of childrenOf(pages, null)) {
    const key = spaceKey(page)
    groups.set(key, [...(groups.get(key) ?? []), page])
  }
  return groups
}

/** The list `page` is ordered in: its parent's children, or the roots of its space. Includes the page itself. */
export function siblingsOf<T extends TreePage>(pages: readonly T[], page: TreePage): T[] {
  return page.parent_id === null ? rootsOf(pages, spaceKey(page)) : childrenOf(pages, page.parent_id)
}

/** Every page below `pageId`, breadth first. */
export function descendantsOf<T extends TreePage>(pages: readonly T[], pageId: string): T[] {
  const out: T[] = []
  const queue = [pageId]
  const seen = new Set(queue)
  while (queue.length > 0) {
    const parent = queue.shift()!
    for (const child of childrenOf(pages, parent)) {
      if (seen.has(child.id)) continue
      seen.add(child.id)
      out.push(child)
      queue.push(child.id)
    }
  }
  return out
}

/** Ancestor chain of `pageId`, root first, excluding the page itself. Stops at missing parents and cycles. */
export function ancestorsOf<T extends TreePage>(pages: readonly T[], pageId: string | null): T[] {
  const byId = new Map(pages.map((page) => [page.id, page]))
  const chain: T[] = []
  const seen = new Set<string>()
  let current = pageId ? byId.get(pageId) : undefined
  while (current?.parent_id && !seen.has(current.parent_id)) {
    seen.add(current.parent_id)
    const parent = byId.get(current.parent_id)
    if (!parent) break
    chain.unshift(parent)
    current = parent
  }
  return chain
}

/**
 * What to open so `pageId` becomes visible in its space section: its space (to un-collapse) and the ancestor rows
 * (root first) that `isOpen` reports closed. Null when the page is not in the tree. Never lists anything to close.
 */
export function revealPage<T extends TreePage>(
  pages: readonly T[],
  pageId: string | null,
  isOpen: (id: string) => boolean,
): { space: SpaceKey; expand: string[] } | null {
  const page = pageId ? pages.find((item) => item.id === pageId) : undefined
  if (!page) return null
  return { space: spaceKey(page), expand: ancestorsOf(pages, page.id).map((item) => item.id).filter((id) => !isOpen(id)) }
}

/** First root page in tree order across all spaces, or null when there are no pages. */
export function firstRootPage<T extends TreePage>(pages: readonly T[]): T | null {
  return childrenOf(pages, null)[0] ?? null
}

/**
 * Where `/docs` lands: the first root page of the default teamspace, else the first private page, else the first
 * page of any other teamspace (in teamspace order), else null.
 */
export function landingPage<T extends TreePage>(
  pages: readonly T[],
  teamspaces: readonly Pick<Teamspace, 'id' | 'is_default' | 'position'>[] | undefined,
): T | null {
  const fallback = defaultTeamspace(teamspaces)
  if (fallback) {
    const first = rootsOf(pages, teamspaceSpace(fallback.id))[0]
    if (first) return first
  }
  const firstPrivate = rootsOf(pages, PRIVATE_SPACE)[0]
  if (firstPrivate) return firstPrivate
  for (const teamspace of [...(teamspaces ?? [])].sort((a, b) => a.position - b.position)) {
    const first = rootsOf(pages, teamspaceSpace(teamspace.id))[0]
    if (first) return first
  }
  return firstRootPage(pages)
}

/** A page cannot move onto itself or into its own subtree. */
export function canDropOn(pages: readonly TreePage[], dragId: string, targetId: string): boolean {
  if (dragId === targetId) return false
  return !descendantsOf(pages, dragId).some((page) => page.id === targetId)
}

export interface MoveTarget {
  parent_id: string | null
  /** Index among the new siblings, not counting the moved page. */
  position: number
  /** With `parent_id: null` only, when the page changes space: the target teamspace. */
  teamspace_id?: string
  /** With `parent_id: null` only, when the page changes space: `true` moves it into the private space. */
  private?: true
}

/** The space a move lands in (a parent's space, the explicit target space, or the page's current one). */
export function moveSpace(pages: readonly TreePage[], page: TreePage, move: MoveTarget): SpaceKey {
  if (move.parent_id !== null) {
    const parent = pages.find((item) => item.id === move.parent_id)
    return parent ? spaceKey(parent) : spaceKey(page)
  }
  if (move.private) return PRIVATE_SPACE
  if (move.teamspace_id) return teamspaceSpace(move.teamspace_id)
  return spaceKey(page)
}

/**
 * Maps a tree drop to the move API: `inside` appends as the target's last child; `before`/`after` put the page
 * next to the target among the target's siblings. The index is computed with the dragged page removed, which is
 * what the server expects. Dropping next to a root page of another space also sends that space (under a parent the
 * space follows the parent). Returns null for invalid drops (cycles, unknown ids) and for drops that change nothing.
 */
export function dropToMove(pages: readonly TreePage[], dragId: string, targetId: string, zone: DropZone): MoveTarget | null {
  const dragged = pages.find((page) => page.id === dragId)
  const target = pages.find((page) => page.id === targetId)
  if (!dragged || !target || !canDropOn(pages, dragId, targetId)) return null

  let move: MoveTarget
  if (zone === 'inside') {
    const siblings = childrenOf(pages, target.id).filter((page) => page.id !== dragId)
    move = { parent_id: target.id, position: siblings.length }
  } else {
    const siblings = siblingsOf(pages, target).filter((page) => page.id !== dragId)
    const index = siblings.findIndex((page) => page.id === target.id)
    move = { parent_id: target.parent_id, position: zone === 'before' ? index : index + 1 }
    if (target.parent_id === null && spaceKey(target) !== spaceKey(dragged)) move = { ...move, ...spaceRequest(spaceKey(target)) }
  }
  return unchanged(pages, dragged, move) ? null : move
}

/** Dropping on a space's header (or its empty row): the page becomes the last root page of that space. */
export function dropOnSpace(pages: readonly TreePage[], dragId: string, space: SpaceKey): MoveTarget | null {
  const dragged = pages.find((page) => page.id === dragId)
  if (!dragged) return null
  const roots = rootsOf(pages, space).filter((page) => page.id !== dragId)
  const move: MoveTarget = { parent_id: null, position: roots.length }
  if (spaceKey(dragged) !== space) Object.assign(move, spaceRequest(space))
  return unchanged(pages, dragged, move) ? null : move
}

function unchanged(pages: readonly TreePage[], dragged: TreePage, move: MoveTarget): boolean {
  if (move.parent_id !== dragged.parent_id || moveSpace(pages, dragged, move) !== spaceKey(dragged)) return false
  return siblingsOf(pages, dragged).findIndex((page) => page.id === dragged.id) === move.position
}

/**
 * Optimistic local version of a move: the page takes its new parent and index, and both the old and the new
 * sibling lists are renumbered 0..n-1 (the server renumbers the same way). A page that changes space takes its
 * whole subtree along.
 */
export function applyMove<T extends TreePage>(pages: readonly T[], pageId: string, move: MoveTarget): T[] {
  const moved = pages.find((page) => page.id === pageId)
  if (!moved) return [...pages]
  const from = spaceKey(moved)
  const to = moveSpace(pages, moved, move)
  const renumbered = new Map<string, Partial<TreePage>>()

  const oldSiblings = siblingsOf(pages, moved).filter((page) => page.id !== pageId)
  oldSiblings.forEach((page, index) => renumbered.set(page.id, { position: index }))

  const newSiblings = (move.parent_id === null ? rootsOf(pages, to) : childrenOf(pages, move.parent_id)).filter((page) => page.id !== pageId)
  const at = Math.max(0, Math.min(move.position, newSiblings.length))
  const ordered = [...newSiblings.slice(0, at), moved, ...newSiblings.slice(at)]
  ordered.forEach((page, index) => renumbered.set(page.id, { parent_id: move.parent_id, position: index }))

  if (from !== to) {
    const fields = spaceFields(to)
    for (const page of [moved, ...descendantsOf(pages, pageId)]) renumbered.set(page.id, { ...renumbered.get(page.id), ...fields })
  }

  return pages.map((page) => {
    const next = renumbered.get(page.id)
    return next ? { ...page, ...next } : page
  })
}

/** Optimistic trash: the page and its whole live subtree leave the tree (the server trashes the subtree). */
export function removeSubtree<T extends TreePage>(pages: readonly T[], pageId: string): T[] {
  const gone = new Set([pageId, ...descendantsOf(pages, pageId).map((page) => page.id)])
  return pages.filter((page) => !gone.has(page.id))
}
