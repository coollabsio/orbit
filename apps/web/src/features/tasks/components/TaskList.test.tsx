import { afterEach, expect, spyOn, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, createEvent, fireEvent, render, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import type { ReactNode } from 'react'
import { toast } from 'sonner'
import type { WorkspaceRecord } from '@/api/generated/types.gen'
import type { Project, Task, TaskStatusDef } from '@/features/tasks/api/models'
import type { GroupContext } from '@/features/views/grouping'
import type { GroupValues } from '@/features/views/layoutGroups'
import { DEFAULT_DISPLAY, type DisplayOptions } from '@/features/views/viewState'
import type { User } from '@/features/workspaces/models'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import { TaskList } from './TaskList'

const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
  window.localStorage.clear()
})

const workspace: WorkspaceRecord = { id: 'workspace-1', name: 'Orbit', role: 'owner', version: 1 }
const status: TaskStatusDef = {
  id: 'todo', projectId: 'project-1', name: 'Todo', description: '', color: '#aaa',
  category: 'unstarted', position: 0, version: 1,
}
const doingStatus: TaskStatusDef = { ...status, id: 'doing', name: 'Doing', category: 'started', position: 1 }
const duplicateStatus: TaskStatusDef = {
  id: 'dup', projectId: 'project-1', name: 'Duplicate', description: '', color: '#8b8f98',
  category: 'duplicate', position: 1, version: 1,
}
const launch = { id: 'project-1', workspace_id: 'workspace-1', name: 'Launch', key: 'ORB', color: '#e0457b', created_at: '', updated_at: '', version: 1 } as Project
const docs = { ...launch, id: 'project-2', name: 'Docs', key: 'DOC' } as Project
const user: User = {
  id: 'user-1', membershipId: 'membership-1', name: 'Ada Lovelace', handle: 'ada', email: 'ada@example.com',
  role: 'Member', color: '#8b5cf6', online: true, title: '', roleIds: [], version: 1,
}

function task(index: number): Task {
  return {
    id: `task-${index}`, statusId: status.id, position: index, version: 1, projectId: 'project-1',
    title: `Task ${index}`, description: '', identifier: `ORB-${index}`, priority: 'none', assigneeIds: [],
    creatorId: 'user-1', labels: [], attachments: [], dueAt: null, createdAt: '', updatedAt: '', comments: [], activity: [],
  }
}

function wrapper({ children }: { children: ReactNode }) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  return <QueryClientProvider client={client}><WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}>{children}</WorkspaceContext.Provider></QueryClientProvider>
}

type ListOptions = {
  onOpen?: (taskId: string) => void
  onAdd?: (values: GroupValues) => void
  users?: User[]
  statuses?: TaskStatusDef[]
  display?: Partial<DisplayOptions>
  scope?: string
}

function renderList(tasks: Task[], options: ListOptions = {}) {
  const statuses = options.statuses ?? [status]
  const groupContext: GroupContext = {
    statuses, members: options.users ?? [], labels: [], projects: [launch, docs], currentUserId: 'user-1', showEmpty: false,
  }
  return render(
    <TaskList
      tasks={tasks} users={options.users ?? []} labels={[]} statuses={statuses} projects={[launch, docs]}
      display={{ ...DEFAULT_DISPLAY, ...options.display }} groupContext={groupContext} collapseScope={options.scope ?? 'all'}
      onOpen={options.onOpen ?? (() => {})} onAdd={options.onAdd ?? (() => {})}
    />,
    { wrapper },
  )
}

function viewFor(tasks: Task[], onOpen = () => {}, users: User[] = []) {
  return renderList(tasks, { onOpen, users })
}

async function chooseUrgent(view: ReturnType<typeof render>) {
  const toolbar = view.getByRole('toolbar', { name: 'Selected tasks' })
  // fireEvent opens the Base UI trigger (userEvent would double-toggle it); userEvent
  // then clicks the option, waiting until the opened popover is actually actionable.
  fireEvent.click(within(toolbar).getByRole('button', { name: 'Priority' }))
  await userEvent.click(await view.findByRole('menuitem', { name: /^Urgent/ }, { timeout: 5000 }))
}

type Write = { method: string; path: string; body: unknown }
function captureWrites(): Write[] {
  const writes: Write[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    if (request.method !== 'GET') writes.push({ method: request.method, path: new URL(request.url).pathname, body: await request.json() })
    return Response.json({ items: [], next_cursor: null })
  }) as unknown as typeof fetch
  return writes
}

const dataTransfer = { effectAllowed: '', dropEffect: '', setData: () => {}, getData: () => '' }
const rowOf = (view: ReturnType<typeof render>, title: string) => view.getByText(title).closest('[data-task-row]')!
const zoneOf = (view: ReturnType<typeof render>, label: string) => view.getByRole('button', { name: `Collapse ${label}` }).closest('section')!
function dropAt(target: Element, clientY: number) {
  const event = createEvent.drop(target, { dataTransfer })
  Object.defineProperty(event, 'clientY', { value: clientY })
  fireEvent(target, event)
}
/** Negative check: `assertion` keeps holding for `ms`, long enough for a stray mutation to reach fetch. */
async function staysTrue(assertion: () => void, ms = 100) {
  const end = Date.now() + ms
  while (Date.now() < end) {
    assertion()
    await new Promise((resolve) => setTimeout(resolve, 10))
  }
  assertion()
}

test('bulk toolbar rejects more than 100 selected tasks without a server request', async () => {
  let requests = 0
  globalThis.fetch = (async () => {
    requests += 1
    return Response.json({ items: [], next_cursor: null })
  }) as unknown as typeof fetch
  const view = viewFor(Array.from({ length: 101 }, (_, index) => task(index)))
  act(() => {
    for (const checkbox of view.getAllByRole('checkbox')) checkbox.click()
  })
  await waitFor(() => expect(view.getByText('101 selected')).toBeTruthy(), { timeout: 5000 })

  await chooseUrgent(view)

  expect((await view.findByRole('alert', {}, { timeout: 5000 })).textContent).toContain('Select 100 or fewer')
  expect(requests).toBe(0)
  expect(view.queryByRole('button', { name: 'Retry' })).toBeNull()
}, 20000)

test('bulk toolbar retry repeats the original valid atomic payload', async () => {
  const bodies: unknown[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    bodies.push(await (input as Request).json())
    if (bodies.length === 1) return Response.json({
      type: 'about:blank', title: 'Failed', status: 500, detail: 'offline', code: 'failed',
      instance: '/tasks/bulk', request_id: 'request-1',
    }, { status: 500, headers: { 'content-type': 'application/problem+json' } })
    return Response.json({ items: [], next_cursor: null })
  }) as unknown as typeof fetch
  const view = viewFor([task(1), task(2)])
  for (const checkbox of view.getAllByRole('checkbox')) fireEvent.click(checkbox)
  await chooseUrgent(view)
  await view.findByRole('alert')

  fireEvent.click(view.getByRole('button', { name: 'Retry' }))
  await waitFor(() => expect(bodies).toHaveLength(2))

  expect(bodies[1]).toEqual(bodies[0])
})

test('bulk toolbar treats an unchanged priority as a local no-op', async () => {
  let requests = 0
  globalThis.fetch = (async () => {
    requests += 1
    return Response.json({ items: [], next_cursor: null })
  }) as unknown as typeof fetch
  const view = viewFor([task(1), task(2)])
  for (const checkbox of view.getAllByRole('checkbox')) fireEvent.click(checkbox)
  const toolbar = view.getByRole('toolbar', { name: 'Selected tasks' })

  fireEvent.click(within(toolbar).getByRole('button', { name: 'Priority' }))
  fireEvent.click(view.getByRole('menuitem', { name: /^No priority/ }))

  await new Promise((resolve) => setTimeout(resolve, 0))
  expect(requests).toBe(0)
  expect(view.queryByRole('alert')).toBeNull()
  expect(view.queryByRole('button', { name: 'Retry' })).toBeNull()
})

test('task list titles link web addresses without opening the task row', () => {
  let opened = 0
  const linkedTask = task(1)
  linkedTask.title = 'Review https://example.com/pull/1'
  const view = viewFor([linkedTask], () => { opened += 1 })

  const link = view.getByRole('link', { name: 'https://example.com/pull/1' }) as HTMLAnchorElement
  expect(link.href).toBe('https://example.com/pull/1')
  expect(link.target).toBe('_blank')
  fireEvent.click(link)
  expect(opened).toBe(0)
})

test('task list assignee opens the assignment dropdown and updates without opening the task', async () => {
  let opened = 0
  let body: unknown
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    body = await (input as Request).json()
    return Response.json({})
  }) as unknown as typeof fetch
  const assignedTask = task(1)
  assignedTask.assigneeIds = [user.id]
  const view = viewFor([assignedTask], () => { opened += 1 }, [user])

  fireEvent.click(view.getByRole('button', { name: 'Assignees: Ada Lovelace' }))
  fireEvent.click(view.getByRole('menuitemcheckbox', { checked: true }))

  expect(view.queryByRole('status')).toBeNull()
  await waitFor(() => expect(body).toEqual({ expected_version: 1, assignee_ids: [] }))
  expect(view.queryByText('Assignees')).toBeNull()
  expect(opened).toBe(0)
})

test('remembers collapsed groups after the task list reloads', () => {
  const firstView = viewFor([task(1)])
  fireEvent.click(firstView.getByRole('button', { name: 'Collapse Todo' }))
  expect(firstView.getByRole('button', { name: 'Expand Todo' }).getAttribute('aria-expanded')).toBe('false')
  firstView.unmount()

  const reloadedView = viewFor([task(1)])
  expect(reloadedView.getByRole('button', { name: 'Expand Todo' }).getAttribute('aria-expanded')).toBe('false')
  expect(reloadedView.queryByText('Task 1')).toBeNull()
})

test('collapsed groups are remembered per page', () => {
  const allTasks = renderList([task(1)], { scope: 'all' })
  fireEvent.click(allTasks.getByRole('button', { name: 'Collapse Todo' }))
  allTasks.unmount()

  const projectPage = renderList([task(1)], { scope: 'project:project-1' })
  expect(projectPage.getByRole('button', { name: 'Collapse Todo' }).getAttribute('aria-expanded')).toBe('true')
  projectPage.unmount()

  expect(renderList([task(1)], { scope: 'all' }).getByRole('button', { name: 'Expand Todo' })).toBeTruthy()
})

test('bulk toolbar Escape closes an open menu first, then clears the selection', async () => {
  const view = viewFor([task(1), task(2)])
  for (const checkbox of view.getAllByRole('checkbox')) fireEvent.click(checkbox)
  const toolbar = view.getByRole('toolbar', { name: 'Selected tasks' })

  fireEvent.click(within(toolbar).getByRole('button', { name: 'Status' }))
  await view.findByRole('menu', {}, { timeout: 5000 })
  fireEvent.keyDown(document.body, { key: 'Escape' })
  expect(view.queryByRole('toolbar', { name: 'Selected tasks' })).not.toBeNull()

  await waitFor(() => expect(within(toolbar).getByRole('button', { name: 'Status' }).getAttribute('aria-expanded')).toBe('false'))
  fireEvent.keyDown(document.body, { key: 'Escape' })
  expect(view.queryByRole('toolbar', { name: 'Selected tasks' })).toBeNull()
}, 20000)

test('grouping by priority orders groups from urgent to no priority', () => {
  const view = renderList([task(1), { ...task(2), priority: 'urgent' }], { display: { group_by: 'priority' } })
  const headers = view.getAllByRole('button', { name: /^Collapse / }).map((button) => button.getAttribute('aria-label'))
  expect(headers).toEqual(['Collapse Urgent', 'Collapse No priority'])
})

test('without grouping the list is flat, with no headers', () => {
  const view = renderList([task(1), task(2)], { display: { group_by: 'none' } })
  expect(view.queryAllByRole('button', { name: /^Collapse / })).toEqual([])
  expect(view.getByText('Task 1')).toBeTruthy()
  expect(view.getByText('Task 2')).toBeTruthy()
})

test('sub-groups render as collapsible headers inside each group', () => {
  const view = renderList([task(1), { ...task(2), priority: 'urgent' }], { display: { sub_group_by: 'priority' } })
  const todo = zoneOf(view, 'Todo')
  expect(within(todo).getByRole('button', { name: 'Collapse No priority' })).toBeTruthy()

  fireEvent.click(within(todo).getByRole('button', { name: 'Collapse Urgent' }))
  expect(view.queryByText('Task 2')).toBeNull()
  expect(view.getByText('Task 1')).toBeTruthy()

  fireEvent.click(view.getByRole('button', { name: 'Collapse Todo' }))
  expect(view.queryByText('Task 1')).toBeNull()
  expect(view.queryByRole('button', { name: 'Expand Urgent' })).toBeNull()
})

test('the + in a sub-group header passes the group and sub-group values', () => {
  const added: GroupValues[] = []
  const view = renderList([{ ...task(1), priority: 'urgent' }], { display: { sub_group_by: 'priority' }, onAdd: (values) => added.push(values) })
  fireEvent.click(within(zoneOf(view, 'Todo')).getByRole('button', { name: 'New task in Urgent' }))
  expect(added).toEqual([[{ field: 'status', value: 'unstarted:todo' }, { field: 'priority', value: 'urgent' }]])
})

test('show_empty_groups adds headers for statuses without tasks', () => {
  const hidden = renderList([task(1)], { statuses: [status, doingStatus] })
  expect(hidden.queryByRole('button', { name: 'Collapse Doing' })).toBeNull()
  hidden.unmount()

  const shown = renderList([task(1)], { statuses: [status, doingStatus], display: { show_empty_groups: true } })
  expect(within(zoneOf(shown, 'Doing')).getByText('0')).toBeTruthy()
})

test('show_empty_groups keeps the group headers when no task matches', () => {
  const empty = renderList([], { statuses: [status, doingStatus] })
  expect(empty.getAllByText('No tasks found')).toHaveLength(1)
  empty.unmount()

  const shown = renderList([], { statuses: [status, doingStatus], display: { show_empty_groups: true } })
  expect(shown.queryAllByText('No tasks found')).toHaveLength(0)
  expect(shown.getAllByRole('button', { name: 'New task in Doing' })).toHaveLength(1)
})

test('rows render only the properties chosen in the display options', () => {
  const dated = { ...task(1), dueAt: '2026-09-30T09:00:00.000Z' }
  const minimal = renderList([dated], { display: { properties: ['id'] } })
  expect(minimal.getByText('ORB-1')).toBeTruthy()
  expect(minimal.queryByRole('button', { name: 'Status: Todo' })).toBeNull()
  expect(minimal.queryByRole('button', { name: /^Priority:/ })).toBeNull()
  expect(minimal.queryByRole('button', { name: 'Assign task' })).toBeNull()
  expect(minimal.container.querySelector('[data-property="due_date"]')).toBeNull()
  minimal.unmount()

  const chosen = renderList([dated], { display: { properties: ['status', 'priority', 'due_date', 'project', 'updated'] } })
  expect(chosen.queryByText('ORB-1')).toBeNull()
  expect(chosen.getByRole('button', { name: 'Status: Todo' })).toBeTruthy()
  expect(chosen.getByRole('button', { name: 'Priority: No priority' })).toBeTruthy()
  expect(chosen.container.querySelector('[data-property="due_date"]')).not.toBeNull()
  expect(chosen.container.querySelector('[data-property="project"]')!.textContent).toBe('Launch')
  expect(chosen.container.querySelector('[data-property="updated"]')).not.toBeNull()
  expect(chosen.container.querySelector('[data-property="created"]')).toBeNull()
})

test('dropping a row on another priority group sets that priority', async () => {
  const writes = captureWrites()
  const view = renderList([task(1), { ...task(2), priority: 'urgent' }], { display: { group_by: 'priority', order_by: 'created' } })
  fireEvent.dragStart(rowOf(view, 'Task 1'), { dataTransfer })
  dropAt(zoneOf(view, 'Urgent'), 0)

  await waitFor(() => expect(writes).toHaveLength(1))
  expect(writes[0]).toEqual({
    method: 'PATCH', path: '/api/v1/workspaces/workspace-1/tasks/task-1', body: { expected_version: 1, priority: 'urgent' },
  })
})

test('a drop with no matching status in the task\'s project shows a toast and writes nothing', async () => {
  const writes = captureWrites()
  const errorToast = spyOn(toast, 'error').mockImplementation(() => 0)
  const docsTodo: TaskStatusDef = { ...status, id: 'todo-2', projectId: 'project-2' }
  const view = renderList(
    [{ ...task(1), projectId: 'project-2', statusId: 'todo-2' }, { ...task(2), statusId: 'doing' }],
    { statuses: [status, doingStatus, docsTodo] },
  )
  fireEvent.dragStart(rowOf(view, 'Task 1'), { dataTransfer })
  dropAt(zoneOf(view, 'Doing'), 0)

  await waitFor(() => expect(errorToast).toHaveBeenCalledTimes(1))
  expect(String(errorToast.mock.calls[0]![0])).toContain('No matching status')
  await staysTrue(() => expect(writes).toEqual([]))
  errorToast.mockRestore()
})

test('manual order: dropping a row above its group\'s first row writes only the changed positions', async () => {
  const writes = captureWrites()
  const view = renderList([task(1), task(2), task(3)])
  fireEvent.dragStart(rowOf(view, 'Task 3'), { dataTransfer })
  dropAt(zoneOf(view, 'Todo'), -1)

  await waitFor(() => expect(writes).toHaveLength(1))
  expect(writes[0]!.path).toBe('/api/v1/workspaces/workspace-1/tasks/bulk')
  expect(writes[0]!.body).toEqual({ updates: [
    { id: 'task-3', expected_version: 1, position: 1 },
    { id: 'task-1', expected_version: 1, position: 2 },
    { id: 'task-2', expected_version: 1, position: 3 },
  ] })
})

test('with a non-manual order, dropping inside the same group changes nothing', async () => {
  const writes = captureWrites()
  const view = renderList([task(1), task(2)], { display: { order_by: 'created' } })
  fireEvent.dragStart(rowOf(view, 'Task 2'), { dataTransfer })
  dropAt(zoneOf(view, 'Todo'), -1)
  await staysTrue(() => expect(writes).toEqual([]))
})

test('the dragged row stays mounted and fades while dragging', () => {
  const view = renderList([task(1), task(2)])
  const row = rowOf(view, 'Task 1')
  fireEvent.dragStart(row, { dataTransfer })
  expect(row.isConnected).toBe(true)
  expect(row.getAttribute('data-dragging')).toBe('true')
  fireEvent.dragEnd(row)
  expect(row.hasAttribute('data-dragging')).toBe(false)
})

test('show_empty_groups also lists empty sub-groups inside each group', () => {
  const view = renderList([task(1)], { display: { sub_group_by: 'priority', show_empty_groups: true } })
  const urgent = within(zoneOf(view, 'Todo')).getByRole('button', { name: 'Collapse Urgent' }).closest('section')!
  expect(within(urgent).getByText('0')).toBeTruthy()
  expect(within(zoneOf(view, 'Todo')).getByRole('button', { name: 'Collapse No priority' })).toBeTruthy()
})

test('a sub-group inside the Duplicate status group offers no +', () => {
  const view = renderList(
    [{ ...task(1), priority: 'urgent' }, { ...asDuplicate(task(2)), priority: 'urgent' }],
    { statuses: [status, duplicateStatus], display: { sub_group_by: 'priority' } },
  )
  expect(within(zoneOf(view, 'Todo')).getByRole('button', { name: 'New task in Urgent' })).toBeTruthy()
  const duplicate = zoneOf(view, 'Duplicate')
  expect(within(duplicate).getByRole('button', { name: 'Collapse Urgent' })).toBeTruthy()
  // a count, not the element: printing a happy-dom element in a failure message exhausts memory
  expect(within(duplicate).queryAllByRole('button', { name: 'New task in Urgent' })).toHaveLength(0)
})

function dropOnCollapsedDoing(order: DisplayOptions['order_by']) {
  const writes = captureWrites()
  // Doing's only (first) sub-group is Urgent; the dragged task has no priority
  const view = renderList([task(1), { ...task(2), statusId: 'doing', priority: 'urgent' }], {
    statuses: [status, doingStatus], display: { sub_group_by: 'priority', order_by: order },
  })
  fireEvent.click(view.getByRole('button', { name: 'Collapse Doing' }))
  const doing = view.getByRole('button', { name: 'Expand Doing' }).closest('section')!
  fireEvent.dragStart(rowOf(view, 'Task 1'), { dataTransfer })
  fireEvent.dragOver(doing, { dataTransfer })
  expect(doing.hasAttribute('data-drop-over')).toBe(true)
  dropAt(doing, 0)
  return writes
}

test('a collapsed group with sub-groups still accepts drops; only the group field changes, appended at its end', async () => {
  const writes = dropOnCollapsedDoing('manual')
  await waitFor(() => expect(writes).toHaveLength(1))
  expect(writes[0]).toEqual({
    method: 'POST', path: '/api/v1/workspaces/workspace-1/tasks/bulk',
    body: { updates: [{ id: 'task-1', expected_version: 1, position: 3, status_id: 'doing' }] },
  })
})

test('with a non-manual order, a drop on a collapsed group with sub-groups patches only the group field', async () => {
  const writes = dropOnCollapsedDoing('created')
  await waitFor(() => expect(writes).toHaveLength(1))
  expect(writes[0]).toEqual({
    method: 'PATCH', path: '/api/v1/workspaces/workspace-1/tasks/task-1', body: { expected_version: 1, status_id: 'doing' },
  })
})

test('grouped by project with a non-manual order, rows cannot be dragged', () => {
  const view = renderList([task(1)], { display: { group_by: 'project', order_by: 'created' } })
  expect(rowOf(view, 'Task 1').getAttribute('draggable')).toBe('false')
})

const canonical = {
  id: 'task-91c0', workspace_id: 'workspace-1', project_id: 'project-1', status_id: 'todo', title: 'Login fails on Safari',
  description: '', position: 9, priority: 'none', assignee_ids: [], creator_id: 'user-1', label_ids: [],
  created_at: '', updated_at: '', version: 1, duplicate_of: null, blocked: false,
}
type Call = { method: string; path: string; body?: Record<string, unknown> }
function relationsApi(calls: Call[]) {
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    const path = new URL(request.url).pathname
    const body = request.method === 'GET' ? undefined : await request.json() as Record<string, unknown>
    calls.push({ method: request.method, path, body })
    if (path.endsWith('/projects')) return Response.json({ items: [launch], next_cursor: null })
    if (path.endsWith('/tasks/bulk')) {
      const updates = body!.updates as Array<{ id: string; expected_version: number }>
      return Response.json({ items: updates.map((update) => ({ ...canonical, id: update.id, version: update.expected_version + 1 })), next_cursor: null })
    }
    if (path.endsWith('/tasks')) return Response.json({ items: [canonical], next_cursor: null })
    return Response.json({ ...canonical, id: path.split('/').at(-1), version: 2 })
  }) as unknown as typeof fetch
}
const writes = (calls: Call[]) => calls.filter((call) => call.method !== 'GET')
const asDuplicate = (value: Task): Task => ({ ...value, statusId: 'dup', duplicateOf: { id: 'task-91c0', projectId: 'project-1', title: 'Login fails on Safari' } })

function viewWithDuplicate(tasks: Task[]) {
  return renderList(tasks, { statuses: [status, duplicateStatus] })
}

test('a blocked task shows the blocked flag in its properties', () => {
  const view = viewFor([{ ...task(1), blocked: true }, task(2)])
  expect(view.getAllByRole('img', { name: 'Blocked' })).toHaveLength(1)
})

test('choosing Duplicate in a row status menu opens the picker instead of saving', async () => {
  const calls: Call[] = []
  relationsApi(calls)
  const view = viewWithDuplicate([task(1)])
  fireEvent.click(view.getByRole('button', { name: 'Status: Todo' }))
  await userEvent.click(await view.findByRole('menuitem', { name: /Duplicate$/ }, { timeout: 5000 }))
  expect(await view.findByRole('dialog', { name: 'Mark ORB-1 as duplicate of…' })).toBeTruthy()
  expect(writes(calls)).toEqual([])
}, 20000)

test('moving a duplicate to another status is a plain status change, not the picker', async () => {
  const calls: Call[] = []
  relationsApi(calls)
  const view = viewWithDuplicate([asDuplicate(task(2))])
  fireEvent.click(view.getByRole('button', { name: 'Status: Duplicate' }))
  await userEvent.click(await view.findByRole('menuitem', { name: /Todo$/ }, { timeout: 5000 }))
  await waitFor(() => expect(writes(calls)[0]?.body).toEqual({ expected_version: 1, status_id: 'todo' }))
  expect(view.queryByRole('dialog')).toBeNull()
}, 20000)

test('dropping a row on the Duplicate group asks for the canonical task and writes nothing yet', async () => {
  const calls: Call[] = []
  relationsApi(calls)
  const view = viewWithDuplicate([task(1), asDuplicate(task(2))])
  expect(view.queryByRole('button', { name: 'New task in Duplicate' })).toBeNull()
  fireEvent.dragStart(rowOf(view, 'Task 1'), { dataTransfer })
  fireEvent.drop(zoneOf(view, 'Duplicate'), { dataTransfer })
  expect(await view.findByRole('dialog', { name: 'Mark ORB-1 as duplicate of…' })).toBeTruthy()
  expect(writes(calls)).toEqual([])
})

test('the bulk bar hides the Duplicate status and marks the selection in one call with an Undo toast', async () => {
  const calls: Call[] = []
  relationsApi(calls)
  const success = spyOn(toast, 'success').mockImplementation(() => 0)
  const view = viewWithDuplicate([task(1), task(2)])
  for (const checkbox of view.getAllByRole('checkbox')) fireEvent.click(checkbox)
  const toolbar = view.getByRole('toolbar', { name: 'Selected tasks' })

  fireEvent.click(within(toolbar).getByRole('button', { name: 'Status' }))
  await view.findByRole('menuitem', { name: /Todo$/ }, { timeout: 5000 })
  expect(view.queryByRole('menuitem', { name: /Duplicate$/ })).toBeNull()
  fireEvent.keyDown(document.body, { key: 'Escape' })
  await waitFor(() => expect(within(toolbar).getByRole('button', { name: 'Status' }).getAttribute('aria-expanded')).toBe('false'))

  const markDuplicate = within(toolbar).getByRole('button', { name: 'Mark as duplicate…' })
  // one word like the other bulk actions, so the bar keeps its width; the full action stays the accessible name
  expect(markDuplicate.textContent).toBe('Duplicate')
  fireEvent.click(markDuplicate)
  const picker = await view.findByRole('dialog', { name: 'Mark 2 tasks as duplicate of…' })
  fireEvent.click(await within(picker).findByRole('option', { name: /Login fails on Safari/ }))
  await waitFor(() => expect(writes(calls)).toHaveLength(1))
  expect(writes(calls)[0]).toEqual({ method: 'POST', path: '/api/v1/workspaces/workspace-1/tasks/bulk', body: { updates: [
    { id: 'task-1', expected_version: 1, duplicate_of_id: 'task-91c0' },
    { id: 'task-2', expected_version: 1, duplicate_of_id: 'task-91c0' },
  ] } })
  await waitFor(() => expect(success).toHaveBeenCalledTimes(1))
  expect(success.mock.calls[0]![0]).toBe('Marked 2 tasks as duplicate of ORB-91C0')
  success.mockRestore()
}, 20000)
