import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import type { ReactNode } from 'react'
import type { Project, Task, TaskStatusDef } from '@/features/tasks/api/models'
import type { GroupContext } from '@/features/views/grouping'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import { TaskTargetProvider } from '@/shortcuts/TaskTargetProvider'
import { taskRowTarget, useTaskTarget } from '@/shortcuts/taskTarget'
import { render } from '@/test/render'
import { testWorkspace } from '@/test/workspace'
import { TaskContextMenu } from './TaskContextMenu'

const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
})

const workspace = testWorkspace()
const todo: TaskStatusDef = { id: 'todo', projectId: 'project-1', name: 'Todo', description: '', color: '#aaa', category: 'unstarted', position: 0, version: 1 }
const launch = { id: 'project-1', workspace_id: 'workspace-1', name: 'Launch', key: 'ORB', color: '#e0457b', created_at: '', updated_at: '', version: 1 } as Project

const task = (index: number): Task => ({
  id: `task-${index}`, statusId: todo.id, position: index, version: 1, projectId: 'project-1',
  title: `Task ${index}`, description: '', identifier: `ORB-${index}`, priority: 'none', assigneeIds: [],
  creatorId: 'user-1', labels: [], attachments: [], dueAt: null, createdAt: '', updatedAt: '', comments: [], activity: [],
})

type Write = { path: string; body: { updates: Array<Record<string, unknown>> } }
function captureWrites(): Write[] {
  const writes: Write[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    if (request.method !== 'GET') writes.push({ path: new URL(request.url).pathname, body: await request.json() })
    return Response.json({ items: [], next_cursor: null })
  }) as unknown as typeof fetch
  return writes
}

function Page({ tasks, select }: { tasks: Task[]; select: string[] }) {
  const { setSelected, selectedIds } = useTaskTarget()
  const groupContext: GroupContext = { statuses: [todo], members: [], labels: [], projects: [launch], currentUserId: 'user-1', showEmpty: false }
  return (
    <>
      <button type="button" onClick={() => setSelected(select)}>select</button>
      <output>{selectedIds.length} selected</output>
      <TaskContextMenu tasks={tasks} users={[]} labels={[]} statuses={[todo]} groupContext={groupContext} currentUserId="user-1">
        {tasks.map((item) => <div key={item.id} {...taskRowTarget(item.id)}>{item.title}</div>)}
        <p>Group header</p>
      </TaskContextMenu>
    </>
  )
}

function wrapper({ children }: { children: ReactNode }) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  return <QueryClientProvider client={client}><WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}><TaskTargetProvider openTaskId={null}>{children}</TaskTargetProvider></WorkspaceContext.Provider></QueryClientProvider>
}

async function setUrgent(view: ReturnType<typeof render>, title: string) {
  fireEvent.contextMenu(view.getByText(title))
  await userEvent.click(await view.findByRole('menuitem', { name: /Priority/ }))
  await userEvent.click(await view.findByRole('menuitemcheckbox', { name: /^Urgent/ }))
}

test('a right-click on a row changes that row only and drops the selection', async () => {
  const writes = captureWrites()
  const view = render(<Page tasks={[task(1), task(2), task(3)]} select={['task-1', 'task-2']} />, { wrapper })
  fireEvent.click(view.getByText('select'))
  await setUrgent(view, 'Task 3')
  await waitFor(() => { if (writes.length !== 1) throw new Error('no write') })
  expect(writes[0].body.updates).toEqual([{ id: 'task-3', expected_version: 1, priority: 'urgent' }])
  expect(view.getByText('0 selected')).toBeTruthy()
})

test('a right-click on a selected row changes the whole selection', async () => {
  const writes = captureWrites()
  const view = render(<Page tasks={[task(1), task(2), task(3)]} select={['task-1', 'task-2']} />, { wrapper })
  fireEvent.click(view.getByText('select'))
  await setUrgent(view, 'Task 2')
  await waitFor(() => { if (writes.length !== 1) throw new Error('no write') })
  expect(writes[0].body.updates.map((update) => update.id)).toEqual(['task-1', 'task-2'])
  expect(view.getByText('2 selected')).toBeTruthy()
})

test('a right-click outside a row opens no menu', () => {
  const view = render(<Page tasks={[task(1)]} select={[]} />, { wrapper })
  fireEvent.contextMenu(view.getByText('Group header'))
  expect(view.queryAllByRole('menu').length).toBe(0)
})
