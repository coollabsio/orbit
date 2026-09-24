import { afterEach, expect, spyOn, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { toast } from 'sonner'
import type { TaskRecord } from '@/api/generated/types.gen'
import { useDuplicateActions } from './useDuplicateActions'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

const record = (id: string, version: number, duplicateOf: string | null): TaskRecord => ({
  id, workspace_id: 'workspace-1', project_id: 'project-1', status_id: duplicateOf ? 'dup' : 'todo', title: id,
  description: '', position: 0, priority: 'none', assignee_ids: [], creator_id: 'user-1', label_ids: [],
  created_at: '2026-09-23T10:00:00Z', updated_at: '2026-09-23T10:00:00Z', version,
  duplicate_of: duplicateOf ? { id: duplicateOf, project_id: 'project-1', title: 'Canonical' } : null, blocked: false,
})

type Call = { method: string; path: string; body: Record<string, unknown> }
type ToastOptions = { action: { label: string; onClick: () => void } }

function captureApi(calls: Call[], respond?: (call: Call) => Response) {
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    const call = { method: request.method, path: new URL(request.url).pathname, body: await request.json() as Record<string, unknown> }
    calls.push(call)
    if (respond) return respond(call)
    if (call.path.endsWith('/tasks/bulk')) {
      const updates = call.body.updates as Array<{ id: string; expected_version: number; duplicate_of_id: string | null }>
      return Response.json({ items: updates.map((u) => record(u.id, u.expected_version + 1, u.duplicate_of_id)), next_cursor: null })
    }
    const id = call.path.split('/').at(-1)!
    return Response.json(record(id, (call.body.expected_version as number) + 1, (call.body.duplicate_of_id as string | null) ?? null))
  }) as unknown as typeof fetch
}

function renderActions() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const wrapper = ({ children }: { children: ReactNode }) => <QueryClientProvider client={client}>{children}</QueryClientProvider>
  return renderHook(() => useDuplicateActions('workspace-1'), { wrapper })
}

const target = { id: 'task-91c0', identifier: 'ORB-91C0' }

test('marking one task shows an Undo toast that still unmarks after the view unmounted', async () => {
  const calls: Call[] = []
  captureApi(calls)
  const success = spyOn(toast, 'success').mockImplementation(() => 0)
  const view = renderActions()
  await act(async () => { await view.result.current.markOne({ id: 'task-3f2a', version: 1 }, target) })

  expect(calls[0]).toEqual({ method: 'PATCH', path: '/api/v1/workspaces/workspace-1/tasks/task-3f2a', body: { expected_version: 1, duplicate_of_id: 'task-91c0' } })
  expect(success.mock.calls[0]![0]).toBe('Marked as duplicate of ORB-91C0')
  const options = success.mock.calls[0]![1] as unknown as ToastOptions
  expect(options.action.label).toBe('Undo')

  // the row re-mounts in the Duplicate group / the detail page closes before Undo is clicked
  view.unmount()
  options.action.onClick()
  await waitFor(() => expect(calls).toHaveLength(2))
  expect(calls[1]!.body).toEqual({ expected_version: 2, duplicate_of_id: null })
  success.mockRestore()
})

test('bulk marking is one atomic call and Undo reverts every task with its new version', async () => {
  const calls: Call[] = []
  captureApi(calls)
  const success = spyOn(toast, 'success').mockImplementation(() => 0)
  const view = renderActions()
  await act(async () => { await view.result.current.markMany([{ id: 'task-1', version: 1 }, { id: 'task-2', version: 4 }], target) })

  expect(calls[0]).toEqual({ method: 'POST', path: '/api/v1/workspaces/workspace-1/tasks/bulk', body: { updates: [
    { id: 'task-1', expected_version: 1, duplicate_of_id: 'task-91c0' },
    { id: 'task-2', expected_version: 4, duplicate_of_id: 'task-91c0' },
  ] } })
  expect(success.mock.calls[0]![0]).toBe('Marked 2 tasks as duplicate of ORB-91C0')
  ;(success.mock.calls[0]![1] as unknown as ToastOptions).action.onClick()
  await waitFor(() => expect(calls).toHaveLength(2))
  expect(calls[1]!.body).toEqual({ updates: [
    { id: 'task-1', expected_version: 2, duplicate_of_id: null },
    { id: 'task-2', expected_version: 5, duplicate_of_id: null },
  ] })
  success.mockRestore()
})

test('a rejected mark shows the server reason and no success toast', async () => {
  const calls: Call[] = []
  captureApi(calls, () => Response.json({
    type: 'about:blank', title: 'Invalid', status: 422, detail: 'The target task is itself a duplicate.',
    code: 'validation_failed', instance: '/tasks/task-3f2a', request_id: 'request-1',
  }, { status: 422, headers: { 'content-type': 'application/problem+json' } }))
  const success = spyOn(toast, 'success').mockImplementation(() => 0)
  const error = spyOn(toast, 'error').mockImplementation(() => 0)
  const view = renderActions()
  await act(async () => { await view.result.current.markOne({ id: 'task-3f2a', version: 1 }, target) })
  expect(error).toHaveBeenCalledWith("Couldn't mark as duplicate. The target task is itself a duplicate.")
  expect(success).not.toHaveBeenCalled()
  success.mockRestore()
  error.mockRestore()
})
