import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, createEvent, fireEvent, render, waitFor, within } from '@testing-library/react'
import type { ReactNode } from 'react'
import type { WorkspaceRecord } from '@/api/generated/types.gen'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import type { Task, TaskStatusDef } from '@/features/tasks/api/models'
import { statusGroups, type StatusGroup } from '@/features/tasks/tasksLib'
import { TaskBoard } from './TaskBoard'
import { waitForAbsence } from '@/test/waitForAbsence'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

function task(id: string, statusId: string, position: number): Task {
  return {
    id, statusId, position, version: 1, projectId: 'project-1', title: id === 'moving' ? 'Moving' : id,
    description: '', identifier: `ORB-${position}`, priority: 'none', assigneeIds: [], creatorId: 'user-1',
    labels: [], attachments: [], dueAt: null, createdAt: '', updatedAt: '', comments: [], activity: [],
  }
}

test('mounted board rejects an oversized atomic reorder before any server commit', async () => {
  const requestBodies: Array<{ updates: Array<{ position?: number | null }> }> = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    requestBodies.push(await request.json())
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
  const dropEvent = createEvent.drop(doing, { dataTransfer })
  Object.defineProperty(dropEvent, 'clientY', { value: -1 })
  fireEvent(doing, dropEvent)

  const error = await view.findByRole('alert')
  expect(error.textContent).toContain('at most 100')
  expect(error.classList.contains('absolute')).toBe(true)
  expect(requestBodies).toHaveLength(0)
  expect(view.queryByRole('button', { name: 'Retry' })).toBeNull()
})

const boardWorkspace: WorkspaceRecord = { id: 'workspace-1', name: 'Orbit', role: 'owner', version: 1 }
const boardStatuses: TaskStatusDef[] = [
  { id: 'todo', projectId: 'project-1', name: 'Todo', description: '', color: '#aaa', category: 'unstarted', position: 0, version: 1 },
  { id: 'dup', projectId: 'project-1', name: 'Duplicate', description: '', color: '#8b8f98', category: 'duplicate', position: 1, version: 1 },
]

function renderBoard(tasks: Task[], writes: Array<{ method: string; body: unknown }>) {
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    if (request.method !== 'GET') writes.push({ method: request.method, body: await request.json() })
    return Response.json({ items: [], next_cursor: null })
  }) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>
      <WorkspaceContext.Provider value={{ workspace: boardWorkspace, workspaces: [boardWorkspace], selectWorkspace: () => {} }}>{children}</WorkspaceContext.Provider>
    </QueryClientProvider>
  )
  return render(
    <TaskBoard tasks={tasks} users={[]} labels={[]} statuses={boardStatuses} groups={statusGroups(boardStatuses, 'project-1')} sort="manual" activeTaskId={null} onOpen={() => {}} />,
    { wrapper },
  )
}

const dataTransfer = { effectAllowed: '', dropEffect: '', setData: () => {} }

test('dropping a card on the Duplicate column opens the picker; cancelling leaves the task where it was', async () => {
  const writes: Array<{ method: string; body: unknown }> = []
  const view = renderBoard([task('moving', 'todo', 0)], writes)
  const moving = view.getByRole('heading', { name: 'Moving' }).closest('article')!
  const duplicateColumn = view.getByText('Duplicate').closest('section')!
  fireEvent.dragStart(moving, { dataTransfer })
  fireEvent.drop(duplicateColumn, { dataTransfer })

  const picker = await view.findByRole('dialog', { name: 'Mark ORB-0 as duplicate of…' })
  fireEvent.keyDown(within(picker).getByPlaceholderText('Search tasks…'), { key: 'Escape' })
  await waitForAbsence(() => view.queryByRole('dialog'))
  expect(writes).toEqual([])
  expect(view.getByText('Todo').closest('section')!.contains(view.getByRole('heading', { name: 'Moving' }))).toBe(true)
})

test('reordering inside the Duplicate column is a plain position update, never the picker', async () => {
  const writes: Array<{ method: string; body: unknown }> = []
  const dupA = { ...task('dupA', 'dup', 0), duplicateOf: { id: 'x', projectId: 'project-1', title: 'X' } }
  const dupB = { ...task('dupB', 'dup', 1), duplicateOf: { id: 'x', projectId: 'project-1', title: 'X' } }
  const view = renderBoard([dupA, dupB], writes)
  const card = view.getByRole('heading', { name: 'dupA' }).closest('article')!
  const duplicateColumn = view.getByText('Duplicate').closest('section')!
  fireEvent.dragStart(card, { dataTransfer })
  const dropEvent = createEvent.drop(duplicateColumn, { dataTransfer })
  Object.defineProperty(dropEvent, 'clientY', { value: 10_000 })
  fireEvent(duplicateColumn, dropEvent)

  await waitFor(() => expect(writes).toHaveLength(1))
  expect(view.queryByRole('dialog')).toBeNull()
  const updates = (writes[0]!.body as { updates: Array<Record<string, unknown>> }).updates
  expect(updates.every((update) => !('status_id' in update) && !('duplicate_of_id' in update))).toBe(true)
})

test('a blocked card shows the blocked flag in its properties', () => {
  const view = renderBoard([{ ...task('late', 'todo', 0), blocked: true }], [])
  expect(view.getByRole('img', { name: 'Blocked' })).toBeTruthy()
})

test('saving a board move does not add a grid item or flash a loading label', async () => {
  let finishRequest: ((response: Response) => void) | undefined
  globalThis.fetch = (() => new Promise<Response>((resolve) => { finishRequest = resolve })) as unknown as typeof fetch
  const workspace: WorkspaceRecord = { id: 'workspace-1', name: 'Orbit', role: 'owner', version: 1 }
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>
      <WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}>
        {children}
      </WorkspaceContext.Provider>
    </QueryClientProvider>
  )
  const status: TaskStatusDef = { id: 'todo', projectId: 'project-1', name: 'Todo', description: '', color: '#aaa', category: 'unstarted', position: 0, version: 1 }
  const group: StatusGroup = { key: 'unstarted:todo', name: 'Todo', category: 'unstarted', status, statusIds: [status.id] }
  const view = render(
    <TaskBoard tasks={[task('moving', 'todo', 0), task('other', 'todo', 1)]} users={[]} labels={[]} statuses={[status]} groups={[group]} sort="manual" activeTaskId={null} onOpen={() => {}} />,
    { wrapper },
  )
  const board = view.container.firstElementChild!
  const moving = view.getByRole('heading', { name: 'Moving' }).closest('article')!
  const column = view.getByText('Todo').closest('section')!

  fireEvent.dragStart(moving, { dataTransfer })
  const dropEvent = createEvent.drop(column, { dataTransfer })
  Object.defineProperty(dropEvent, 'clientY', { value: 10000 })
  fireEvent(column, dropEvent)

  await waitFor(() => expect(finishRequest).toBeDefined())
  expect(board.getAttribute('aria-busy')).toBe('true')
  expect(board.children).toHaveLength(1)
  expect(view.queryByText('Saving board order…')).toBeNull()

  await act(async () => { finishRequest?.(Response.json({ items: [], next_cursor: null })) })
  await waitFor(() => expect(board.getAttribute('aria-busy')).toBe('false'))
  expect(board.children).toHaveLength(1)
  expect(view.queryByRole('alert')).toBeNull()
})
