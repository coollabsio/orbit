import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { queryKeys } from '../../../api/queryKeys'
import type { PageTaskRecord, TaskRecord } from '../../../api/generated/types.gen'
import { useBulkTasks, useCreateTaskComment, useUploadTaskAttachments } from './tasks'

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
  created_at: '2026-09-05T10:00:00Z', updated_at: '2026-09-05T10:00:00Z', version: 1,
})

const attachment = {
  id: 'attachment-1', workspace_id: 'workspace-1', task_id: 'task-1', owner_id: 'user-1',
  display_name: 'file.txt', media_type: 'text/plain', byte_size: 1, created_at: '2026-09-05T10:00:00Z',
}

const failure = () => Response.json({
  type: 'about:blank', title: 'Upload failed', status: 500, detail: 'offline', code: 'upload_failed',
  instance: '/attachments', request_id: 'request-1',
}, { status: 500, headers: { 'content-type': 'application/problem+json' } })

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

test('bulk hook retries only the failed and not-yet-sent board batches', async () => {
  const requestIds: string[][] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    const body = await request.json() as { updates: Array<{ id: string }> }
    requestIds.push(body.updates.map((update) => update.id))
    if (requestIds.length === 2) return failure()
    return Response.json({ items: [], next_cursor: null })
  }) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const view = renderHook(() => useBulkTasks('workspace-1'), { wrapper: withClient(client) })
  const updates = Array.from({ length: 205 }, (_, index) => ({
    id: `task-${index}`, expected_version: 1, position: index,
  }))

  await act(async () => { await view.result.current.mutateAsync(updates).catch(() => undefined) })
  await waitFor(() => expect(view.result.current.isError).toBeTrue())
  await act(async () => { view.result.current.retry() })
  await waitFor(() => expect(view.result.current.isSuccess).toBeTrue())

  expect(requestIds.map((ids) => ids.length)).toEqual([100, 100, 100, 5])
  expect(requestIds[2]?.[0]).toBe('task-100')
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
