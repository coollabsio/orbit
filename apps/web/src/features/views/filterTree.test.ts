import { expect, test } from 'bun:test'
import { MAX_FILTER_CONDITIONS, appendChild, appendCondition, blankCondition, firstLocalIssue, parseIssuePath, toIssuePath, updateNode } from './filterTree'
import { effectiveFilter, type Condition, type FilterGroup, type FilterNode } from './viewState'

const HIGH = { field: 'priority', operator: 'is', value: ['high'] } as const
const TREE: FilterGroup = { op: 'and', children: [{ ...HIGH, value: ['high'] }, { op: 'or', children: [{ ...HIGH, value: ['urgent'] }] }] }

test('issue paths round-trip between the server format and node paths', () => {
  expect(toIssuePath([1, 0])).toBe('filter.children[1].children[0]')
  expect(parseIssuePath('filter.children[1].children[0].value')).toEqual([1, 0])
  expect(parseIssuePath('filter')).toEqual([])
  expect(parseIssuePath('display.layout')).toBeNull()
})

test('updateNode replaces or removes a nested node without touching siblings', () => {
  expect(updateNode(TREE, [1, 0], () => null)).toEqual({ op: 'and', children: [{ ...HIGH, value: ['high'] }, { op: 'or', children: [] }] })
  expect(updateNode(TREE, [1], (node) => ({ ...(node as FilterGroup), op: 'and' }))).toEqual({
    op: 'and',
    children: [{ ...HIGH, value: ['high'] }, { op: 'and', children: [{ ...HIGH, value: ['urgent'] }] }],
  })
})

test('appendChild adds to the group at a path', () => {
  expect(appendChild(TREE, [1], blankCondition())).toEqual({
    op: 'and',
    children: [{ ...HIGH, value: ['high'] }, { op: 'or', children: [{ ...HIGH, value: ['urgent'] }, { field: 'status', operator: 'is', value: [] }] }],
  })
})

test('firstLocalIssue points at the first incomplete condition', () => {
  expect(firstLocalIssue(TREE)).toBeNull()
  expect(firstLocalIssue(appendChild(TREE, [1], blankCondition()))).toEqual({ path: 'filter.children[1].children[1].value', message: 'Choose a value.' })
  expect(firstLocalIssue({ op: 'and', children: [{ field: 'text', operator: 'contains', value: '' }] })).toEqual({
    path: 'filter.children[0].value',
    message: 'Enter text to search for.',
  })
})

// --- depth and size, measured the way the server sees the effective tree (preflight #45) ---

/** Mirrors the server's `validate_group`: groups may nest 3 levels below the root (level 0), 50 conditions in all. */
function serverRejects(effective: FilterGroup): 'depth' | 'count' | null {
  let conditions = 0
  const visit = (group: FilterGroup, nesting: number): 'depth' | 'count' | null => {
    if (nesting > 3) return 'depth'
    for (const child of group.children) {
      if ('op' in child) {
        const inner = visit(child, nesting + 1)
        if (inner) return inner
      } else if (++conditions > 50) {
        return 'count'
      }
    }
    return null
  }
  return visit(effective, 0)
}

const LOW: Condition = { field: 'priority', operator: 'is', value: ['low'] }
const SCOPE = { preset: 'my_week', projectId: 'project-a' } as const

/** An OR root at the editor's max depth (root → And group → Or group), with two children so the chip bar wraps it. */
const OR_AT_MAX_DEPTH: FilterGroup = { op: 'or', children: [{ ...HIGH, value: ['high'] }, { op: 'and', children: [LOW, { op: 'or', children: [LOW] }] }] }

test('the chip bar wraps an OR root with two or more children, and only then', () => {
  expect(appendCondition(OR_AT_MAX_DEPTH, LOW)).toEqual({ op: 'and', children: [OR_AT_MAX_DEPTH, LOW] })
  expect(appendCondition({ op: 'or', children: [LOW] }, LOW)).toEqual({ op: 'and', children: [LOW, LOW] })
  expect(appendCondition({ op: 'and', children: [LOW] }, LOW)).toEqual({ op: 'and', children: [LOW, LOW] })
})

test('an OR root at max depth wrapped by the chip bar is accepted and valid on an unscoped page', () => {
  const wrapped = appendCondition(OR_AT_MAX_DEPTH, LOW)
  expect(firstLocalIssue(OR_AT_MAX_DEPTH)).toBeNull()
  expect(firstLocalIssue(wrapped)).toBeNull()
  // unscoped: the wrapper is sent as-is and adds a level (the innermost group sits at server level 3)
  expect(effectiveFilter(wrapped, {})).toBe(wrapped)
  expect(serverRejects(effectiveFilter(wrapped, {}))).toBeNull()
  expect(serverRejects(effectiveFilter(OR_AT_MAX_DEPTH, {}))).toBeNull()
})

test('an OR root at max depth wrapped by the chip bar is accepted and valid on a scoped page', () => {
  const wrapped = appendCondition(OR_AT_MAX_DEPTH, LOW)
  expect(firstLocalIssue(wrapped)).toBeNull()
  // scoped: the AND wrapper flattens into the scope; an unwrapped OR root nests under it instead
  expect(serverRejects(effectiveFilter(wrapped, SCOPE))).toBeNull()
  expect(serverRejects(effectiveFilter(wrapped, { preset: 'overdue' }))).toBeNull()
  expect(serverRejects(effectiveFilter(OR_AT_MAX_DEPTH, SCOPE))).toBeNull()
})

test('switching the wrapped root to OR is flagged, because a scope would nest it past the server limit', () => {
  const flipped = updateNode(appendCondition(OR_AT_MAX_DEPTH, LOW), [], (node) => ({ ...(node as FilterGroup), op: 'or' }))
  expect(serverRejects(effectiveFilter(flipped, {}))).toBeNull()
  expect(serverRejects(effectiveFilter(flipped, SCOPE))).toBe('depth')
  expect(firstLocalIssue(flipped)).toEqual({ path: 'filter.children[0].children[1].children[1]', message: 'Groups can nest up to 2 levels below an Or root.' })
})

test('an AND root one level deeper than the wrapper is flagged at the offending group', () => {
  const tooDeep: FilterGroup = { op: 'and', children: [{ op: 'or', children: [{ op: 'and', children: [{ op: 'or', children: [{ op: 'and', children: [LOW] }] }] }] }] }
  expect(serverRejects(effectiveFilter(tooDeep, {}))).toBe('depth')
  expect(firstLocalIssue(tooDeep)).toEqual({ path: 'filter.children[0].children[0].children[0].children[0]', message: 'Groups can nest up to 3 levels below the root.' })
})

test('the condition cap leaves room for the largest scope, wrapped or not', () => {
  const conditions = (count: number): FilterNode[] => Array.from({ length: count }, () => LOW)
  const atCap = appendCondition({ op: 'or', children: [...conditions(MAX_FILTER_CONDITIONS - 2), { op: 'and', children: [LOW] }] }, LOW)
  expect(firstLocalIssue(atCap)).toBeNull()
  expect(serverRejects(effectiveFilter(atCap, SCOPE))).toBeNull()
  const overCap = appendCondition(atCap, LOW)
  expect(firstLocalIssue(overCap)).toEqual({ path: 'filter', message: `Filters can have at most ${MAX_FILTER_CONDITIONS} conditions.` })
})
