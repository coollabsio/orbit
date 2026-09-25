import { describe, expect, test } from 'bun:test'
import {
  ancestorsOf,
  applyMove,
  canDropOn,
  childrenOf,
  descendantsOf,
  dropOnSpace,
  dropToMove,
  firstRootPage,
  landingPage,
  pageTitle,
  removeSubtree,
  rootsBySpace,
  rootsOf,
  siblingsOf,
  spaceKey,
  spaceLabel,
  type TreePage,
} from './pageTree'

// a            d
// ├─ b
// │  └─ c
// └─ e
const pages: TreePage[] = [
  { id: 'd', parent_id: null, position: 1 },
  { id: 'e', parent_id: 'a', position: 1 },
  { id: 'a', parent_id: null, position: 0 },
  { id: 'c', parent_id: 'b', position: 0 },
  { id: 'b', parent_id: 'a', position: 0 },
]

const ids = (list: TreePage[]) => list.map((page) => page.id)
const layout = (list: TreePage[]) =>
  Object.fromEntries([...list].sort((x, y) => (x.id < y.id ? -1 : 1)).map((page) => [page.id, `${page.parent_id ?? '-'}:${page.position}`]))

describe('page tree helpers', () => {
  test('children are ordered by position, ancestors root first, descendants cover the subtree', () => {
    expect(ids(childrenOf(pages, null))).toEqual(['a', 'd'])
    expect(ids(childrenOf(pages, 'a'))).toEqual(['b', 'e'])
    expect(ids(ancestorsOf(pages, 'c'))).toEqual(['a', 'b'])
    expect(ids(ancestorsOf(pages, 'a'))).toEqual([])
    expect(ids(descendantsOf(pages, 'a')).sort()).toEqual(['b', 'c', 'e'])
    expect(firstRootPage(pages)?.id).toBe('a')
    expect(firstRootPage([])).toBeNull()
  })

  test('empty titles read as Untitled', () => {
    expect(pageTitle({ title: '' })).toBe('Untitled')
    expect(pageTitle({ title: '   ' })).toBe('Untitled')
    expect(pageTitle({ title: 'Roadmap' })).toBe('Roadmap')
  })

  test('a page cannot be dropped on itself or inside its own subtree', () => {
    expect(canDropOn(pages, 'a', 'a')).toBe(false)
    expect(canDropOn(pages, 'a', 'c')).toBe(false)
    expect(canDropOn(pages, 'c', 'a')).toBe(true)
    expect(dropToMove(pages, 'a', 'c', 'inside')).toBeNull()
    expect(dropToMove(pages, 'a', 'b', 'after')).toBeNull()
  })

  test('inside appends as the last child of the target', () => {
    expect(dropToMove(pages, 'd', 'a', 'inside')).toEqual({ parent_id: 'a', position: 2 })
    expect(dropToMove(pages, 'e', 'c', 'inside')).toEqual({ parent_id: 'c', position: 0 })
    // Already the last child of that parent: nothing to do.
    expect(dropToMove(pages, 'e', 'a', 'inside')).toBeNull()
  })

  test('before/after map to the index among the target siblings without the dragged page', () => {
    expect(dropToMove(pages, 'd', 'a', 'before')).toEqual({ parent_id: null, position: 0 })
    expect(dropToMove(pages, 'd', 'b', 'after')).toEqual({ parent_id: 'a', position: 1 })
    expect(dropToMove(pages, 'c', 'a', 'after')).toEqual({ parent_id: null, position: 1 })
    // Reorder within the same parent: e (index 1) before b (index 0).
    expect(dropToMove(pages, 'e', 'b', 'before')).toEqual({ parent_id: 'a', position: 0 })
    // Same slot → no-op.
    expect(dropToMove(pages, 'b', 'e', 'before')).toBeNull()
    expect(dropToMove(pages, 'e', 'b', 'after')).toBeNull()
  })

  test('applyMove renumbers the old and the new siblings like the server', () => {
    const moved = applyMove(pages, 'b', { parent_id: null, position: 1 })
    expect(layout(moved)).toEqual({ a: '-:0', b: '-:1', c: 'b:0', d: '-:2', e: 'a:0' })
    expect(ids(childrenOf(moved, null))).toEqual(['a', 'b', 'd'])

    const reordered = applyMove(pages, 'e', { parent_id: 'a', position: 0 })
    expect(ids(childrenOf(reordered, 'a'))).toEqual(['e', 'b'])
    // Out-of-range positions clamp to the end.
    expect(ids(childrenOf(applyMove(pages, 'c', { parent_id: 'a', position: 99 }), 'a'))).toEqual(['b', 'e', 'c'])
  })

  test('a drop mapped with dropToMove and applied locally lands where it was dropped', () => {
    const move = dropToMove(pages, 'd', 'b', 'after')!
    expect(ids(childrenOf(applyMove(pages, 'd', move), 'a'))).toEqual(['b', 'd', 'e'])
  })

  test('removeSubtree drops the page and everything below it', () => {
    expect(ids(removeSubtree(pages, 'b')).sort()).toEqual(['a', 'd', 'e'])
    expect(ids(removeSubtree(pages, 'a'))).toEqual(['d'])
  })
})

// General (t1): g1 ─ g2          Design (t2): (empty)          Private: p1 ─ p2
//               └─ g1a                                                 └─ p1a ─ p1b
const team = (id: string, parent_id: string | null, position: number, teamspace_id: string): TreePage => ({
  id, parent_id, position, teamspace_id, private: false,
})
const mine = (id: string, parent_id: string | null, position: number): TreePage => ({
  id, parent_id, position, teamspace_id: null, private: true,
})
const spaced: TreePage[] = [
  team('g2', null, 1, 't1'),
  team('g1', null, 0, 't1'),
  team('g1a', 'g1', 0, 't1'),
  mine('p1', null, 0),
  mine('p2', null, 1),
  mine('p1a', 'p1', 0),
  mine('p1b', 'p1a', 0),
]
const teamspaces = [
  { id: 't2', name: 'Design', position: 1, is_default: false },
  { id: 't1', name: 'General', position: 0, is_default: true },
]

describe('page spaces', () => {
  test('pages group into spaces; root siblings are per space', () => {
    expect(spaceKey(spaced[0])).toBe('teamspace:t1')
    expect(spaceKey(spaced[3])).toBe('private')
    const groups = rootsBySpace(spaced)
    expect([...groups.keys()].sort()).toEqual(['private', 'teamspace:t1'])
    expect(ids(groups.get('teamspace:t1')!)).toEqual(['g1', 'g2'])
    expect(ids(rootsOf(spaced, 'private'))).toEqual(['p1', 'p2'])
    expect(rootsOf(spaced, 'teamspace:t2')).toEqual([])
    expect(ids(siblingsOf(spaced, spaced[0]))).toEqual(['g1', 'g2'])
    expect(ids(siblingsOf(spaced, spaced[5]))).toEqual(['p1a'])
  })

  test('space labels', () => {
    expect(spaceLabel('private', teamspaces)).toBe('Private')
    expect(spaceLabel('teamspace:t2', teamspaces)).toBe('Design')
    expect(spaceLabel('teamspace:gone', teamspaces)).toBe('Teamspace')
  })

  test('/docs lands on the default teamspace, else a private page, else another teamspace', () => {
    expect(landingPage(spaced, teamspaces)?.id).toBe('g1')
    const privateOnly = spaced.filter((page) => page.private)
    expect(landingPage(privateOnly, teamspaces)?.id).toBe('p1')
    // Design becomes the default when it is flagged, but it is empty: private pages come next.
    expect(landingPage(spaced, [{ ...teamspaces[0], is_default: true }, { ...teamspaces[1], is_default: false }])?.id).toBe('p1')
    const designOnly = [team('d1', null, 0, 't2')]
    expect(landingPage(designOnly, teamspaces)?.id).toBe('d1')
    expect(landingPage([], teamspaces)).toBeNull()
  })

  test('before/after a root page of another space sends that space', () => {
    expect(dropToMove(spaced, 'p1', 'g1', 'after')).toEqual({ parent_id: null, position: 1, teamspace_id: 't1' })
    expect(dropToMove(spaced, 'g2', 'p1', 'before')).toEqual({ parent_id: null, position: 0, private: true })
    // A sub-page moved up to the root of another space.
    expect(dropToMove(spaced, 'g1a', 'p2', 'after')).toEqual({ parent_id: null, position: 2, private: true })
  })

  test('a drop onto (or next to a sub-page of) another space sends only the parent; the space follows it', () => {
    expect(dropToMove(spaced, 'p1', 'g1', 'inside')).toEqual({ parent_id: 'g1', position: 1 })
    expect(dropToMove(spaced, 'p2', 'g1a', 'before')).toEqual({ parent_id: 'g1', position: 0 })
  })

  test('same-space reorders are unchanged (no space fields, no-ops stay null)', () => {
    expect(dropToMove(spaced, 'g2', 'g1', 'before')).toEqual({ parent_id: null, position: 0 })
    expect(dropToMove(spaced, 'p2', 'p1', 'before')).toEqual({ parent_id: null, position: 0 })
    expect(dropToMove(spaced, 'g1', 'g2', 'before')).toBeNull()
    // The same index in another space's root list is a real move, not a no-op.
    expect(dropToMove(spaced, 'p1', 'g2', 'before')).toEqual({ parent_id: null, position: 1, teamspace_id: 't1' })
  })

  test('a drop on a space header moves the page to the end of that space root', () => {
    expect(dropOnSpace(spaced, 'p1', 'teamspace:t1')).toEqual({ parent_id: null, position: 2, teamspace_id: 't1' })
    expect(dropOnSpace(spaced, 'g1', 'teamspace:t2')).toEqual({ parent_id: null, position: 0, teamspace_id: 't2' })
    expect(dropOnSpace(spaced, 'g1', 'private')).toEqual({ parent_id: null, position: 2, private: true })
    // Same space: a sub-page becomes the last root page; the last root page is a no-op.
    expect(dropOnSpace(spaced, 'p1b', 'private')).toEqual({ parent_id: null, position: 2 })
    expect(dropOnSpace(spaced, 'p2', 'private')).toBeNull()
    expect(dropOnSpace(spaced, 'g1', 'teamspace:t1')).toEqual({ parent_id: null, position: 1 })
  })

  test('a move into another space takes the whole subtree along and renumbers both root lists', () => {
    const moved = applyMove(spaced, 'p1', { parent_id: null, position: 2, teamspace_id: 't1' })
    const space = (id: string) => spaceKey(moved.find((page) => page.id === id)!)
    expect(['p1', 'p1a', 'p1b'].map(space)).toEqual(['teamspace:t1', 'teamspace:t1', 'teamspace:t1'])
    expect(moved.find((page) => page.id === 'p1a')).toMatchObject({ teamspace_id: 't1', private: false, parent_id: 'p1' })
    expect(ids(rootsOf(moved, 'teamspace:t1'))).toEqual(['g1', 'g2', 'p1'])
    expect(ids(rootsOf(moved, 'private'))).toEqual(['p2'])
    expect(moved.find((page) => page.id === 'p2')?.position).toBe(0)
    // Untouched spaces keep their pages as they were.
    expect(space('g1a')).toBe('teamspace:t1')

    // Under a parent, the subtree takes the parent's space.
    const nested = applyMove(spaced, 'g1', { parent_id: 'p2', position: 0 })
    expect(['g1', 'g1a'].map((id) => spaceKey(nested.find((page) => page.id === id)!))).toEqual(['private', 'private'])
    expect(ids(rootsOf(nested, 'teamspace:t1'))).toEqual(['g2'])
    expect(nested.find((page) => page.id === 'g2')?.position).toBe(0)

    // Back to private (header drop).
    const back = applyMove(spaced, 'g1', dropOnSpace(spaced, 'g1', 'private')!)
    expect(back.find((page) => page.id === 'g1a')).toMatchObject({ teamspace_id: null, private: true })
    expect(ids(rootsOf(back, 'private'))).toEqual(['p1', 'p2', 'g1'])
  })
})
