import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import type { ReactNode } from 'react'
import { MemoryRouter } from 'react-router'
import type { DatabaseDetail, DatabaseRow, DatabaseView } from '@orbit/api/generated/types.gen'
import { parseDatabase } from '@orbit/database/values'
import { render } from '@/test/render'
import type { DatabaseEnv } from './context'
import { DatabaseSurface } from './DatabaseSurface'

const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
})

const row = (id: string, position: number, title: string, values: DatabaseRow['values']): DatabaseRow => ({
  id, title, icon: null, position, version: 0, has_content: false,
  created_at: '2026-11-09T10:00:00Z', created_by: 'u1', updated_at: '2026-11-09T10:00:00Z', updated_by: 'u1', values,
})

const high = { property_id: 'p-sel', operator: 'is' as const, value: ['o-high'] }
const notes = { property_id: 'p-notes', operator: 'contains' as const, value: 'beta' }

function detail(config: DatabaseView['config']): DatabaseDetail {
  return {
    database: { id: 'db', title: 'Work', icon: null, locked: false, row_count: 3 },
    properties: [
      { id: 'p-title', name: 'Name', type: 'title', config: {}, position: 0 },
      { id: 'p-sel', name: 'Priority', type: 'select', config: { options: [{ id: 'o-high', name: 'High', color: 'red' }, { id: 'o-low', name: 'Low', color: 'blue' }] }, position: 1 },
      { id: 'p-notes', name: 'Notes', type: 'text', config: {}, position: 2 },
    ],
    views: [{ id: 'v1', name: 'Table', type: 'table', config, position: 0 }],
    rows: [row('r1', 0, 'Alpha', { 'p-sel': 'o-high' }), row('r2', 1, 'Beta', { 'p-sel': 'o-low', 'p-notes': 'beta notes' }), row('r3', 2, 'Gamma', {})],
  }
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

function renderSurface(config: DatabaseView['config']) {
  const data = detail(config)
  const calls: { method: string; body: { config: DatabaseView['config'] } }[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    const body = JSON.parse((await request.text()) || 'null')
    calls.push({ method: request.method, body })
    return Response.json({ ...data.views[0], config: body.config })
  }) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  client.setQueryData(['workspaces', 'ws', 'databases', 'db'], data)
  const wrapper = ({ children }: { children: ReactNode }) => (
    <MemoryRouter>
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    </MemoryRouter>
  )
  render(
    <DatabaseSurface
      database={parseDatabase(data)}
      views={data.views}
      env={env}
      locked={false}
      viewKey="test-view"
      layout={({ controls, content }) => (
        <>
          {controls}
          {content}
        </>
      )}
    />,
    { wrapper },
  )
  return calls
}

const rowTitles = () => screen.getAllByRole('row').flatMap((row) => (row.getAttribute('aria-rowindex') && row.getAttribute('aria-rowindex') !== '1' ? [row.textContent ?? ''] : []))

test('a filter with an or group shows one chip, applies the groups and edits them in the advanced filter', async () => {
  const calls = renderSurface({ filter: { op: 'or', children: [high, { op: 'and', children: [notes] }] } })
  expect(rowTitles().map((text) => text.slice(0, 4))).toEqual(['Alph', 'Beta'])
  expect(screen.queryAllByRole('button', { name: /Priority filter/ }).length).toBe(0)

  fireEvent.click(screen.getByRole('button', { name: 'Advanced filter: 2 rules' }))
  const groups = await screen.findAllByRole('group', { name: 'Filter group' })
  expect(groups.length).toBe(1)
  expect(within(groups[0]).getAllByRole('group', { name: 'Filter rule' }).length).toBe(1)

  // The nested group is at level 2: it can hold a group, which cannot.
  await userEvent.click(within(groups[0]).getByRole('button', { name: 'Add filter group' }))
  const nested = screen.getAllByRole('group', { name: 'Filter group' })
  expect(nested.length).toBe(2)
  expect(within(nested[1]).queryAllByRole('button', { name: 'Add filter group' }).length).toBe(0)

  await waitFor(() => expect(calls.length).toBe(1), { timeout: 2000 })
  expect(calls[0].body.config.filter).toEqual({
    op: 'or',
    children: [high, { op: 'and', children: [notes, { op: 'or', children: [{ property_id: 'p-title', operator: 'contains', value: null }] }] }],
  })
})

test('a simple filter keeps its chips; "Add advanced filter" opens the group editor', async () => {
  renderSurface({ filter: { op: 'and', children: [high] } })
  expect(screen.getByRole('button', { name: 'Priority filter: High' })).toBeTruthy()
  expect(rowTitles().length).toBe(1)
  fireEvent.click(screen.getByRole('button', { name: 'Add filter' }))
  await userEvent.click(await screen.findByRole('option', { name: 'Add advanced filter' }))
  expect(await screen.findByRole('button', { name: 'Advanced filter: 1 rule' })).toBeTruthy()
  expect(screen.getAllByRole('group', { name: 'Filter rule' }).length).toBe(1)
})
