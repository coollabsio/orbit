import { describe, expect, test } from 'bun:test'
import {
  buildImportTree,
  checkStates,
  minimalRoots,
  selectAll,
  selectedCount,
  selectionBody,
  subtreeIds,
  toggleNode,
  visibleRows,
  type ImportNode,
} from './selection'

const node = (notion_id: string, parent_id: string | null, title = notion_id, kind: ImportNode['kind'] = 'page'): ImportNode => ({
  notion_id, parent_id, title, kind, icon: null, child_count: 0,
})

// handbook ─┬─ alpha ── toggle
//           └─ beta ── tasks(db) ─┬─ row1
//                                 └─ row2
// orphan (parent not visible)
const tree = buildImportTree([
  node('handbook', null, 'Handbook'),
  node('alpha', 'handbook', 'Alpha'),
  node('toggle', 'alpha', 'In toggle'),
  node('beta', 'handbook', 'Beta'),
  node('tasks', 'beta', 'Tasks', 'database'),
  node('row1', 'tasks', 'Row one'),
  node('row2', 'tasks', ''),
  node('orphan', 'hidden-parent', 'Orphan'),
])

const ids = (set: ReadonlySet<string>) => [...set].sort()

describe('import tree', () => {
  test('nodes whose parent is not listed become roots', () => {
    expect(tree.children.get(null)).toEqual(['handbook', 'orphan'])
    expect(subtreeIds(tree, 'beta')).toEqual(['beta', 'tasks', 'row1', 'row2'])
  })
})

describe('tri-state selection', () => {
  test('checking a node includes all its descendants', () => {
    const selected = toggleNode(tree, new Set(), 'beta')
    expect(ids(selected)).toEqual(['beta', 'row1', 'row2', 'tasks'])
    const states = checkStates(tree, selected)
    expect(states.get('beta')).toBe('checked')
    expect(states.get('row2')).toBe('checked')
    expect(states.get('handbook')).toBe('indeterminate')
    expect(states.get('alpha')).toBe('unchecked')
    expect(states.get('orphan')).toBe('unchecked')
  })

  test('unchecking a descendant of a checked node un-checks its ancestors (they no longer include everything)', () => {
    let selected = toggleNode(tree, new Set(), 'handbook')
    selected = toggleNode(tree, selected, 'row1')
    expect(ids(selected)).toEqual(['alpha', 'row2', 'toggle'])
    const states = checkStates(tree, selected)
    expect(states.get('handbook')).toBe('indeterminate')
    expect(states.get('beta')).toBe('indeterminate')
    expect(states.get('tasks')).toBe('indeterminate')
    expect(states.get('alpha')).toBe('checked')
    expect(states.get('row1')).toBe('unchecked')
  })

  test('checking an indeterminate node selects its whole subtree again', () => {
    let selected = toggleNode(tree, new Set(), 'row1')
    expect(checkStates(tree, selected).get('beta')).toBe('indeterminate')
    selected = toggleNode(tree, selected, 'beta')
    expect(checkStates(tree, selected).get('beta')).toBe('checked')
    expect(ids(selected)).toEqual(['beta', 'row1', 'row2', 'tasks'])
  })

  test('checking every child does not select the parent page itself', () => {
    let selected = toggleNode(tree, new Set(), 'row1')
    selected = toggleNode(tree, selected, 'row2')
    expect(checkStates(tree, selected).get('tasks')).toBe('indeterminate')
  })
})

describe('selection body', () => {
  test('nothing selected → null', () => {
    expect(selectionBody(tree, new Set())).toBeNull()
  })

  test('everything checked → { all: true }', () => {
    expect(selectionBody(tree, selectAll(tree))).toEqual({ all: true })
    let selected = toggleNode(tree, new Set(), 'handbook')
    selected = toggleNode(tree, selected, 'orphan')
    expect(selectionBody(tree, selected)).toEqual({ all: true })
  })

  test('otherwise the minimal set of checked roots, in tree order', () => {
    let selected = toggleNode(tree, new Set(), 'orphan')
    selected = toggleNode(tree, selected, 'tasks')
    selected = toggleNode(tree, selected, 'alpha')
    expect(minimalRoots(tree, selected)).toEqual(['alpha', 'tasks', 'orphan'])
    expect(selectionBody(tree, selected)).toEqual({ notion_ids: ['alpha', 'tasks', 'orphan'] })
    expect(selectedCount(tree, selected)).toBe(6)
  })

  test('all but one page → the remaining roots', () => {
    const selected = toggleNode(tree, selectAll(tree), 'toggle')
    expect(selectionBody(tree, selected)).toEqual({ notion_ids: ['beta', 'orphan'] })
  })
})

describe('visible rows', () => {
  test('roots, plus children of expanded nodes', () => {
    expect(visibleRows(tree, new Set()).map((row) => row.node.notion_id)).toEqual(['handbook', 'orphan'])
    const rows = visibleRows(tree, new Set(['handbook', 'beta']))
    expect(rows.map((row) => [row.node.notion_id, row.depth, row.hasChildren, row.expanded])).toEqual([
      ['handbook', 0, true, true],
      ['alpha', 1, true, false],
      ['beta', 1, true, true],
      ['tasks', 2, true, false],
      ['orphan', 0, false, false],
    ])
  })

  test('search shows matches with their ancestors expanded; "Untitled" matches empty titles', () => {
    const rows = visibleRows(tree, new Set(), 'row ONE')
    expect(rows.map((row) => [row.node.notion_id, row.match, row.expanded])).toEqual([
      ['handbook', false, true],
      ['beta', false, true],
      ['tasks', false, true],
      ['row1', true, false],
    ])
    expect(visibleRows(tree, new Set(), 'untitled').map((row) => row.node.notion_id)).toEqual(['handbook', 'beta', 'tasks', 'row2'])
    expect(visibleRows(tree, new Set(), 'nothing')).toEqual([])
  })
})
