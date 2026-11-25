import { expect, test } from 'bun:test'
import type { TaskField } from '@orbit/api/generated/types.gen'

import {
  conditionMeta,
  DEFAULT_DISPLAY,
  filterTargets,
  isCompleteCondition,
  parseTargetKey,
  targetKey,
  taskQueryBody,
  valueSummary,
  withOperator,
  type Condition,
  type FilterOptions,
} from './tasks-filter'

const field = (id: string, name: string, kind: TaskField['kind'], options: TaskField['options'] = []): TaskField => ({
  id,
  name,
  kind,
  options,
  can_manage: true,
  created_at: '',
  created_by: null,
  project_count: 1,
  updated_at: '',
  version: 1,
  workspace_id: 'w',
})

const SIZE = field('f-size', 'Size', 'select', [
  { id: 'o-s', name: 'Small', color: '#111111' },
  { id: 'o-l', name: 'Large', color: '#222222' },
])
const POINTS = field('f-points', 'Points', 'number')
const DONE = field('f-done', 'Reviewed', 'checkbox')
const TIME = field('f-time', 'Time tracking', 'time_tracking')

const options: FilterOptions = {
  statuses: [],
  members: [],
  labels: [],
  projectId: 'p1',
  projects: [],
  milestones: [],
  cycles: [],
  customFields: [SIZE, POINTS, TIME, DONE],
  currentUserId: 'me',
}

test('a custom sort sends its field; a sort without one falls back to manual', () => {
  expect(taskQueryBody({ op: 'and', children: [] }, { ...DEFAULT_DISPLAY, order_by: 'custom_field', order_field_id: 'f-points' })).toMatchObject({ order_by: 'custom_field', order_field_id: 'f-points' })
  const missing = taskQueryBody({ op: 'and', children: [] }, { ...DEFAULT_DISPLAY, order_by: 'custom_field' })
  expect(missing.order_by).toBe('manual')
  expect('order_field_id' in missing).toBe(false)
  expect('order_field_id' in taskQueryBody({ op: 'and', children: [] }, { ...DEFAULT_DISPLAY, order_field_id: 'f-points' })).toBe(false)
})

test('the filter list offers the page custom fields after the built-in ones, without time tracking', () => {
  const keys = filterTargets(options).map(targetKey)
  expect(keys.slice(-3)).toEqual(['custom:f-size', 'custom:f-points', 'custom:f-done'])
  expect(parseTargetKey('custom:f-size')).toEqual({ field: 'custom_field', field_id: 'f-size' })
  expect(parseTargetKey('status')).toEqual({ field: 'status' })
})

test('a custom select chip names its options and keeps its field through operator changes', () => {
  const condition: Condition = { field: 'custom_field', field_id: 'f-size', operator: 'is', value: ['o-l'] }
  expect(conditionMeta(condition, options).label).toBe('Size')
  expect(valueSummary(condition, options, '#000')).toBe('Large')
  expect(withOperator(condition, 'is_empty', conditionMeta(condition, options))).toEqual({ field: 'custom_field', field_id: 'f-size', operator: 'is_empty' })
  expect(withOperator({ field: 'status', operator: 'is', value: ['x'] }, 'is_not', conditionMeta({ field: 'status' }, options))).toEqual({ field: 'status', operator: 'is_not', value: ['x'] })
})

test('custom chips from the web read as text, and are complete per operator', () => {
  expect(valueSummary({ field: 'custom_field', field_id: 'f-points', operator: 'between', value: [1, 5] }, options, '#000')).toBe('1 – 5')
  expect(valueSummary({ field: 'custom_field', field_id: 'f-done', operator: 'is', value: true }, options, '#000')).toBe('Checked')
  expect(conditionMeta({ field: 'custom_field', field_id: 'gone' }, options).label).toBe('Field')
  expect(isCompleteCondition({ field: 'custom_field', field_id: 'f-points', operator: 'less_than', value: 3 })).toBe(true)
  expect(isCompleteCondition({ field: 'custom_field', field_id: 'f-done', operator: 'is', value: false })).toBe(true)
  expect(isCompleteCondition({ field: 'custom_field', field_id: 'f-size', operator: 'is', value: [] })).toBe(false)
  expect(isCompleteCondition({ field: 'custom_field', operator: 'is_empty' })).toBe(false)
})
