import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { queryKeys } from '@/api/queryKeys'
import type { PageTaskRecord, TaskRecord, TaskRelationRecord } from '@/api/generated/types.gen'
import { useAddTaskRelation, useBulkTasks, useCreateTaskComment, useRemoveTaskRelation, useTaskRelations, useTasks, useUploadTaskAttachments } from './tasks'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

function wrapper({ children }: { children: ReactNode }) {
  return <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })}>{children}</QueryClientProvider>
}

function withClient(client: QueryClient) {
  return ({ children }: { children: ReactNode }) => <QueryClientProvider client={client}>{children}</QueryClientProvider>
}

const task = (id: string, position: number): TaskRecord => ({
  id, workspace_id: 'workspace-1', project_id: 'project-1', status_id: 'todo', title: id,
  description: '', position, priority: 'none', assignee_ids: [], creator_id: 'user-1', label_ids: [],
  created_at: '2026-09-05T10:00:00Z', updated_at: '2026-09-05T10:00:00Z', duplicate_of: null, blocked: false, version: 1,
})

const attachment = {
  id: 'attachment-1', workspace_id: 'workspace-1', task_id: 'task-1', owner_id: 'user-1',
  display_name: 'file.txt', media_type: 'text/plain', byte_size: 1, created_at: '2026-09-05T10:00:00Z',
}

const failure = () => Response.json({
  type: 'about:blank', title: 'Upload failed', status: 500, detail: 'offline', code: 'upload_failed',
  instance: '/attachments', request_id: 'request-1',
}, { status: 500, headers: { 'content-type': 'application/problem+json' } })

test('changing search keeps the task list mounted while the next result loads', async () => {
  let calls = 0
  let finishSearch: ((response: Response) => void) | undefined
  globalThis.fetch = (async () => {
    calls += 1
    if (calls === 1) return Response.json({ items: [task('task-1', 0)], next_cursor: null })
    return new Promise<Response>((resolve) => { finishSearch = resolve })
  }) as unknown as typeof fetch
  const view = renderHook(({ search }) => useTasks('workspace-1', { search }), {
    initialProps: { search: '' }, wrapper,
  })
  await waitFor(() => expect(view.result.current.isSuccess).toBeTrue())

  view.rerender({ search: 'r' })

  expect(view.result.current.isPending).toBeFalse()
  expect(view.result.current.data?.pages[0]?.items[0]?.id).toBe('task-1')
  finishSearch?.(Response.json({ items: [], next_cursor: null }))
})

test('task attachment hook reconciles partial success and retries only remaining files', async () => {
  let calls = 0
  globalThis.fetch = (async () => {
    calls += 1
    return calls === 2 ? failure() : Response.json(attachment, { status: 201 })
  }) as unknown as typeof fetch
  const view = renderHook(() => useUploadTaskAttachments('workspace-1', 'task-1'), { wrapper })
  const files = ['one', 'two', 'three'].map((name) => new File([name], `${name}.txt`))

  await act(async () => { await view.result.current.mutateAsync(files).catch(() => undefined) })
  expect(view.result.current.remainingCount).toBe(2)
  await act(async () => { view.result.current.retry() })
  await waitFor(() => expect(calls).toBe(4))

  expect(view.result.current.remainingCount).toBe(0)
  expect(view.result.current.progress).toBe(100)
})

test('comment attachment retry resumes the created comment instead of duplicating it', async () => {
  let calls = 0
  let commentCreates = 0
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    calls += 1
    if (request.url.endsWith('/comments')) {
      commentCreates += 1
      return Response.json({
        id: 'comment-1', workspace_id: 'workspace-1', task_id: 'task-1', author_id: 'user-1',
        body: 'Persist once', created_at: '2026-09-05T10:00:00Z', updated_at: '2026-09-05T10:00:00Z', version: 0,
      }, { status: 201 })
    }
    return calls === 3 ? failure() : Response.json(attachment, { status: 201 })
  }) as unknown as typeof fetch
  const view = renderHook(() => useCreateTaskComment('workspace-1', 'task-1'), { wrapper })
  const input = { body: 'Persist once', files: [new File(['a'], 'a.txt'), new File(['b'], 'b.txt')] }

  await act(async () => { await view.result.current.mutateAsync(input).catch(() => undefined) })
  expect(view.result.current.remainingCount).toBe(1)
  await act(async () => { await view.result.current.mutateAsync(input) })

  expect(commentCreates).toBe(1)
  expect(calls).toBe(4)
  expect(view.result.current.progress).toBe(100)
})

test('oversized bulk operation makes zero requests and leaves every cache unchanged', async () => {
  let requests = 0
  globalThis.fetch = (async () => {
    requests += 1
    return Response.json({ items: [], next_cursor: null })
  }) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const records = Array.from({ length: 101 }, (_, index) => task(`task-${index}`, index))
  const listKey = queryKeys.tasks.list('workspace-1')
  const detailKey = queryKeys.tasks.detail('workspace-1', records[0]!.id)
  client.setQueryData<PageTaskRecord>(listKey, { items: records, next_cursor: null })
  client.setQueryData(detailKey, records[0])
  const view = renderHook(() => useBulkTasks('workspace-1'), { wrapper: withClient(client) })
  const updates = records.map((record) => ({
    id: record.id, expected_version: record.version, priority: 'urgent',
  }))

  await act(async () => { await view.result.current.mutateAsync(updates).catch(() => undefined) })
  await waitFor(() => expect(view.result.current.isError).toBeTrue())

  expect(requests).toBe(0)
  expect(client.getQueryData<PageTaskRecord>(listKey)?.items).toEqual(records)
  expect(client.getQueryData<TaskRecord>(detailKey)).toEqual(records[0])
  expect(view.result.current.error?.message).toContain('at most 100')
})

test('retry repeats one valid atomic bulk payload unchanged', async () => {
  const bodies: unknown[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    bodies.push(await (input as Request).json())
    return bodies.length === 1 ? failure() : Response.json({ items: [], next_cursor: null })
  }) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const view = renderHook(() => useBulkTasks('workspace-1'), { wrapper: withClient(client) })
  const updates = [{ id: 'task-1', expected_version: 1, priority: 'urgent' }]

  await act(async () => { await view.result.current.mutateAsync(updates).catch(() => undefined) })
  await waitFor(() => expect(view.result.current.isError).toBeTrue())
  await act(async () => { view.result.current.retry() })
  await waitFor(() => expect(view.result.current.isSuccess).toBeTrue())

  expect(bodies).toEqual([{ updates }, { updates }])
})

test('rejected bulk hook request rolls back list and detail caches', async () => {
  let rejectRequest: ((response: Response) => void) | undefined
  let requestBody: unknown
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    requestBody = await request.json()
    return new Promise<Response>((resolve) => { rejectRequest = resolve })
  }) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const listKey = queryKeys.tasks.list('workspace-1')
  const detailKey = queryKeys.tasks.detail('workspace-1', 'task-1')
  const original = task('task-1', 0)
  client.setQueryData<PageTaskRecord>(listKey, { items: [original], next_cursor: null })
  client.setQueryData(detailKey, original)
  const view = renderHook(() => useBulkTasks('workspace-1'), { wrapper: withClient(client) })
  let mutation = Promise.resolve()
  act(() => {
    mutation = view.result.current.mutateAsync([{ id: 'task-1', expected_version: 1, position: 9 }]).then(() => undefined, () => undefined)
  })

  await waitFor(() => expect(requestBody).toEqual({ updates: [{ id: 'task-1', expected_version: 1, position: 9 }] }))
  expect(client.getQueryData<PageTaskRecord>(listKey)?.items[0]?.position).toBe(9)
  expect(client.getQueryData<TaskRecord>(detailKey)?.position).toBe(9)
  await act(async () => {
    rejectRequest?.(failure())
    await mutation
  })
  await waitFor(() => expect(view.result.current.isError).toBeTrue())

  expect(client.getQueryData<PageTaskRecord>(listKey)?.items[0]).toEqual(original)
  expect(client.getQueryData<TaskRecord>(detailKey)).toEqual(original)
})

test('task relations load, add and remove through the relation endpoints and invalidate task queries', async () => {
  const calls: Array<{ method: string; path: string; body?: unknown }> = []
  const relation: TaskRelationRecord = {
    id: 'rel-1', type: 'blocks', direction: 'incoming', created_at: '2026-09-23T10:00:00Z',
    task: { id: 'task-2', project_id: 'project-1', title: 'Auth token refresh', status_id: 'todo' },
  }
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    calls.push({ method: request.method, path: new URL(request.url).pathname, body: request.method === 'POST' ? await request.json() : undefined })
    if (request.method === 'DELETE') return new Response(null, { status: 204 })
    return request.method === 'POST' ? Response.json(relation, { status: 201 }) : Response.json([relation])
  }) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const listKey = queryKeys.tasks.list('workspace-1')
  client.setQueryData(listKey, { pages: [], pageParams: [] })
  const view = renderHook(() => ({
    relations: useTaskRelations('workspace-1', 'task-1'),
    add: useAddTaskRelation('workspace-1', 'task-1'),
    remove: useRemoveTaskRelation('workspace-1', 'task-1'),
  }), { wrapper: withClient(client) })

  await waitFor(() => expect(view.result.current.relations.data).toEqual([relation]))
  expect(calls[0]).toEqual({ method: 'GET', path: '/api/v1/workspaces/workspace-1/tasks/task-1/relations', body: undefined })
  await act(async () => { await view.result.current.add.mutateAsync({ type: 'blocked_by', task_id: 'task-2' }) })
  expect(client.getQueryState(listKey)?.isInvalidated).toBeTrue()
  await act(async () => { await view.result.current.remove.mutateAsync('rel-1') })

  expect(calls.filter((call) => call.method !== 'GET')).toEqual([
    { method: 'POST', path: '/api/v1/workspaces/workspace-1/tasks/task-1/relations', body: { type: 'blocked_by', task_id: 'task-2' } },
    { method: 'DELETE', path: '/api/v1/workspaces/workspace-1/tasks/task-1/relations/rel-1', body: undefined },
  ])
})
