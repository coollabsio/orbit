import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, Route, Routes, useLocation } from 'react-router'
import type { TrelloImport, TrelloImportTree } from '@orbit/api/generated/types.gen'
import { queryKeys } from '@/api/queryKeys'
import { ConfirmationModalHost } from '@/components/common/ConfirmationModal'
import { WorkspaceProvider } from '@/features/workspaces/WorkspaceProvider'
import { testWorkspace } from '@/test/workspace'
import { TrelloImportPage } from './TrelloImportPage'

const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
})

const TREE: TrelloImportTree = {
  workspaces: [
    {
      id: 'ws-acme',
      name: 'Acme',
      boards: [
        { id: 'board-road', name: 'Roadmap', card_count: 12, archived: false },
        { id: 'board-old', name: 'Old plans', card_count: 4, archived: true },
      ],
    },
    { id: null, name: 'Personal', boards: [{ id: 'board-home', name: 'Home', card_count: null, archived: false }] },
  ],
  truncated: false,
}

const item = (patch: Partial<TrelloImport> = {}): TrelloImport => ({
  id: 'import-1', workspace_id: 'workspace-1', status: 'ready', trello_user_name: 'Ada', error: null,
  progress: { total: 0, done: 0, failed: 0 }, tree: null, report: null, project_ids: [],
  created_at: '2026-11-04T10:00:00Z', updated_at: '2026-11-04T10:00:00Z', ...patch,
})

const problem = (status: number, code: string, detail = code) =>
  Response.json(
    { type: 'about:blank', title: 'Problem', status, code, detail, instance: '/imports', request_id: 'request-1' },
    { status, headers: { 'content-type': 'application/problem+json' } },
  )

type Call = { method: string; path: string; search: string; body: Record<string, unknown> | undefined }

function Location() {
  return <output data-testid="location">{useLocation().pathname}</output>
}

function setup(path: string, handler: (call: Call) => Response | undefined) {
  window.localStorage.clear()
  const calls: Call[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    const url = new URL(request.url)
    const text = await request.text()
    const call: Call = {
      method: request.method,
      path: url.pathname.replace('/api/v1/workspaces/workspace-1', ''),
      search: url.search,
      body: text ? JSON.parse(text) : undefined,
    }
    calls.push(call)
    const response = handler(call)
    if (response) return response
    if (call.method === 'GET' && call.path === '/imports/trello') return Response.json({ items: [] })
    if (call.method === 'GET' && call.path === '/projects') return Response.json({ items: [], next_cursor: null })
    return problem(404, 'not_found')
  }) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity }, mutations: { retry: false } } })
  client.setQueryData(queryKeys.workspaces, [testWorkspace('member', { id: 'workspace-1' })])
  const view = render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[`${path}?workspace=workspace-1`]}>
        <ConfirmationModalHost />
        <WorkspaceProvider>
          <Routes>
            <Route path="tasks-import/trello/:importId?" element={<TrelloImportPage />} />
          </Routes>
        </WorkspaceProvider>
        <Location />
      </MemoryRouter>
    </QueryClientProvider>,
  )
  return { view, calls }
}

const KEY_DETAIL = 'Trello rejected this API key (invalid key). Copy the key again from the Trello Auth tab of your Power-Up.'
const TOKEN_DETAIL = 'Trello rejected this token (expired token). Generate a new token for this API key and paste it.'

test('connect: key and token are password fields, the token link carries the key, and each rejection shows under its field', async () => {
  const { view, calls } = setup('/tasks-import/trello', (call) => {
    if (call.method === 'POST' && call.path === '/imports/trello') {
      if (call.body?.key !== 'goodkey') return problem(422, 'trello_key_invalid', KEY_DETAIL)
      if (call.body?.token !== 'goodtoken') return problem(422, 'trello_token_invalid', TOKEN_DETAIL)
      return Response.json(item({ status: 'scanning' }), { status: 201 })
    }
    if (call.path === '/imports/trello/import-1') return Response.json(item({ status: 'scanning' }))
  })
  const key = (await view.findByLabelText('API key')) as HTMLInputElement
  const token = view.getByLabelText('Token') as HTMLInputElement
  expect([key.type, token.type]).toEqual(['password', 'password'])
  expect(view.getByRole('link', { name: 'Trello Power-Up admin page' }).getAttribute('href')).toBe('https://trello.com/apps/admin')
  expect(view.queryByRole('link', { name: 'Generate a token' })).toBeNull()

  await userEvent.type(key, 'bad key')
  const tokenLink = view.getByRole('link', { name: 'Generate a token' })
  expect(tokenLink.getAttribute('href')).toBe(
    'https://trello.com/1/authorize?expiration=1day&scope=read&response_type=token&name=Orbit&key=bad%20key',
  )
  expect(tokenLink.getAttribute('rel')).toBe('noreferrer noopener')
  await userEvent.type(token, 'goodtoken')
  fireEvent.click(view.getByRole('button', { name: 'Find boards' }))
  expect((await view.findByRole('alert')).textContent).toBe(KEY_DETAIL)
  expect(key.getAttribute('aria-describedby')).toBe('trello-key-error')
  expect(view.getByRole('alert').id).toBe('trello-key-error')
  expect(token.getAttribute('aria-invalid')).toBeNull()

  await userEvent.clear(key)
  expect(view.queryByRole('alert')).toBeNull()
  await userEvent.type(key, 'goodkey')
  await userEvent.clear(token)
  await userEvent.type(token, 'oldtoken')
  fireEvent.click(view.getByRole('button', { name: 'Find boards' }))
  expect((await view.findByRole('alert')).textContent).toBe(TOKEN_DETAIL)
  expect(token.getAttribute('aria-describedby')).toBe('trello-token-error')
  expect(key.getAttribute('aria-invalid')).toBeNull()

  await userEvent.clear(token)
  await userEvent.type(token, 'goodtoken')
  fireEvent.click(view.getByRole('button', { name: 'Find boards' }))
  await waitFor(() => expect(view.getByTestId('location').textContent).toBe('/tasks-import/trello/import-1'))
  expect(await view.findByText('Finding your Trello boards…')).toBeTruthy()
  expect(calls.filter((call) => call.method === 'POST').map((call) => call.body)).toEqual([
    { key: 'bad key', token: 'goodtoken' },
    { key: 'goodkey', token: 'oldtoken' },
    { key: 'goodkey', token: 'goodtoken' },
  ])
})

test('connect: an error that concerns neither field shows below the form', async () => {
  const { view } = setup('/tasks-import/trello', (call) => {
    if (call.method === 'POST' && call.path === '/imports/trello') return problem(502, 'trello_unavailable')
  })
  await userEvent.type(await view.findByLabelText('API key'), 'key1')
  await userEvent.type(view.getByLabelText('Token'), 'token1')
  fireEvent.click(view.getByRole('button', { name: 'Find boards' }))
  expect((await view.findByRole('alert')).textContent?.trim()).toBe('Trello could not be reached. Try again in a moment.')
  expect(view.getByLabelText('API key').getAttribute('aria-invalid')).toBeNull()
  expect(view.getByLabelText('Token').getAttribute('aria-invalid')).toBeNull()
})

test('choose: boards are grouped by Workspace, archived boards start unselected, and start sends the selection', async () => {
  const { view, calls } = setup('/tasks-import/trello/import-1', (call) => {
    if (call.method === 'GET' && call.path === '/imports/trello/import-1') {
      return Response.json(item({ tree: call.search === '?include=tree' ? TREE : null }))
    }
    if (call.method === 'POST' && call.path === '/imports/trello/import-1/start') return Response.json(item({ status: 'queued' }))
  })
  expect(await view.findByRole('button', { name: 'Import 2 boards' })).toBeTruthy()
  expect(view.getByRole('region', { name: 'Acme' })).toBeTruthy()
  expect(view.getByRole('region', { name: 'Other boards' })).toBeTruthy()
  expect(view.queryByText('Personal')).toBeNull()
  expect(view.getByText('12 cards')).toBeTruthy()
  expect(view.getByText('Archived')).toBeTruthy()
  expect(view.getByRole('checkbox', { name: 'Select Old plans' }).getAttribute('aria-checked')).toBe('false')
  expect(view.getByRole('checkbox', { name: 'Select all boards' }).getAttribute('aria-checked')).toBe('mixed')

  fireEvent.click(view.getByRole('button', { name: 'Home' }))
  expect(await view.findByRole('button', { name: 'Import 1 board' })).toBeTruthy()

  fireEvent.click(view.getByRole('button', { name: 'Select all' }))
  fireEvent.click(await view.findByRole('button', { name: 'Import 3 boards' }))
  await waitFor(() => expect(calls.some((call) => call.path === '/imports/trello/import-1/start')).toBe(true))
  const start = calls.find((call) => call.path === '/imports/trello/import-1/start')
  const ids = (start?.body?.board_ids ?? []) as string[]
  expect([...ids].sort()).toEqual(['board-home', 'board-old', 'board-road'])
})

test('result: shows counts, notes, imported projects and failures', async () => {
  const done = item({
    status: 'completed',
    progress: { total: 5, done: 4, failed: 1 },
    project_ids: ['project-1'],
    report: {
      comments_imported: 7, files_imported: 2, missing_files: 1, refused_files: 3, links_added: 1, checklists_added: 2,
      unmatched_members: 0, archived_lists_skipped: 1, archived_cards_skipped: 5,
      failures: [{ trello_id: 'card-9', kind: 'card', title: 'Broken card', error: 'Orbit refused the task.' }],
    },
  })
  const { view } = setup('/tasks-import/trello/import-1', (call) => {
    if (call.path === '/imports/trello/import-1') return Response.json(done)
    if (call.path === '/projects') {
      return Response.json({
        items: [{ id: 'project-1', workspace_id: 'workspace-1', name: 'Website Launch', key: 'WL', color: '#5e6ad2', auto_close_parent: true,
          auto_close_sub_issues: true, version: 0, deleted_at: null, created_at: done.created_at, updated_at: done.created_at }],
        next_cursor: null,
      })
    }
  })
  expect(await view.findByText('Import complete')).toBeTruthy()
  expect((await view.findByRole('link', { name: 'Website Launch' })).getAttribute('href')).toBe('/tasks?project=project-1')
  expect(view.getByText('2 checklists were written into card descriptions.')).toBeTruthy()
  expect(view.getByText('1 link attachment was added to card descriptions.')).toBeTruthy()
  expect(view.getByText('1 archived list and its cards were left out.')).toBeTruthy()
  expect(view.getByText('5 archived cards were left out.')).toBeTruthy()
  expect(view.getByText('Trello refused 3 file downloads; some Enterprise workspaces block them.')).toBeTruthy()
  expect(view.getByText('1 Trello file could not be downloaded or attached (too large, or gone).')).toBeTruthy()
  expect(view.queryByText(/no Orbit member of the same full name/)).toBeNull()
  expect(view.getByText('Broken card')).toBeTruthy()
  expect(view.getByText('Orbit refused the task.')).toBeTruthy()
})
