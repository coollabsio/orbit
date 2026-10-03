import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { queryKeys } from '@/api/queryKeys'
import { fireEvent, renderHook, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import type { ReactNode } from 'react'
import type { Project, Task, TaskStatusDef } from '@/features/tasks/api/models'
import type { GroupContext } from '@/features/views/grouping'
import type { User } from '@/features/workspaces/models'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import { ShortcutProvider } from '@/shortcuts/ShortcutProvider'
import { TaskTargetProvider } from '@/shortcuts/TaskTargetProvider'
import { taskRowTarget, useTaskTarget } from '@/shortcuts/taskTarget'
import { useAvailableCommands } from '@/shortcuts/useCommand'
import { render } from '@/test/render'
import { TaskCommands } from './TaskCommands'
import { testWorkspace } from '@/test/workspace'
import { NO_MEMBER_ABILITIES } from '@/features/workspaces/models'

const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
})

const workspace = testWorkspace()
const todo: TaskStatusDef = { id: 'todo', projectId: 'project-1', name: 'Todo', description: '', color: '#aaa', category: 'unstarted', position: 0, version: 1 }
const doing: TaskStatusDef = { ...todo, id: 'doing', name: 'Doing', category: 'started', position: 1 }
const launch = { id: 'project-1', workspace_id: 'workspace-1', name: 'Launch', key: 'ORB', color: '#e0457b', created_at: '', updated_at: '', version: 1 } as Project
const ada: User = { id: 'user-1', membershipId: 'membership-1', name: 'Ada Lovelace', handle: 'ada', email: 'ada@example.com', role: 'Member', color: '#8b5cf6', online: true, title: '', roleIds: [], can: NO_MEMBER_ABILITIES, version: 1 }
const bo: User = { ...ada, id: 'user-2', membershipId: 'membership-2', name: 'Bo Diddley', handle: 'bo', email: 'bo@example.com' }

const task = (index: number, patch: Partial<Task> = {}): Task => ({
  id: `task-${index}`, statusId: todo.id, position: index, version: 1, projectId: 'project-1',
  title: `Task ${index}`, description: '', identifier: `ORB-${index}`, priority: 'none', assigneeIds: [],
  creatorId: 'user-1', labels: [], attachments: [], dueAt: null, createdAt: '', updatedAt: '', comments: [], activity: [], ...patch,
})

type Write = { method: string; path: string; body: { updates: Array<Record<string, unknown>> } }
function captureWrites(): Write[] {
  const writes: Write[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    if (request.method !== 'GET') writes.push({ method: request.method, path: new URL(request.url).pathname, body: await request.json() })
    return Response.json({ items: [], next_cursor: null })
  }) as unknown as typeof fetch
  return writes
}

function Row({ item }: { item: Task }) {
  return <div role="button" tabIndex={0} {...taskRowTarget(item.id)}>{item.title}</div>
}

function Page({ tasks, select = [] }: { tasks: Task[]; select?: string[] }) {
  const { setSelected } = useTaskTarget()
  const groupContext: GroupContext = { statuses: [todo, doing], members: [ada, bo], labels: [], projects: [launch], currentUserId: 'user-1', showEmpty: false }
  return (
    <>
      {tasks.map((item) => <Row key={item.id} item={item} />)}
      <button type="button" onClick={() => setSelected(select)}>select</button>
      <TaskCommands tasks={tasks} users={[ada, bo]} labels={[]} statuses={[todo, doing]} groupContext={groupContext} currentUserId="user-1" />
    </>
  )
}

function Wrapper({ children }: { children: ReactNode }) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  // the project menus read every project from the cache; one project offers no move
  client.setQueryDefaults(queryKeys.projects(workspace.id), { staleTime: Infinity })
  client.setQueryData(queryKeys.projects(workspace.id), [launch])
  return <QueryClientProvider client={client}><WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}><TaskTargetProvider openTaskId={null}>{children}</TaskTargetProvider></WorkspaceContext.Provider></QueryClientProvider>
}

const hover = (element: Element) => fireEvent.pointerMove(element, { clientX: 5, clientY: 5 })

test('S on a hovered task opens the status list and changes that task', async () => {
  const writes = captureWrites()
  const view = render(<Page tasks={[task(1), task(2)]} />, { wrapper: Wrapper })
  hover(view.getByText('Task 2'))
  await userEvent.keyboard('s')
  await userEvent.click(await view.findByRole('option', { name: /Doing/ }))
  await waitFor(() => expect(writes).toHaveLength(1))
  expect(writes[0].body.updates).toEqual([{ id: 'task-2', expected_version: 1, status_id: 'doing' }])
  // one choice closes the list
  await waitFor(() => expect(view.queryByRole('option', { name: /Doing/ })).toBeNull())
})

test('P with three selected tasks changes all three', async () => {
  const writes = captureWrites()
  const view = render(<Page tasks={[task(1), task(2), task(3)]} select={['task-1', 'task-2', 'task-3']} />, { wrapper: Wrapper })
  await userEvent.click(view.getByRole('button', { name: 'select' }))
  ;(document.activeElement as HTMLElement).blur()
  await userEvent.keyboard('p')
  await userEvent.click(await view.findByRole('option', { name: /Urgent/ }))
  await waitFor(() => expect(writes).toHaveLength(1))
  expect(writes[0].body.updates.map((update) => [update.id, update.priority])).toEqual([['task-1', 'urgent'], ['task-2', 'urgent'], ['task-3', 'urgent']])
})

test('without a target a task key does nothing', async () => {
  const writes = captureWrites()
  const view = render(<Page tasks={[task(1)]} />, { wrapper: Wrapper })
  await userEvent.keyboard('s')
  expect(view.queryByRole('option')).toBeNull()
  await userEvent.keyboard('i')
  expect(writes).toEqual([])
})

test('I assigns the current user and keeps the other assignees', async () => {
  const writes = captureWrites()
  const view = render(<Page tasks={[task(1, { assigneeIds: ['user-2'] })]} />, { wrapper: Wrapper })
  hover(view.getByText('Task 1'))
  await userEvent.keyboard('i')
  await waitFor(() => expect(writes).toHaveLength(1))
  expect(writes[0].body.updates).toEqual([{ id: 'task-1', expected_version: 1, assignee_ids: ['user-2', 'user-1'] }])
})

test('commands without a bulk form are off while several tasks are selected', async () => {
  const wrapper = ({ children }: { children: ReactNode }) => (
    <ShortcutProvider><Wrapper><Page tasks={[task(1), task(2)]} select={['task-1', 'task-2']} />{children}</Wrapper></ShortcutProvider>
  )
  const { result } = renderHook(() => useAvailableCommands(), { wrapper })
  const ids = () => result.current().map((command) => command.id)
  hover(document.querySelector('[data-task-id="task-1"]')!)
  expect(ids()).toContain('task.copyId')
  expect(ids()).toContain('task.setStatus')
  await userEvent.click(document.querySelector('button')!)
  expect(ids()).not.toContain('task.copyId')
  expect(ids()).not.toContain('task.copyLink')
  expect(ids()).toContain('task.setStatus')
})

test('A toggles assignees and keeps the list open', async () => {
  const writes = captureWrites()
  const view = render(<Page tasks={[task(1)]} />, { wrapper: Wrapper })
  hover(view.getByText('Task 1'))
  await userEvent.keyboard('a')
  await userEvent.click(await view.findByRole('option', { name: /Bo Diddley/ }))
  await waitFor(() => expect(writes).toHaveLength(1))
  expect(writes[0].body.updates).toEqual([{ id: 'task-1', expected_version: 1, assignee_ids: ['user-2'] }])
  expect(view.getByRole('option', { name: /Ada Lovelace/ })).toBeTruthy()
})

test('a selected task that left the list does not switch off the one-task commands', async () => {
  const wrapper = ({ children }: { children: ReactNode }) => (
    <ShortcutProvider><Wrapper><Page tasks={[task(1), task(2)]} select={['task-1', 'gone']} />{children}</Wrapper></ShortcutProvider>
  )
  const { result } = renderHook(() => useAvailableCommands(), { wrapper })
  await userEvent.click(document.querySelector('button')!)
  expect(result.current().map((command) => command.id)).toContain('task.copyId')
})
