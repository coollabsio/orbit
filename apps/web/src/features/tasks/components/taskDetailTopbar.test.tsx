import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, within } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import type { WorkspaceRecord } from '../../../api/generated/types.gen'
import { TopbarSlotProvider, useTopbarSlotTarget } from '../../../components/shell/TopbarSlot'
import { WorkspaceContext } from '../../workspaces/workspaceContext'
import type { Task, TaskViewState } from '../api/models'
import { TaskDetail } from './TaskDetail'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

const workspace: WorkspaceRecord = { id: 'workspace-1', name: 'Orbit', role: 'owner', version: 1 }

const task: Task = {
  id: 'task-1', identifier: 'ORB-1', title: 'Ship it', descriptionJson: { type: 'doc', content: [] }, descriptionText: '', statusId: 'todo',
  position: 0, priority: 'none', assigneeIds: [], creatorId: 'user-1', projectId: 'project-1',
  labels: [], attachments: [], dueAt: null, parentId: null, subIssueTotal: 0, subIssueDone: 0, duplicateOfTaskId: null, duplicateIds: [], referencedBy: [], createdAt: '', updatedAt: '', comments: [], activity: [], version: 1,
}

const state: TaskViewState = {
  currentUserId: 'user-1',
  users: [],
  statuses: [{ id: 'todo', projectId: 'project-1', name: 'Todo', description: '', color: '#8b5cf6', category: 'unstarted', position: 0, version: 1 }],
  labels: [],
  tasks: [task],
}

function SlotHost() {
  const left = useTopbarSlotTarget('left')
  const right = useTopbarSlotTarget('right')
  return (
    <>
      <div data-testid="left-target" ref={left} />
      <div data-testid="right-target" ref={right} />
    </>
  )
}

function renderDetail() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}>
          <TopbarSlotProvider>
            <SlotHost />
            <TaskDetail task={task} project={undefined} projects={[]} state={state} onBack={() => {}} onOpenTask={() => {}} />
          </TopbarSlotProvider>
        </WorkspaceContext.Provider>
      </MemoryRouter>
    </QueryClientProvider>,
  )
}

test('task detail publishes its identifier crumb and status pill into the topbar', () => {
  const view = renderDetail()
  const slot = within(view.getByTestId('left-target'))

  expect(slot.getByText('ORB-1')).not.toBeNull()
  expect(slot.getByText('Todo').classList.contains('topbar-status')).toBe(true)
})

test('the detail actions live in the topbar, not a second header row', () => {
  const view = renderDetail()
  const right = within(view.getByTestId('right-target'))

  expect(right.getByRole('button', { name: 'More task actions' })).not.toBeNull()
  expect(right.getByRole('button', { name: 'Close task' })).not.toBeNull()
})

test('the duplicate pane header and its back arrow are gone', () => {
  const view = renderDetail()

  expect(view.container.querySelector('.tasks-detail-pane > .pane-header')).toBeNull()
  expect(view.queryByRole('button', { name: 'Back to tasks' })).toBeNull()
  expect(view.queryByRole('button', { name: 'Delete' })).toBeNull()
})

test('a sub-issue puts its parent crumb before its own identifier', () => {
  const parent: Task = { ...task, id: 'task-9', identifier: 'ORB-9' }
  const child: Task = { ...task, id: 'task-12', identifier: 'ORB-12', parentId: 'task-9' }
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const view = render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}>
          <TopbarSlotProvider>
            <SlotHost />
            <TaskDetail task={child} project={undefined} projects={[]} state={{ ...state, tasks: [parent, child] }} onBack={() => {}} onOpenTask={() => {}} />
          </TopbarSlotProvider>
        </WorkspaceContext.Provider>
      </MemoryRouter>
    </QueryClientProvider>,
  )
  const slot = within(view.getByTestId('left-target'))
  expect(slot.getByRole('link', { name: 'ORB-9' }).getAttribute('href')).toBe('/tasks/task-9')
  expect(slot.getByText('ORB-12').getAttribute('data-current')).toBe('true')
})

test('a parent that is off the page and answers 404 shows as "In trash"', async () => {
  globalThis.fetch = (async () => Response.json({
    type: 'about:blank', title: 'Task resource not found', status: 404, detail: 'gone',
    code: 'task_resource_not_found', instance: '/x', request_id: 'r',
  }, { status: 404, headers: { 'content-type': 'application/problem+json' } })) as unknown as typeof fetch
  const child: Task = { ...task, id: 'task-12', identifier: 'ORB-12', parentId: 'task-9' }
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const view = render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}>
          <TopbarSlotProvider>
            <SlotHost />
            <TaskDetail task={child} project={undefined} projects={[]} state={{ ...state, tasks: [child] }} onBack={() => {}} onOpenTask={() => {}} />
          </TopbarSlotProvider>
        </WorkspaceContext.Provider>
      </MemoryRouter>
    </QueryClientProvider>,
  )
  const slot = within(view.getByTestId('left-target'))
  const crumb = await slot.findByText('In trash')
  expect(crumb.getAttribute('data-muted')).toBe('true')
  expect(slot.queryByRole('link')).toBeNull()
})
