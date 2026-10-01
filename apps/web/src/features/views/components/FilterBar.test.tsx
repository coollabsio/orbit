import { expect, test } from 'bun:test'
import { fireEvent, type RenderResult } from '@testing-library/react'
import { render } from '@/test/render'
import userEvent from '@testing-library/user-event'
import { useState } from 'react'
import { FILTER_OPTIONS } from '../testFixtures'
import type { FilterGroup } from '../viewState'
import { FilterBar, FilterButton } from './FilterBar'

function Harness({ initial, presetLabel = null, onOpenAdvanced }: { initial: FilterGroup; presetLabel?: string | null; onOpenAdvanced?: () => void }) {
  const [filter, setFilter] = useState(initial)
  return (
    <>
      <input aria-label="Other field" />
      <FilterButton filter={filter} options={FILTER_OPTIONS} onChange={setFilter} onOpenAdvanced={onOpenAdvanced} />
      <FilterBar filter={filter} options={FILTER_OPTIONS} onChange={setFilter} presetLabel={presetLabel} onOpenAdvanced={onOpenAdvanced} />
      <output data-testid="filter">{JSON.stringify(filter)}</output>
    </>
  )
}

const EMPTY: FilterGroup = { op: 'and', children: [] }
const readFilter = (view: RenderResult) => JSON.parse(view.getByTestId('filter').textContent ?? 'null') as FilterGroup

test('F opens the filter menu without animation and adds a label chip', async () => {
  const view = render(<Harness initial={EMPTY} />)
  const trigger = view.getByRole('button', { name: 'Filter tasks' })
  // focused first, as when typing: React's input polyfill trips over key events on a never-focused input
  const other = view.getByRole('textbox', { name: 'Other field' })
  other.focus()
  fireEvent.keyDown(other, { key: 'f', code: 'KeyF' })
  expect(trigger.getAttribute('aria-expanded')).toBe('false')
  other.blur()
  fireEvent.keyDown(document.body, { key: 'f', code: 'KeyF' })
  expect(trigger.getAttribute('aria-expanded')).toBe('true')
  expect(document.querySelector('[data-slot="popover-content"]')?.hasAttribute('data-instant')).toBe(true)
  await userEvent.click(await view.findByRole('option', { name: 'Label' }))
  await userEvent.click(await view.findByRole('option', { name: 'Bug' }))
  expect(readFilter(view)).toEqual({ op: 'and', children: [{ field: 'label', operator: 'includes_any', value: ['label-bug'] }] })
  const chip = view.container.querySelector('[data-filter-chip="label"]')
  expect(chip?.textContent).toContain('Label')
  expect(chip?.textContent).toContain('includes')
  expect(chip?.textContent).toContain('Bug')
  expect(chip?.classList.contains('animate-filter-chip-enter')).toBe(true)
})

test('chips switch operators, re-edit values, and remove instantly', async () => {
  const view = render(<Harness initial={{ op: 'and', children: [{ field: 'label', operator: 'includes_any', value: ['label-bug'] }] }} />)
  const chip = () => view.container.querySelector('[data-filter-chip="label"]')
  expect(chip()?.classList.contains('animate-filter-chip-enter')).toBe(false)

  fireEvent.click(view.getByRole('button', { name: /^Label operator/ }))
  await userEvent.click(await view.findByRole('menuitem', { name: 'excludes' }))
  expect(readFilter(view).children[0]).toEqual({ field: 'label', operator: 'excludes', value: ['label-bug'] })

  fireEvent.click(view.getByRole('button', { name: /^Label values/ }))
  await userEvent.click(await view.findByRole('option', { name: 'UI' }))
  expect(readFilter(view).children[0]).toEqual({ field: 'label', operator: 'excludes', value: ['label-bug', 'label-ui'] })
  expect(chip()?.textContent).toContain('Bug, UI')

  await userEvent.keyboard('{Escape}')
  fireEvent.click(view.getByRole('button', { name: 'Remove label filter' }))
  expect(readFilter(view).children).toEqual([])
  expect(view.container.querySelectorAll('[data-filter-chip]').length).toBe(0)
})

test('switching from an empty check to a value operator asks for values first', async () => {
  const view = render(<Harness initial={{ op: 'and', children: [{ field: 'assignee', operator: 'is_empty' }] }} />)
  fireEvent.click(view.getByRole('button', { name: /^Assignee operator/ }))
  await userEvent.click(await view.findByRole('menuitem', { name: 'is' }))
  expect(readFilter(view).children[0]).toEqual({ field: 'assignee', operator: 'is_empty' })
  await userEvent.click(await view.findByRole('option', { name: 'Me' }))
  expect(readFilter(view).children[0]).toEqual({ field: 'assignee', operator: 'is', value: ['me'] })
})

test('the preset chip is fixed and nested trees collapse into one advanced chip', async () => {
  let opened = 0
  const nested: FilterGroup = {
    op: 'and',
    children: [{ op: 'or', children: [{ field: 'priority', operator: 'is', value: ['high'] }, { field: 'label', operator: 'includes_any', value: ['label-bug'] }] }],
  }
  const view = render(<Harness presetLabel="Overdue" initial={nested} onOpenAdvanced={() => { opened += 1 }} />)
  expect(view.getByRole('toolbar', { name: 'Filters' }).textContent).toContain('Overdue')
  expect(view.queryAllByRole('button', { name: /^Remove/ }).length).toBe(0)
  await userEvent.click(view.getByRole('button', { name: 'Advanced filter · 2 conditions' }))
  expect(opened).toBe(1)
})

test('the filter button stops adding conditions at the limit, so a scoped page never exceeds the server limit', () => {
  const term = (index: number) => ({ field: 'text' as const, operator: 'contains' as const, value: `term ${index}` })
  const full: FilterGroup = { op: 'and', children: Array.from({ length: 46 }, (_, index) => term(index)) }
  const view = render(<Harness initial={full} />)
  const trigger = view.getByRole('button', { name: 'Filter tasks' }) as HTMLButtonElement
  expect(trigger.disabled).toBe(true)
  fireEvent.keyDown(document.body, { key: 'f', code: 'KeyF' })
  expect(trigger.getAttribute('aria-expanded')).not.toBe('true')
  expect(readFilter(view).children).toHaveLength(46)
})

test('adding to an advanced tree ANDs the new condition at the root', async () => {
  const nested: FilterGroup = { op: 'or', children: [{ field: 'priority', operator: 'is', value: ['high'] }, { field: 'priority', operator: 'is', value: ['urgent'] }] }
  const view = render(<Harness initial={nested} />)
  fireEvent.click(view.getByRole('button', { name: 'Filter tasks' }))
  await userEvent.click(await view.findByRole('option', { name: 'Text' }))
  await userEvent.type(await view.findByRole('textbox', { name: 'Title or description contains' }), 'login{Enter}')
  expect(readFilter(view)).toEqual({ op: 'and', children: [nested, { field: 'text', operator: 'contains', value: 'login' }] })
})

test('the chip bar stops adding conditions at the limit too', () => {
  const term = (index: number) => ({ field: 'text' as const, operator: 'contains' as const, value: `term ${index}` })
  const full: FilterGroup = { op: 'and', children: Array.from({ length: 46 }, (_, index) => term(index)) }
  const view = render(<FilterBar filter={full} options={FILTER_OPTIONS} onChange={() => {}} />)
  const add = view.getByRole('button', { name: 'Add filter' }) as HTMLButtonElement
  expect(add.disabled).toBe(true)
  expect(add.title).toBe('Filters can have at most 46 conditions')
  fireEvent.click(add)
  expect(add.getAttribute('aria-expanded')).not.toBe('true')
  view.rerender(<FilterBar filter={{ op: 'and', children: full.children.slice(1) }} options={FILTER_OPTIONS} onChange={() => {}} />)
  expect((view.getByRole('button', { name: 'Add filter' }) as HTMLButtonElement).disabled).toBe(false)
})

test('an OR root with a single child takes the new condition without another level', async () => {
  const single: FilterGroup = { op: 'or', children: [{ op: 'and', children: [{ field: 'priority', operator: 'is', value: ['high'] }, { field: 'label', operator: 'includes_any', value: ['label-bug'] }] }] }
  const view = render(<Harness initial={single} />)
  fireEvent.click(view.getByRole('button', { name: 'Filter tasks' }))
  await userEvent.click(await view.findByRole('option', { name: 'Text' }))
  await userEvent.type(await view.findByRole('textbox', { name: 'Title or description contains' }), 'login{Enter}')
  expect(readFilter(view)).toEqual({ op: 'and', children: [...single.children, { field: 'text', operator: 'contains', value: 'login' }] })
})

test('unticking a just-added value restores an OR root exactly', async () => {
  const nested: FilterGroup = { op: 'or', children: [{ field: 'priority', operator: 'is', value: ['high'] }, { field: 'priority', operator: 'is', value: ['urgent'] }] }
  const view = render(<Harness initial={nested} />)
  fireEvent.click(view.getByRole('button', { name: 'Filter tasks' }))
  await userEvent.click(await view.findByRole('option', { name: 'Label' }))
  await userEvent.click(await view.findByRole('option', { name: 'Bug' }))
  expect(readFilter(view)).toEqual({ op: 'and', children: [nested, { field: 'label', operator: 'includes_any', value: ['label-bug'] }] })
  await userEvent.click(await view.findByRole('option', { name: 'Bug' }))
  expect(readFilter(view)).toEqual(nested)
})

test('closing keeps the value picker through the exit, and the next open starts at the field list', async () => {
  const view = render(<Harness initial={EMPTY} />)
  const trigger = view.getByRole('button', { name: 'Filter tasks' })
  fireEvent.click(trigger)
  await userEvent.click(await view.findByRole('option', { name: 'Text' }))
  fireEvent.keyDown(await view.findByRole('textbox', { name: 'Title or description contains' }), { key: 'Escape' })
  expect(trigger.getAttribute('aria-expanded')).toBe('false')
  // the closing popup must not swap to the "Filter by…" list (its autofocused input) during the exit animation
  expect(view.queryAllByRole('combobox', { name: 'Filter by', hidden: true }).length).toBe(0)
  fireEvent.click(trigger)
  expect(await view.findByRole('combobox', { name: 'Filter by' })).toBeTruthy()
})
