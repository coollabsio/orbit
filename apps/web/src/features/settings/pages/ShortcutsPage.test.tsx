import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import type { ReactNode } from 'react'
import { ConfirmationModalHost } from '@/components/common/ConfirmationModal'
import { render } from '@/test/render'
import { ShortcutsPage } from './ShortcutsPage'

const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
})

type Bindings = Record<string, string | null>

/** A server that holds one shortcut map; `puts` records each saved map. */
function api(initial: Bindings = {}, options: { failPut?: boolean } = {}) {
  let stored = initial
  const puts: Bindings[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    if (request.method === 'PUT') {
      const body = (await request.json()) as { bindings: Bindings }
      puts.push(body.bindings)
      if (options.failPut) return Response.json({ code: 'internal_error' }, { status: 500 })
      stored = body.bindings
    }
    return Response.json({ bindings: stored })
  }) as unknown as typeof fetch
  return puts
}

function Wrapper({ children }: { children: ReactNode }) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  return <QueryClientProvider client={client}>{children}<ConfirmationModalHost /></QueryClientProvider>
}

const mount = () => render(<ShortcutsPage />, { wrapper: Wrapper })
type View = ReturnType<typeof mount>
const row = (view: View, title: string) => view.getByRole('listitem', { name: title })
const keysOf = (view: View, title: string) => [...row(view, title).querySelectorAll('[data-slot=kbd]')].map((key) => key.textContent)
/** Starts the recorder of a command and types the keys; the recorder saves after a pause. */
async function record(view: View, title: string, keys: string) {
  await userEvent.click(within(row(view, title)).getByRole('button', { name: `Change shortcut for ${title}` }))
  await userEvent.keyboard(keys)
  await act(() => new Promise((resolve) => setTimeout(resolve, 1100)))
}

test('recording a free key saves it as an override', async () => {
  const puts = api()
  const view = mount()
  await record(view, 'Change status', 'q')
  await waitFor(() => expect(puts).toEqual([{ 'task.setStatus': 'Q' }]))
  expect(keysOf(view, 'Change status')).toEqual(['Q'])
})

test('a key with the platform modifier is saved as Mod', async () => {
  const puts = api()
  const view = mount()
  await record(view, 'Change status', '{Control>}e{/Control}')
  await waitFor(() => expect(puts).toEqual([{ 'task.setStatus': 'Mod+E' }]))
})

test('two keys in a row are saved as a sequence', async () => {
  const puts = api()
  const view = mount()
  await record(view, 'Change status', 'gz')
  await waitFor(() => expect(puts).toEqual([{ 'task.setStatus': 'G Z' }]))
})

test('a key of another command asks to swap, and Swap changes both', async () => {
  const puts = api()
  const view = mount()
  await record(view, 'Change status', 'p')
  expect(puts).toEqual([])
  expect(row(view, 'Change status').textContent).toContain('Used by "Change priority".')
  await userEvent.click(within(row(view, 'Change status')).getByRole('button', { name: 'Swap' }))
  await waitFor(() => expect(puts).toEqual([{ 'task.setStatus': 'P', 'task.setPriority': 'S' }]))
})

test('Cancel on the swap question keeps the keys', async () => {
  const puts = api()
  const view = mount()
  await record(view, 'Change status', 'p')
  await userEvent.click(within(row(view, 'Change status')).getByRole('button', { name: 'Cancel' }))
  expect(puts).toEqual([])
  expect(keysOf(view, 'Change status')).toEqual(['S'])
})

test('a key the browser keeps is refused', async () => {
  const puts = api()
  const view = mount()
  await record(view, 'Change status', '{Control>}w{/Control}')
  expect(row(view, 'Change status').textContent).toContain('The browser uses this shortcut.')
  expect(puts).toEqual([])
})

test('Backspace removes the shortcut', async () => {
  const puts = api()
  const view = mount()
  await userEvent.click(within(row(view, 'Change status')).getByRole('button', { name: 'Change shortcut for Change status' }))
  await userEvent.keyboard('{Backspace}')
  await waitFor(() => expect(puts).toEqual([{ 'task.setStatus': null }]))
  expect(row(view, 'Change status').textContent).toContain('No shortcut')
})

test('Escape leaves the recorder without a change', async () => {
  const puts = api()
  const view = mount()
  await userEvent.click(within(row(view, 'Change status')).getByRole('button', { name: 'Change shortcut for Change status' }))
  await userEvent.keyboard('{Escape}')
  expect(puts).toEqual([])
  expect(keysOf(view, 'Change status')).toEqual(['S'])
})

test('a changed shortcut can be reset to its default', async () => {
  const puts = api({ 'task.setStatus': 'Q', 'task.create': 'N' })
  const view = mount()
  await waitFor(() => expect(keysOf(view, 'Change status')).toEqual(['Q']))
  expect(within(row(view, 'Change priority')).queryByRole('button', { name: 'Reset Change priority' })).toBeNull()
  await userEvent.click(within(row(view, 'Change status')).getByRole('button', { name: 'Reset Change status' }))
  await waitFor(() => expect(puts).toEqual([{ 'task.create': 'N' }]))
  expect(keysOf(view, 'Change status')).toEqual(['S'])
})

test('Reset all asks first and then removes every override', async () => {
  const puts = api({ 'task.setStatus': 'Q', 'task.create': 'N' })
  const view = mount()
  await waitFor(() => expect(keysOf(view, 'Change status')).toEqual(['Q']))
  await userEvent.click(view.getByRole('button', { name: 'Reset all shortcuts' }))
  await userEvent.click(await view.findByRole('button', { name: 'Reset all' }))
  await waitFor(() => expect(puts).toEqual([{}]))
})

test('a fixed shortcut cannot be changed', () => {
  api()
  const view = mount()
  expect(within(row(view, 'Close the task')).queryByRole('button')).toBeNull()
  expect(keysOf(view, 'Close the task')).toEqual(['Esc'])
})

test('a failed save puts the old keys back', async () => {
  const puts = api({}, { failPut: true })
  const view = mount()
  await record(view, 'Change status', 'q')
  await waitFor(() => expect(puts).toHaveLength(1))
  await waitFor(() => expect(keysOf(view, 'Change status')).toEqual(['S']))
})

test('the search narrows the list', async () => {
  api()
  const view = mount()
  await userEvent.type(view.getByRole('searchbox', { name: 'Search shortcuts' }), 'priority')
  expect(view.queryByRole('listitem', { name: 'Change status' })).toBeNull()
  expect(view.getByRole('listitem', { name: 'Change priority' })).toBeTruthy()
})

test('a key that many shortcuts start with cannot be swapped', async () => {
  const puts = api()
  const view = mount()
  await record(view, 'Change status', 'g')
  expect(row(view, 'Change status').textContent).toContain('Used by "Go to inbox" and 10 more.')
  expect(within(row(view, 'Change status')).queryByRole('button', { name: 'Swap' })).toBeNull()
  expect(puts).toEqual([])
})

test('a key the app cannot bind is refused, not saved', async () => {
  const puts = api()
  const view = mount()
  await record(view, 'Change status', '[Numpad1]')
  expect(row(view, 'Change status').textContent).toContain('These keys cannot be used.')
  expect(puts).toEqual([])
})
