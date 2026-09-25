import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { createApiClient } from '@/api/client'
import type { PageTaskRecord, TaskRecord } from '@/api/generated/types.gen'
import { queryKeys } from '@/api/queryKeys'
import { patchWorkspaceTask } from '@/features/tasks/api/optimistic'
import { DEFAULT_DISPLAY, PRESET_FILTERS, emptyFilter } from '@/features/views/viewState'
import { fetchAllTaskQueryPages, taskQueryBody, useTaskQuery } from './taskQuery'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

const task = (id: string): TaskRecord => ({
  id, workspace_id: 'workspace-1', project_id: 'project-1', status_id: 'todo', title: id,
  description: '', position: 0, priority: 'none', assignee_ids: [], creator_id: 'user-1', label_ids: [],
  created_at: '2026-09-05T10:00:00Z', updated_at: '2026-09-05T10:00:00Z', duplicate_of: null, blocked: false, version: 1,
})

function withClient(client: QueryClient) {
  return ({ children }: { children: ReactNode }) => <QueryClientProvider client={client}>{children}</QueryClientProvider>
}
const testClient = () => new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })

test('the query body carries the effective filter and the display ordering', () => {
  expect(taskQueryBody(PRESET_FILTERS.mine, { ...DEFAULT_DISPLAY, order_by: 'due_date', order_direction: 'desc', show_completed: 'past_week' })).toEqual({
    filter: PRESET_FILTERS.mine, order_by: 'due_date', order_direction: 'desc', show_completed: 'past_week',
  })
})

test('task queries POST every page until the cursor runs out', async () => {
  const requests: Array<{ method: string; path: string; body: Record<string, unknown> }> = []
  const client = createApiClient({
    fetch: async (request) => {
      const body = await request.json() as Record<string, unknown>
      requests.push({ method: request.method, path: new URL(request.url).pathname, body })
      return Response.json(body.cursor ? { items: [task('task-2')], next_cursor: null } : { items: [task('task-1')], next_cursor: 'next' })
    },
  })
  const body = taskQueryBody(PRESET_FILTERS.overdue, DEFAULT_DISPLAY)

  const page = await fetchAllTaskQueryPages(client, 'workspace-1', body)

  expect(page).toEqual({ items: [task('task-1'), task('task-2')], next_cursor: null })
  expect(requests).toEqual([
    { method: 'POST', path: '/api/v1/workspaces/workspace-1/tasks/query', body: { ...body, limit: 100 } },
    { method: 'POST', path: '/api/v1/workspaces/workspace-1/tasks/query', body: { ...body, cursor: 'next', limit: 100 } },
  ])
})

test('task query keys live under the tasks prefix that task mutations invalidate and patch', async () => {
  const body = taskQueryBody(emptyFilter(), DEFAULT_DISPLAY)
  expect(queryKeys.taskQuery('workspace-1', body).slice(0, 3)).toEqual([...queryKeys.tasks.all('workspace-1')])

  let calls = 0
  globalThis.fetch = (async () => {
    calls += 1
    return Response.json({ items: [task('task-1')], next_cursor: null })
  }) as unknown as typeof fetch
  const client = testClient()
  const view = renderHook(() => useTaskQuery('workspace-1', emptyFilter(), DEFAULT_DISPLAY), { wrapper: withClient(client) })
  await waitFor(() => expect(view.result.current.tasks.map(({ id }) => id)).toEqual(['task-1']))
  expect(view.result.current.isLoading).toBeFalse()

  patchWorkspaceTask(client, 'workspace-1', 'task-1', { title: 'Renamed' })
  expect(client.getQueryData<PageTaskRecord>(queryKeys.taskQuery('workspace-1', body))?.items[0]?.title).toBe('Renamed')

  await client.invalidateQueries({ queryKey: queryKeys.tasks.all('workspace-1') })
  await waitFor(() => expect(calls).toBe(2))
})

test('a disabled task query stays loading without a request', async () => {
  let calls = 0
  globalThis.fetch = (async () => { calls += 1; return Response.json({ items: [], next_cursor: null }) }) as unknown as typeof fetch
  const view = renderHook(() => useTaskQuery('workspace-1', emptyFilter(), DEFAULT_DISPLAY, false), { wrapper: withClient(testClient()) })
  await new Promise((resolve) => setTimeout(resolve, 20))
  expect(view.result.current).toMatchObject({ tasks: [], isLoading: true, error: null })
  expect(calls).toBe(0)
})

test('changing the filter keeps the previous tasks visible while the next result loads', async () => {
  let finish: ((response: Response) => void) | undefined
  let calls = 0
  globalThis.fetch = (async () => {
    calls += 1
    if (calls === 1) return Response.json({ items: [task('task-1')], next_cursor: null })
    return new Promise<Response>((resolve) => { finish = resolve })
  }) as unknown as typeof fetch
  const view = renderHook(({ filter }) => useTaskQuery('workspace-1', filter, DEFAULT_DISPLAY), {
    initialProps: { filter: emptyFilter() }, wrapper: withClient(testClient()),
  })
  await waitFor(() => expect(view.result.current.tasks).toHaveLength(1))

  view.rerender({ filter: PRESET_FILTERS.mine })

  expect(view.result.current.isLoading).toBeFalse()
  expect(view.result.current.tasks.map(({ id }) => id)).toEqual(['task-1'])
  finish?.(Response.json({ items: [], next_cursor: null }))
})
