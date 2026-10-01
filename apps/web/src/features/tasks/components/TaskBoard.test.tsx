import { afterEach, expect, spyOn, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, createEvent, fireEvent, render, waitFor, within } from '@testing-library/react'
import type { ReactNode } from 'react'
import { toast } from 'sonner'
import type { Project, Task, TaskStatusDef } from '@/features/tasks/api/models'
import type { GroupContext } from '@/features/views/grouping'
import { DEFAULT_DISPLAY, type DisplayOptions } from '@/features/views/viewState'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import { TaskBoard } from './TaskBoard'
import { waitForAbsence } from '@/test/waitForAbsence'
import { testWorkspace } from '@/test/workspace'

const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
  window.localStorage.clear()
})

const workspace = testWorkspace()
const todo: TaskStatusDef = { id: 'todo', projectId: 'project-1', name: 'Todo', description: '', color: '#aaa', category: 'unstarted', position: 0, version: 1 }
const doing: TaskStatusDef = { id: 'doing', projectId: 'project-1', name: 'Doing', description: '', color: '#bbb', category: 'started', position: 1, version: 1 }
const duplicate: TaskStatusDef = { id: 'dup', projectId: 'project-1', name: 'Duplicate', description: '', color: '#8b8f98', category: 'duplicate', position: 2, version: 1 }
const launch = { id: 'project-1', workspace_id: 'workspace-1', name: 'Launch', key: 'ORB', color: '#e0457b', created_at: '', updated_at: '', version: 1 } as Project
const docs = { ...launch, id: 'project-2', name: 'Docs', key: 'DOC', color: '#26b5ce' } as Project

function task(id: string, statusId: string, position: number): Task {
  return {
    id, statusId, position, version: 1, projectId: 'project-1', title: id === 'moving' ? 'Moving' : id,
    description: '', identifier: `ORB-${position}`, priority: 'none', assigneeIds: [], creatorId: 'user-1',
    labels: [], attachments: [], dueAt: null, createdAt: '', updatedAt: '', comments: [], activity: [],
  }
}

type Write = { method: string; body: unknown }
function captureWrites(writes: Write[]) {
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    if (request.method !== 'GET') writes.push({ method: request.method, body: await request.json() })
    return Response.json({ items: [], next_cursor: null })
  }) as unknown as typeof fetch
}

function renderBoard(tasks: Task[], options: { statuses?: TaskStatusDef[]; display?: Partial<DisplayOptions>; scope?: string; onOpen?: (taskId: string) => void } = {}) {
  const statuses = options.statuses ?? [todo, doing]
  const groupContext: GroupContext = { statuses, members: [], labels: [], projects: [launch, docs], currentUserId: 'user-1', showEmpty: false }
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>
      <WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}>{children}</WorkspaceContext.Provider>
    </QueryClientProvider>
  )
  return render(
    <TaskBoard
      tasks={tasks} users={[]} labels={[]} statuses={statuses} projects={[launch, docs]}
      display={{ ...DEFAULT_DISPLAY, layout: 'board', ...options.display }} groupContext={groupContext}
      collapseScope={options.scope ?? 'all'} activeTaskId={null} onOpen={options.onOpen ?? (() => {})}
    />,
    { wrapper },
  )
}

const dataTransfer = { effectAllowed: '', dropEffect: '', setData: () => {} }
const cardOf = (view: ReturnType<typeof render>, title: string) => view.getByRole('heading', { name: title }).closest('article')!
const columnOf = (view: ReturnType<typeof render>, label: string) => view.getByText(label).closest('section')!
const cellOf = (view: ReturnType<typeof render>, zone: string) => view.container.querySelector<HTMLElement>(`[data-board-cell="${zone}"]`)!
function dropAt(target: Element, clientY: number) {
  const event = createEvent.drop(target, { dataTransfer })
  Object.defineProperty(event, 'clientY', { value: clientY })
  fireEvent(target, event)
}

test('mounted board rejects an oversized atomic reorder before any server commit, with the shared move-error toast', async () => {
  const writes: Write[] = []
  captureWrites(writes)
  const errorToast = spyOn(toast, 'error').mockImplementation(() => 0)
  const tasks = [task('moving', 'todo', 0), ...Array.from({ length: 101 }, (_, index) => task(`task-${index}`, 'doing', index))]
  const view = renderBoard(tasks)

  fireEvent.dragStart(cardOf(view, 'Moving'), { dataTransfer })
  dropAt(columnOf(view, 'Doing'), -1)

  await waitFor(() => expect(errorToast).toHaveBeenCalledTimes(1))
  expect(String(errorToast.mock.calls[0]![0])).toContain('at most 100')
  expect(writes).toHaveLength(0)
  // no board overlay and no Retry that could resend stale versions
  expect(view.queryAllByRole('alert')).toHaveLength(0)
  expect(view.queryAllByRole('button', { name: 'Retry' })).toHaveLength(0)
  errorToast.mockRestore()
})

test('dropping a card on the Duplicate column opens the picker; cancelling leaves the task where it was', async () => {
  const writes: Write[] = []
  captureWrites(writes)
  const view = renderBoard([task('moving', 'todo', 0)], { statuses: [todo, duplicate] })
  fireEvent.dragStart(cardOf(view, 'Moving'), { dataTransfer })
  fireEvent.drop(columnOf(view, 'Duplicate'), { dataTransfer })

  const picker = await view.findByRole('dialog', { name: 'Mark ORB-0 as duplicate of…' })
  fireEvent.keyDown(within(picker).getByPlaceholderText('Search tasks…'), { key: 'Escape' })
  await waitForAbsence(() => view.queryByRole('dialog'))
  expect(writes).toEqual([])
  expect(columnOf(view, 'Todo').contains(view.getByRole('heading', { name: 'Moving' }))).toBe(true)
})

test('reordering inside the Duplicate column is a plain position update, never the picker', async () => {
  const writes: Write[] = []
  captureWrites(writes)
  const dupA = { ...task('dupA', 'dup', 0), duplicateOf: { id: 'x', projectId: 'project-1', title: 'X' } }
  const dupB = { ...task('dupB', 'dup', 1), duplicateOf: { id: 'x', projectId: 'project-1', title: 'X' } }
  const view = renderBoard([dupA, dupB], { statuses: [todo, duplicate] })
  fireEvent.dragStart(cardOf(view, 'dupA'), { dataTransfer })
  dropAt(columnOf(view, 'Duplicate'), 10_000)

  await waitFor(() => expect(writes).toHaveLength(1))
  expect(view.queryAllByRole('dialog')).toHaveLength(0)
  const updates = (writes[0]!.body as { updates: Array<Record<string, unknown>> }).updates
  expect(updates.every((update) => !('status_id' in update) && !('duplicate_of_id' in update))).toBe(true)
})

test('a blocked card shows the blocked icon beside its identifier', () => {
  const view = renderBoard([{ ...task('late', 'todo', 0), blocked: true }])
  expect(view.getByRole('img', { name: 'Blocked' })).toBeTruthy()
})

test('saving a board move does not add a grid item or flash a loading label', async () => {
  let finishRequest: ((response: Response) => void) | undefined
  globalThis.fetch = (() => new Promise<Response>((resolve) => { finishRequest = resolve })) as unknown as typeof fetch
  const view = renderBoard([task('moving', 'todo', 0), task('other', 'todo', 1)], { statuses: [todo] })
  const board = view.container.firstElementChild!

  fireEvent.dragStart(cardOf(view, 'Moving'), { dataTransfer })
  dropAt(columnOf(view, 'Todo'), 10_000)

  await waitFor(() => expect(finishRequest).toBeDefined())
  expect(board.getAttribute('aria-busy')).toBe('true')
  expect(board.children).toHaveLength(1)
  expect(view.queryAllByText('Saving board order…')).toHaveLength(0)

  await act(async () => { finishRequest?.(Response.json({ items: [], next_cursor: null })) })
  await waitFor(() => expect(board.getAttribute('aria-busy')).toBe('false'))
  expect(board.children).toHaveLength(1)
  expect(view.queryAllByRole('alert')).toHaveLength(0)
})

test('cards render only the properties chosen in the display options', () => {
  const dated = { ...task('dated', 'todo', 7), dueAt: '2026-09-30T09:00:00.000Z' }
  const minimal = renderBoard([dated], { display: { properties: ['priority'] } })
  expect(minimal.queryAllByText('ORB-7')).toHaveLength(0)
  expect(minimal.getByRole('button', { name: 'Priority: No priority' })).toBeTruthy()
  expect(minimal.container.querySelectorAll('[data-property="due_date"]')).toHaveLength(0)
  minimal.unmount()

  const chosen = renderBoard([dated], { display: { properties: ['id', 'due_date', 'project'] } })
  expect(chosen.getByText('ORB-7')).toBeTruthy()
  expect(chosen.queryAllByRole('button', { name: /^Priority:/ })).toHaveLength(0)
  expect(chosen.container.querySelectorAll('[data-property="due_date"]')).toHaveLength(1)
  expect(chosen.container.querySelector('[data-property="project"]')!.textContent).toBe('Launch')
})

test('swim lanes: one collapsible lane per sub-group with a sticky label, cards in the matching cell', () => {
  const hot = { ...task('hot', 'doing', 1), priority: 'urgent' as const }
  const view = renderBoard([task('moving', 'todo', 0), hot], { display: { sub_group_by: 'priority' } })
  expect(within(cellOf(view, 'status=started:doing/priority=urgent')).getByRole('heading', { name: 'hot' })).toBeTruthy()
  expect(cellOf(view, 'status=unstarted:todo/priority=urgent').querySelectorAll('[data-board-card]')).toHaveLength(0)

  const label = view.getByRole('button', { name: 'Collapse Urgent' })
  expect(label.className).toContain('sticky')
  fireEvent.click(label)
  expect(view.queryAllByRole('heading', { name: 'hot' })).toHaveLength(0)
  expect(view.getByRole('button', { name: 'Expand Urgent' }).getAttribute('aria-expanded')).toBe('false')
  expect(view.getByRole('heading', { name: 'Moving' })).toBeTruthy()
})

test('a drop into another column and lane changes both fields in one write', async () => {
  const writes: Write[] = []
  captureWrites(writes)
  const hot = { ...task('hot', 'doing', 1), priority: 'urgent' as const }
  const view = renderBoard([task('moving', 'todo', 0), hot], { display: { sub_group_by: 'priority', order_by: 'priority' } })
  fireEvent.dragStart(cardOf(view, 'Moving'), { dataTransfer })
  fireEvent.drop(cellOf(view, 'status=started:doing/priority=urgent'), { dataTransfer })

  await waitFor(() => expect(writes).toHaveLength(1))
  expect(writes[0]).toEqual({ method: 'PATCH', body: { expected_version: 1, status_id: 'doing', priority: 'urgent' } })
})

test('a drop on a collapsed lane changes only the lane field and appends the card at the lane end', async () => {
  const writes: Write[] = []
  captureWrites(writes)
  const hot = { ...task('hot', 'doing', 1), priority: 'urgent' as const }
  const view = renderBoard([task('moving', 'todo', 0), hot], { display: { sub_group_by: 'priority' } })
  fireEvent.click(view.getByRole('button', { name: 'Collapse Urgent' }))

  fireEvent.dragStart(cardOf(view, 'Moving'), { dataTransfer })
  const collapsedLane = view.getByRole('button', { name: 'Expand Urgent' })
  fireEvent.dragOver(collapsedLane, { dataTransfer })
  expect(collapsedLane.closest('[data-drop-over]')).not.toBeNull()
  fireEvent.drop(collapsedLane, { dataTransfer })

  await waitFor(() => expect(writes).toHaveLength(1))
  expect(writes[0]).toEqual({ method: 'POST', body: { updates: [{ id: 'moving', expected_version: 1, position: 2, priority: 'urgent' }] } })
})

test('a refused drop shows a toast and writes nothing', async () => {
  const writes: Write[] = []
  captureWrites(writes)
  const errorToast = spyOn(toast, 'error').mockImplementation(() => 0)
  const docsTodo: TaskStatusDef = { ...todo, id: 'todo-2', projectId: 'project-2' }
  const view = renderBoard([{ ...task('moving', 'todo-2', 0), projectId: 'project-2' }], { statuses: [todo, doing, docsTodo] })
  fireEvent.dragStart(cardOf(view, 'Moving'), { dataTransfer })
  fireEvent.drop(columnOf(view, 'Doing'), { dataTransfer })

  await waitFor(() => expect(errorToast).toHaveBeenCalledTimes(1))
  expect(String(errorToast.mock.calls[0]![0])).toContain('No matching status')
  expect(writes).toEqual([])
  errorToast.mockRestore()
})

test('project columns never accept cards from another project', async () => {
  const writes: Write[] = []
  captureWrites(writes)
  const tasks = [task('moving', 'todo', 0), { ...task('other', 'todo', 1), projectId: 'project-2' }]
  const view = renderBoard(tasks, { display: { group_by: 'project', properties: ['id'] } })
  fireEvent.dragStart(cardOf(view, 'Moving'), { dataTransfer })
  // not a drop target: dragover is left uncancelled and no indicator shows
  expect(fireEvent.dragOver(columnOf(view, 'Docs'), { dataTransfer })).toBe(true)
  expect(view.container.querySelectorAll('[data-drop-over]')).toHaveLength(0)
  dropAt(columnOf(view, 'Docs'), 10_000)
  await act(async () => { await new Promise((resolve) => setTimeout(resolve, 20)) })
  expect(writes).toEqual([])
  expect(columnOf(view, 'Launch').contains(view.getByRole('heading', { name: 'Moving' }))).toBe(true)
})

test('grouped by project with a non-manual order, cards cannot be dragged', () => {
  const view = renderBoard([task('moving', 'todo', 0)], { display: { group_by: 'project', order_by: 'created', properties: ['id'] } })
  expect(cardOf(view, 'Moving').getAttribute('draggable')).toBe('false')
})

test('the dragged card stays mounted while dragging', () => {
  const view = renderBoard([task('moving', 'todo', 0), task('other', 'todo', 1)])
  const card = cardOf(view, 'Moving')
  fireEvent.dragStart(card, { dataTransfer })
  expect(card.isConnected).toBe(true)
  expect(card.getAttribute('data-dragging')).toBe('true')
})

test('cards show the parent above the title and sub-issue progress', () => {
  const opened: string[] = []
  const view = renderBoard([
    { ...task('child', 'todo', 1), parentTaskId: 'parent', parent: { id: 'parent', title: 'Checkout redesign', projectKey: 'ORB' } },
    { ...task('parent', 'todo', 2), subIssueCount: 3, subIssueClosedCount: 1 },
  ], { onOpen: (id) => opened.push(id) })
  fireEvent.click(within(cardOf(view, 'child')).getByRole('button', { name: 'Checkout redesign' }))
  expect(opened).toEqual(['parent'])
  expect(within(cardOf(view, 'parent')).getByRole('img', { name: '1 of 3 sub-issues closed' })).toBeTruthy()
})

test('dropping a card on the middle of another card makes it a sub-issue', async () => {
  const writes: Write[] = []
  captureWrites(writes)
  const view = renderBoard([task('moving', 'todo', 1), task('other', 'doing', 2)])
  fireEvent.dragStart(cardOf(view, 'Moving'), { dataTransfer })
  dropAt(cardOf(view, 'other'), 0)
  await waitFor(() => expect(writes).toHaveLength(1))
  expect(writes[0]).toEqual({ method: 'PATCH', body: { expected_version: 1, parent_task_id: 'other' } })
})

function overAt(target: Element, clientY: number) {
  const event = createEvent.dragOver(target, { dataTransfer })
  Object.defineProperty(event, 'clientY', { value: clientY })
  fireEvent(target, event)
}
const placeholders = (view: ReturnType<typeof render>) => Array.from(view.container.querySelectorAll('[data-board-placeholder]'))

test('manual order: a card taking the hover over holds the placeholder in place (hidden) instead of removing it', async () => {
  const writes: Write[] = []
  captureWrites(writes)
  const view = renderBoard([task('moving', 'todo', 1), task('other', 'doing', 2), task('third', 'doing', 3)])
  fireEvent.dragStart(cardOf(view, 'Moving'), { dataTransfer })
  overAt(columnOf(view, 'Doing'), 0)
  expect(placeholders(view)).toHaveLength(1)
  expect(placeholders(view)[0]!.hasAttribute('data-held')).toBe(false)
  expect(columnOf(view, 'Doing').hasAttribute('data-drop-over')).toBe(true)

  // the pointer moves into the middle of a card: the slot stays (no layout shift under the pointer), hidden
  overAt(cardOf(view, 'other'), 0)
  expect(cardOf(view, 'other').getAttribute('data-nest')).toBe('inside')
  expect(placeholders(view)).toHaveLength(1)
  expect(placeholders(view)[0]!.hasAttribute('data-held')).toBe(true)
  expect(placeholders(view)[0]!.className).toContain('data-held:invisible')
  expect(placeholders(view)[0]!.previousElementSibling!.contains(cardOf(view, 'third'))).toBe(true)
  expect(columnOf(view, 'Doing').hasAttribute('data-drop-over')).toBe(false)

  // back over the column: the slot shows again
  overAt(columnOf(view, 'Doing'), 0)
  expect(placeholders(view)[0]!.hasAttribute('data-held')).toBe(false)

  overAt(cardOf(view, 'other'), 0)
  dropAt(cardOf(view, 'other'), 0)
  await waitFor(() => expect(writes).toHaveLength(1))
  expect(writes[0]).toEqual({ method: 'PATCH', body: { expected_version: 1, parent_task_id: 'other' } })
  expect(placeholders(view)).toHaveLength(0)
})

test('nest-target cards skip the hover styles, so the tint always wins', () => {
  const view = renderBoard([task('moving', 'todo', 1)])
  const hoverTokens = cardOf(view, 'Moving').className.split(' ').filter((name) => name.includes('hover:'))
  expect(hoverTokens.length).toBeGreaterThan(0)
  expect(hoverTokens.filter((name) => !name.includes('not-data-[nest=inside]:'))).toHaveLength(0)
})
