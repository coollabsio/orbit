/**
 * The scroll position of a thread while it moves between the pane and the full view (expand, collapse): the view that
 * closes leaves it here, and the one that opens starts from it.
 */
const positions = new Map<string, number>()

export function keepThreadScroll(rootId: string, fromBottom: number) {
  positions.set(rootId, fromBottom)
}

export function keptThreadScroll(rootId: string): number | undefined {
  return positions.get(rootId)
}

export function forgetThreadScroll(rootId: string) {
  positions.delete(rootId)
}
