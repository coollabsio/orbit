import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { useCreateTaskComment, useUploadTaskAttachments } from './tasks'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

function wrapper({ children }: { children: ReactNode }) {
  return <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })}>{children}</QueryClientProvider>
}

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
