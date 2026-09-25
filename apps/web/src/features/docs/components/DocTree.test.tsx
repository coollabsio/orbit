import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import type { Page, PageSummary, Teamspace } from '@/api/generated/types.gen'
import { ConfirmationModalHost } from '@/components/common/ConfirmationModal'
import { DocTree } from './DocTree'

const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
})

const teamspace = (id: string, name: string, position: number): Teamspace => ({
  id, workspace_id: 'workspace-1', name, icon: null, position, version: 1, is_default: position === 0,
  created_at: '2026-09-25T10:00:00Z', updated_at: '2026-09-25T10:00:00Z',
})

const page = (id: string, parent_id: string | null, position: number, teamspace_id: string | null, title = id): PageSummary => ({
  id, parent_id, position, title, teamspace_id, private: teamspace_id === null, icon: null, version: 1, updated_at: '2026-09-25T10:00:00Z',
})

const fullPage = (summary: PageSummary): Page => ({
  ...summary, workspace_id: 'workspace-1', cover_url: null, cover_position: null, content: [], creator_id: 'user-1',
  updated_by: 'user-1', created_at: '2026-09-25T10:00:00Z', deleted_at: null,
})

const problem = (status: number, code: string) =>
  Response.json(
    { type: 'about:blank', title: 'Problem', status, code, detail: code, instance: '/', request_id: 'request-1' },
    { status, headers: { 'content-type': 'application/problem+json' } },
  )

type Call = { method: string; path: string; search: string; body: Record<string, unknown> | undefined }

const tree = [
  page('roadmap', null, 0, 't1', 'Roadmap'),
  page('notes', null, 0, null, 'My notes'),
  page('draft', 'notes', 0, null, 'Draft'),
]

function setup({ canDelete = true, handler }: { canDelete?: boolean; handler?: (call: Call) => Response | undefined } = {}) {
  window.localStorage.clear()
  const calls: Call[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    const url = new URL(request.url)
    const text = await request.text()
    const call: Call = {
      method: request.method, path: url.pathname.replace('/api/v1/workspaces/workspace-1', ''), search: url.search,
      body: text ? JSON.parse(text) : undefined,
    }
    calls.push(call)
    const custom = handler?.(call)
    if (custom) return custom
    if (call.method === 'GET' && call.path === '/teamspaces') return Response.json({ items: [teamspace('t1', 'General', 0), teamspace('t2', 'Design', 1)] })
    return new Promise<Response>(() => {})
  }) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity }, mutations: { retry: false } } })
  const view = render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={['/docs']}>
        <ConfirmationModalHost />
        <DocTree
          workspaceId="workspace-1"
          pages={tree}
          isPending={false}
          isError={false}
          onRetry={() => {}}
          activeId={null}
          trashActive={false}
          canDeleteTeamspaces={canDelete}
          onTrash={() => {}}
        />
      </MemoryRouter>
    </QueryClientProvider>,
  )
  return { view, calls }
}

const row = (view: ReturnType<typeof render>, space: string) => view.container.querySelector(`[data-space="${space}"]`) as HTMLElement
const pageRow = (view: ReturnType<typeof render>, id: string) => view.container.querySelector(`[data-page-id="${id}"]`) as HTMLElement

test('the sidebar groups pages into teamspace sections and a Private section; empty sections say so', async () => {
  const { view } = setup()
  const general = await view.findByRole('group', { name: 'General' })
  expect(within(general).getByText('Roadmap')).toBeTruthy()
  const design = view.getByRole('group', { name: 'Design' })
  expect(within(design).getByText('No pages inside')).toBeTruthy()
  const mine = view.getByRole('group', { name: 'Private' })
  expect(within(mine).getByText('My notes')).toBeTruthy()
  expect(within(mine).queryByText('Roadmap')).toBeNull()
  expect(view.getByText('Teamspaces')).toBeTruthy()
  // Teamspace pages sit one level below their teamspace row.
  expect(pageRow(view, 'roadmap').getAttribute('aria-level')).toBe('2')

  // Collapsing a section hides its pages and persists per workspace.
  fireEvent.click(row(view, 'teamspace:t1'))
  expect(view.queryByText('Roadmap')).toBeNull()
  expect(JSON.parse(window.localStorage.getItem('orbit:docs_collapsed_spaces:workspace-1') ?? '[]')).toEqual(['teamspace:t1'])
})

test('members see Rename but not "Delete teamspace"', async () => {
  const { view } = setup({ canDelete: false })
  const trigger = await view.findByRole('button', { name: 'General options' })
  fireEvent.click(trigger)
  expect(await view.findByRole('menuitem', { name: 'Rename' })).toBeTruthy()
  expect(view.queryByRole('menuitem', { name: 'Delete teamspace' })).toBeNull()
})

test('owners delete a teamspace after confirming (with expected_version)', async () => {
  const { view, calls } = setup({ handler: (call) => (call.method === 'DELETE' ? problem(409, 'teamspace_not_empty') : undefined) })
  fireEvent.click(await view.findByRole('button', { name: 'General options' }))
  await userEvent.click(await view.findByRole('menuitem', { name: 'Delete teamspace' }))
  await userEvent.click(await view.findByRole('button', { name: 'Delete teamspace' }))
  await waitFor(() => expect(calls.some((call) => call.method === 'DELETE')).toBeTrue())
  expect(calls.find((call) => call.method === 'DELETE')).toMatchObject({ path: '/teamspaces/t1', search: '?expected_version=1' })
})

test('renaming inline patches the teamspace name', async () => {
  const { view, calls } = setup({
    handler: (call) => (call.method === 'PATCH' ? Response.json({ ...teamspace('t2', String(call.body?.name), 1), version: 2 }) : undefined),
  })
  fireEvent.click(await view.findByRole('button', { name: 'Design options' }))
  await userEvent.click(await view.findByRole('menuitem', { name: 'Rename' }))
  const input = (await view.findByRole('textbox', { name: 'Teamspace name' })) as HTMLInputElement
  fireEvent.change(input, { target: { value: 'Product design' } })
  fireEvent.keyDown(input, { key: 'Enter' })
  await waitFor(() => expect(calls.some((call) => call.method === 'PATCH')).toBeTrue())
  expect(calls.find((call) => call.method === 'PATCH')).toMatchObject({ path: '/teamspaces/t2', body: { expected_version: 1, name: 'Product design' } })
  expect(await view.findByText('Product design')).toBeTruthy()
})

test('"+" on a section creates a root page in that space', async () => {
  const { view, calls } = setup({
    handler: (call) => (call.method === 'POST' && call.path === '/pages' ? Response.json(fullPage(page('fresh', null, 1, null)), { status: 201 }) : undefined),
  })
  const posts = () => calls.filter((call) => call.method === 'POST')
  fireEvent.click(await view.findByRole('button', { name: 'Add private page' }))
  await waitFor(() => expect(posts()).toHaveLength(1))
  expect(posts()[0].body).toEqual({ private: true })
  fireEvent.click(view.getByRole('button', { name: 'Add page to Design' }))
  await waitFor(() => expect(posts()).toHaveLength(2))
  expect(posts()[1].body).toEqual({ teamspace_id: 't2' })
})

test('creating a teamspace from the dialog posts its name', async () => {
  let created = false
  const { view, calls } = setup({
    handler: (call) => {
      if (call.path !== '/teamspaces') return undefined
      if (call.method === 'POST') {
        created = true
        return Response.json(teamspace('t3', 'Ops', 2), { status: 201 })
      }
      return created ? Response.json({ items: [teamspace('t1', 'General', 0), teamspace('t2', 'Design', 1), teamspace('t3', 'Ops', 2)] }) : undefined
    },
  })
  fireEvent.click(await view.findByRole('button', { name: 'New teamspace' }))
  const dialog = await view.findByRole('dialog')
  await userEvent.type(within(dialog).getByRole('textbox', { name: 'Teamspace name' }), '  Ops ')
  fireEvent.click(within(dialog).getByRole('button', { name: 'Create teamspace' }))
  await waitFor(() => expect(calls.some((call) => call.path === '/teamspaces' && call.method === 'POST')).toBeTrue())
  expect(calls.find((call) => call.path === '/teamspaces' && call.method === 'POST')?.body).toEqual({ name: 'Ops' })
  expect(await view.findByRole('group', { name: 'Ops' })).toBeTruthy()
})

function dataTransfer() {
  const data = new Map<string, string>()
  return { effectAllowed: 'all', dropEffect: 'none', setData: (type: string, value: string) => data.set(type, value), getData: (type: string) => data.get(type) ?? '' }
}

test('dropping a private page on a teamspace header moves it (and its sub-pages) to the end of that teamspace', async () => {
  const { view, calls } = setup()
  await view.findByRole('group', { name: 'General' })
  const transfer = dataTransfer()
  fireEvent.dragStart(pageRow(view, 'notes'), { dataTransfer: transfer })
  const header = row(view, 'teamspace:t1')
  fireEvent.dragOver(header, { dataTransfer: transfer })
  expect(header.getAttribute('data-drop')).toBe('inside')
  fireEvent.drop(header, { dataTransfer: transfer })
  await waitFor(() => expect(calls.some((call) => call.path === '/pages/notes/move')).toBeTrue())
  expect(calls.find((call) => call.path === '/pages/notes/move')?.body).toEqual({
    expected_version: 1, parent_id: null, position: 1, teamspace_id: 't1',
  })
})

test('dropping on an empty section row moves the page into that space', async () => {
  const { view, calls } = setup()
  await view.findByRole('group', { name: 'Design' })
  const transfer = dataTransfer()
  fireEvent.dragStart(pageRow(view, 'roadmap'), { dataTransfer: transfer })
  const empty = view.container.querySelector('[data-empty-space="teamspace:t2"]') as HTMLElement
  fireEvent.dragOver(empty, { dataTransfer: transfer })
  fireEvent.drop(empty, { dataTransfer: transfer })
  await waitFor(() => expect(calls.some((call) => call.path === '/pages/roadmap/move')).toBeTrue())
  expect(calls.find((call) => call.path === '/pages/roadmap/move')?.body).toEqual({
    expected_version: 1, parent_id: null, position: 0, teamspace_id: 't2',
  })
})

const favoritesHandler = (ids: string[]) => (call: Call) =>
  call.method === 'GET' && call.path === '/pages/favorites'
    ? Response.json({ items: ids.map((page_id, position) => ({ page_id, position })) })
    : undefined

const favoriteRow = (view: ReturnType<typeof render>, id: string) =>
  view.container.querySelector(`[data-section="favorites"][data-page-id="${id}"]`) as HTMLElement

test('the Favorites section is hidden without favorites', async () => {
  const { view, calls } = setup({ handler: favoritesHandler([]) })
  await waitFor(() => expect(calls.some((call) => call.path === '/pages/favorites')).toBeTrue())
  await view.findByRole('group', { name: 'General' })
  expect(view.queryByRole('group', { name: 'Favorites' })).toBeNull()
  expect(view.queryByText('Favorites')).toBeNull()
})

test('favorites come first in their own order; rows expand to sub-pages and skip unknown pages', async () => {
  const { view } = setup({ handler: favoritesHandler(['notes', 'gone', 'roadmap']) })
  const section = await view.findByRole('group', { name: 'Favorites' })
  const nav = view.getByRole('tree', { name: 'Pages' })
  expect(nav.firstElementChild).toBe(section)
  const rows = [...section.querySelectorAll('[data-section="favorites"][data-page-id]')].map((row) => row.getAttribute('data-page-id'))
  expect(rows).toEqual(['notes', 'roadmap'])
  // The page stays in its own space as well.
  expect(within(view.getByRole('group', { name: 'Private' })).getByText('My notes')).toBeTruthy()
  // Sub-pages expand inside Favorites, independently of the Private section.
  expect(within(section).queryByText('Draft')).toBeNull()
  fireEvent.click(within(favoriteRow(view, 'notes')).getByRole('button', { name: 'Expand' }))
  expect(within(section).getByText('Draft')).toBeTruthy()
  expect(within(view.getByRole('group', { name: 'Private' })).queryByText('Draft')).toBeNull()
})

test('dragging a favorite reorders the favorites without moving the page', async () => {
  const { view, calls } = setup({ handler: favoritesHandler(['roadmap', 'notes']) })
  await view.findByRole('group', { name: 'Favorites' })
  const transfer = dataTransfer()
  fireEvent.dragStart(favoriteRow(view, 'roadmap'), { dataTransfer: transfer })
  expect(favoriteRow(view, 'roadmap').getAttribute('data-dragging')).toBe('true')
  const target = favoriteRow(view, 'notes')
  // happy-dom has no layout, so the pointer lands in the lower half: "after", never "inside".
  fireEvent.dragOver(target, { dataTransfer: transfer })
  expect(target.getAttribute('data-drop')).toBe('after')
  fireEvent.drop(target, { dataTransfer: transfer })
  await waitFor(() => expect(calls.some((call) => call.path === '/pages/favorites/roadmap/move')).toBeTrue())
  expect(calls.find((call) => call.path === '/pages/favorites/roadmap/move')?.body).toEqual({ position: 1 })
  expect(calls.some((call) => call.path.endsWith('/move') && !call.path.includes('favorites'))).toBeFalse()
  const rows = [...view.container.querySelectorAll('[data-section="favorites"][data-page-id]')].map((row) => row.getAttribute('data-page-id'))
  expect(rows).toEqual(['notes', 'roadmap'])
})

test('drops between Favorites and the other sections do nothing', async () => {
  const { view, calls } = setup({ handler: favoritesHandler(['roadmap']) })
  await view.findByRole('group', { name: 'Favorites' })
  const spaceRoadmap = view.container.querySelector('[data-page-id="roadmap"]:not([data-section])') as HTMLElement
  // Favorite row → a page row and a teamspace header.
  let transfer = dataTransfer()
  fireEvent.dragStart(favoriteRow(view, 'roadmap'), { dataTransfer: transfer })
  fireEvent.dragOver(pageRow(view, 'notes'), { dataTransfer: transfer })
  expect(pageRow(view, 'notes').getAttribute('data-drop')).toBeNull()
  fireEvent.drop(pageRow(view, 'notes'), { dataTransfer: transfer })
  fireEvent.dragOver(row(view, 'teamspace:t2'), { dataTransfer: transfer })
  fireEvent.drop(row(view, 'teamspace:t2'), { dataTransfer: transfer })
  fireEvent.dragEnd(favoriteRow(view, 'roadmap'), { dataTransfer: transfer })
  // Page row → a favorite row.
  transfer = dataTransfer()
  fireEvent.dragStart(pageRow(view, 'notes'), { dataTransfer: transfer })
  fireEvent.dragOver(favoriteRow(view, 'roadmap'), { dataTransfer: transfer })
  expect(favoriteRow(view, 'roadmap').getAttribute('data-drop')).toBeNull()
  fireEvent.drop(favoriteRow(view, 'roadmap'), { dataTransfer: transfer })
  fireEvent.dragEnd(pageRow(view, 'notes'), { dataTransfer: transfer })
  await new Promise((resolve) => setTimeout(resolve, 10))
  expect(calls.filter((call) => call.method !== 'GET')).toEqual([])
  expect(spaceRoadmap.isConnected).toBeTrue()
})

test('dropping a page on the Favorites header adds it as a favorite', async () => {
  const { view, calls } = setup({
    handler: (call) =>
      call.method === 'PUT' ? Response.json({ page_id: 'notes', position: 1 }) : favoritesHandler(['roadmap'])(call),
  })
  await view.findByRole('group', { name: 'Favorites' })
  const transfer = dataTransfer()
  fireEvent.dragStart(pageRow(view, 'notes'), { dataTransfer: transfer })
  const header = view.container.querySelector('[data-section="favorites"]:not([data-page-id])') as HTMLElement
  fireEvent.dragOver(header, { dataTransfer: transfer })
  expect(header.getAttribute('data-drop')).toBe('inside')
  fireEvent.drop(header, { dataTransfer: transfer })
  await waitFor(() => expect(calls.some((call) => call.method === 'PUT')).toBeTrue())
  expect(calls.find((call) => call.method === 'PUT')?.path).toBe('/pages/notes/favorite')
  expect(calls.some((call) => call.path.endsWith('/move'))).toBeFalse()
})

test('the row menu adds and removes favorites', async () => {
  let ids = ['roadmap']
  const { view, calls } = setup({
    handler: (call) => {
      const id = call.path.split('/')[2]
      if (call.method === 'PUT') {
        ids = [...ids, id]
        return Response.json({ page_id: id, position: ids.length - 1 })
      }
      if (call.method === 'DELETE') {
        ids = ids.filter((item) => item !== id)
        return new Response(null, { status: 204 })
      }
      return favoritesHandler(ids)(call)
    },
  })
  await view.findByRole('group', { name: 'Favorites' })
  fireEvent.click(within(pageRow(view, 'notes')).getByRole('button', { name: 'Page options' }))
  await userEvent.click(await view.findByRole('menuitem', { name: 'Add to favorites' }))
  await waitFor(() => expect(calls.some((call) => call.method === 'PUT' && call.path === '/pages/notes/favorite')).toBeTrue())
  expect(favoriteRow(view, 'notes')).toBeTruthy()

  fireEvent.click(within(favoriteRow(view, 'roadmap')).getByRole('button', { name: 'Page options' }))
  await userEvent.click(await view.findByRole('menuitem', { name: 'Remove from favorites' }))
  await waitFor(() => expect(calls.some((call) => call.method === 'DELETE' && call.path === '/pages/roadmap/favorite')).toBeTrue())
  await waitFor(() => expect(favoriteRow(view, 'roadmap')).toBeNull())
  expect(favoriteRow(view, 'notes')).toBeTruthy()
})
