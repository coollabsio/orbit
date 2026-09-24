import { expect, test } from 'bun:test'
import { SORT_OPTIONS } from '@/features/tasks/tasksLib'
import { legacyFilterValues, withLegacyFilter, withLegacySort } from './legacyFilterAdapter'
import { DEFAULT_DISPLAY, emptyFilter, type FilterGroup } from './viewState'

test('the old menu reads single-value root conditions and the order', () => {
  const filter: FilterGroup = { op: 'and', children: [
    { field: 'status', operator: 'is', value: ['unstarted:todo'] },
    { field: 'assignee', operator: 'is_empty' },
    { field: 'label', operator: 'includes_any', value: ['label-1', 'label-2'] },
    { field: 'priority', operator: 'is', value: ['high'] },
  ] }
  expect(legacyFilterValues(filter, { ...DEFAULT_DISPLAY, order_by: 'title' })).toEqual({
    statusKey: 'unstarted:todo', assigneeId: null, unassigned: true, labelId: null, priority: 'high', sort: 'title',
  })
  expect(legacyFilterValues({ op: 'or', children: filter.children }, { ...DEFAULT_DISPLAY, order_by: 'due_date' })).toEqual({
    statusKey: null, assigneeId: null, unassigned: false, labelId: null, priority: null, sort: 'manual',
  })
})

test('assignee menu clicks compose into one assignee condition', () => {
  let filter = withLegacyFilter(emptyFilter(), { field: 'assignee', value: 'user-1' })
  // "Unassigned": onAssigneeChange(null) then onUnassignedChange(true)
  filter = withLegacyFilter(filter, { field: 'assignee', value: null })
  filter = withLegacyFilter(filter, { field: 'unassigned', value: true })
  expect(filter.children).toEqual([{ field: 'assignee', operator: 'is_empty' }])
  // a user: onUnassignedChange(false) then onAssigneeChange(id)
  filter = withLegacyFilter(filter, { field: 'unassigned', value: false })
  filter = withLegacyFilter(filter, { field: 'assignee', value: 'user-2' })
  expect(filter.children).toEqual([{ field: 'assignee', operator: 'is', value: ['user-2'] }])
})

test('menu changes keep other conditions and nested groups; an OR root is wrapped', () => {
  const nested: FilterGroup = { op: 'or', children: [{ field: 'text', operator: 'contains', value: 'crash' }] }
  const filter: FilterGroup = { op: 'and', children: [{ field: 'priority', operator: 'is', value: ['low'] }, nested] }
  expect(withLegacyFilter(filter, { field: 'priority', value: 'urgent' })).toEqual({
    op: 'and', children: [nested, { field: 'priority', operator: 'is', value: ['urgent'] }],
  })
  expect(withLegacyFilter(filter, { field: 'status', value: null })).toEqual(filter)
  expect(withLegacyFilter(nested, { field: 'label', value: 'label-1' })).toEqual({
    op: 'and', children: [nested, { field: 'label', operator: 'includes_any', value: ['label-1'] }],
  })
})

test('every advertised sort maps to server ordering with its old direction', () => {
  expect(SORT_OPTIONS.map(({ key }) => withLegacySort(key))).toEqual([
    { order_by: 'manual', order_direction: 'asc' },
    { order_by: 'priority', order_direction: 'asc' },
    { order_by: 'created', order_direction: 'desc' },
    { order_by: 'updated', order_direction: 'desc' },
    { order_by: 'title', order_direction: 'asc' },
  ])
})
