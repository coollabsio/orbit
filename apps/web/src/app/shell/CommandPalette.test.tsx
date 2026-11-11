import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, waitFor } from '@testing-library/react'
import { render } from '@/test/render'
import userEvent from '@testing-library/user-event'
import { useEffect, useState, type ReactNode } from 'react'
import { MemoryRouter, useLocation } from 'react-router'
import { useCommand } from '@/shortcuts/useCommand'
import { queryKeys } from '@/api/queryKeys'
import { chatKeys } from '@/features/chat/api/keys'
import { chatEnabled } from './productNavigation'
import { WorkspaceProvider } from '@/features/workspaces/WorkspaceProvider'
import { CommandPalette } from './CommandPalette'

const PLACEHOLDER = /^Search tasks/
const SHIP_IT = {
  id: 'task-ship-it', number: 12, workspace_id: 'workspace-1', project_id: 'project-1', status_id: 'todo', title: 'Ship the invite flow',
  description: '', position: 0, priority: 'none', assignee_ids: [], creator_id: 'user-1', label_ids: [],
  created_at: '2026-09-05T10:00:00Z', updated_at: '2026-09-05T10:00:00Z', version: 1,
}
const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
})

function Location() {
  const location = useLocation()
  return <output data-testid="location">{location.pathname}</output>
}

let pageSearches: string[] = []
let taskSearches: string[] = []
let recentPages: unknown[] = []

const ran: string[] = []
/** Commands of the page in view, mounted before the palette opens as in the app. */
function PageCommands({ target, children }: { target: boolean; children: ReactNode }) {
  useCommand('task.create', () => void ran.push('create'))
  useCommand('task.setStatus', () => void ran.push('status'), { available: () => target })
  useCommand('nav.inbox', () => {})
  const [ready, setReady] = useState(false)
  useEffect(() => setReady(true), [])
  return ready ? children : null
}

function setup(onClose = () => {}, target = false, path = '/tasks') {
  pageSearches = []
  taskSearches = []
  window.localStorage.clear()
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } })
  client.setQueryData(chatKeys.conversations('workspace-1'), [
    { id: 'c-general', kind: 'public', name: 'general', topic: 'Company news', memberIds: [], isMember: true, archived: false },
  ])
  client.setQueryData(queryKeys.workspaces, [
    { id: 'workspace-1', name: 'Alpha', role: 'owner', permissions: [], version: 1 },
  ])
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const url = String(input instanceof Request ? input.url : input)
    if (url.includes('/statuses')) {
      return Response.json({ items: [{ id: 'todo', project_id: 'project-1', name: 'Todo', description: '', color: '#888', category: 'unstarted', position: 0, version: 1 }], next_cursor: null })
    }
    if (url.includes('/projects')) return Response.json({ items: [{ id: 'project-1', key: 'ENG', name: 'Engineering', color: '#888', version: 1 }], next_cursor: null })
    if (url.includes('/pages/recent')) return recentPages.length ? Response.json({ items: recentPages }) : new Response('not found', { status: 404 })
    if (url.includes('/pages/search')) {
      pageSearches.push(new URL(url).searchParams.get('q') ?? '')
      return Response.json({
        items: [
          {
            id: 'page-handbook', parent_id: null, teamspace_id: 'teamspace-1', private: false, title: 'Team handbook', icon: null,
            snippet: 'How we onboard new people', snippet_highlights: [{ start: 7, end: 14 }], title_highlights: [], database_title: null,
          },
          {
            id: 'page-diary', parent_id: null, teamspace_id: null, private: true, title: 'Onboarding diary', icon: null,
            snippet: 'My <b>onboard</b> notes', snippet_highlights: [{ start: 6, end: 13 }], title_highlights: [{ start: 0, end: 10 }], database_title: null,
          },
          {
            id: 'row-paper', parent_id: 'db-research', teamspace_id: 'teamspace-1', private: false, title: 'Onboard paper', icon: null,
            snippet: 'Status: Onboarding', snippet_highlights: [{ start: 8, end: 18 }], title_highlights: [{ start: 0, end: 7 }], database_title: 'Research',
          },
        ],
      })
    }
    if (url.includes('/teamspaces')) {
      return Response.json({ items: [{ id: 'teamspace-1', workspace_id: 'workspace-1', name: 'General', icon: null, position: 0, version: 1, is_default: true, created_at: '2026-09-05T10:00:00Z', updated_at: '2026-09-05T10:00:00Z' }] })
    }
    // a task by its identifier: the palette looks up the task a typed ID names
    if (/\/tasks\/ENG-12$/.test(new URL(url).pathname)) return Response.json(SHIP_IT)
    if (url.includes('/tasks')) {
      taskSearches.push(new URL(url).searchParams.get('search') ?? '')
      return Response.json({
        items: [{
          id: 'task-ship-it',
          number: 12,
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
      <MemoryRouter initialEntries={[`${path}?workspace=workspace-1`]}>
        <WorkspaceProvider>
          <PageCommands target={target}><CommandPalette onClose={onClose} /></PageCommands>
          <Location />
        </WorkspaceProvider>
      </MemoryRouter>
    </QueryClientProvider>,
  )
}

test('command palette never advertises disabled products or mock store titles', async () => {
  const view = setup()
  await waitFor(() => expect(view.getByPlaceholderText(PLACEHOLDER)).toBeTruthy())

  const labels = view.getAllByRole('option').map((option) => option.textContent ?? '')
  for (const forbidden of ['Go to Mail', 'Go to Direct Messages', 'Infrastructure', 'Proxy issue', '#general']) {
    expect(labels.some((label) => label.includes(forbidden))).toBe(false)
  }
})

test('searching a live task opens it in the active workspace', async () => {
  const closed: string[] = []
  const view = setup(() => closed.push('closed'))
  const input = await view.findByPlaceholderText(PLACEHOLDER)
  await userEvent.type(input, 'invite')
  const task = await view.findByRole('option', { name: /Ship the invite flow/ })
  expect(view.queryByRole('option', { name: /Go to Home/ })).toBeNull()
  fireEvent.click(task)
  expect(closed).toEqual(['closed'])
  expect(view.getByTestId('location').textContent).toBe('/tasks/ENG-12')
})

test('empty search shows enabled navigation and Escape closes the palette', async () => {
  let closed = 0
  const view = setup(() => { closed += 1 })
  const input = await view.findByPlaceholderText(PLACEHOLDER)
  expect(view.getByRole('option', { name: /Go to Tasks/ })).toBeTruthy()
  expect(view.getByRole('option', { name: /Go to Settings/ })).toBeTruthy()
  fireEvent.keyDown(input, { key: 'Escape' })
  expect(closed).toBe(1)
})

test('unknown search terms show an empty state', async () => {
  const view = setup()
  const input = await view.findByPlaceholderText(PLACEHOLDER)
  await userEvent.type(input, 'zzzz-no-such-item')
  expect(await view.findByText(/No results for/)).toBeTruthy()
  expect(view.getByText(/zzzz-no-such-item/)).toBeTruthy()
})

test('Enter opens the highlighted navigation result', async () => {
  let closed = 0
  const view = setup(() => { closed += 1 })
  const input = await view.findByPlaceholderText(PLACEHOLDER)
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
  await view.findByPlaceholderText(PLACEHOLDER)
  fireEvent.click(view.getByRole('option', { name: /Go to Docs/ }))
  expect(closed).toBe(1)
  expect(view.getByTestId('location').textContent).toBe('/docs')
})

test('page search runs only for a non-empty query and opens the page', async () => {
  const view = setup()
  const input = await view.findByPlaceholderText(PLACEHOLDER)
  await new Promise((resolve) => setTimeout(resolve, 300))
  expect(pageSearches).toEqual([])

  await userEvent.type(input, 'onboard')
  const page = await view.findByRole('option', { name: /Team handbook/ }, { timeout: 2000 })
  // Debounced: one request for the settled query, not one per keystroke.
  expect(pageSearches).toEqual(['onboard'])
  // The secondary text names the page's space.
  await waitFor(() => expect(page.textContent).toContain('General'))
  expect(view.getByRole('option', { name: /Onboarding diary/ }).textContent).toContain('Private')
  // A row names its database, and a row found by a property value shows it.
  const row = view.getByRole('option', { name: /Onboard paper/ })
  expect(row.textContent).toContain('Row · Research')
  expect(row.querySelector('[data-snippet]')!.textContent).toBe('Status: Onboarding')
  fireEvent.click(page)
  expect(view.getByTestId('location').textContent).toBe('/docs/page-handbook')
})

test('typing another key keeps the current hits until the new search answers', async () => {
  const view = setup()
  const input = await view.findByPlaceholderText(PLACEHOLDER)
  await userEvent.type(input, 'onboard')
  await view.findByRole('option', { name: /Team handbook/ }, { timeout: 2000 })
  await view.findByRole('option', { name: /Ship the invite flow/ })

  await userEvent.type(input, 'i')
  // the debounced searches have not run yet: the last hits stay instead of blinking out
  expect(view.queryByRole('option', { name: /Team handbook/ })).not.toBeNull()
  expect(view.queryByRole('option', { name: /Ship the invite flow/ })).not.toBeNull()
  await waitFor(() => expect(pageSearches).toEqual(['onboard', 'onboardi']))
  // tasks are searched once per pause too, not once per key
  expect(taskSearches).toEqual(['onboard', 'onboardi'])
})

test('page hits show the snippet with the matched words in bold, as text', async () => {
  const view = setup()
  const input = await view.findByPlaceholderText(PLACEHOLDER)
  await userEvent.type(input, 'onboard')
  const handbook = await view.findByRole('option', { name: /Team handbook/ }, { timeout: 2000 })
  const snippet = handbook.querySelector('[data-snippet]')!
  expect(snippet.textContent).toBe('How we onboard new people')
  expect([...snippet.querySelectorAll('strong')].map((node) => node.textContent)).toEqual(['onboard'])

  const diary = view.getByRole('option', { name: /Onboarding diary/ })
  expect([...diary.querySelectorAll('strong')].map((node) => node.textContent)).toEqual(['Onboarding', 'onboard'])
  // Markup in page text stays text.
  expect(diary.querySelector('[data-snippet]')!.textContent).toBe('My <b>onboard</b> notes')
  expect(diary.querySelector('b')).toBeNull()
})

test('an empty query lists recently opened pages first; typing hides them', async () => {
  recentPages = [
    { id: 'page-roadmap', parent_id: null, teamspace_id: 'teamspace-1', private: false, title: 'Roadmap', icon: null, kind: 'page', database_title: null, visited_at: '2026-09-25T10:00:00Z' },
    { id: 'page-notes', parent_id: null, teamspace_id: null, private: true, title: '', icon: null, kind: 'page', database_title: null, visited_at: '2026-09-25T09:00:00Z' },
  ]
  try {
    const view = setup()
    const input = await view.findByPlaceholderText(PLACEHOLDER)
    const roadmap = await view.findByRole('option', { name: /Roadmap/ })
    const group = roadmap.closest('[data-recent-pages]')!
    expect(group.textContent).toContain('Recent')
    expect([...group.querySelectorAll('[role=option]')].map((option) => option.textContent)).toEqual([
      expect.stringContaining('Roadmap'),
      expect.stringContaining('Untitled'),
    ])
    await waitFor(() => expect(roadmap.textContent).toContain('General'))
    expect(view.getByRole('option', { name: /Untitled/ }).textContent).toContain('Private')
    // Navigation stays below, shortened.
    expect(view.getByRole('option', { name: /Go to Tasks/ })).toBeTruthy()

    await userEvent.type(input, 'settings')
    await view.findByRole('option', { name: /Go to Settings/ })
    expect(view.container.ownerDocument.querySelector('[data-recent-pages]')).toBeNull()
    await userEvent.type(input, '{Backspace}'.repeat('settings'.length))
    fireEvent.click(await view.findByRole('option', { name: /Roadmap/ }))
    expect(view.getByTestId('location').textContent).toBe('/docs/page-roadmap')
  } finally {
    recentPages = []
  }
})

test('a task command is listed only while there is a task to act on', async () => {
  const without = setup()
  await without.findByPlaceholderText(PLACEHOLDER)
  expect(without.queryByRole('option', { name: /Change status/ })).toBeNull()
  without.unmount()

  let closed = 0
  ran.length = 0
  const view = setup(() => { closed += 1 }, true)
  fireEvent.click(await view.findByRole('option', { name: /Change status/ }))
  expect(closed).toBe(1)
  await waitFor(() => expect(ran).toEqual(['status']))
})

test('a command is found by its name and runs after the palette closes', async () => {
  ran.length = 0
  const order: string[] = []
  const view = setup(() => order.push('closed'))
  const input = await view.findByPlaceholderText(PLACEHOLDER)
  await userEvent.type(input, 'create')
  fireEvent.click(await view.findByRole('option', { name: /Create task/ }))
  await waitFor(() => expect(ran).toEqual(['create']))
  expect(order).toEqual(['closed'])
})

test('a navigation command with a shortcut shows its keys and opens its page', async () => {
  const view = setup()
  const input = await view.findByPlaceholderText(PLACEHOLDER)
  await userEvent.type(input, 'activity')
  const activity = await view.findByRole('option', { name: /Go to Activity/ })
  expect([...activity.querySelectorAll('[data-slot=kbd]')].map((key) => key.textContent)).toEqual(['G', 'A'])
  fireEvent.click(activity)
  expect(view.getByTestId('location').textContent).toBe('/activity')
})

test('a task hit shows its ID, status and project, and a typed ID puts tasks first', async () => {
  const view = setup(() => {}, false, '/docs')
  const input = await view.findByPlaceholderText(PLACEHOLDER)
  await userEvent.type(input, 'eng-12')
  const task = await view.findByRole('option', { name: /Ship the invite flow/ }, { timeout: 2000 })
  expect(task.querySelector('[data-task-id]')!.textContent).toBe('ENG-12')
  await waitFor(() => expect(task.textContent).toContain('Engineering'))
  expect(taskSearches).toContain('eng-12')
  // in Docs, pages usually lead; an ID means the user wants the task
  await view.findByRole('option', { name: /Team handbook/ }, { timeout: 2000 })
  const sections = [...view.container.ownerDocument.querySelectorAll('[data-section]')].map((node) => node.getAttribute('data-section'))
  expect(sections.slice(0, 2)).toEqual(['tasks', 'docs'])
})

test('results of the part of the app in view come first', async () => {
  const view = setup(() => {}, false, '/docs')
  const input = await view.findByPlaceholderText(PLACEHOLDER)
  await userEvent.type(input, 'onboard')
  await view.findByRole('option', { name: /Team handbook/ }, { timeout: 2000 })
  await view.findByRole('option', { name: /Ship the invite flow/ })
  const sections = [...view.container.ownerDocument.querySelectorAll('[data-section]')].map((node) => node.getAttribute('data-section'))
  expect(sections.slice(0, 2)).toEqual(['docs', 'tasks'])
})

test('a channel is found by "#name", as messages write it', async () => {
  if (!chatEnabled) return
  const view = setup()
  const input = await view.findByPlaceholderText(PLACEHOLDER)
  await userEvent.type(input, '#gen')
  expect(await view.findByRole('option', { name: /general/ })).toBeTruthy()
})
