import { expect, test } from 'bun:test'
import { render, type RenderResult } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { useState } from 'react'
import { FILTER_OPTIONS } from '../testFixtures'
import type { Condition } from '../viewState'
import { FilterValuePicker } from './FilterValuePicker'

function Harness({ initial, onDone = () => {} }: { initial: Condition; onDone?: () => void }) {
  const [condition, setCondition] = useState(initial)
  return (
    <>
      <FilterValuePicker condition={condition} options={FILTER_OPTIONS} onChange={setCondition} onDone={onDone} />
      <output data-testid="condition">{JSON.stringify(condition)}</output>
    </>
  )
}

const read = (view: RenderResult) => JSON.parse(view.getByTestId('condition').textContent ?? 'null') as Condition

test('list values toggle as a searchable multi-select', async () => {
  const view = render(<Harness initial={{ field: 'label', operator: 'includes_any', value: [] }} />)
  await userEvent.click(view.getByRole('option', { name: 'Bug' }))
  await userEvent.click(view.getByRole('option', { name: 'UI' }))
  expect(read(view).value).toEqual(['label-bug', 'label-ui'])
  expect(view.getByRole('option', { name: 'Bug' }).getAttribute('aria-checked')).toBe('true')
  await userEvent.click(view.getByRole('option', { name: 'Bug' }))
  expect(read(view).value).toEqual(['label-ui'])
  await userEvent.type(view.getByRole('combobox', { name: 'Search labels' }), 'doc')
  expect(view.queryAllByRole('option', { name: 'UI' }).length).toBe(0)
  expect(view.getByRole('option', { name: 'Docs' })).toBeTruthy()
})

test('date presets commit relative dates, and due date offers empty checks', async () => {
  let done = 0
  const view = render(<Harness initial={{ field: 'due_date', operator: 'before', value: { relative: 'today' } }} onDone={() => { done += 1 }} />)
  expect(view.getByRole('button', { name: 'Today' }).getAttribute('aria-pressed')).toBe('true')
  await userEvent.click(view.getByRole('button', { name: 'In 7 days' }))
  expect(read(view)).toEqual({ field: 'due_date', operator: 'before', value: { relative: 'today', offset_days: 7 } })
  expect(done).toBe(1)
  await userEvent.click(view.getByRole('button', { name: 'No due date' }))
  expect(read(view)).toEqual({ field: 'due_date', operator: 'is_empty' })
})

test('between offers range presets; created dates have no empty checks', async () => {
  const view = render(<Harness initial={{ field: 'due_date', operator: 'between', value: [{ relative: 'start_of_week' }, { relative: 'end_of_week' }] }} />)
  await userEvent.click(view.getByRole('button', { name: 'Next 7 days' }))
  expect(read(view).value).toEqual([{ relative: 'today' }, { relative: 'today', offset_days: 6 }])
  view.unmount()
  const created = render(<Harness initial={{ field: 'created_at', operator: 'after', value: { relative: 'today' } }} />)
  expect(created.queryAllByRole('button', { name: 'No due date' }).length).toBe(0)
})

test('text filters apply trimmed input on Enter', async () => {
  let done = 0
  const view = render(<Harness initial={{ field: 'text', operator: 'contains', value: '' }} onDone={() => { done += 1 }} />)
  expect((view.getByRole('button', { name: 'Apply' }) as HTMLButtonElement).disabled).toBe(true)
  await userEvent.type(view.getByRole('textbox', { name: 'Title or description contains' }), '  release notes {Enter}')
  expect(read(view)).toEqual({ field: 'text', operator: 'contains', value: 'release notes' })
  expect(done).toBe(1)
})

test('a selected member missing from the list, like your own ID from a migrated filter, shows checked and can be cleared', async () => {
  const view = render(<Harness initial={{ field: 'assignee', operator: 'is', value: ['user-1'] }} />)
  const own = view.getByRole('option', { name: 'Ada' })
  expect(own.getAttribute('aria-checked')).toBe('true')
  expect(view.getByRole('option', { name: 'Me' }).getAttribute('aria-checked')).toBe('false')
  await userEvent.click(own)
  expect(read(view).value).toEqual([])
})

test('an unknown selected value shows as a checked row that can be cleared', async () => {
  const view = render(<Harness initial={{ field: 'label', operator: 'includes_any', value: ['label-gone', 'label-bug'] }} />)
  const unknown = view.getByRole('option', { name: 'Unknown label' })
  expect(unknown.getAttribute('aria-checked')).toBe('true')
  await userEvent.click(unknown)
  expect(read(view).value).toEqual(['label-bug'])
  expect(view.queryAllByRole('option', { name: 'Unknown label' }).length).toBe(0)
})

test('a condition stops at 100 values: other rows are disabled until one is cleared', async () => {
  // 99 values the list lacks plus Bug: the server allows at most 100 per condition
  const gone = Array.from({ length: 99 }, (_, index) => `label-gone-${index}`)
  const view = render(<Harness initial={{ field: 'label', operator: 'includes_any', value: [...gone, 'label-bug'] }} />)
  const ui = view.getByRole('option', { name: 'UI' })
  expect(ui.getAttribute('aria-disabled')).toBe('true')
  expect(ui.getAttribute('title')).toBe('Up to 100 values per filter')
  await userEvent.click(ui)
  expect(read(view).value).toHaveLength(100)
  expect((read(view).value as string[]).includes('label-ui')).toBeFalse()

  // a selected row stays enabled, and clearing it frees a slot
  const bug = view.getByRole('option', { name: 'Bug' })
  expect(bug.getAttribute('aria-disabled')).toBe('false')
  await userEvent.click(bug)
  expect(view.getByRole('option', { name: 'UI' }).getAttribute('aria-disabled')).toBe('false')
  await userEvent.click(view.getByRole('option', { name: 'UI' }))
  expect((read(view).value as string[]).includes('label-ui')).toBeTrue()
})
