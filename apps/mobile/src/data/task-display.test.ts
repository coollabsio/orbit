import { expect, test } from 'bun:test'
import type { TaskField } from '@orbit/api/generated/types.gen'

import { displayFields, orderKey, orderOptions, orderPatch, shownCustomFields, taskFieldChips, toggleCustomField, urlHost } from './task-display'
import { DEFAULT_DISPLAY, taskQueryBody } from './tasks-filter'

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

const CUSTOMER = field('f-customer', 'Customer', 'multi_select', [
  { id: 'o-a', name: 'Acme', color: '#111111' },
  { id: 'o-b', name: 'Beta', color: '#222222' },
  { id: 'o-c', name: 'Corp', color: '#333333' },
])
const BUDGET = field('f-budget', 'Budget', 'number')
const SIGNED = field('f-signed', 'Signed off', 'checkbox')
const SITE = field('f-site', 'Site', 'url')
const DUE = field('f-due', 'Launch', 'date')
const GALLERY = [BUDGET, CUSTOMER, DUE, SIGNED, SITE]

const task = (values: Record<string, unknown>) => ({ field_values: Object.entries(values).map(([field_id, value]) => ({ field_id, value })) })

test('a row shows the chosen fields in the display order, only those with a value', () => {
  const shown = shownCustomFields(GALLERY, ['f-signed', 'f-gone', 'f-budget', 'f-customer'])
  expect(shown.map((item) => item.name)).toEqual(['Signed off', 'Budget', 'Customer'])
  const chips = taskFieldChips(task({ 'f-budget': 1200, 'f-signed': false, 'f-customer': ['o-c', 'o-a', 'o-b'] }), shown)
  expect(chips).toEqual([
    { fieldId: 'f-budget', label: `Budget: ${(1200).toLocaleString()}`, kind: 'text', icon: 'hashtag', text: (1200).toLocaleString() },
    { fieldId: 'f-customer', label: 'Customer: Acme, Beta, Corp', kind: 'options', options: CUSTOMER.options.slice(0, 2), more: 1 },
  ])
  expect(taskFieldChips(task({}), shown)).toEqual([])
})

test('a checked box shows the field name, a URL its host, a date a short local day', () => {
  const chips = taskFieldChips(task({ 'f-signed': true, 'f-site': 'https://www.example.com/a?b', 'f-due': '2026-03-04' }), [SIGNED, SITE, DUE])
  expect(chips.map((chip) => (chip.kind === 'text' ? [chip.icon, chip.text] : null))).toEqual([
    ['checkSquare', 'Signed off'],
    ['link', 'example.com'],
    ['calendar', new Date(2026, 2, 4).toLocaleDateString(undefined, { month: 'short', day: 'numeric' })],
  ])
  expect(urlHost('not a url')).toBe('not a url')
})

test('a project page offers its fields, plus gallery fields the display still uses', () => {
  expect(displayFields(GALLERY, null, DEFAULT_DISPLAY)).toEqual(GALLERY)
  expect(displayFields(GALLERY, [CUSTOMER], { custom_fields: ['f-signed'], order_field_id: 'f-budget' }).map((item) => item.name)).toEqual(['Customer', 'Budget', 'Signed off'])
})

test('turning fields on appends them; off removes them; deleted ones drop out', () => {
  const display = { ...DEFAULT_DISPLAY, custom_fields: ['f-gone', 'f-budget'] }
  expect(toggleCustomField(display, 'f-customer', GALLERY)).toEqual({ custom_fields: ['f-budget', 'f-customer'] })
  expect(toggleCustomField(display, 'f-budget', GALLERY)).toEqual({ custom_fields: [] })
})

test('orderings: built-ins, then each field; choosing a field sends it to the server', () => {
  expect(orderOptions([BUDGET], 'manual').map((item) => item.key)).toEqual(['manual', 'priority', 'created', 'updated', 'title', 'due_date', 'estimate', 'field:f-budget'])
  expect(orderOptions([], 'field:f-gone').at(-1)).toEqual({ key: 'field:f-gone', label: 'Unknown field' })

  const byBudget = { ...DEFAULT_DISPLAY, ...orderPatch('field:f-budget'), order_direction: 'desc' as const }
  expect(orderKey(byBudget)).toBe('field:f-budget')
  expect(taskQueryBody({ op: 'and', children: [] }, byBudget)).toMatchObject({ order_by: 'custom_field', order_field_id: 'f-budget', order_direction: 'desc' })

  const byTitle = { ...byBudget, ...orderPatch('title') }
  expect(orderKey(byTitle)).toBe('title')
  expect(taskQueryBody({ op: 'and', children: [] }, byTitle)).not.toHaveProperty('order_field_id')
  expect(orderKey({ order_by: 'custom_field', order_field_id: null })).toBe('manual')
})
