import { afterEach, expect, spyOn, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, fireEvent, render, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import type { ReactNode } from 'react'
import type { WorkspaceRecord } from '@/api/generated/types.gen'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import type { Task, TaskStatusDef } from '@/features/tasks/api/models'
import type { User } from '@/features/workspaces/models'
import { statusGroups, type StatusGroup } from '@/features/tasks/tasksLib'
import { toast } from 'sonner'
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
const groups: StatusGroup[] = [{
  key: 'unstarted:todo', name: 'Todo', category: 'unstarted', status, statusIds: [status.id],
}]
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

function viewFor(tasks: Task[], onOpen = () => {}, users: User[] = []) {
  return render(<TaskList tasks={tasks} users={users} labels={[]} statuses={[status]} groups={groups} sort="manual" onOpen={onOpen} onAdd={() => {}} />, { wrapper })
}

async function chooseUrgent(view: ReturnType<typeof render>) {
  const toolbar = view.getByRole('toolbar', { name: 'Selected tasks' })
  // fireEvent opens the Base UI trigger (userEvent would double-toggle it); userEvent
  // then clicks the option, waiting until the opened popover is actually actionable.
  fireEvent.click(within(toolbar).getByRole('button', { name: 'Priority' }))
  await userEvent.click(await view.findByRole('menuitem', { name: /^Urgent/ }, { timeout: 5000 }))
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

const duplicateStatus: TaskStatusDef = {
  id: 'dup', projectId: 'project-1', name: 'Duplicate', description: '', color: '#8b8f98',
  category: 'duplicate', position: 1, version: 1,
}
const listProject = { id: 'project-1', workspace_id: 'workspace-1', name: 'Launch', key: 'ORB', color: '#e0457b', created_at: '', updated_at: '', version: 1 }
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
    if (path.endsWith('/projects')) return Response.json({ items: [listProject], next_cursor: null })
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
  const statuses = [status, duplicateStatus]
  return render(<TaskList tasks={tasks} users={[]} labels={[]} statuses={statuses} groups={statusGroups(statuses, 'project-1')} sort="manual" onOpen={() => {}} onAdd={() => {}} />, { wrapper })
}

test('a blocked task shows the blocked icon beside its identifier', () => {
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
  const row = view.getByText('Task 1').closest('[draggable="true"]')!
  const duplicateGroup = view.getByRole('button', { name: 'Collapse Duplicate' }).closest('section')!
  const dataTransfer = { effectAllowed: '', dropEffect: '', setData: () => {}, getData: () => 'task-1' }
  fireEvent.dragStart(row, { dataTransfer })
  fireEvent.drop(duplicateGroup, { dataTransfer })
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
