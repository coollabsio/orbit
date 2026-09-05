import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import type { WorkspaceRecord } from '../../../api/generated/types.gen'
import { WorkspaceContext } from '../../workspaces/workspaceContext'
import type { Task, TaskStatusDef } from '../api/models'
import type { StatusGroup } from '../tasksLib'
import { TaskBoard } from './TaskBoard'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

function task(id: string, statusId: string, position: number): Task {
  return {
    id, statusId, position, version: 1, projectId: 'project-1', title: id === 'moving' ? 'Moving' : id,
    description: '', identifier: `ORB-${position}`, priority: 'none', assigneeIds: [], creatorId: 'user-1',
    labels: [], attachments: [], dueAt: null, createdAt: '', updatedAt: '', comments: [], activity: [],
  }
}

test('mounted board keeps generated reorder writes bounded and exposes failure retry', async () => {
  const requestBodies: Array<{ updates: Array<{ position?: number | null }> }> = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    requestBodies.push(await request.json())
    if (requestBodies.length === 1) return Response.json({
      type: 'about:blank', title: 'Failed', status: 500, detail: 'offline', code: 'failed',
      instance: request.url, request_id: 'request-1',
    }, { status: 500, headers: { 'content-type': 'application/problem+json' } })
    return Response.json({ items: [], next_cursor: null })
  }) as unknown as typeof fetch
  const workspace: WorkspaceRecord = { id: 'workspace-1', name: 'Orbit', role: 'owner', version: 1 }
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>
      <WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}>
        {children}
      </WorkspaceContext.Provider>
    </QueryClientProvider>
  )
  const statuses: TaskStatusDef[] = [
    { id: 'todo', projectId: 'project-1', name: 'Todo', description: '', color: '#aaa', category: 'unstarted', position: 0, version: 1 },
    { id: 'doing', projectId: 'project-1', name: 'Doing', description: '', color: '#bbb', category: 'started', position: 1, version: 1 },
  ]
  const groups: StatusGroup[] = statuses.map((status) => ({
    key: `${status.category}:${status.name.toLowerCase()}`, name: status.name, category: status.category,
    status, statusIds: [status.id],
  }))
  const tasks = [task('moving', 'todo', 0), ...Array.from({ length: 101 }, (_, index) => task(`task-${index}`, 'doing', index))]
  const view = render(
    <TaskBoard tasks={tasks} users={[]} labels={[]} statuses={statuses} groups={groups} sort="manual" activeTaskId={null} onOpen={() => {}} />,
    { wrapper },
  )
  const moving = view.getByRole('heading', { name: 'Moving' }).closest('article')!
  const doing = view.getByText('Doing').closest('section')!
  const dataTransfer = { effectAllowed: '', dropEffect: '', setData: () => {} }

  fireEvent.dragStart(moving, { dataTransfer })
  fireEvent.drop(doing, { clientY: 999, dataTransfer })

  expect((await view.findByRole('alert')).textContent).toContain('Board reorder failed')
  expect(requestBodies).toHaveLength(1)
  expect(requestBodies[0]?.updates).toHaveLength(100)
  expect(requestBodies[0]?.updates.every((update) => Number.isInteger(update.position))).toBeTrue()

  fireEvent.click(view.getByRole('button', { name: 'Retry' }))
  await waitFor(() => expect(requestBodies).toHaveLength(3))
  expect(requestBodies.map((body) => body.updates.length)).toEqual([100, 100, 2])
  expect(requestBodies.every((body) => body.updates.length <= 100)).toBeTrue()
})
