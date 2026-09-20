import { expect, test } from 'bun:test'
import { act, fireEvent, render, waitFor } from '@testing-library/react'
import { ConfirmationModalHost } from './ConfirmationModal'
import { confirmAction } from './confirmAction'

test('confirmation shows a dialog, focuses Cancel, and resolves only after a choice', async () => {
  const view = render(<ConfirmationModalHost />)
  let result!: Promise<boolean>
  act(() => {
    result = confirmAction({ title: 'Move task to trash?', description: 'You can restore it later.', confirmLabel: 'Move to trash', danger: true })
  })
  const dialog = view.getByRole('dialog', { name: 'Move task to trash?' })
  expect(dialog.getAttribute('aria-describedby')).toBeTruthy()
  expect(view.getByRole('button', { name: 'Move to trash' }).className).toContain('destructive')
  await waitFor(() => expect(dialog.contains(document.activeElement)).toBe(true))
  fireEvent.click(view.getByRole('button', { name: 'Move to trash' }))
  expect(await result).toBe(true)
})

for (const dismissal of ['Cancel', 'Close', 'Escape']) {
  test(`${dismissal} cancels confirmation and restores focus`, async () => {
    const view = render(
      <>
        <button>Trigger</button>
        <ConfirmationModalHost />
      </>,
    )
    const trigger = view.getByRole('button', { name: 'Trigger' })
    trigger.focus()
    let result!: Promise<boolean>
    act(() => {
      result = confirmAction({ title: 'Continue?' })
    })
    await view.findByRole('dialog', { name: 'Continue?' })
    if (dismissal === 'Escape') fireEvent.keyDown(document.activeElement ?? document.body, { key: 'Escape' })
    else fireEvent.click(view.getByRole('button', { name: dismissal }))
    expect(await result).toBe(false)
    await waitFor(() => expect(document.activeElement).toBe(trigger))
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
  await view.findByRole('dialog', { name: 'Second?' })
  view.unmount()
  expect(await second).toBe(false)
  expect(await confirmAction({ title: 'No host' })).toBe(false)
})

test('custom labels and neutral styling work in Strict Mode', async () => {
  const { StrictMode } = await import('react')
  const view = render(
    <StrictMode>
      <ConfirmationModalHost />
    </StrictMode>,
  )
  let result!: Promise<boolean>
  act(() => {
    result = confirmAction({ title: 'Refresh task?', confirmLabel: 'Refresh', cancelLabel: 'Keep editing' })
  })
  expect(view.getByRole('button', { name: 'Refresh' }).className).toContain('bg-primary')
  fireEvent.click(view.getByRole('button', { name: 'Keep editing' }))
  expect(await result).toBe(false)
})
