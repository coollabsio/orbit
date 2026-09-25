import { afterEach, beforeAll, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, fireEvent, render, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, Route, Routes, useLocation } from 'react-router'
import type { Page, PageSummary, Teamspace } from '@/api/generated/types.gen'
import { queryKeys } from '@/api/queryKeys'
import { ConfirmationModalHost } from '@/components/common/ConfirmationModal'
import { WorkspaceProvider } from '@/features/workspaces/WorkspaceProvider'
import { toSummary } from '@/features/docs/api/pages'
import { DocsPage } from './DocsPage'
import { waitForAbsence } from '@/test/waitForAbsence'

beforeAll(() => {
  // BlockNote warns about mobile keyboards without this viewport flag (index.html sets it in the app).
  const meta = document.createElement('meta')
  meta.name = 'viewport'
  meta.content = 'width=device-width, initial-scale=1, interactive-widget=resizes-content'
  document.head.append(meta)
})

const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
})

const summary = (id: string, parent_id: string | null, position: number, title = id): PageSummary => ({
  id, parent_id, teamspace_id: 'teamspace-1', private: false, position, title, icon: null, version: 1,
  updated_at: '2026-09-25T10:00:00Z',
})

const teamspace = (id: string, name: string, position: number, is_default = position === 0): Teamspace => ({
  id, workspace_id: 'workspace-1', name, icon: null, position, version: 1, is_default,
  created_at: '2026-09-25T10:00:00Z', updated_at: '2026-09-25T10:00:00Z',
})

const defaultTeamspaces = [teamspace('teamspace-1', 'General', 0)]

const fullPage = (id: string, patch: Partial<Page> = {}): Page => ({
  ...summary(id, null, 0), workspace_id: 'workspace-1', cover_url: null, cover_position: null, content: [],
  creator_id: 'user-1', updated_by: 'user-1', created_at: '2026-09-25T10:00:00Z', deleted_at: null, ...patch,
})

const problem = (status: number, code: string, extra: Record<string, unknown> = {}) =>
  Response.json(
    { type: 'about:blank', title: 'Problem', status, code, detail: code, instance: '/pages', request_id: 'request-1', ...extra },
    { status, headers: { 'content-type': 'application/problem+json' } },
  )

type Call = { method: string; path: string; search: string; body: Record<string, unknown> | undefined }
type Handler = (call: Call) => Response | Promise<Response> | undefined

function Location() {
  const location = useLocation()
  return <output data-testid="location">{location.pathname}</output>
}

function setup(path: string, handler: Handler, role = 'owner') {
  window.localStorage.clear()
  const calls: Call[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    const url = new URL(request.url)
    const text = await request.text()
    const call: Call = { method: request.method, path: url.pathname.replace('/api/v1/workspaces/workspace-1', ''), search: url.search, body: text ? JSON.parse(text) : undefined }
    calls.push(call)
    const response = handler(call)
    if (response) return response
    if (call.method === 'GET' && call.path === '/teamspaces') return Response.json({ items: defaultTeamspaces })
    return problem(404, 'not_found')
  }) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity }, mutations: { retry: false } } })
  client.setQueryData(queryKeys.workspaces, [{ id: 'workspace-1', name: 'Alpha', role, version: 1 }])
  const view = render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[`${path}?workspace=workspace-1`]}>
        <ConfirmationModalHost />
        <WorkspaceProvider>
          <Routes>
            <Route path="docs/trash" element={<DocsPage view="trash" />} />
            <Route path="docs/:pageId?" element={<DocsPage />} />
          </Routes>
        </WorkspaceProvider>
        <Location />
      </MemoryRouter>
    </QueryClientProvider>,
  )
  return { view, calls, client }
}

const location = (view: ReturnType<typeof render>) => view.getByTestId('location').textContent

test('/docs without pages shows "Create your first page" and creating opens the new page', async () => {
  const { view, calls } = setup('/docs', (call) => {
    if (call.method === 'GET' && call.path === '/pages') return Response.json({ items: [] })
    if (call.method === 'POST' && call.path === '/pages') return Response.json(fullPage('fresh'), { status: 201 })
    if (call.path === '/pages/fresh') return Response.json(fullPage('fresh'))
  })
  expect(await view.findByText('Create your first page')).toBeTruthy()
  fireEvent.click(view.getByRole('button', { name: 'Create page' }))
  await waitFor(() => expect(location(view)).toBe('/docs/fresh'))
  expect(calls.find((call) => call.method === 'POST')?.body).toEqual({})
})

test('/docs opens the first root page, whose tree renders untitled pages as "Untitled"', async () => {
  const { view } = setup('/docs', (call) => {
    if (call.path === '/pages') return Response.json({ items: [summary('second', null, 1, 'Second'), summary('first', null, 0, '')] })
    if (call.path === '/pages/first') return Response.json(fullPage('first', { title: '' }))
  })
  await waitFor(() => expect(location(view)).toBe('/docs/first'))
  const title = (await view.findByLabelText('Page title')) as HTMLInputElement
  expect(title.value).toBe('')
  expect(title.placeholder).toBe('Untitled')
  const tree = view.getByRole('tree', { name: 'Pages' })
  expect(tree.textContent).toContain('Untitled')
  expect(tree.textContent).toContain('Second')
})

test('a missing or trashed page shows a message with a way back', async () => {
  const { view } = setup('/docs/gone', (call) => {
    if (call.path === '/pages') return Response.json({ items: [summary('first', null, 0)] })
    if (call.path === '/pages/gone') return problem(404, 'page_not_found')
  })
  expect(await view.findByText('Page not found')).toBeTruthy()
  fireEvent.click(view.getByRole('link', { name: 'Back to Docs' }))
  await waitFor(() => expect(location(view)).not.toBe('/docs/gone'))
})

test('title edits autosave after the debounce with expected_version, and the tree follows the title', async () => {
  let version = 1
  const { view, calls } = setup('/docs/first', (call) => {
    if (call.path === '/pages') return Response.json({ items: [summary('first', null, 0, 'Draft')] })
    if (call.path === '/pages/first' && call.method === 'GET') return Response.json(fullPage('first', { title: 'Draft' }))
    if (call.path === '/pages/first' && call.method === 'PATCH') {
      version += 1
      return Response.json(fullPage('first', { title: String(call.body?.title), version }))
    }
  })
  const title = (await view.findByLabelText('Page title')) as HTMLInputElement
  await userEvent.clear(title)
  await userEvent.type(title, 'Roadmap Q4')
  await waitFor(() => expect(view.getByRole('tree').textContent).toContain('Roadmap Q4'))
  expect(calls.filter((call) => call.method === 'PATCH')).toHaveLength(0)

  await waitFor(() => expect(view.container.querySelector('[data-save-status="saved"]')?.textContent).toBe('Saved'), { timeout: 3000 })
  const patches = calls.filter((call) => call.method === 'PATCH')
  expect(patches).toHaveLength(1)
  expect(patches[0].body).toEqual({ title: 'Roadmap Q4', expected_version: 1 })

  // Blur flushes the next edit without waiting for the debounce, on top of the returned version.
  await userEvent.type(title, ' final')
  fireEvent.blur(title)
  await waitFor(() => expect(calls.filter((call) => call.method === 'PATCH')).toHaveLength(2))
  expect(calls.filter((call) => call.method === 'PATCH')[1].body).toEqual({ title: 'Roadmap Q4 final', expected_version: 2 })
})

test('a 409 shows the conflict banner; Overwrite re-sends the local page on the current version', async () => {
  const { view, calls } = setup('/docs/first', (call) => {
    if (call.path === '/pages') return Response.json({ items: [summary('first', null, 0, 'Draft')] })
    if (call.path === '/pages/first' && call.method === 'GET') return Response.json(fullPage('first', { title: 'Draft' }))
    if (call.path === '/pages/first' && call.method === 'PATCH') {
      if (call.body?.expected_version === 1) {
        return problem(409, 'conflict', { conflict: { current_version: 4, refresh: '/pages/first', current: fullPage('first', { title: 'Theirs', version: 4 }) } })
      }
      return Response.json(fullPage('first', { title: String(call.body?.title), version: 5 }))
    }
  })
  const title = (await view.findByLabelText('Page title')) as HTMLInputElement
  await userEvent.clear(title)
  await userEvent.type(title, 'Mine')
  fireEvent.blur(title)

  const banner = await view.findByRole('alert')
  expect(banner.textContent).toContain('changed somewhere else')
  expect(view.container.querySelector('[data-save-status="conflict"]')?.textContent).toContain('Conflict')

  await act(async () => {
    fireEvent.click(view.getByRole('button', { name: 'Overwrite' }))
  })
  await waitFor(() => expect(view.container.querySelector('[data-save-status="saved"]')).toBeTruthy())
  const last = calls.filter((call) => call.method === 'PATCH').at(-1)!
  expect(last.body).toMatchObject({ title: 'Mine', expected_version: 4, icon: null, cover_url: null, cover_position: null })
  expect(view.queryByRole('alert')).toBeNull()
})

test('a 409 then Reload page discards local edits and shows the server page', async () => {
  let reads = 0
  const { view } = setup('/docs/first', (call) => {
    if (call.path === '/pages') return Response.json({ items: [summary('first', null, 0, 'Draft')] })
    if (call.path === '/pages/first' && call.method === 'GET') {
      reads += 1
      return Response.json(reads === 1 ? fullPage('first', { title: 'Draft', version: 4 }) : fullPage('first', { title: 'Theirs', version: 5 }))
    }
    // Another tab saved first: our save is stale even though we loaded version 4.
    if (call.path === '/pages/first' && call.method === 'PATCH') return problem(409, 'conflict', { conflict: { current_version: 5 } })
  })
  const title = (await view.findByLabelText('Page title')) as HTMLInputElement
  expect(title.value).toBe('Draft')
  await userEvent.clear(title)
  await userEvent.type(title, 'Mine')
  fireEvent.blur(title)
  await view.findByRole('alert')
  await act(async () => {
    fireEvent.click(view.getByRole('button', { name: 'Reload page' }))
  })
  await waitFor(() => expect(title.value).toBe('Theirs'))
  expect(view.queryByRole('alert')).toBeNull()
})

test('the trash view lists trashed pages and restores them', async () => {
  const { view, calls } = setup('/docs/trash', (call) => {
    if (call.path === '/pages') return Response.json({ items: [] })
    if (call.path === '/pages/trash') {
      return Response.json({ items: [{ ...summary('old', null, 0, 'Old notes'), version: 3, deleted_at: '2026-09-25T09:00:00Z' }] })
    }
    if (call.path === '/pages/old/restore') return Response.json(fullPage('old', { title: 'Old notes', version: 4 }))
  })
  expect(await view.findByText('Old notes')).toBeTruthy()
  // Each row names the space the page will come back to.
  await waitFor(() => expect(view.container.querySelector('[data-space-label]')?.textContent).toBe('General'))
  fireEvent.click(view.getByRole('button', { name: 'Restore' }))
  await waitFor(() => expect(calls.some((call) => call.path === '/pages/old/restore')).toBeTrue())
  expect(calls.find((call) => call.path === '/pages/old/restore')?.body).toEqual({ expected_version: 3 })
})

const privatePage = (id: string, position: number, title = id): PageSummary => ({ ...summary(id, null, position, title), teamspace_id: null, private: true })

test('/docs lands on the default teamspace first, then private pages, then other teamspaces', async () => {
  const both = setup('/docs', (call) => {
    if (call.path === '/pages') return Response.json({ items: [privatePage('mine', 0), summary('team', null, 0)] })
    if (call.path === '/teamspaces') return Response.json({ items: [teamspace('teamspace-0', 'Design', 0, false), teamspace('teamspace-1', 'General', 1, true)] })
    if (call.path === '/pages/team') return Response.json(fullPage('team'))
  })
  await waitFor(() => expect(location(both.view)).toBe('/docs/team'))
  both.view.unmount()

  const privateFirst = setup('/docs', (call) => {
    if (call.path === '/pages') return Response.json({ items: [{ ...summary('other', null, 0), teamspace_id: 'teamspace-2' }, privatePage('mine', 0)] })
    if (call.path === '/teamspaces') return Response.json({ items: [teamspace('teamspace-1', 'General', 0), teamspace('teamspace-2', 'Ops', 1)] })
    if (call.path === '/pages/mine') return Response.json(fullPage('mine', { teamspace_id: null, private: true }))
  })
  await waitFor(() => expect(location(privateFirst.view)).toBe('/docs/mine'))
})

test('without any page the sidebar still offers the sections, and the empty state creates in the default teamspace', async () => {
  const { view } = setup('/docs', (call) => {
    if (call.method === 'GET' && call.path === '/pages') return Response.json({ items: [] })
  })
  expect(await view.findByText('Create your first page')).toBeTruthy()
  expect(view.getByRole('group', { name: 'General' }).textContent).toContain('No pages inside')
  expect(view.getByRole('group', { name: 'Private' }).textContent).toContain('No pages inside')
  expect(view.getByRole('button', { name: 'Add private page' })).toBeTruthy()
})

test('the page header shows the space and "Move to" moves the page to the end of another space', async () => {
  let moved = false
  const movedPage = fullPage('team', { title: 'Plan', teamspace_id: null, private: true, position: 1, version: 2 })
  const { view, calls } = setup('/docs/team', (call) => {
    if (call.path === '/pages') return Response.json({ items: [moved ? toSummary(movedPage) : summary('team', null, 0, 'Plan'), privatePage('mine', 0)] })
    if (call.path === '/pages/team' && call.method === 'GET') return Response.json(moved ? movedPage : fullPage('team', { title: 'Plan' }))
    if (call.path === '/pages/team/move') {
      moved = true
      return Response.json(movedPage)
    }
  }, 'member')
  const path = await view.findByRole('navigation', { name: 'Page path' })
  await waitFor(() => expect(path.textContent).toContain('General'))
  fireEvent.click(within(view.container.querySelector('section:last-of-type') as HTMLElement).getAllByRole('button', { name: 'Page options' }).at(-1)!)
  const current = await view.findByRole('menuitem', { name: /General/ })
  expect(current.getAttribute('aria-disabled') ?? current.getAttribute('data-disabled')).not.toBeNull()
  await userEvent.click(await view.findByRole('menuitem', { name: /Private/ }))
  await waitFor(() => expect(calls.some((call) => call.path === '/pages/team/move')).toBeTrue())
  expect(calls.find((call) => call.path === '/pages/team/move')?.body).toEqual({ expected_version: 1, parent_id: null, position: 1, private: true })
  await waitFor(() => expect(path.textContent).toContain('Private'))
})

test('the header star and the page menu add and remove the page from favorites', async () => {
  let favorites: string[] = []
  const { view, calls } = setup('/docs/first', (call) => {
    if (call.path === '/pages') return Response.json({ items: [summary('first', null, 0, 'Plan')] })
    if (call.path === '/pages/first' && call.method === 'GET') return Response.json(fullPage('first', { title: 'Plan' }))
    if (call.path === '/pages/favorites') return Response.json({ items: favorites.map((page_id, position) => ({ page_id, position })) })
    if (call.path === '/pages/first/favorite' && call.method === 'PUT') {
      favorites = ['first']
      return Response.json({ page_id: 'first', position: 0 })
    }
    if (call.path === '/pages/first/favorite' && call.method === 'DELETE') {
      favorites = []
      return new Response(null, { status: 204 })
    }
  })
  const star = await view.findByRole('button', { name: 'Add to favorites' })
  await waitFor(() => expect(star.hasAttribute('disabled')).toBeFalse())
  expect(star.getAttribute('aria-pressed')).toBe('false')
  expect(view.queryByRole('group', { name: 'Favorites' })).toBeNull()

  fireEvent.click(star)
  await waitFor(() => expect(calls.some((call) => call.method === 'PUT' && call.path === '/pages/first/favorite')).toBeTrue())
  const pressed = await view.findByRole('button', { name: 'Remove from favorites' })
  expect(pressed.getAttribute('aria-pressed')).toBe('true')
  const section = await view.findByRole('group', { name: 'Favorites' })
  expect(section.textContent).toContain('Plan')

  fireEvent.click(within(view.container.querySelector('section:last-of-type') as HTMLElement).getAllByRole('button', { name: 'Page options' }).at(-1)!)
  await userEvent.click(await view.findByRole('menuitem', { name: 'Remove from favorites' }))
  await waitFor(() => expect(calls.some((call) => call.method === 'DELETE' && call.path === '/pages/first/favorite')).toBeTrue())
  expect((await view.findByRole('button', { name: 'Add to favorites' })).getAttribute('aria-pressed')).toBe('false')
  await waitForAbsence(() => view.queryByRole('group', { name: 'Favorites' }))
})
