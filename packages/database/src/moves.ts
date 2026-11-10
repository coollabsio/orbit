/** A page as the move picker sees it (a tree summary). */
export interface MovablePage {
  id: string
  parent_id: string | null
  kind: 'page' | 'database'
}

/**
 * The pages and databases `moving` can move under: not itself, not its own sub-pages, not its current parent, and no
 * database when `moving` is a database (a database cannot be a row). In the order of `pages`.
 */
export function moveDestinations<T extends MovablePage>(pages: readonly T[], moving: MovablePage): T[] {
  const below = new Set([moving.id])
  let grew = true
  while (grew) {
    grew = false
    for (const page of pages) {
      if (page.parent_id !== null && below.has(page.parent_id) && !below.has(page.id)) {
        below.add(page.id)
        grew = true
      }
    }
  }
  return pages.filter(
    (page) => !below.has(page.id) && page.id !== moving.parent_id && !(moving.kind === 'database' && page.kind === 'database'),
  )
}

/** The confirmation line for a move that deletes `count` property values of a row (a multi-select that keeps only some options counts). */
export function droppedValuesText(count: number): string {
  return count === 1 ? '1 property value will be removed.' : `${count} property values will be removed.`
}
