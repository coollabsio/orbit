import { expect, test } from 'bun:test'
import { FIELD_META, availableFields, fieldMeta, isCompleteCondition, newCondition, operatorLabel, statusOptions, unlistedOptions, valueOptions, valueSummary, withOperator } from './filterFields'
import { FILTER_OPTIONS } from './testFixtures'
import type { Condition } from './viewState'

test('assignee values start with Me and list the other members once', () => {
  expect(valueOptions({ field: 'assignee' }, FILTER_OPTIONS).map((option) => [option.value, option.label])).toEqual([
    ['me', 'Me'],
    ['user-2', 'Grace'],
  ])
})

test('status values merge same-named statuses across projects', () => {
  expect(statusOptions(FILTER_OPTIONS.statuses).map((option) => option.value)).toEqual(['unstarted:todo', 'completed:done'])
})

test('operator labels read naturally for one or many values', () => {
  expect(operatorLabel('is', 1)).toBe('is')
  expect(operatorLabel('is', 2)).toBe('is any of')
  expect(operatorLabel('is_not', 3)).toBe('is none of')
  expect(operatorLabel('includes_any', 1)).toBe('includes')
  expect(operatorLabel('includes_any', 2)).toBe('includes any of')
})

test('value summaries name up to two values and count the rest', () => {
  expect(valueSummary({ field: 'label', operator: 'includes_any', value: ['label-bug', 'label-ui'] }, FILTER_OPTIONS)).toBe('Bug, UI')
  expect(valueSummary({ field: 'label', operator: 'includes_any', value: ['label-bug', 'label-ui', 'label-docs'] }, FILTER_OPTIONS)).toBe('3 labels')
  expect(valueSummary({ field: 'assignee', operator: 'is', value: ['me', 'user-2'] }, FILTER_OPTIONS)).toBe('Me, Grace')
  expect(valueSummary({ field: 'label', operator: 'includes_any', value: ['label-gone'] }, FILTER_OPTIONS)).toBe('Unknown label')
  expect(valueSummary({ field: 'due_date', operator: 'between', value: [{ relative: 'start_of_week' }, { relative: 'end_of_week' }] }, FILTER_OPTIONS)).toBe('Start of week – End of week')
  expect(valueSummary({ field: 'due_date', operator: 'before', value: { absolute: '2026-10-01' } }, FILTER_OPTIONS)).toBe('Oct 1, 2026')
  expect(valueSummary({ field: 'due_date', operator: 'after', value: { relative: 'today', offset_days: 7 } }, FILTER_OPTIONS)).toBe('In 7 days')
  expect(valueSummary({ field: 'text', operator: 'contains', value: 'login' }, FILTER_OPTIONS)).toBe('“login”')
})

test('switching operators keeps compatible values and marks missing ones incomplete', () => {
  const before: Condition = { field: 'due_date', operator: 'before', value: { relative: 'today' } }
  const between = withOperator(before, 'between', FILTER_OPTIONS)
  expect(between).toEqual({ field: 'due_date', operator: 'between', value: [{ relative: 'today' }, { relative: 'today' }] })
  expect(withOperator(between, 'after', FILTER_OPTIONS)).toEqual({ field: 'due_date', operator: 'after', value: { relative: 'today' } })
  expect(withOperator({ field: 'assignee', operator: 'is', value: ['me'] }, 'is_empty', FILTER_OPTIONS)).toEqual({ field: 'assignee', operator: 'is_empty' })
  const reopened = withOperator({ field: 'assignee', operator: 'is_empty' }, 'is', FILTER_OPTIONS)
  expect(reopened).toEqual({ field: 'assignee', operator: 'is', value: [] })
  expect(isCompleteCondition(reopened)).toBe(false)
  expect(isCompleteCondition({ field: 'text', operator: 'contains', value: '  ' })).toBe(false)
  expect(isCompleteCondition({ field: 'assignee', operator: 'is_not_empty' })).toBe(true)
})

test('parent chips read identifier and title, "No parent", "Deleted task", or a count', () => {
  const options = { ...FILTER_OPTIONS, taskRefs: { 'task-0012': { identifier: 'LAU-0012', title: 'Checkout redesign' }, 'task-gone': null } }
  expect(FIELD_META.parent.operators).toEqual(['is', 'is_not'])
  expect(valueSummary({ field: 'parent', operator: 'is', value: ['task-0012'] }, options)).toBe('LAU-0012 Checkout redesign')
  expect(valueSummary({ field: 'parent', operator: 'is', value: ['none'] }, options)).toBe('No parent')
  expect(valueSummary({ field: 'parent', operator: 'is_not', value: ['task-gone'] }, options)).toBe('Deleted task')
  expect(valueSummary({ field: 'parent', operator: 'is', value: ['a', 'b', 'c'] }, options)).toBe('3 tasks')
})

test('the sub-issues filter offers has / none with one operator', () => {
  expect(FIELD_META.sub_issues.operators).toEqual(['is'])
  expect(valueOptions({ field: 'sub_issues' }, FILTER_OPTIONS).map((option) => [option.value, option.label])).toEqual([['has', 'Has sub-issues'], ['none', 'No sub-issues']])
  expect(valueSummary({ field: 'sub_issues', operator: 'is', value: ['has'] }, FILTER_OPTIONS)).toBe('Has sub-issues')
})

const custom = (fieldId: string, operator: Condition['operator'], value?: unknown): Condition => ({ field: 'custom_field', field_id: fieldId, operator, ...(value !== undefined && { value }) })

test('each custom field kind offers the operators the server takes', () => {
  const operators = (fieldId: string) => fieldMeta({ field: 'custom_field', field_id: fieldId }, FILTER_OPTIONS).operators
  expect(operators('field-notes')).toEqual(['contains', 'is_empty', 'is_not_empty'])
  expect(operators('field-budget')).toEqual(['is', 'less_than', 'greater_than', 'between', 'is_empty', 'is_not_empty'])
  expect(operators('field-launch')).toEqual(['before', 'after', 'between', 'is_empty', 'is_not_empty'])
  expect(operators('field-customer')).toEqual(['is', 'is_not', 'is_empty', 'is_not_empty'])
  expect(operators('field-tags')).toEqual(['includes_any', 'includes_all', 'excludes', 'is_empty', 'is_not_empty'])
  expect(operators('field-signed')).toEqual(['is'])
  expect(fieldMeta({ field: 'custom_field', field_id: 'field-gone' }, FILTER_OPTIONS)).toMatchObject({ label: 'Unknown field', kind: 'missing' })
})

test('the field picker lists every gallery field after the built-in ones, and keeps a deleted one a condition uses', () => {
  const keys = availableFields(FILTER_OPTIONS).map((choice) => choice.key)
  expect(keys.slice(-TASK_FIELD_COUNT)).toEqual(FILTER_OPTIONS.taskFields.map((field) => `custom_field:${field.id}`))
  const kept = availableFields(FILTER_OPTIONS, { field: 'custom_field', field_id: 'field-gone' }).at(-1)
  expect(kept).toMatchObject({ key: 'custom_field:field-gone', meta: { label: 'Unknown field' } })
})

const TASK_FIELD_COUNT = FILTER_OPTIONS.taskFields.length

test('custom field chips read the option names, numbers, ranges and checkbox states', () => {
  expect(valueSummary(custom('field-customer', 'is', ['opt-acme']), FILTER_OPTIONS)).toBe('Acme')
  expect(valueSummary(custom('field-customer', 'is', ['opt-removed']), FILTER_OPTIONS)).toBe('Removed option')
  expect(unlistedOptions(custom('field-customer', 'is', ['opt-removed']), FILTER_OPTIONS).map((option) => option.label)).toEqual(['Removed option'])
  expect(valueOptions({ field: 'custom_field', field_id: 'field-customer' }, FILTER_OPTIONS).map((option) => option.label)).toEqual(['Acme', 'Globex'])
  expect(valueSummary(custom('field-budget', 'greater_than', 1500), FILTER_OPTIONS)).toBe((1500).toLocaleString())
  expect(valueSummary(custom('field-budget', 'between', [1, 5]), FILTER_OPTIONS)).toBe('1 – 5')
  expect(valueSummary(custom('field-signed', 'is', false), FILTER_OPTIONS)).toBe('Not checked')
  expect(valueSummary(custom('field-notes', 'contains', 'vip'), FILTER_OPTIONS)).toBe('“vip”')
  expect(valueSummary(custom('field-launch', 'after', { relative: 'today' }), FILTER_OPTIONS)).toBe('Today')
  expect(valueSummary(custom('field-gone', 'is', ['x']), FILTER_OPTIONS)).toBe('')
  expect(operatorLabel('less_than', 1)).toBe('is less than')
})

test('a new custom field condition keeps its field id and waits for a value where one is needed', () => {
  const budget = FILTER_OPTIONS.taskFields.find((field) => field.id === 'field-budget')!
  const fresh = newCondition({ field: 'custom_field', field_id: budget.id }, fieldMeta({ field: 'custom_field', field_id: budget.id }, FILTER_OPTIONS))
  expect(fresh).toEqual({ field: 'custom_field', field_id: 'field-budget', operator: 'is', value: undefined })
  expect(isCompleteCondition(fresh)).toBe(false)
  expect(isCompleteCondition({ ...fresh, value: 3 })).toBe(true)
  expect(isCompleteCondition(custom('field-signed', 'is'))).toBe(false)
  expect(isCompleteCondition(custom('field-signed', 'is', false))).toBe(true)
  expect(isCompleteCondition(custom('field-budget', 'between', [1, 'x']))).toBe(false)
  expect(isCompleteCondition({ field: 'custom_field', operator: 'is_empty' })).toBe(false)
})

test('switching a number operator keeps the number, as a range of it for between', () => {
  const is = custom('field-budget', 'is', 4)
  const between = withOperator(is, 'between', FILTER_OPTIONS)
  expect(between).toEqual({ field: 'custom_field', field_id: 'field-budget', operator: 'between', value: [4, 4] })
  expect(withOperator(between, 'less_than', FILTER_OPTIONS)).toEqual({ field: 'custom_field', field_id: 'field-budget', operator: 'less_than', value: 4 })
  expect(withOperator(is, 'is_empty', FILTER_OPTIONS)).toEqual({ field: 'custom_field', field_id: 'field-budget', operator: 'is_empty' })
})
