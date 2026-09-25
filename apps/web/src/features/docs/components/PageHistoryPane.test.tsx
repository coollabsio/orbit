import { afterEach, beforeAll, expect, mock, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, fireEvent, render, waitFor } from '@testing-library/react'
import type { PageVersionSummary } from '@/api/generated/types.gen'
import { registerConfirmationHandler } from '@/components/common/confirmAction'
import { dayLabel, fullDate, timeOfDay } from '@/lib/format'
import { PageHistoryPane } from './PageHistoryPane'

beforeAll(() => {
  // BlockNote warns about mobile keyboards without this viewport flag (index.html sets it in the app).
  const meta = document.createElement('meta')
  meta.name = 'viewport'
  meta.content = 'width=device-width, initial-scale=1, interactive-widget=resizes-content'
  document.head.append(meta)
})

const originalFetch = globalThis.fetch
let unregister: (() => void) | undefined
afterEach(() => {
  globalThis.fetch = originalFetch
  unregister?.()
})

const HOUR = 60 * 60 * 1000
const recent = new Date(Date.now() - HOUR).toISOString()
const older = '2026-03-02T09:15:00Z'

const version = (id: string, created_at: string, patch: Partial<PageVersionSummary> = {}): PageVersionSummary => ({
  id, page_id: 'page-1', kind: 'auto', title: `Title ${id}`, icon: null, created_at,
  created_by: { id: 'user-1', display_name: 'Ada Lovelace' }, ...patch,
})

const paragraph = (text: string) => [{ id: `block-${text}`, type: 'paragraph', props: {}, content: [{ type: 'text', text, styles: {} }], children: [] }]

type Call = { method: string; path: string; search: URLSearchParams }

function setup(versions: PageVersionSummary[], options: { nextCursor?: string; more?: PageVersionSummary[] } = {}) {
  const calls: Call[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    const url = new URL(request.url)
    const call = { method: request.method, path: url.pathname.replace('/api/v1/workspaces/workspace-1/pages/page-1', ''), search: url.searchParams }
    calls.push(call)
    if (call.path === '/versions') {
      if (call.search.get('cursor')) return Response.json({ items: options.more ?? [], next_cursor: null })
      return Response.json({ items: versions, next_cursor: options.nextCursor ?? null })
    }
    const id = call.path.split('/').at(-1)!
    const found = [...versions, ...(options.more ?? [])].find((item) => item.id === id)
    return found ? Response.json({ ...found, content: paragraph(`Body of ${id}`) }) : Response.json({ code: 'page_version_not_found' }, { status: 404 })
  }) as unknown as typeof fetch
  const onRestore = mock(async () => true)
  const onClose = mock(() => {})
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const view = render(
    <QueryClientProvider client={client}>
      <PageHistoryPane
        workspaceId="workspace-1"
        pageId="page-1"
        readCurrent={() => ({ title: 'Live title', icon: null, content: paragraph('Live body') })}
        resolvePage={() => null}
        onOpenPage={() => {}}
        onRestore={onRestore}
        onClose={onClose}
      />
    </QueryClientProvider>,
  )
  return { view, calls, onRestore, onClose }
}

const selectedOption = (view: ReturnType<typeof render>) =>
  view.getAllByRole('option').find((option) => option.getAttribute('aria-selected') === 'true')

test('without history the pane explains when versions appear', async () => {
  const { view } = setup([])
  expect(await view.findByText('Versions appear after you edit this page.')).toBeTruthy()
  expect(view.queryByRole('listbox')).toBeNull()
  expect(view.queryByRole('button', { name: 'Restore this version' })).toBeNull()
})

test('versions are grouped by day under the current version, and the newest one is previewed read-only', async () => {
  const { view, calls } = setup([version('a', recent), version('b', older, { kind: 'restore', created_by: null })])
  const list = await view.findByRole('listbox', { name: 'Versions' })
  expect(view.getByRole('group', { name: 'Today' }).textContent).toContain(timeOfDay(recent))
  expect(view.getByRole('group', { name: fullDate(older) }).textContent).toContain('Before a restore')
  expect(list.textContent).toContain('Former member')
  expect(view.getAllByRole('option')[0].textContent).toContain('Current version')
  expect(selectedOption(view)?.getAttribute('data-version-id')).toBe('a')

  const preview = view.getByRole('region', { name: 'Version preview' })
  await waitFor(() => expect(preview.textContent).toContain('Body of a'), { timeout: 4000 })
  expect(preview.textContent).toContain('Title a')
  expect(preview.querySelector('[contenteditable="true"]')).toBeNull()
  expect(view.getByTestId('history-preview-label').textContent).toBe(`${dayLabel(recent)}, ${timeOfDay(recent)}`)
  expect(calls.filter((call) => call.path === '/versions/a')).toHaveLength(1)
})

test('arrow keys move through the list, "Current version" shows the live page, and Escape closes', async () => {
  const { view, onClose } = setup([version('a', recent), version('b', older)])
  await view.findByRole('listbox', { name: 'Versions' })
  await waitFor(() => expect(document.activeElement?.getAttribute('data-version-id')).toBe('a'))

  fireEvent.keyDown(document.activeElement!, { key: 'ArrowDown' })
  expect(selectedOption(view)?.getAttribute('data-version-id')).toBe('b')
  expect(document.activeElement?.getAttribute('data-version-id')).toBe('b')
  const preview = view.getByRole('region', { name: 'Version preview' })
  await waitFor(() => expect(preview.textContent).toContain('Body of b'), { timeout: 4000 })

  fireEvent.keyDown(document.activeElement!, { key: 'Home' })
  expect(selectedOption(view)?.textContent).toContain('Current version')
  await waitFor(() => expect(preview.textContent).toContain('Live body'), { timeout: 4000 })
  expect(preview.textContent).toContain('Live title')
  expect(view.getByRole('button', { name: 'Restore this version' }).hasAttribute('disabled')).toBeTrue()

  fireEvent.keyDown(document.activeElement!, { key: 'Escape' })
  expect(onClose).toHaveBeenCalledTimes(1)
})

test('"Restore this version" asks first and restores only when confirmed', async () => {
  const answers = [false, true]
  const asked: string[] = []
  unregister = registerConfirmationHandler(async (options) => {
    asked.push(options.title)
    return answers.shift() ?? false
  })
  const { view, onRestore } = setup([version('a', recent), version('b', older)])
  await view.findByRole('listbox', { name: 'Versions' })
  fireEvent.click(view.getAllByRole('option')[2])
  const restore = view.getByRole('button', { name: 'Restore this version' })
  await waitFor(() => expect(restore.hasAttribute('disabled')).toBeFalse())

  await act(async () => {
    fireEvent.click(restore)
  })
  expect(asked).toEqual(['Restore this version?'])
  expect(onRestore).not.toHaveBeenCalled()

  await act(async () => {
    fireEvent.click(restore)
  })
  await waitFor(() => expect(onRestore).toHaveBeenCalledTimes(1))
  expect((onRestore.mock.calls[0] as unknown as [PageVersionSummary])[0].id).toBe('b')
})

test('older versions load on demand with the cursor', async () => {
  const { view, calls } = setup([version('a', recent)], { nextCursor: 'a', more: [version('z', older)] })
  await view.findByRole('listbox', { name: 'Versions' })
  // fireEvent: userEvent's pointer moves reach BlockNote's side menu, which needs layout happy-dom lacks.
  fireEvent.click(view.getByRole('button', { name: 'Load older versions' }))
  await waitFor(() => expect(view.getAllByRole('option')).toHaveLength(3))
  expect(calls.find((call) => call.search.get('cursor'))?.search.get('cursor')).toBe('a')
})
