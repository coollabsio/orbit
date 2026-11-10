import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import type { ReactNode } from 'react'
import type { DatabaseDetail, DatabaseRow, DatabaseView } from '@orbit/api/generated/types.gen'
import { parseDatabase } from '@orbit/database/values'
import { repairConfig } from '@orbit/database/view'
import { render } from '@/test/render'
import { DatabaseEnvContext, type DatabaseEnv } from './context'
import { DatabaseBoard } from './DatabaseBoard'
import { ViewTabs } from './ViewTabs'

const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
})

type Call = { method: string; path: string; body: unknown }

function mockServer(reply: (call: Call) => unknown) {
  const calls: Call[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    const text = await request.text()
    const call = { method: request.method, path: new URL(request.url).pathname, body: text ? JSON.parse(text) : undefined }
    calls.push(call)
    return Response.json(reply(call), { status: call.method === 'POST' && call.path.endsWith('/views') ? 201 : 200 })
  }) as unknown as typeof fetch
  return calls
}

const row = (id: string, position: number, title: string, values: DatabaseRow['values']): DatabaseRow => ({
  id, title, icon: null, position, version: 0, has_content: false,
  created_at: '2026-11-09T10:00:00Z', created_by: 'u1', updated_at: '2026-11-09T10:00:00Z', updated_by: 'u1', values,
})

const board: DatabaseView = {
  id: 'v-board',
  name: 'Board',
  type: 'board',
  config: { group_by: 'p-sel', group_order: ['o-low'], hidden_groups: [null], card_properties: ['p-notes'] },
  position: 1,
}

const detail: DatabaseDetail = {
  database: { id: 'db', title: 'Work', icon: null, locked: false, row_count: 3 },
  properties: [
    { id: 'p-title', name: 'Name', type: 'title', config: {}, position: 0 },
    { id: 'p-sel', name: 'Priority', type: 'select', config: { options: [{ id: 'o-high', name: 'High', color: 'red' }, { id: 'o-low', name: 'Low', color: 'blue' }] }, position: 1 },
    { id: 'p-notes', name: 'Notes', type: 'text', config: {}, position: 2 },
  ],
  views: [{ id: 'v-table', name: 'Table', type: 'table', config: {}, position: 0 }, board],
  rows: [
    row('r1', 0, 'Alpha', { 'p-sel': 'o-high', 'p-notes': 'first note' }),
    row('r2', 1, 'Beta', { 'p-sel': 'o-low' }),
    row('r3', 2, 'Gamma', {}),
  ],
}

const env: DatabaseEnv = {
  workspaceId: 'ws',
  databaseId: 'db',
  ctx: { userId: 'u1', now: 0, timeZone: 'UTC', locale: 'en-US', members: new Map() },
  users: new Map(),
  canEditValues: true,
  canEditSchema: true,
  canEditViews: true,
  linkId: null,
}

function wrapper({ children }: { children: ReactNode }) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  client.setQueryData(['workspaces', 'ws', 'databases', 'db'], detail)
  return (
    <QueryClientProvider client={client}>
      <DatabaseEnvContext.Provider value={env}>{children}</DatabaseEnvContext.Provider>
    </QueryClientProvider>
  )
}

function renderBoard(view: DatabaseView = board) {
  const database = parseDatabase(detail)
  return render(
    <DatabaseBoard
      database={database}
      config={repairConfig(view, database.properties)}
      rows={database.rows}
      onConfig={() => {}}
      onAddCard={() => {}}
      onOpenRow={() => {}}
      filtered={false}
      onClearFilters={() => {}}
      editRowId={null}
      onEditRowShown={() => {}}
    />,
    { wrapper },
  )
}

test('columns follow group_order, hidden groups list at the end, cards show their properties', () => {
  renderBoard()
  const columns = screen.getAllByRole('region').map((region) => region.getAttribute('aria-label'))
  expect(columns).toEqual(['Low', 'High', 'Hidden groups'])
  const high = screen.getByRole('region', { name: 'High' })
  expect(within(high).getByText('Alpha')).toBeTruthy()
  expect(within(high).getByText('first note')).toBeTruthy()
  expect(within(screen.getByRole('region', { name: 'Hidden groups' })).getByText('No Priority')).toBeTruthy()
})

test('a card dropped into another column moves it with its new value in one request', async () => {
  const calls = mockServer(() => row('r1', 1, 'Alpha', { 'p-sel': 'o-low' }))
  renderBoard()
  const card = screen.getByText('Alpha').closest('[data-board-card]')!
  const low = screen.getByRole('region', { name: 'Low' })
  fireEvent.dragStart(card, { dataTransfer: new DataTransfer() })
  fireEvent.dragOver(low, { dataTransfer: new DataTransfer() })
  fireEvent.drop(low, { dataTransfer: new DataTransfer() })
  await waitFor(() => expect(calls.length).toBeGreaterThan(0))
  expect(calls[0]).toEqual({
    method: 'POST',
    path: '/api/v1/workspaces/ws/databases/db/rows/r1/move',
    body: { after_row_id: 'r2', values: { 'p-sel': 'o-low' } },
  })
})

test('a board without a select or status property asks for one', () => {
  const plain = { ...detail, properties: detail.properties.filter((property) => property.type !== 'select') }
  const database = parseDatabase(plain)
  render(
    <DatabaseBoard
      database={database}
      config={repairConfig({ ...board, config: {} }, database.properties)}
      rows={database.rows}
      onConfig={() => {}}
      onAddCard={() => {}}
      onOpenRow={() => {}}
      filtered={false}
      onClearFilters={() => {}}
      editRowId={null}
      onEditRowShown={() => {}}
    />,
    { wrapper },
  )
  expect(screen.getByText('Choose a property to group by')).toBeTruthy()
})

test('"Add a new view" moves with the arrow keys and creates a board with a unique name', async () => {
  const calls = mockServer((call) => ({ ...board, id: 'v-new', name: (call.body as { name: string }).name }))
  const selected: string[] = []
  render(<ViewTabs views={detail.views} activeId="v-table" onSelect={(id) => selected.push(id)} />, { wrapper })
  fireEvent.click(screen.getByRole('button', { name: 'Add a view' }))
  const table = await screen.findByRole('button', { name: 'Table' })
  await waitFor(() => expect(document.activeElement).toBe(table))
  expect(screen.getByRole('button', { name: 'Gallery' }).getAttribute('aria-disabled')).not.toBe('true')
  await userEvent.keyboard('{ArrowRight}{Enter}')
  await waitFor(() => expect(selected).toEqual(['v-new']))
  expect(calls[0]).toEqual({ method: 'POST', path: '/api/v1/workspaces/ws/databases/db/views', body: { name: 'Board 2', type: 'board' } })
})

const people: DatabaseDetail = {
  ...detail,
  properties: [...detail.properties, { id: 'p-owner', name: 'Owner', type: 'person', config: {}, position: 3 }],
  rows: [
    row('r1', 0, 'Alpha', { 'p-sel': 'o-high', 'p-owner': ['u1', 'u2'] }),
    row('r2', 1, 'Beta', { 'p-sel': 'o-low', 'p-owner': ['u3'] }),
    row('r3', 2, 'Gamma', {}),
  ],
}
const members = new Map([
  ['u1', 'Ann'],
  ['u2', 'Bo'],
  ['u3', 'Cy'],
])

function renderPeople(config: DatabaseView['config']) {
  const database = parseDatabase(people)
  const peopleEnv = { ...env, ctx: { ...env.ctx, members } }
  return render(
    <DatabaseEnvContext.Provider value={peopleEnv}>
      <DatabaseBoard
        database={database}
        config={repairConfig({ ...board, config }, database.properties)}
        rows={database.rows}
        onConfig={() => {}}
        onAddCard={() => {}}
        onOpenRow={() => {}}
        filtered={false}
        onClearFilters={() => {}}
        editRowId={null}
        onEditRowShown={() => {}}
      />
    </DatabaseEnvContext.Provider>,
    { wrapper },
  )
}

const drag = (card: Element, target: Element) => {
  fireEvent.dragStart(card, { dataTransfer: new DataTransfer() })
  fireEvent.dragOver(target, { dataTransfer: new DataTransfer() })
  fireEvent.drop(target, { dataTransfer: new DataTransfer() })
}

test('a card with two people shows in both columns; a drag swaps one person and keeps the other', async () => {
  const calls = mockServer(() => row('r1', 0, 'Alpha', { 'p-owner': ['u2', 'u3'] }))
  renderPeople({ group_by: 'p-owner' })
  expect(screen.getAllByRole('region').map((region) => region.getAttribute('aria-label'))).toEqual(['Ann', 'Bo', 'Cy', 'No Owner'])
  expect(within(screen.getByRole('region', { name: 'Ann' })).getByText('Alpha')).toBeTruthy()
  expect(within(screen.getByRole('region', { name: 'Bo' })).getByText('Alpha')).toBeTruthy()
  const card = within(screen.getByRole('region', { name: 'Ann' })).getByText('Alpha').closest('[data-board-card]')!
  drag(card, screen.getByRole('region', { name: 'Cy' }))
  await waitFor(() => expect(calls.length).toBeGreaterThan(0))
  expect(calls[0]).toEqual({
    method: 'POST',
    path: '/api/v1/workspaces/ws/databases/db/rows/r1/move',
    body: { after_row_id: 'r2', values: { 'p-owner': ['u2', 'u3'] } },
  })
})

test('a drop into another lane and column sends one request with both values', async () => {
  const calls = mockServer(() => row('r1', 0, 'Alpha', { 'p-sel': 'o-low', 'p-owner': ['u2', 'u3'] }))
  renderPeople({ group_by: 'p-owner', sub_group_by: 'p-sel', sub_group_order: ['o-high'] })
  expect(screen.getAllByRole('columnheader').map((header) => header.getAttribute('aria-label'))).toEqual(['Ann', 'Bo', 'Cy', 'No Owner'])
  const lanes = screen.getAllByRole('region').filter((region) => region.getAttribute('aria-label')?.includes(',') === false)
  expect(lanes.map((lane) => lane.getAttribute('aria-label'))).toEqual(['High', 'No Priority', 'Low'])
  const card = within(screen.getByRole('region', { name: 'Ann, High' })).getByText('Alpha').closest('[data-board-card]')!
  drag(card, screen.getByRole('region', { name: 'Cy, Low' }))
  await waitFor(() => expect(calls.length).toBeGreaterThan(0))
  expect(calls).toHaveLength(1)
  expect(calls[0]).toEqual({
    method: 'POST',
    path: '/api/v1/workspaces/ws/databases/db/rows/r1/move',
    body: { after_row_id: 'r2', values: { 'p-owner': ['u2', 'u3'], 'p-sel': 'o-low' } },
  })
})

test('a collapsed lane hides its cards and stays collapsed', () => {
  localStorage.clear()
  const first = renderPeople({ group_by: 'p-owner', sub_group_by: 'p-sel' })
  expect(screen.getByRole('region', { name: 'Cy, Low' })).toBeTruthy()
  fireEvent.click(screen.getByRole('button', { name: 'Collapse Low' }))
  expect(screen.queryByRole('region', { name: 'Cy, Low' })).toBeNull()
  first.unmount()
  renderPeople({ group_by: 'p-owner', sub_group_by: 'p-sel' })
  expect(screen.getByRole('button', { name: 'Expand Low' })).toBeTruthy()
  localStorage.clear()
})
