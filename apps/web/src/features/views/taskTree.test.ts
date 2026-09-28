import { expect, test } from 'bun:test'
import { buildTaskTree, descendantIds, flattenTree, MAX_TREE_INDENT, subtreeSize } from './taskTree'

type Node = { id: string; parentTaskId?: string | null }
const node = (id: string, parentTaskId: string | null = null): Node => ({ id, parentTaskId })
const ids = (rows: Array<{ task: Node }>) => rows.map((row) => row.task.id)

test('sub-issues nest under their parent and keep the input order among siblings', () => {
  const tasks = [node('b2', 'a'), node('a'), node('b1', 'a'), node('c'), node('d', 'b1')]
  const tree = buildTaskTree(tasks)
  expect(tree.roots.map((task) => task.id)).toEqual(['a', 'c'])
  expect(tree.childrenOf.get('a')!.map((task) => task.id)).toEqual(['b2', 'b1'])
  const rows = flattenTree(tree, tree.roots, new Set())
  expect(ids(rows)).toEqual(['a', 'b2', 'b1', 'd', 'c'])
  expect(rows.map((row) => row.depth)).toEqual([0, 1, 1, 2, 0])
  expect(rows.map((row) => row.hasChildren)).toEqual([true, false, true, false, false])
})

test('a task whose parent is not in the result is a root, not a nested row', () => {
  const tree = buildTaskTree([node('child', 'missing'), node('other')])
  expect(tree.roots.map((task) => task.id)).toEqual(['child', 'other'])
  expect(tree.nested.has('child')).toBe(false)
})

test('indentation stops at depth 6; deeper rows keep the depth-6 indent', () => {
  const chain = Array.from({ length: 9 }, (_, index) => node(`t${index}`, index === 0 ? null : `t${index - 1}`))
  const rows = flattenTree(buildTaskTree(chain), [chain[0]!], new Set())
  expect(MAX_TREE_INDENT).toBe(6)
  expect(rows.map((row) => row.depth)).toEqual([0, 1, 2, 3, 4, 5, 6, 7, 8])
  expect(rows.map((row) => row.indent)).toEqual([0, 1, 2, 3, 4, 5, 6, 6, 6])
})

test('a collapsed task keeps its row and hides its whole subtree', () => {
  const tree = buildTaskTree([node('a'), node('b', 'a'), node('c', 'b'), node('d')])
  expect(ids(flattenTree(tree, tree.roots, new Set(['a'])))).toEqual(['a', 'd'])
  expect(ids(flattenTree(tree, tree.roots, new Set(['b'])))).toEqual(['a', 'b', 'd'])
})

test('a parent cycle in stale data still shows every task exactly once', () => {
  const tree = buildTaskTree([node('x', 'y'), node('y', 'x'), node('z')])
  const rows = flattenTree(tree, tree.roots, new Set())
  expect(ids(rows)).toHaveLength(3)
  expect([...ids(rows)].sort()).toEqual(['x', 'y', 'z'])
})

test('subtree size and descendant ids count every nested task, collapsed or not', () => {
  const tree = buildTaskTree([node('a'), node('b', 'a'), node('c', 'b'), node('d', 'a'), node('e')])
  expect(subtreeSize(tree, 'a')).toBe(3)
  expect(subtreeSize(tree, 'e')).toBe(0)
  expect(descendantIds(tree, 'a').sort()).toEqual(['b', 'c', 'd'])
})
