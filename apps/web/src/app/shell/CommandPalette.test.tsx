import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, useLocation } from 'react-router'
import { queryKeys } from '@/api/queryKeys'
import { WorkspaceProvider } from '@/features/workspaces/WorkspaceProvider'
import { CommandPalette } from './CommandPalette'

const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
})

function Location() {
  const location = useLocation()
  return <output data-testid="location">{location.pathname}</output>
}

let pageSearches: string[] = []

function setup(onClose = () => {}) {
  pageSearches = []
  window.localStorage.clear()
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } })
  client.setQueryData(queryKeys.workspaces, [
    { id: 'workspace-1', name: 'Alpha', role: 'owner', version: 1 },
  ])
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const url = String(input instanceof Request ? input.url : input)
    if (url.includes('/projects')) return Response.json({ items: [], next_cursor: null })
    if (url.includes('/pages/search')) {
      pageSearches.push(new URL(url).searchParams.get('q') ?? '')
      return Response.json({
        items: [
          { id: 'page-handbook', parent_id: null, teamspace_id: 'teamspace-1', private: false, title: 'Team handbook', icon: null, snippet: 'How we onboard new people' },
          { id: 'page-diary', parent_id: null, teamspace_id: null, private: true, title: 'Onboarding diary', icon: null, snippet: 'My onboard notes' },
        ],
      })
    }
    if (url.includes('/teamspaces')) {
      return Response.json({ items: [{ id: 'teamspace-1', workspace_id: 'workspace-1', name: 'General', icon: null, position: 0, version: 1, is_default: true, created_at: '2026-09-05T10:00:00Z', updated_at: '2026-09-05T10:00:00Z' }] })
    }
    if (url.includes('/tasks')) {
      return Response.json({
        items: [{
          id: 'task-ship-it',
          workspace_id: 'workspace-1',
          project_id: 'project-1',
          status_id: 'todo',
          title: 'Ship the invite flow',
          description: '',
          position: 0,
          priority: 'none',
          assignee_ids: [],
          creator_id: 'user-1',
          label_ids: [],
          created_at: '2026-09-05T10:00:00Z',
          updated_at: '2026-09-05T10:00:00Z',
          version: 1,
        }],
        next_cursor: null,
      })
    }
    return new Response('not found', { status: 404 })
  }) as unknown as typeof fetch
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={['/tasks?workspace=workspace-1']}>
        <WorkspaceProvider>
          <CommandPalette onClose={onClose} />
          <Location />
        </WorkspaceProvider>
      </MemoryRouter>
    </QueryClientProvider>,
  )
}

test('command palette never advertises disabled products or mock store titles', async () => {
  const view = setup()
  await waitFor(() => expect(view.getByPlaceholderText('Search tasks, pages and navigation…')).toBeTruthy())

  const labels = view.getAllByRole('option').map((option) => option.textContent ?? '')
  for (const forbidden of ['Go to Mail', 'Go to Chat', 'Go to Direct Messages', 'Infrastructure', 'Proxy issue', '#general']) {
    expect(labels.some((label) => label.includes(forbidden))).toBe(false)
  }
})

test('searching a live task opens it in the active workspace', async () => {
  const closed: string[] = []
  const view = setup(() => closed.push('closed'))
  const input = await view.findByPlaceholderText('Search tasks, pages and navigation…')
  await userEvent.type(input, 'invite')
  const task = await view.findByRole('option', { name: /Ship the invite flow/ })
  expect(view.queryByRole('option', { name: /Go to Home/ })).toBeNull()
  fireEvent.click(task)
  expect(closed).toEqual(['closed'])
  expect(view.getByTestId('location').textContent).toBe('/tasks/task-ship-it')
})

test('empty search shows enabled navigation and Escape closes the palette', async () => {
  let closed = 0
  const view = setup(() => { closed += 1 })
  const input = await view.findByPlaceholderText('Search tasks, pages and navigation…')
  expect(view.getByRole('option', { name: /Go to Tasks/ })).toBeTruthy()
  expect(view.getByRole('option', { name: /Go to Settings/ })).toBeTruthy()
  fireEvent.keyDown(input, { key: 'Escape' })
  expect(closed).toBe(1)
})

test('unknown search terms show an empty state', async () => {
  const view = setup()
  const input = await view.findByPlaceholderText('Search tasks, pages and navigation…')
  await userEvent.type(input, 'zzzz-no-such-item')
  expect(await view.findByText(/No results for/)).toBeTruthy()
  expect(view.getByText(/zzzz-no-such-item/)).toBeTruthy()
})

test('Enter opens the highlighted navigation result', async () => {
  let closed = 0
  const view = setup(() => { closed += 1 })
  const input = await view.findByPlaceholderText('Search tasks, pages and navigation…')
  await userEvent.type(input, 'settings')
  expect(await view.findByRole('option', { name: /Go to Settings/ })).toBeTruthy()
  expect(view.queryByRole('option', { name: /Go to Home/ })).toBeNull()
  fireEvent.keyDown(input, { key: 'Enter' })
  expect(closed).toBe(1)
  expect(view.getByTestId('location').textContent).toBe('/settings')
})

test('docs are reachable from the palette', async () => {
  let closed = 0
  const view = setup(() => { closed += 1 })
  await view.findByPlaceholderText('Search tasks, pages and navigation…')
  fireEvent.click(view.getByRole('option', { name: /Go to Docs/ }))
  expect(closed).toBe(1)
  expect(view.getByTestId('location').textContent).toBe('/docs')
})

test('page search runs only for a non-empty query and opens the page', async () => {
  const view = setup()
  const input = await view.findByPlaceholderText('Search tasks, pages and navigation…')
  await new Promise((resolve) => setTimeout(resolve, 300))
  expect(pageSearches).toEqual([])

  await userEvent.type(input, 'onboard')
  const page = await view.findByRole('option', { name: /Team handbook/ }, { timeout: 2000 })
  // Debounced: one request for the settled query, not one per keystroke.
  expect(pageSearches).toEqual(['onboard'])
  // The secondary text names the page's space.
  await waitFor(() => expect(page.textContent).toContain('Page · General'))
  expect(view.getByRole('option', { name: /Onboarding diary/ }).textContent).toContain('Page · Private')
  fireEvent.click(page)
  expect(view.getByTestId('location').textContent).toBe('/docs/page-handbook')
})
