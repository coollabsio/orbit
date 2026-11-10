import { afterEach, expect, setSystemTime, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { usePressButton } from './buttonApi'

const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
  setSystemTime()
})

test('a press sends one POST with the client day and nothing else', async () => {
  setSystemTime(new Date(2026, 10, 10, 12))
  const calls: { method: string; path: string; body: unknown }[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    calls.push({ method: request.method, path: new URL(request.url).pathname, body: JSON.parse((await request.text()) || 'null') })
    return Response.json({
      row: { id: 'r1', title: 'Task', icon: null, position: 0, version: 1, has_content: false, created_at: '2026-11-10T12:00:00Z', created_by: 'u1', updated_at: '2026-11-10T12:00:00Z', updated_by: 'u1', values: {} },
      created_rows: [],
    })
  }) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const wrapper = ({ children }: { children: ReactNode }) => <QueryClientProvider client={client}>{children}</QueryClientProvider>
  const view = renderHook(() => usePressButton('ws', 'db'), { wrapper })

  act(() => view.result.current.press({ rowId: 'r1', propertyId: 'p-btn' }, false))

  const sent = [{ method: 'POST', path: '/api/v1/workspaces/ws/databases/db/rows/r1/press/p-btn', body: { today: '2026-11-10' } }]
  await waitFor(() => expect(calls).toEqual(sent))
  await waitFor(() => expect(view.result.current.pending).toBe(false))
  expect(calls).toEqual(sent)
})
