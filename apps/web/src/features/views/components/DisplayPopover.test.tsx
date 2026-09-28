import { expect, test } from 'bun:test'
import { fireEvent, render, type RenderResult } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { useState } from 'react'
import { DEFAULT_DISPLAY, type DisplayOptions } from '../viewState'
import { DisplayPopover } from './DisplayPopover'

function Harness({ initial = DEFAULT_DISPLAY }: { initial?: DisplayOptions }) {
  const [display, setDisplay] = useState(initial)
  return (
    <>
      <input aria-label="Other field" />
      <DisplayPopover display={display} defaultDisplay={DEFAULT_DISPLAY} onChange={(patch) => setDisplay((current) => ({ ...current, ...patch }))} />
      <output data-testid="display">{JSON.stringify(display)}</output>
    </>
  )
}

const read = (view: RenderResult) => JSON.parse(view.getByTestId('display').textContent ?? 'null') as DisplayOptions
const open = (view: RenderResult) => fireEvent.click(view.getByRole('button', { name: 'Display options' }))

test('Shift+V opens the popover without animation, except while typing', async () => {
  const view = render(<Harness />)
  const trigger = view.getByRole('button', { name: 'Display options' })
  // focused first, as when typing: React's input polyfill trips over key events on a never-focused input
  const other = view.getByRole('textbox', { name: 'Other field' })
  other.focus()
  fireEvent.keyDown(other, { key: 'V', shiftKey: true })
  expect(trigger.getAttribute('aria-expanded')).toBe('false')
  fireEvent.keyDown(document.body, { key: 'V', shiftKey: true })
  expect(trigger.getAttribute('aria-expanded')).toBe('true')
  expect(document.querySelector('[data-slot="popover-content"]')?.hasAttribute('data-instant')).toBe(true)
  expect(await view.findByRole('combobox', { name: 'Grouping' })).toBeTruthy()
})

test('switching an ungrouped list to board groups it by status', async () => {
  const view = render(<Harness initial={{ ...DEFAULT_DISPLAY, group_by: 'none' }} />)
  open(view)
  await userEvent.click(await view.findByRole('button', { name: 'Board' }))
  expect(read(view).layout).toBe('board')
  expect(read(view).group_by).toBe('status')
})

test('sub-grouping skips the current grouping and hides on timeline', async () => {
  const view = render(<Harness />)
  open(view)
  const trigger = view.getByRole('button', { name: 'Display options' })
  fireEvent.click(await view.findByRole('combobox', { name: 'Sub-grouping' }))
  const options = await view.findAllByRole('option')
  expect(options.map((option) => option.textContent)).toEqual(['Assignee', 'Priority', 'Project', 'Label', 'No sub-grouping'])
  await userEvent.click(view.getByRole('option', { name: 'Priority' }))
  expect(read(view).sub_group_by).toBe('priority')
  expect(trigger.getAttribute('aria-expanded')).toBe('true')
  await userEvent.click(view.getByRole('button', { name: 'Timeline' }))
  expect(view.queryByRole('combobox', { name: 'Sub-grouping' })).toBeNull()
})

test('option selects are wide enough for "No sub-grouping"', async () => {
  const view = render(<Harness />)
  open(view)
  // at w-36 (144px) the longest label clipped to "No sub-groupir"; the 320px panel has room for w-40
  for (const name of ['Grouping', 'Sub-grouping', 'Ordering', 'Completed tasks']) {
    expect((await view.findByRole('combobox', { name })).classList.contains('w-40')).toBe(true)
  }
})

test('direction is disabled for manual ordering and toggles otherwise', async () => {
  const view = render(<Harness />)
  open(view)
  expect((view.getByRole('button', { name: 'Ascending' }) as HTMLButtonElement).disabled).toBe(true)
  fireEvent.click(view.getByRole('combobox', { name: 'Ordering' }))
  await userEvent.click(await view.findByRole('option', { name: 'Priority' }))
  expect(read(view).order_by).toBe('priority')
  await userEvent.click(view.getByRole('button', { name: 'Ascending' }))
  expect(read(view).order_direction).toBe('desc')
  expect(view.getByRole('button', { name: 'Descending' })).toBeTruthy()
})

test('property chips, empty groups, and reset to default', async () => {
  const view = render(<Harness />)
  open(view)
  expect((view.getByRole('button', { name: 'Reset to default' }) as HTMLButtonElement).disabled).toBe(true)
  await userEvent.click(view.getByRole('button', { name: 'Labels' }))
  expect(read(view).properties).not.toContain('labels')
  await userEvent.click(view.getByRole('button', { name: 'Created' }))
  expect(read(view).properties).toContain('created')
  await userEvent.click(view.getByRole('switch', { name: 'Show empty groups' }))
  expect(read(view).show_empty_groups).toBe(true)
  await userEvent.click(view.getByRole('button', { name: 'Reset to default' }))
  expect(read(view)).toEqual(DEFAULT_DISPLAY)
  expect((view.getByRole('button', { name: 'Reset to default' }) as HTMLButtonElement).disabled).toBe(true)
})

test('ordering hides on timeline, where rows always follow dates', async () => {
  const view = render(<Harness initial={{ ...DEFAULT_DISPLAY, layout: 'timeline' }} />)
  open(view)
  await view.findByRole('combobox', { name: 'Grouping' })
  expect(view.queryAllByRole('combobox', { name: 'Ordering' })).toHaveLength(0)
  expect(view.queryAllByRole('button', { name: 'Ascending' })).toHaveLength(0)
  await userEvent.click(view.getByRole('button', { name: 'List' }))
  expect(view.getAllByRole('combobox', { name: 'Ordering' })).toHaveLength(1)
})

test('sub-issues: nested by default, flat or hidden, plus a progress property', async () => {
  const view = render(<Harness />)
  open(view)
  const select = await view.findByRole('combobox', { name: 'Sub-issues' })
  expect(select.textContent).toContain('Nested')
  fireEvent.click(select)
  const options = await view.findAllByRole('option')
  expect(options.map((option) => option.textContent)).toEqual(['Nested', 'Flat', 'Hidden'])
  await userEvent.click(view.getByRole('option', { name: 'Hidden' }))
  expect(read(view).sub_issues).toBe('hidden')
  await userEvent.click(view.getByRole('button', { name: 'Sub-issue progress' }))
  expect(read(view).properties).not.toContain('sub_issue_progress')
})

test('on board and timeline, Nested sub-issues say they show flat there', async () => {
  const view = render(<Harness />)
  open(view)
  const select = await view.findByRole('combobox', { name: 'Sub-issues' })
  expect(view.queryAllByText(/shows sub-issues flat/)).toHaveLength(0)
  await userEvent.click(view.getByRole('button', { name: 'Board' }))
  const hint = view.getByText('Board shows sub-issues flat')
  expect(select.getAttribute('aria-describedby')).toBe(hint.id)
  await userEvent.click(view.getByRole('button', { name: 'Timeline' }))
  expect(view.getByText('Timeline shows sub-issues flat')).toBeTruthy()
  expect(read(view).sub_issues).toBe('nested')
})
