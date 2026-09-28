import { expect, test } from 'bun:test'
import {
  DEFAULT_DISPLAY,
  PRESET_FILTERS,
  countConditions,
  defaultViewState,
  effectiveFilter,
  emptyFilter,
  isFlatFilter,
  isGroup,
  isTaskPreset,
  legacyPreferencesToViewState,
  legacySortDisplay,
  normalizeDisplay,
  normalizeViewState,
  pageKeyFor,
  resolveDateValue,
  viewStatesEqual,
  type Condition,
  type FilterGroup,
  type ViewState,
} from './viewState'

const notDone: Condition = { field: 'status_category', operator: 'is_not', value: ['completed', 'cancelled', 'duplicate'] }
const thisWeek: Condition = { field: 'due_date', operator: 'between', value: [{ relative: 'start_of_week' }, { relative: 'end_of_week' }] }
const me: Condition = { field: 'assignee', operator: 'is', value: ['me'] }
const high: Condition = { field: 'priority', operator: 'is', value: ['high'] }

test('the default display matches the spec and every default is a fresh copy', () => {
  expect(DEFAULT_DISPLAY).toEqual({
    layout: 'list', group_by: 'status', sub_group_by: 'none', order_by: 'manual', order_direction: 'asc',
    properties: ['id', 'status', 'assignee', 'priority', 'project', 'due_date', 'labels', 'sub_issue_progress'],
    show_completed: 'all', show_empty_groups: false, sub_issues: 'nested',
  })
  const edited = defaultViewState()
  edited.display.properties.push('created')
  edited.filter.children.push(high)
  expect(defaultViewState()).toEqual({ filter: emptyFilter(), display: DEFAULT_DISPLAY })
})

test('preset filters match the spec table', () => {
  expect(PRESET_FILTERS).toEqual({
    mine: { op: 'and', children: [me] },
    overdue: { op: 'and', children: [{ field: 'due_date', operator: 'before', value: { relative: 'today' } }, notDone] },
    due_soon: { op: 'and', children: [{ field: 'due_date', operator: 'between', value: [{ relative: 'today' }, { relative: 'today', offset_days: 6 }] }, notDone] },
    current_week: { op: 'and', children: [thisWeek, notDone] },
    my_week: { op: 'and', children: [thisWeek, notDone, me] },
  })
})

test('preset names and page keys', () => {
  expect(['mine', 'overdue', 'due_soon', 'current_week', 'my_week'].every((name) => isTaskPreset(name))).toBeTrue()
  expect(isTaskPreset('all')).toBeFalse()
  expect(isTaskPreset(null)).toBeFalse()
  expect(isTaskPreset(undefined)).toBeFalse()
  expect(pageKeyFor({})).toBe('all')
  expect(pageKeyFor({ projectId: 'project-1' })).toBe('project:project-1')
  expect(pageKeyFor({ preset: 'overdue' })).toBe('preset:overdue')
  expect(pageKeyFor({ projectId: 'project-1', preset: 'mine' })).toBe('preset:mine')
})

test('normalizeDisplay coerces board grouping, repeated sub-grouping and property order', () => {
  expect(normalizeDisplay({ ...DEFAULT_DISPLAY, layout: 'board', group_by: 'none' }).group_by).toBe('status')
  expect(normalizeDisplay({ ...DEFAULT_DISPLAY, layout: 'list', group_by: 'none' }).group_by).toBe('none')
  expect(normalizeDisplay({ ...DEFAULT_DISPLAY, group_by: 'assignee', sub_group_by: 'assignee' }).sub_group_by).toBe('none')
  expect(normalizeDisplay({ ...DEFAULT_DISPLAY, group_by: 'none', sub_group_by: 'label' }).sub_group_by).toBe('none')
  expect(normalizeDisplay({ ...DEFAULT_DISPLAY, layout: 'board', group_by: 'none', sub_group_by: 'status' })).toMatchObject({ group_by: 'status', sub_group_by: 'none' })
  expect(normalizeDisplay({ ...DEFAULT_DISPLAY, group_by: 'status', sub_group_by: 'label' }).sub_group_by).toBe('label')
  expect(normalizeDisplay({ ...DEFAULT_DISPLAY, properties: ['labels', 'id', 'labels'] }).properties).toEqual(['id', 'labels'])
})

test('normalizeViewState fills display keys missing from stored state', () => {
  const stored = { filter: { op: 'and', children: [high] }, display: { layout: 'board' } } as unknown as ViewState
  expect(normalizeViewState(stored)).toEqual({ filter: { op: 'and', children: [high] }, display: { ...DEFAULT_DISPLAY, layout: 'board' } })
})

test('a stored display without sub_issues opens nested; the progress property sorts last', () => {
  const { sub_issues: _mode, ...legacy } = DEFAULT_DISPLAY
  const state = normalizeViewState({ filter: emptyFilter(), display: { ...legacy, properties: ['sub_issue_progress', 'id'] } as never })
  expect(state.display.sub_issues).toBe('nested')
  expect(state.display.properties).toEqual(['id', 'sub_issue_progress'])
})

test('viewStatesEqual ignores property and multi-value order but not the tree', () => {
  const soon: Condition = { field: 'due_date', operator: 'between', value: [{ relative: 'today' }, { relative: 'today', offset_days: 6 }] }
  const a: ViewState = {
    filter: { op: 'and', children: [{ field: 'label', operator: 'includes_any', value: ['bug', 'ui'] }, soon, { field: 'assignee', operator: 'is_empty' }] },
    display: { ...DEFAULT_DISPLAY, properties: ['labels', 'id'] },
  }
  const b: ViewState = {
    filter: { op: 'and', children: [
      { field: 'label', operator: 'includes_any', value: ['ui', 'bug'] },
      { field: 'due_date', operator: 'between', value: [{ relative: 'today', offset_days: 0 }, { offset_days: 6, relative: 'today' }] },
      { field: 'assignee', operator: 'is_empty', value: null },
    ] },
    display: { ...DEFAULT_DISPLAY, properties: ['id', 'labels'] },
  }
  expect(viewStatesEqual(a, b)).toBeTrue()

  const swappedBounds: ViewState = { ...a, filter: { op: 'and', children: [a.filter.children[0]!, { field: 'due_date', operator: 'between', value: [{ relative: 'today', offset_days: 6 }, { relative: 'today' }] }, a.filter.children[2]!] } }
  expect(viewStatesEqual(a, swappedBounds)).toBeFalse()
  expect(viewStatesEqual(a, { ...a, filter: { op: 'and', children: [...a.filter.children].reverse() } })).toBeFalse()
  expect(viewStatesEqual(a, { ...a, filter: { ...a.filter, op: 'or' } })).toBeFalse()
  expect(viewStatesEqual(a, { ...a, filter: { op: 'and', children: [{ op: 'and', children: a.filter.children }] } })).toBeFalse()
  expect(viewStatesEqual(a, { ...a, display: { ...a.display, order_direction: 'desc' } })).toBeFalse()

  const board: ViewState = { filter: emptyFilter(), display: { ...DEFAULT_DISPLAY, layout: 'board', group_by: 'none' } }
  expect(viewStatesEqual(board, { filter: emptyFilter(), display: { ...DEFAULT_DISPLAY, layout: 'board' } })).toBeTrue()
  expect(viewStatesEqual(board, defaultViewState())).toBeFalse()
})

test('condition counting, nesting and flat detection', () => {
  const nested: FilterGroup = { op: 'and', children: [
    high,
    { op: 'or', children: [
      { field: 'label', operator: 'excludes', value: ['bug'] },
      { op: 'and', children: [{ field: 'text', operator: 'contains', value: 'crash' }, { field: 'assignee', operator: 'is_empty' }] },
    ] },
  ] }
  expect(countConditions(nested)).toBe(4)
  expect(countConditions(emptyFilter())).toBe(0)
  expect(isGroup(nested.children[1]!)).toBeTrue()
  expect(isGroup(nested.children[0]!)).toBeFalse()
  expect(isFlatFilter(nested)).toBeFalse()
  expect(isFlatFilter({ op: 'and', children: [high, me] })).toBeTrue()
  expect(isFlatFilter({ op: 'or', children: [high] })).toBeFalse()
  expect(isFlatFilter(emptyFilter())).toBeTrue()
})

test('effectiveFilter ANDs preset, project scope and the user tree', () => {
  const user: FilterGroup = { op: 'and', children: [high] }
  const project: Condition = { field: 'project', operator: 'is', value: ['project-1'] }
  expect(effectiveFilter(user, {})).toBe(user)
  expect(effectiveFilter(emptyFilter(), { preset: 'overdue', projectId: 'project-1' })).toEqual({
    op: 'and', children: [...PRESET_FILTERS.overdue.children, project],
  })
  expect(effectiveFilter(user, { preset: 'mine' })).toEqual({ op: 'and', children: [me, high] })

  const either: FilterGroup = { op: 'or', children: [high, me] }
  expect(effectiveFilter(either, {})).toBe(either)
  expect(effectiveFilter(either, { projectId: 'project-1' })).toEqual({ op: 'and', children: [project, either] })
  expect(effectiveFilter({ op: 'or', children: [] }, { projectId: 'project-1' })).toEqual({ op: 'and', children: [project] })
  // presets are never mutated by scoping
  expect(PRESET_FILTERS.mine.children).toEqual([me])
})

test('a tree at the editor limits stays inside the server limits on the most scoped page', () => {
  // editor maximum: the user root plus two nested levels, 46 conditions (Consolidated contract notes: limits)
  const leaf = (index: number): Condition => ({ field: 'text', operator: 'contains', value: `term ${index}` })
  const deepest: FilterGroup = { op: 'and', children: Array.from({ length: 44 }, (_, index) => leaf(index)) }
  const user: FilterGroup = { op: 'or', children: [leaf(44), { op: 'or', children: [leaf(45), deepest] }] }
  const depth = (group: FilterGroup): number => Math.max(0, ...group.children.filter(isGroup).map((child) => 1 + depth(child)))
  expect(countConditions(user)).toBe(46)
  expect(depth(user)).toBe(2)
  // an OR root is nested one level deeper under the preset + project scope
  const effective = effectiveFilter(user, { preset: 'my_week', projectId: 'project-1' })
  expect(depth(effective)).toBe(3) // server MAX_NESTING
  expect(countConditions(effective)).toBe(50) // server MAX_CONDITIONS
})

test('relative dates resolve in UTC with Monday week starts', () => {
  const tuesdayEvening = new Date('2026-09-22T18:30:00.000Z')
  expect(resolveDateValue({ relative: 'today' }, tuesdayEvening)).toBe('2026-09-22')
  expect(resolveDateValue({ relative: 'today', offset_days: 6 }, tuesdayEvening)).toBe('2026-09-28')
  expect(resolveDateValue({ relative: 'start_of_week' }, tuesdayEvening)).toBe('2026-09-21')
  expect(resolveDateValue({ relative: 'end_of_week' }, tuesdayEvening)).toBe('2026-09-27')
  expect(resolveDateValue({ relative: 'start_of_week' }, new Date('2026-09-27T23:59:59.000Z'))).toBe('2026-09-21')
  expect(resolveDateValue({ relative: 'start_of_week' }, new Date('2026-09-28T00:00:00.000Z'))).toBe('2026-09-28')
  expect(resolveDateValue({ absolute: '2026-01-02' }, tuesdayEvening)).toBe('2026-01-02')
})

test('legacy preferences convert every field to a flat filter and display', () => {
  expect(legacyPreferencesToViewState({
    sort: 'created', statusFilter: 'unstarted:todo', assigneeFilter: 'user-1', unassignedFilter: false,
    labelFilter: 'label-1', priorityFilter: 'urgent', searchFilter: 'release',
  }, 'board')).toEqual({
    filter: { op: 'and', children: [
      { field: 'status', operator: 'is', value: ['unstarted:todo'] },
      { field: 'assignee', operator: 'is', value: ['user-1'] },
      { field: 'label', operator: 'includes_any', value: ['label-1'] },
      { field: 'priority', operator: 'is', value: ['urgent'] },
    ] },
    display: { ...DEFAULT_DISPLAY, layout: 'board', order_by: 'created', order_direction: 'desc' },
  })
})

test('unassigned wins over a stale assignee and every old sort keeps its direction', () => {
  expect(legacyPreferencesToViewState({ assigneeFilter: 'user-1', unassignedFilter: true }, null)?.filter.children).toEqual([
    { field: 'assignee', operator: 'is_empty' },
  ])
  const order = (sort: string) => {
    const display = legacyPreferencesToViewState({ sort }, null)!.display
    return [display.order_by, display.order_direction]
  }
  expect(['manual', 'priority', 'created', 'updated', 'title', 'bogus'].map(order)).toEqual([
    ['manual', 'asc'], ['priority', 'asc'], ['created', 'desc'], ['updated', 'desc'], ['title', 'asc'], ['manual', 'asc'],
  ])
  expect(legacySortDisplay('toString')).toEqual({ order_by: 'manual', order_direction: 'asc' })
})

test('a lone layout migrates, invalid values are dropped, and nothing stored means nothing to move', () => {
  expect(legacyPreferencesToViewState(null, 'timeline')).toEqual({ filter: emptyFilter(), display: { ...DEFAULT_DISPLAY, layout: 'timeline' } })
  expect(legacyPreferencesToViewState({
    sort: 'unknown', statusFilter: 42, unassignedFilter: 'true', priorityFilter: 'critical', labelFilter: '',
  }, 'gantt')).toEqual(defaultViewState())
  expect(legacyPreferencesToViewState(null, null)).toBeNull()
  expect(legacyPreferencesToViewState('broken', null)).toBeNull()
  expect(legacyPreferencesToViewState([], null)).toBeNull()
})
