import { expect, test } from 'bun:test'
import userEvent from '@testing-library/user-event'
import { useEffect, useState } from 'react'
import { MemoryRouter } from 'react-router'
import { render } from '@/test/render'
import { ShortcutHelpDialog } from './ShortcutHelpDialog'
import { useCommand } from './useCommand'

function Page() {
  useCommand('view.filter', () => {})
  // as in the app: the page is there before the dialog opens
  const [open, setOpen] = useState(false)
  useEffect(() => setOpen(true), [])
  return open ? <MemoryRouter><ShortcutHelpDialog onClose={() => {}} /></MemoryRouter> : null
}

test('the search keeps the shortcuts that match the name or the key', async () => {
  const view = render(<Page />)
  expect(view.getByText('Go to inbox')).toBeTruthy()
  await userEvent.type(view.getByRole('searchbox', { name: 'Search shortcuts' }), 'status')
  expect(view.getByText('Change status')).toBeTruthy()
  expect(view.queryByText('Go to inbox')).toBeNull()
  await userEvent.clear(view.getByRole('searchbox', { name: 'Search shortcuts' }))
  await userEvent.type(view.getByRole('searchbox', { name: 'Search shortcuts' }), 'zzz')
  expect(view.getByText('No shortcut matches "zzz".')).toBeTruthy()
})

test('the commands of the page in view come first', () => {
  const view = render(<Page />)
  const section = view.getByRole('group', { name: 'On this page' })
  expect(section.textContent).toContain('Filter')
  expect(section.textContent).not.toContain('Change status')
})

test('a command without a key is not listed', () => {
  const view = render(<MemoryRouter><ShortcutHelpDialog onClose={() => {}} /></MemoryRouter>)
  expect(view.queryByText('Create page')).toBeNull()
})
