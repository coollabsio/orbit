import { afterEach, expect, spyOn, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { toast } from 'sonner'
import type { TaskRecord } from '@/api/generated/types.gen'
import { queryKeys } from '@/api/queryKeys'
import type { Task } from '@/features/tasks/api/models'
import { useParentActions } from './useParentActions'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

const record = (id: string, fields: Partial<TaskRecord> = {}): TaskRecord => ({
  id, workspace_id: 'workspace-1', project_id: 'project-1', status_id: 'todo', title: id,
  description: '', position: 0, priority: 'none', assignee_ids: [], creator_id: 'user-1', label_ids: [],
  created_at: '2026-09-23T10:00:00Z', updated_at: '2026-09-23T10:00:00Z', version: 1,
  duplicate_of: null, blocked: false, parent: null, parent_task_id: null, sub_issue_count: 0, sub_issue_closed_count: 0,
  ...fields,
})

type Call = { method: string; path: string; body: Record<string, unknown> | null }
type ToastOptions = { action: { label: string; onClick: () => void } }

test('Undo of a detach also reopens the parent the detach auto-closed', async () => {
  const calls: Call[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    const call = { method: request.method, path: new URL(request.url).pathname, body: request.method === 'GET' ? null : await request.json() as Record<string, unknown> }
    calls.push(call)
    if (call.method === 'GET') return Response.json(record('parent', { status_id: 'done', version: 5 }))
    if (call.path.endsWith('/tasks/bulk')) return Response.json({ items: [], next_cursor: null, auto_closed: [] })
    // the detach: rule A closed the parent in the same write
    return Response.json({ ...record('child', { version: 3 }), auto_closed: [{ id: 'parent', status_id: 'done' }] })
  }) as unknown as typeof fetch
  const success = spyOn(toast, 'success').mockImplementation(() => 0)
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  client.setQueryData(queryKeys.tasks.detail('workspace-1', 'parent'), record('parent', { status_id: 'doing', version: 4 }))
  const wrapper = ({ children }: { children: ReactNode }) => <QueryClientProvider client={client}>{children}</QueryClientProvider>
  const view = renderHook(() => useParentActions('workspace-1'), { wrapper })
  const child = { id: 'child', identifier: 'ORB-1', version: 2, parentTaskId: 'parent' } as Task

  await act(async () => { await view.result.current.setParent([child], null) })
  const undo = success.mock.calls.map((args) => args[1] as unknown as ToastOptions | undefined).find((options) => options?.action?.label === 'Undo')!
  undo.action.onClick()

  await waitFor(() => expect(calls.some((call) => call.path.endsWith('/tasks/bulk'))).toBe(true))
  expect(calls.find((call) => call.path.endsWith('/tasks/bulk'))!.body).toEqual({ updates: [
    { id: 'child', expected_version: 3, parent_task_id: 'parent' },
    { id: 'parent', expected_version: 5, status_id: 'doing' },
  ] })
  success.mockRestore()
})
