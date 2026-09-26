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
import { CollabConnectContext, type CollabTarget } from '@/features/docs/collab/connection'
import { DocsPage } from './DocsPage'
import { PRINT_CLASS } from '@/features/docs/pageExport'
import { fakeCollab, fragmentText, type FakeCollab } from '@/test/fakeCollab'
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
  creator_id: 'user-1', updated_by: 'user-1', created_at: '2026-09-25T10:00:00Z', deleted_at: null, collab_epoch: 'epoch-1',
  full_width: false, locked_at: null, locked_by: null, updated_by_user: { id: 'user-1', display_name: 'Ada' },
  ...patch,
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

interface SetupOptions {
  /** What the collab server holds per page (default: an empty page). */
  content?: (target: CollabTarget) => unknown[]
  collab?: FakeCollab
}

function setup(path: string, handler: Handler, role = 'owner', options: SetupOptions = {}) {
  const collab = options.collab ?? fakeCollab({ content: options.content })
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
      <CollabConnectContext value={collab.connect}>
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
      </CollabConnectContext>
    </QueryClientProvider>,
  )
  return { view, calls, client, collab }
}

const location = (view: ReturnType<typeof render>) => view.getByTestId('location').textContent

/** The editor mounts after the collaborative document's first sync. */
/**
 * Empties the title through its real onChange. `userEvent.clear` selects and then deletes; a re-render while the editor and
 * comment threads finish loading can drop that selection under happy-dom, so the delete was lost and typing appended to
 * the old title ("DraftRoadmap Q4") in about 1 of 3 full-file runs.
 */
function clearTitle(title: HTMLInputElement) {
  fireEvent.change(title, { target: { value: '' } })
}

async function editorMounted(view: ReturnType<typeof render>) {
  await waitFor(() => {
    if (!view.container.querySelector('.bn-editor')) throw new Error('editor not mounted yet')
  }, { timeout: 4000 })
}

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
  await editorMounted(view)
  clearTitle(title)
  await userEvent.type(title, 'Roadmap Q4')
  await waitFor(() => expect(view.getByRole('tree').textContent).toContain('Roadmap Q4'))
  expect(calls.filter((call) => call.method === 'PATCH')).toHaveLength(0)

  await waitFor(() => expect(view.container.querySelector('[data-save-status="saved"]')).toBeTruthy(), { timeout: 3000 })
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
  await editorMounted(view)
  clearTitle(title)
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
  clearTitle(title)
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

const trashed = (page: PageSummary, version = 3) => ({ ...page, version, deleted_at: '2026-09-25T09:00:00Z' })

test('members delete their own private trash forever; teamspace pages offer only Restore', async () => {
  let purged = false
  const { view, calls } = setup('/docs/trash', (call) => {
    if (call.path === '/pages') return Response.json({ items: [] })
    if (call.path === '/pages/trash') {
      return Response.json({
        items: [trashed(summary('old', null, 0, 'Old notes')), ...(purged ? [] : [trashed(privatePage('diary', 0, 'Diary'))])],
      })
    }
    if (call.method === 'DELETE' && call.path === '/pages/diary/permanent') {
      purged = true
      return new Response(null, { status: 204 })
    }
  }, 'member')
  expect(await view.findByText('Old notes')).toBeTruthy()
  expect(view.queryByRole('button', { name: 'Delete “Old notes” forever' })).toBeNull()
  fireEvent.click(await view.findByRole('button', { name: 'Delete “Diary” forever' }))
  const dialog = await view.findByRole('dialog')
  expect(dialog.textContent).toContain('Delete “Diary” forever?')
  fireEvent.click(within(dialog).getByRole('button', { name: 'Delete forever' }))
  await waitFor(() => expect(calls.some((call) => call.method === 'DELETE')).toBeTrue())
  expect(calls.find((call) => call.method === 'DELETE')).toMatchObject({ path: '/pages/diary/permanent', search: '?expected_version=3' })
  await waitForAbsence(() => view.queryByText('Diary'))
  // Nothing left that a member may purge: no "Empty trash".
  await waitForAbsence(() => view.queryByRole('button', { name: 'Empty trash' }))
})

test('"Empty trash" confirms with the purgeable count and posts once', async () => {
  let emptied = false
  const { view, calls } = setup('/docs/trash', (call) => {
    if (call.path === '/pages') return Response.json({ items: [] })
    if (call.path === '/pages/trash') {
      return Response.json({ items: emptied ? [] : [trashed(summary('old', null, 0, 'Old notes')), trashed(privatePage('diary', 0, 'Diary'))] })
    }
    if (call.method === 'POST' && call.path === '/pages/trash/empty') {
      emptied = true
      return Response.json({ purged: 2 })
    }
  })
  // Owners may purge teamspace pages too.
  expect(await view.findByRole('button', { name: 'Delete “Old notes” forever' })).toBeTruthy()
  fireEvent.click(view.getByRole('button', { name: 'Empty trash' }))
  const dialog = await view.findByRole('dialog')
  expect(dialog.textContent).toContain('2 pages and the sub-pages trashed with them will be deleted forever')
  fireEvent.click(within(dialog).getByRole('button', { name: 'Empty trash' }))
  await waitFor(() => expect(calls.filter((call) => call.path === '/pages/trash/empty')).toHaveLength(1))
  expect(await view.findByText('Trash is empty')).toBeTruthy()
})

test('"Duplicate with sub-pages" in the page menu copies the subtree and opens the copy', async () => {
  const { view, calls } = setup('/docs/first', (call) => {
    if (call.path === '/pages') return Response.json({ items: [summary('first', null, 0, 'Plan'), summary('child', 'first', 0, 'Child')] })
    if (call.path === '/pages/first' && call.method === 'GET') return Response.json(fullPage('first', { title: 'Plan' }))
    if (call.path === '/pages/first/duplicate') return Response.json(fullPage('copy', { title: 'Plan (copy)', position: 1 }), { status: 201 })
    if (call.path === '/pages/copy') return Response.json(fullPage('copy', { title: 'Plan (copy)', position: 1 }))
  })
  await view.findByLabelText('Page title')
  fireEvent.click(within(view.container.querySelector('section:last-of-type') as HTMLElement).getAllByRole('button', { name: 'Page options' }).at(-1)!)
  expect(await view.findByRole('menuitem', { name: 'Duplicate' })).toBeTruthy()
  await userEvent.click(await view.findByRole('menuitem', { name: 'Duplicate with sub-pages' }))
  await waitFor(() => expect(location(view)).toBe('/docs/copy'))
  expect(calls.find((call) => call.path === '/pages/first/duplicate')?.body).toEqual({ include_children: true })
})

test('the row menu duplicates a page without sub-pages; leaf rows offer no sub-page copy', async () => {
  const { view, calls } = setup('/docs/first', (call) => {
    if (call.path === '/pages') return Response.json({ items: [summary('first', null, 0, 'Plan'), summary('leaf', null, 1, 'Leaf')] })
    if (call.path === '/pages/first' && call.method === 'GET') return Response.json(fullPage('first', { title: 'Plan' }))
    if (call.path === '/pages/leaf/duplicate') return Response.json(fullPage('leaf-copy', { title: 'Leaf (copy)', position: 2 }), { status: 201 })
    if (call.path === '/pages/leaf-copy') return Response.json(fullPage('leaf-copy', { title: 'Leaf (copy)' }))
  })
  const tree = await view.findByRole('tree', { name: 'Pages' })
  const row = await within(tree).findByText('Leaf')
  fireEvent.click(within(row.closest('[data-page-id]') as HTMLElement).getByRole('button', { name: 'Page options' }))
  expect(await view.findByRole('menuitem', { name: 'Duplicate' })).toBeTruthy()
  expect(view.queryByRole('menuitem', { name: 'Duplicate with sub-pages' })).toBeNull()
  await userEvent.click(view.getByRole('menuitem', { name: 'Duplicate' }))
  await waitFor(() => expect(location(view)).toBe('/docs/leaf-copy'))
  expect(calls.find((call) => call.path === '/pages/leaf/duplicate')?.body).toEqual({ include_children: false })
})

test('"Version history" restores a version: metadata from the response, content live through the document', async () => {
  const body = (text: string) => [{ id: `block-${text}`, type: 'paragraph', props: {}, content: [{ type: 'text', text, styles: {} }], children: [] }]
  const stored = {
    id: 'v1', page_id: 'first', kind: 'auto', title: 'Old plan', icon: null, created_at: '2026-09-24T09:30:00Z',
    created_by: { id: 'user-1', display_name: 'Ada' },
  }
  const collab = fakeCollab({ content: () => body('Current text') })
  const { view, calls } = setup('/docs/first', (call) => {
    if (call.path === '/pages') return Response.json({ items: [summary('first', null, 0, 'Plan')] })
    if (call.path === '/pages/first' && call.method === 'GET')
      return Response.json(fullPage('first', { title: 'Plan', content: body('Current text'), version: 3 }))
    if (call.path === '/pages/first/versions') return Response.json({ items: [stored], next_cursor: null })
    if (call.path === '/pages/first/versions/v1') return Response.json({ ...stored, content: body('Old text') })
    if (call.path === '/pages/first/versions/v1/restore') {
      // The server replaces the collaborative document; every open editor gets it over the socket.
      collab.last().provider.replace(body('Old text'))
      return Response.json(fullPage('first', { title: 'Old plan', content: body('Old text'), version: 4 }))
    }
  }, 'owner', { collab })
  const title = (await view.findByLabelText('Page title')) as HTMLInputElement
  const editorSection = view.container.querySelector('section:last-of-type') as HTMLElement
  await waitFor(() => expect(editorSection.textContent).toContain('Current text'), { timeout: 4000 })

  fireEvent.click(within(editorSection).getAllByRole('button', { name: 'Page options' }).at(-1)!)
  fireEvent.click(await view.findByRole('menuitem', { name: 'Version history' }))
  const pane = await view.findByRole('complementary', { name: 'Version history' })
  const preview = within(pane).getByRole('region', { name: 'Version preview' })
  await waitFor(() => expect(preview.textContent).toContain('Old text'), { timeout: 4000 })

  const restore = within(pane).getByRole('button', { name: 'Restore this version' })
  await waitFor(() => expect(restore.hasAttribute('disabled')).toBeFalse())
  fireEvent.click(restore)
  const dialog = await view.findByRole('dialog')
  expect(dialog.textContent).toContain('Restore this version?')
  fireEvent.click(within(dialog).getByRole('button', { name: 'Restore' }))

  await waitFor(() => expect(title.value).toBe('Old plan'))
  expect(calls.find((call) => call.path === '/pages/first/versions/v1/restore')?.body).toEqual({ expected_version: 3 })
  await waitFor(() => expect(editorSection.textContent).toContain('Old text'))
  expect(editorSection.textContent).not.toContain('Current text')
  await waitForAbsence(() => view.queryByRole('complementary', { name: 'Version history' }))
  // The restored page is the new base: nothing is saved back.
  expect(calls.some((call) => call.method === 'PATCH')).toBeFalse()
})

// Real-time co-editing (the collab socket is a fake provider; see src/test/fakeCollab.ts).

const text = (value: string) => [{ id: `block-${value}`, type: 'paragraph', props: {}, content: [{ type: 'text', text: value, styles: {} }], children: [] }]
const collabStatus = (view: ReturnType<typeof render>) => view.container.querySelector('[data-collab-status]') as HTMLElement

/** Types through the editor the way a user would (focus via Enter in the title, then paste). */
async function pasteIntoEditor(view: ReturnType<typeof render>, value: string) {
  fireEvent.keyDown(view.getByLabelText('Page title'), { key: 'Enter' })
  const editorEl = view.container.querySelector('.bn-editor') as HTMLElement
  const data: Record<string, string> = { 'text/plain': value }
  await act(async () => {
    fireEvent.paste(editorEl, { clipboardData: { types: Object.keys(data), getData: (type: string) => data[type] ?? '' } })
  })
}

function beforeUnloadBlocked(): boolean {
  const event = new Event('beforeunload', { cancelable: true })
  window.dispatchEvent(event)
  return event.defaultPrevented
}

const pageRoutes = (extra: Handler = () => undefined): Handler => (call) =>
  extra(call) ??
  (call.path === '/pages' ? Response.json({ items: [summary('first', null, 0, 'Plan'), summary('second', null, 1, 'Second')] }) : undefined) ??
  (call.path === '/pages/first' && call.method === 'GET' ? Response.json(fullPage('first', { title: 'Plan' })) : undefined) ??
  (call.path === '/pages/second' && call.method === 'GET' ? Response.json(fullPage('second', { title: 'Second' })) : undefined)

test('content is live: the editor mounts after the first sync, shows remote edits, and typing sends no PATCH', async () => {
  const collab = fakeCollab({ content: () => text('Shared text'), autoSync: false })
  const { view, calls } = setup('/docs/first', pageRoutes(), 'owner', { collab })
  await view.findByLabelText('Page title')
  await waitFor(() => expect(collab.connections).toHaveLength(1))
  expect(collab.last().target).toEqual({ workspaceId: 'workspace-1', pageId: 'first', epoch: 'epoch-1' })
  // Never bound to an unsynced (empty) document.
  expect(view.getByTestId('editor-loading')).toBeTruthy()
  expect(view.container.querySelector('.bn-editor')).toBeNull()
  expect(collabStatus(view).dataset.collabStatus).toBe('connecting')
  expect(collabStatus(view).textContent).toBe('Connecting…')

  act(() => collab.last().provider.connectAndSync())
  await editorMounted(view)
  const editorSection = view.container.querySelector('section:last-of-type') as HTMLElement
  await waitFor(() => expect(editorSection.textContent).toContain('Shared text'))
  expect(collabStatus(view).textContent).toBe('Live')

  act(() => collab.last().provider.replace(text('Edited elsewhere')))
  await waitFor(() => expect(editorSection.textContent).toContain('Edited elsewhere'))

  await pasteIntoEditor(view, 'typed here')
  await waitFor(() => expect(fragmentText(collab.last().fragment)).toContain('typed here'))
  await new Promise((resolve) => setTimeout(resolve, 900))
  expect(calls.filter((call) => call.method === 'PATCH')).toHaveLength(0)
  expect(beforeUnloadBlocked()).toBeFalse()
})

test('offline: "Connecting…" then "Offline", beforeunload warns only with unsynced edits, reconnect is live again', async () => {
  const { view, collab } = setup('/docs/first', pageRoutes(), 'owner', { content: () => text('Body') })
  await editorMounted(view)
  await waitFor(() => expect(collabStatus(view).textContent).toBe('Live'))
  const provider = collab.last().provider

  act(() => provider.drop(1006))
  expect(collabStatus(view).textContent).toBe('Connecting…')
  await waitFor(() => expect(collabStatus(view).textContent).toBe('Offline — changes will sync'), { timeout: 3500 })
  // Nothing typed since long before the drop: leaving is safe.
  expect(beforeUnloadBlocked()).toBeFalse()

  await pasteIntoEditor(view, 'offline words')
  expect(fragmentText(collab.last().fragment)).toContain('offline words')
  expect(beforeUnloadBlocked()).toBeTrue()

  act(() => provider.connectAndSync())
  await waitFor(() => expect(collabStatus(view).textContent).toBe('Live'))
  expect(beforeUnloadBlocked()).toBeFalse()
})

test('4404 (someone else trashed the page) makes it read-only, drops it from the tree and leaves for /docs', async () => {
  let trashed = false
  const { view, collab, calls } = setup(
    '/docs/first',
    pageRoutes((call) => (trashed && call.path === '/pages' ? Response.json({ items: [summary('second', null, 1, 'Second')] }) : undefined)),
  )
  await editorMounted(view)
  trashed = true
  const before = calls.length
  act(() => collab.last().provider.drop(4404))
  await waitFor(() => expect(location(view)).toBe('/docs/second'))
  // Leaving stops observing the page: nothing refetches it (it would only 404).
  expect(calls.slice(before).filter((call) => call.path === '/pages/first')).toHaveLength(0)
  const tree = view.getByRole('tree', { name: 'Pages' })
  expect(tree.textContent).not.toContain('Plan')
  expect(collab.connections[0].provider.destroyed).toBeTrue()
})

test('refused handshakes are checked over REST: a 404 page is left like a trashed one', async () => {
  let gone = false
  const { view, collab } = setup(
    '/docs/first',
    pageRoutes((call) => {
      if (!gone) return undefined
      if (call.path === '/pages/first') return problem(404, 'page_not_found')
      if (call.path === '/pages') return Response.json({ items: [summary('second', null, 1, 'Second')] })
    }),
  )
  await editorMounted(view)
  gone = true
  const provider = collab.last().provider
  act(() => provider.drop(1006))
  act(() => provider.refuse())
  act(() => provider.refuse())
  await waitFor(() => expect(location(view)).toBe('/docs/second'))
  expect(provider.destroyed).toBeTrue()
})

test('4409 (document reset) refetches the page and reconnects with the new epoch', async () => {
  let reads = 0
  const { view, collab } = setup(
    '/docs/first',
    pageRoutes((call) => {
      if (call.path !== '/pages/first' || call.method !== 'GET') return undefined
      reads += 1
      return Response.json(fullPage('first', { title: 'Plan', collab_epoch: reads === 1 ? 'epoch-1' : 'epoch-2' }))
    }),
    'owner',
    { content: (target) => text(target.epoch === 'epoch-2' ? 'After reset' : 'Before reset') },
  )
  await editorMounted(view)
  const editorSection = view.container.querySelector('section:last-of-type') as HTMLElement
  await waitFor(() => expect(editorSection.textContent).toContain('Before reset'))
  act(() => collab.last().provider.drop(4409))
  await waitFor(() => expect(collab.connections).toHaveLength(2))
  expect(collab.last().target.epoch).toBe('epoch-2')
  expect(collab.connections[0].provider.destroyed).toBeTrue()
  await waitFor(() => expect(editorSection.textContent).toContain('After reset'))
  expect(collabStatus(view).textContent).toBe('Live')
  expect(location(view)).toBe('/docs/first')
})

test('4401 goes through the app-wide unauthorized flow', async () => {
  const { view, collab } = setup('/docs/first', pageRoutes())
  await editorMounted(view)
  let unauthorized = 0
  const listener = () => {
    unauthorized += 1
  }
  window.addEventListener('orbit:unauthorized', listener)
  try {
    act(() => collab.last().provider.drop(4401))
    await waitFor(() => expect(unauthorized).toBe(1))
    expect(collabStatus(view).textContent).toBe('Access lost')
  } finally {
    window.removeEventListener('orbit:unauthorized', listener)
  }
})

test('4413 shows a sync notice, stops editing, and "Reload page" opens a fresh document', async () => {
  const { view, collab } = setup('/docs/first', pageRoutes(), 'owner', { content: () => text('Body') })
  await editorMounted(view)
  act(() => collab.last().provider.drop(4413))
  const notice = await view.findByRole('alert')
  expect(notice.getAttribute('data-sync-notice')).toBe('too-large')
  expect(collabStatus(view).textContent).toBe('Not syncing')
  await waitFor(() => expect(view.container.querySelector('.bn-editor')?.getAttribute('contenteditable')).toBe('false'))
  fireEvent.click(within(notice).getByRole('button', { name: 'Reload page' }))
  await waitFor(() => expect(collab.connections).toHaveLength(2))
  await waitFor(() => expect(collabStatus(view).textContent).toBe('Live'))
  await waitForAbsence(() => view.queryByRole('alert'))
})

test('presence: other people show in the header once each, never the current user', async () => {
  const { view, collab } = setup(
    '/docs/first',
    pageRoutes((call) => (call.path === '/api/v1/auth/me' ? Response.json({ id: 'user-1', display_name: 'Test User', email: 'test@example.com' }) : undefined)),
  )
  await editorMounted(view)
  const provider = collab.last().provider
  act(() => {
    provider.addPeer({ id: 'user-2', name: 'Ada Lovelace', color: '#e11d48' })
    provider.addPeer({ id: 'user-2', name: 'Ada Lovelace', color: '#e11d48' })
    provider.addPeer({ id: 'user-3', name: 'Bob', color: '#2563eb' })
  })
  await waitFor(() => expect(view.getByTestId('presence').querySelectorAll('[data-presence-user]')).toHaveLength(2))
  // Our own other tab: same user id, hidden (once /me is known).
  act(() => void provider.addPeer({ id: 'user-1', name: 'Test User', color: '#9333ea' }))
  await waitFor(() => expect(view.getByTestId('presence').getAttribute('aria-label')).toBe('Also here: Ada Lovelace, Bob'))
  // The local awareness state carries our name for the server (which stamps id/name/color anyway).
  const local = provider.awareness.getLocalState() as { user: { name: string } }
  expect(local.user.name).toBe('Test User')
})

test('the page menu exports Markdown (with sub-pages only when there are some) and prints PDF with the print class', async () => {
  const { view, calls } = setup('/docs/first', (call) => {
    if (call.path === '/pages') return Response.json({ items: [summary('first', null, 0, 'Plan'), summary('child', 'first', 0, 'Child')] })
    if (call.path === '/pages/first' && call.method === 'GET') return Response.json(fullPage('first', { title: 'Plan' }))
    if (call.path === '/pages/child' && call.method === 'GET') return Response.json(fullPage('child', { title: 'Child', parent_id: 'first' }))
    if (call.path.endsWith('/export')) {
      return new Response(new Uint8Array([80, 75, 5, 6]), {
        headers: { 'content-type': 'application/zip', 'content-disposition': `attachment; filename="Plan.zip"; filename*=UTF-8''Plan.zip` },
      })
    }
  })
  const downloads: string[] = []
  const originalClick = HTMLAnchorElement.prototype.click
  const { createObjectURL, revokeObjectURL } = URL
  const originalPrint = window.print
  HTMLAnchorElement.prototype.click = function (this: HTMLAnchorElement) {
    downloads.push(this.download)
  }
  URL.createObjectURL = () => 'blob:export'
  URL.revokeObjectURL = () => {}
  const printed: Array<{ printing: boolean; title: string }> = []
  window.print = () => {
    printed.push({ printing: document.documentElement.classList.contains(PRINT_CLASS), title: document.title })
  }
  try {
    await view.findByLabelText('Page title')
    const openExport = async () => {
      fireEvent.click(within(view.container.querySelector('section:last-of-type') as HTMLElement).getAllByRole('button', { name: 'Page options' }).at(-1)!)
      fireEvent.click(await view.findByRole('menuitem', { name: 'Export' }))
      await view.findByRole('menuitem', { name: 'PDF' })
    }
    await openExport()
    expect(view.getByRole('menuitem', { name: 'Markdown' })).toBeTruthy()
    await userEvent.click(view.getByRole('menuitem', { name: 'Markdown with sub-pages' }))
    await waitFor(() => expect(downloads).toEqual(['Plan.zip']))
    expect(calls.find((call) => call.path === '/pages/first/export')?.search).toBe('?format=markdown&children=true')

    await openExport()
    await userEvent.click(view.getByRole('menuitem', { name: 'PDF' }))
    expect(printed).toEqual([{ printing: true, title: 'Plan' }])
    window.dispatchEvent(new Event('afterprint'))
    expect(document.documentElement.classList.contains(PRINT_CLASS)).toBe(false)
    // The print stylesheet keeps only this subtree: the title and the editor are inside it.
    const printRoot = view.container.querySelector('[data-print-root]') as HTMLElement
    expect(within(printRoot).getByLabelText('Page title')).toBeTruthy()

  } finally {
    HTMLAnchorElement.prototype.click = originalClick
    URL.createObjectURL = createObjectURL
    URL.revokeObjectURL = revokeObjectURL
    window.print = originalPrint
  }
})

test('a page without sub-pages offers Markdown and PDF export only', async () => {
  const { view } = setup('/docs/solo', (call) => {
    if (call.path === '/pages') return Response.json({ items: [summary('solo', null, 0, 'Solo')] })
    if (call.path === '/pages/solo' && call.method === 'GET') return Response.json(fullPage('solo', { title: 'Solo' }))
  })
  await view.findByLabelText('Page title')
  fireEvent.click(within(view.container.querySelector('section:last-of-type') as HTMLElement).getAllByRole('button', { name: 'Page options' }).at(-1)!)
  fireEvent.click(await view.findByRole('menuitem', { name: 'Export' }))
  expect(await view.findByRole('menuitem', { name: 'PDF' })).toBeTruthy()
  expect(view.getByRole('menuitem', { name: 'Markdown' })).toBeTruthy()
  expect(view.queryByRole('menuitem', { name: 'Markdown with sub-pages' })).toBeNull()
})
