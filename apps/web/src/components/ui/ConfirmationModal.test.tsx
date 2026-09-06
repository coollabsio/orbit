import { expect, test } from 'bun:test'
import { act, fireEvent, render, waitFor } from '@testing-library/react'
import { ConfirmationModalHost } from './ConfirmationModal'
import { confirmAction } from './confirmAction'

test('confirmation uses the shared modal, focuses Cancel, and resolves only after a choice', async () => {
  const view = render(<ConfirmationModalHost />)
  let result!: Promise<boolean>
  act(() => { result = confirmAction({ title: 'Move task to trash?', description: 'You can restore it later.', confirmLabel: 'Move to trash', danger: true }) })
  const dialog = view.getByRole('dialog', { name: 'Move task to trash?' })
  expect(dialog.getAttribute('aria-describedby')).toBeTruthy()
  expect(view.getByRole('button', { name: 'Move to trash' }).classList.contains('button-danger')).toBe(true)
  await waitFor(() => expect(document.activeElement).toBe(view.getByRole('button', { name: 'Cancel' })))
  fireEvent.click(view.getByRole('button', { name: 'Move to trash' }))
  expect(await result).toBe(true)
  expect(view.queryByRole('dialog')).toBeNull()
})

for (const dismissal of ['Cancel', 'Close', 'Escape', 'backdrop']) {
  test(`${dismissal} cancels confirmation and restores focus`, async () => {
    const view = render(<><button>Trigger</button><ConfirmationModalHost /></>)
    const trigger = view.getByRole('button', { name: 'Trigger' })
    trigger.focus()
    let result!: Promise<boolean>
    act(() => { result = confirmAction({ title: 'Continue?' }) })
    if (dismissal === 'Escape') fireEvent.keyDown(document, { key: 'Escape' })
    else if (dismissal === 'backdrop') fireEvent.click(view.getByRole('dialog').parentElement!)
    else fireEvent.click(view.getByRole('button', { name: dismissal }))
    expect(await result).toBe(false)
    expect(document.activeElement).toBe(trigger)
  })
}

test('concurrent requests are shown in order and unmount cancels pending requests', async () => {
  const view = render(<ConfirmationModalHost />)
  let first!: Promise<boolean>
  let second!: Promise<boolean>
  act(() => {
    first = confirmAction({ title: 'First?' })
    second = confirmAction({ title: 'Second?' })
  })
  expect(view.getByRole('dialog', { name: 'First?' })).toBeTruthy()
  fireEvent.click(view.getByRole('button', { name: 'Confirm' }))
  expect(await first).toBe(true)
  expect(view.getByRole('dialog', { name: 'Second?' })).toBeTruthy()
  view.unmount()
  expect(await second).toBe(false)
  expect(await confirmAction({ title: 'No host' })).toBe(false)
})

test('custom labels and neutral styling work in Strict Mode', async () => {
  const { StrictMode } = await import('react')
  const view = render(<StrictMode><ConfirmationModalHost /></StrictMode>)
  let result!: Promise<boolean>
  act(() => { result = confirmAction({ title: 'Refresh task?', confirmLabel: 'Refresh', cancelLabel: 'Keep editing' }) })
  expect(view.getByRole('button', { name: 'Refresh' }).classList.contains('button-primary')).toBe(true)
  fireEvent.click(view.getByRole('button', { name: 'Keep editing' }))
  expect(await result).toBe(false)
})
