import { afterEach, expect, spyOn, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { toast } from 'sonner'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import { testWorkspace } from '@/test/workspace'
import { useTaskPickerUpdate } from './useTaskPickerUpdate'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

const workspace = testWorkspace()
function wrapper({ children }: { children: ReactNode }) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  return (
    <QueryClientProvider client={client}>
      <WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}>{children}</WorkspaceContext.Provider>
    </QueryClientProvider>
  )
}

/** PATCH bodies; each response is held until `release` and answers with the next version. */
function heldApi() {
  const bodies: Record<string, unknown>[] = []
  const pending: Array<() => void> = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    if (request.method !== 'PATCH') return Response.json({ items: [], next_cursor: null })
    const body = await request.json() as Record<string, unknown>
    bodies.push(body)
    await new Promise<void>((resolve) => pending.push(resolve))
    return Response.json({ id: 'task-1', project_id: 'project-1', status_id: 'todo', version: (body.expected_version as number) + 1 })
  }) as unknown as typeof fetch
  return { bodies, release: () => pending.shift()?.() }
}

test('a pick made while the previous save is in flight waits for it and sends the version that save returned', async () => {
  const api = heldApi()
  const { result } = renderHook(() => useTaskPickerUpdate({ id: 'task-1', version: 1 }, 'Due date update failed.'), { wrapper })

  act(() => result.current({ due_start_at: null, due_at: '2030-01-02T09:00:00.000Z' }))
  await waitFor(() => expect(api.bodies).toHaveLength(1))
  // two more picks while the first is saving: only the latest goes out, after it
  act(() => result.current({ due_start_at: '2030-01-02T00:00:00.000Z', due_at: '2030-01-04T09:00:00.000Z' }))
  act(() => result.current({ due_start_at: '2030-01-02T00:00:00.000Z', due_at: '2030-01-05T09:00:00.000Z' }))
  expect(api.bodies).toHaveLength(1)

  act(() => api.release())
  await waitFor(() => expect(api.bodies).toHaveLength(2))
  expect(api.bodies[1]).toEqual({ expected_version: 2, due_start_at: '2030-01-02T00:00:00.000Z', due_at: '2030-01-05T09:00:00.000Z' })
  act(() => api.release())
})

test('a failed save shows a toast with Retry and drops the waiting pick', async () => {
  const errorToast = spyOn(toast, 'error').mockImplementation(() => 0)
  const bodies: unknown[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    bodies.push(await (input as Request).json())
    return Response.json({ type: 'about:blank', title: 'Failed', status: 500, detail: 'offline', code: 'failed', instance: '/tasks/task-1', request_id: 'r' }, { status: 500, headers: { 'content-type': 'application/problem+json' } })
  }) as unknown as typeof fetch
  const { result } = renderHook(() => useTaskPickerUpdate({ id: 'task-1', version: 1 }, 'Status update failed.'), { wrapper })

  act(() => result.current({ status_id: 'doing' }))
  act(() => result.current({ status_id: 'done' }))
  await waitFor(() => expect(errorToast).toHaveBeenCalledTimes(1))
  expect(errorToast.mock.calls[0]![0]).toBe('Status update failed.')
  expect(bodies).toEqual([{ expected_version: 1, status_id: 'doing' }])
  errorToast.mockRestore()
})

test('two pickers of one task share its queue: the second waits and sends the version the first save returned', async () => {
  const api = heldApi()
  const task = { id: 'task-shared', version: 1 }
  // e.g. the status and priority pickers of one list row
  const { result } = renderHook(() => ({
    status: useTaskPickerUpdate(task, 'Status update failed.'),
    priority: useTaskPickerUpdate(task, 'Priority update failed.'),
  }), { wrapper })

  act(() => result.current.status({ status_id: 'doing' }))
  await waitFor(() => expect(api.bodies).toHaveLength(1))
  act(() => result.current.priority({ priority: 'high' }))
  expect(api.bodies).toEqual([{ expected_version: 1, status_id: 'doing' }])

  act(() => api.release())
  await waitFor(() => expect(api.bodies).toHaveLength(2))
  expect(api.bodies[1]).toEqual({ expected_version: 2, priority: 'high' })
  act(() => api.release())
})

test('waiting changes from different pickers merge into one save; a later value for the same field wins', async () => {
  const api = heldApi()
  const task = { id: 'task-merged', version: 4 }
  const { result } = renderHook(() => ({
    status: useTaskPickerUpdate(task, 'Status update failed.'),
    priority: useTaskPickerUpdate(task, 'Priority update failed.'),
  }), { wrapper })

  act(() => result.current.status({ status_id: 'doing' }))
  await waitFor(() => expect(api.bodies).toHaveLength(1))
  act(() => result.current.status({ status_id: 'review' }))
  act(() => result.current.priority({ priority: 'high' }))
  act(() => result.current.status({ status_id: 'done' }))

  act(() => api.release())
  await waitFor(() => expect(api.bodies).toHaveLength(2))
  expect(api.bodies[1]).toEqual({ expected_version: 5, status_id: 'done', priority: 'high' })
  act(() => api.release())
  // the queue is idle again: nothing else goes out, and the next pick uses the caller's version
  await new Promise((resolve) => setTimeout(resolve, 20))
  expect(api.bodies).toHaveLength(2)
})

test('a waiting change still goes out when the picker that started the save has unmounted', async () => {
  const api = heldApi()
  const task = { id: 'task-unmounted', version: 1 }
  // a list row whose status change makes it leave the filtered list, and the same task in a relation row
  const row = renderHook(() => useTaskPickerUpdate(task, 'Status update failed.'), { wrapper })
  const relation = renderHook(() => useTaskPickerUpdate(task, 'Status update failed.'), { wrapper })

  act(() => row.result.current({ status_id: 'done' }))
  await waitFor(() => expect(api.bodies).toHaveLength(1))
  row.unmount()
  act(() => relation.result.current({ priority: 'low' }))

  act(() => api.release())
  await waitFor(() => expect(api.bodies).toHaveLength(2))
  expect(api.bodies[1]).toEqual({ expected_version: 2, priority: 'low' })
  act(() => api.release())
})

test('different tasks do not wait for each other', async () => {
  const api = heldApi()
  const { result } = renderHook(() => ({
    first: useTaskPickerUpdate({ id: 'task-a', version: 1 }, 'Status update failed.'),
    second: useTaskPickerUpdate({ id: 'task-b', version: 7 }, 'Status update failed.'),
  }), { wrapper })

  act(() => result.current.first({ status_id: 'doing' }))
  act(() => result.current.second({ status_id: 'done' }))
  await waitFor(() => expect(api.bodies).toHaveLength(2))
  expect(api.bodies).toEqual([{ expected_version: 1, status_id: 'doing' }, { expected_version: 7, status_id: 'done' }])
  act(() => api.release())
  act(() => api.release())
})
