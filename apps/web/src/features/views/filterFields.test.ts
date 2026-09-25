import { expect, test } from 'bun:test'
import { isCompleteCondition, operatorLabel, statusOptions, valueOptions, valueSummary, withOperator } from './filterFields'
import { FILTER_OPTIONS } from './testFixtures'
import type { Condition } from './viewState'

test('assignee values start with Me and list the other members once', () => {
  expect(valueOptions('assignee', FILTER_OPTIONS).map((option) => [option.value, option.label])).toEqual([
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
  const between = withOperator(before, 'between')
  expect(between).toEqual({ field: 'due_date', operator: 'between', value: [{ relative: 'today' }, { relative: 'today' }] })
  expect(withOperator(between, 'after')).toEqual({ field: 'due_date', operator: 'after', value: { relative: 'today' } })
  expect(withOperator({ field: 'assignee', operator: 'is', value: ['me'] }, 'is_empty')).toEqual({ field: 'assignee', operator: 'is_empty' })
  const reopened = withOperator({ field: 'assignee', operator: 'is_empty' }, 'is')
  expect(reopened).toEqual({ field: 'assignee', operator: 'is', value: [] })
  expect(isCompleteCondition(reopened)).toBe(false)
  expect(isCompleteCondition({ field: 'text', operator: 'contains', value: '  ' })).toBe(false)
  expect(isCompleteCondition({ field: 'assignee', operator: 'is_not_empty' })).toBe(true)
})
