import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import type { DatabaseDetail, DatabaseRow } from '@orbit/api/generated/types.gen'
import { cellOf, displayText } from '@orbit/database/values'
import { useDatabase, useUpdateValues } from './api'

const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
})

const ctx = { userId: 'u1', now: 0, timeZone: 'UTC', locale: 'en-US', members: new Map<string, string>() }

const row = (values: DatabaseRow['values'], updated_by = 'u1'): DatabaseRow => ({
  id: 'r1', title: 'Paper A', icon: null, position: 0, version: 3, has_content: false,
  created_at: '2026-11-09T10:00:00Z', created_by: 'u1', updated_at: '2026-11-09T10:00:00Z', updated_by, values,
})

const detail = (values: DatabaseRow['values']): DatabaseDetail => ({
  database: { id: 'db', title: 'Research', icon: null, locked: false, row_count: 1 },
  properties: [
    { id: 'p-title', name: 'Name', type: 'title', config: {}, position: 0 },
    { id: 'p-score', name: 'Score', type: 'number', config: { format: 'percent' }, position: 1 },
  ],
  views: [{ id: 'v1', name: 'Table', type: 'table', config: {}, position: 0 }],
  rows: [row(values)],
})

type Call = { method: string; path: string; body: unknown }

function mockServer(patch: (body: Record<string, unknown>) => Response) {
  const calls: Call[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    const text = await request.text()
    const call = { method: request.method, path: new URL(request.url).pathname, body: text ? JSON.parse(text) : undefined }
    calls.push(call)
    return call.method === 'PATCH' ? patch(call.body as Record<string, unknown>) : Response.json(detail({ 'p-score': 0.1 }))
  }) as unknown as typeof fetch
  return calls
}

function renderDatabase() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const wrapper = ({ children }: { children: ReactNode }) => <QueryClientProvider client={client}>{children}</QueryClientProvider>
  return renderHook(() => ({ database: useDatabase('ws', 'db').data, update: useUpdateValues('ws', 'db') }), { wrapper })
}

function shownScore(view: ReturnType<typeof renderDatabase>) {
  const database = view.result.current.database!
  const score = database.properties.find((property) => property.id === 'p-score')!
  return displayText(score, cellOf(database.rows[0], score), ctx)
}

test('a cell edit sends one values PATCH and shows the saved value', async () => {
  const calls = mockServer((body) => Response.json(row(body, 'u2')))
  const view = renderDatabase()
  await waitFor(() => expect(view.result.current.database).toBeDefined())
  expect(shownScore(view)).toBe('10%')

  await act(async () => {
    await view.result.current.update.mutateAsync({ rowId: 'r1', values: { 'p-score': 0.5 } })
  })

  expect(calls.filter((call) => call.method === 'PATCH')).toEqual([
    { method: 'PATCH', path: '/api/v1/workspaces/ws/databases/db/rows/r1/values', body: { 'p-score': 0.5 } },
  ])
  await waitFor(() => expect(shownScore(view)).toBe('50%'))
  expect(view.result.current.database!.rows[0].updated_by).toBe('u2')
})

test('a refused cell edit goes back to the stored value', async () => {
  mockServer(() =>
    Response.json(
      { type: 'about:blank', title: 'Invalid', status: 422, code: 'validation_failed', detail: 'values.p-score', instance: '/', request_id: 'r' },
      { status: 422, headers: { 'content-type': 'application/problem+json' } },
    ),
  )
  const view = renderDatabase()
  await waitFor(() => expect(view.result.current.database).toBeDefined())

  await act(async () => {
    await view.result.current.update.mutateAsync({ rowId: 'r1', values: { 'p-score': 0.5 } }).catch(() => undefined)
  })

  await waitFor(() => expect(shownScore(view)).toBe('10%'))
})
