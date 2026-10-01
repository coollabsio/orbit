import { expect, test } from 'bun:test'
import { act } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { render } from '@testing-library/react'
import type { Overrides } from './bindings'
import { SequenceHint } from './SequenceHint'
import { ShortcutProvider } from './ShortcutProvider'
import { useCommand } from './useCommand'

function Commands() {
  useCommand('nav.inbox', () => {})
  useCommand('nav.docs', () => {})
  return null
}

const mount = (overrides?: Overrides) => render(<ShortcutProvider overrides={overrides}><Commands /><SequenceHint /></ShortcutProvider>)
const wait = (ms: number) => act(() => new Promise((resolve) => setTimeout(resolve, ms)))

test('the hint shows the next keys only after a pause, and goes when the sequence ends', async () => {
  const view = mount()
  await userEvent.keyboard('g')
  await wait(300)
  expect(view.queryByRole('status')).toBeNull()
  await wait(150)
  expect(view.getByRole('status').textContent).toContain('Go to inbox')
  expect(view.getByRole('status').textContent).toContain('Go to docs')
  await userEvent.keyboard('i')
  expect(view.queryByRole('status')).toBeNull()
})

test('the hint goes away when no second key comes', async () => {
  const view = mount()
  await userEvent.keyboard('g')
  await wait(500)
  expect(view.queryByRole('status')).not.toBeNull()
  await wait(600)
  expect(view.queryByRole('status')).toBeNull()
})

test('a fast sequence never shows the hint', async () => {
  const view = mount()
  await userEvent.keyboard('gi')
  await wait(450)
  expect(view.queryByRole('status')).toBeNull()
})

test('the hint follows a rebound sequence', async () => {
  const view = mount({ 'nav.inbox': 'H I' })
  await userEvent.keyboard('g')
  await wait(450)
  expect(view.getByRole('status').textContent).not.toContain('Go to inbox')
  expect(view.getByRole('status').textContent).toContain('Go to docs')
})
