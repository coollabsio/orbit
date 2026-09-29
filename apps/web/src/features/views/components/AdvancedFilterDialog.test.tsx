import { expect, test } from 'bun:test'
import { act, fireEvent, render, within, type RenderResult } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { useState } from 'react'
import { appendCondition, type FilterIssue } from '../filterTree'
import { FILTER_OPTIONS } from '../testFixtures'
import type { Condition, FilterGroup } from '../viewState'
import { AdvancedFilterDialog } from './AdvancedFilterDialog'

const HIGH: Condition = { field: 'priority', operator: 'is', value: ['high'] }

function setup(filter: FilterGroup, validate?: (filter: FilterGroup) => Promise<FilterIssue | null>) {
  const applied: FilterGroup[] = []
  const view = render(
    <AdvancedFilterDialog open onOpenChange={() => {}} filter={filter} options={FILTER_OPTIONS} onApply={(next) => applied.push(next)} validate={validate} />,
  )
  return { view, applied }
}

test('toggles a group between and/or and applies the tree', async () => {
  const { view, applied } = setup({ op: 'and', children: [HIGH] })
  await userEvent.click(within(view.getByRole('group', { name: 'Root group operator' })).getByRole('button', { name: 'Or' }))
  await userEvent.click(view.getByRole('button', { name: 'Apply filter' }))
  expect(applied).toEqual([{ op: 'or', children: [HIGH] }])
})

test('nests groups up to three levels and no deeper', async () => {
  const { view } = setup({ op: 'and', children: [] })
  await userEvent.click(within(view.getByRole('group', { name: 'Root group' })).getByRole('button', { name: 'Add group' }))
  await userEvent.click(within(view.getByRole('group', { name: 'Group 1' })).getByRole('button', { name: 'Add group' }))
  const third = view.getByRole('group', { name: 'Group 1.2' })
  expect(within(third).queryAllByRole('button', { name: 'Add group' })).toHaveLength(0)
  expect(within(third).getByRole('button', { name: 'Add condition' })).toBeTruthy()
})

test('deletes conditions and groups', async () => {
  const { view, applied } = setup({ op: 'and', children: [HIGH, { op: 'or', children: [HIGH] }] })
  await userEvent.click(view.getByRole('button', { name: 'Delete group 2' }))
  await userEvent.click(view.getByRole('button', { name: 'Delete condition 1' }))
  await userEvent.click(view.getByRole('button', { name: 'Apply filter' }))
  expect(applied).toEqual([{ op: 'and', children: [] }])
})

test('a missing value shows next to its condition instead of applying', async () => {
  const { view, applied } = setup({ op: 'and', children: [HIGH] })
  await userEvent.click(within(view.getByRole('group', { name: 'Root group' })).getByRole('button', { name: 'Add condition' }))
  await userEvent.click(view.getByRole('button', { name: 'Apply filter' }))
  expect(applied).toEqual([])
  expect(within(view.getByRole('group', { name: 'Condition 2' })).getByRole('alert').textContent).toBe('Choose a value.')
})

test('a server 422 path lands next to the node it points at', async () => {
  const validate = async () => ({ path: 'filter.children[1].children[0].value', message: 'Unknown status key.' })
  const { view, applied } = setup({ op: 'and', children: [HIGH, { op: 'or', children: [{ field: 'status', operator: 'is', value: ['started:gone'] }] }] }, validate)
  await userEvent.click(view.getByRole('button', { name: 'Apply filter' }))
  const alert = await within(view.getByRole('group', { name: 'Condition 2.1' })).findByRole('alert')
  expect(alert.textContent).toBe('Unknown status key.')
  expect(applied).toEqual([])
})

test('an OR root at max depth that the chip bar wrapped still applies', async () => {
  const orRoot: FilterGroup = { op: 'or', children: [HIGH, { op: 'and', children: [HIGH, { op: 'or', children: [HIGH] }] }] }
  const wrapped = appendCondition(orRoot, HIGH)
  const checked: FilterGroup[] = []
  const { view, applied } = setup(wrapped, async (tree) => {
    checked.push(tree)
    return null
  })
  await userEvent.click(view.getByRole('button', { name: 'Apply filter' }))
  expect(view.queryAllByRole('alert')).toHaveLength(0)
  expect(checked).toEqual([wrapped])
  expect(applied).toEqual([wrapped])
})

test('a server path to a node that is not in the tree shows at the top instead of vanishing', async () => {
  const { view, applied } = setup({ op: 'and', children: [HIGH] }, async () => ({ path: 'filter.children[4]', message: 'Something is off here.' }))
  await userEvent.click(view.getByRole('button', { name: 'Apply filter' }))
  expect((await view.findByRole('alert')).textContent).toBe('Something is off here.')
  expect(applied).toEqual([])
})

test('opens without animation when a key press opened it', () => {
  const props = { onOpenChange: () => {}, filter: { op: 'and', children: [HIGH] } as FilterGroup, options: FILTER_OPTIONS, onApply: () => {} }
  const view = render(<AdvancedFilterDialog open={false} {...props} />)
  fireEvent.pointerDown(document.body)
  view.rerender(<AdvancedFilterDialog open {...props} />)
  const popup = () => document.querySelector('[data-slot="dialog-content"]')
  expect(popup()?.hasAttribute('data-instant')).toBe(false)
  view.rerender(<AdvancedFilterDialog open={false} {...props} />)
  fireEvent.keyDown(document.body, { key: 'Enter' })
  view.rerender(<AdvancedFilterDialog open {...props} />)
  expect(popup()?.hasAttribute('data-instant')).toBe(true)
})

function deferred<T>() {
  let resolve: (value: T) => void = () => {}
  const promise = new Promise<T>((done) => {
    resolve = done
  })
  return { promise, resolve }
}

function ClosableDialog({ filter, validate, onApply }: { filter: FilterGroup; validate: (filter: FilterGroup) => Promise<FilterIssue | null>; onApply: (filter: FilterGroup) => void }) {
  const [open, setOpen] = useState(true)
  return (
    <>
      <output data-testid="open">{String(open)}</output>
      <AdvancedFilterDialog open={open} onOpenChange={setOpen} filter={filter} options={FILTER_OPTIONS} onApply={onApply} validate={validate} />
    </>
  )
}

for (const [how, close] of [
  ['Cancel', (view: RenderResult) => userEvent.click(view.getByRole('button', { name: 'Cancel' }))],
  ['Escape', () => userEvent.keyboard('{Escape}')],
] as const) {
  test(`closing with ${how} while the server check runs never applies the filter`, async () => {
    const check = deferred<FilterIssue | null>()
    const applied: FilterGroup[] = []
    const errors: unknown[][] = []
    const original = console.error
    console.error = (...args: unknown[]) => errors.push(args)
    try {
      const view = render(<ClosableDialog filter={{ op: 'and', children: [HIGH] }} validate={() => check.promise} onApply={(next) => applied.push(next)} />)
      await userEvent.click(view.getByRole('button', { name: 'Apply filter' }))
      expect(view.getByRole('button', { name: 'Checking…' })).toBeTruthy()
      expect((view.getByRole('button', { name: 'Cancel' }) as HTMLButtonElement).disabled).toBe(false)
      await close(view)
      expect(view.getByTestId('open').textContent).toBe('false')
      await act(async () => {
        check.resolve(null)
        await check.promise
      })
      expect(applied).toEqual([])
      expect(errors).toEqual([])
    } finally {
      console.error = original
    }
  })
}

test('a server reply during the exit animation never applies, and reopening starts from the current filter', async () => {
  const check = deferred<FilterIssue | null>()
  const applied: FilterGroup[] = []
  const props = { onOpenChange: () => {}, filter: { op: 'and', children: [HIGH] } as FilterGroup, options: FILTER_OPTIONS, onApply: (next: FilterGroup) => applied.push(next), validate: () => check.promise }
  const view = render(<AdvancedFilterDialog open {...props} />)
  fireEvent.click(view.getByRole('button', { name: 'Delete condition 1' }))
  fireEvent.click(view.getByRole('button', { name: 'Apply filter' }))
  view.rerender(<AdvancedFilterDialog open={false} {...props} />)
  await act(async () => {
    check.resolve(null)
    await check.promise
  })
  expect(applied).toEqual([])
  view.rerender(<AdvancedFilterDialog open {...props} />)
  expect(view.getAllByRole('group', { name: /^Condition/, hidden: true }).length).toBe(1)
})
