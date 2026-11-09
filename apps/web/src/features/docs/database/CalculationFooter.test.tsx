import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import type { ReactNode } from 'react'
import { MemoryRouter } from 'react-router'
import type { DatabaseDetail, DatabaseRow } from '@orbit/api/generated/types.gen'
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

const detail: DatabaseDetail = {
  database: { id: 'db', title: 'Budget', icon: null, locked: false, row_count: 3 },
  properties: [
    { id: 'p-title', name: 'Name', type: 'title', config: {}, position: 0 },
    { id: 'p-cost', name: 'Cost', type: 'number', config: { format: 'number_with_commas' }, position: 1 },
  ],
  views: [{ id: 'v1', name: 'Table', type: 'table', config: { calculations: { 'p-title': 'count_all' } }, position: 0 }],
  rows: [row('r1', 0, 'Rent', { 'p-cost': 1000 }), row('r2', 1, 'Food', { 'p-cost': 240 }), row('r3', 2, 'Gift', {})],
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
    <MemoryRouter>
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    </MemoryRouter>
  )
}

test('picking "Sum" in the footer shows the sum and saves the view in one PATCH', async () => {
  const calls: { method: string; path: string; body: unknown }[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    const body: unknown = JSON.parse((await request.text()) || 'null')
    calls.push({ method: request.method, path: new URL(request.url).pathname, body })
    return Response.json({ ...detail.views[0], config: (body as { config: unknown }).config })
  }) as unknown as typeof fetch
  render(
    <DatabaseSurface database={parseDatabase(detail)} views={detail.views} env={env} locked={false} viewKey="test-view" layout={({ content }) => content} />,
    { wrapper },
  )
  expect(screen.getByRole('button', { name: 'Name: Count all 3' })).toBeTruthy()

  fireEvent.click(screen.getByRole('button', { name: 'Calculate Cost' }))
  await userEvent.click(await screen.findByRole('menuitemradio', { name: 'Sum' }))

  expect(await screen.findByRole('button', { name: 'Cost: Sum 1,240' })).toBeTruthy()
  await waitFor(() => expect(calls.length).toBe(1), { timeout: 2000 })
  expect(calls[0].method).toBe('PATCH')
  expect(calls[0].path).toBe('/api/v1/workspaces/ws/databases/db/views/v1')
  expect((calls[0].body as { config: { calculations: unknown } }).config.calculations).toEqual({ 'p-title': 'count_all', 'p-cost': 'sum' })
})
