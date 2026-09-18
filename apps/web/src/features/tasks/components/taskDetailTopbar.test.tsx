import { expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, within } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import type { WorkspaceRecord } from '../../../api/generated/types.gen'
import { TopbarSlotProvider, useTopbarSlotTarget } from '../../../components/shell/TopbarSlot'
import { WorkspaceContext } from '../../workspaces/workspaceContext'
import type { Task, TaskViewState } from '../api/models'
import { TaskDetail } from './TaskDetail'

const workspace: WorkspaceRecord = { id: 'workspace-1', name: 'Orbit', role: 'owner', version: 1 }

const task: Task = {
  id: 'task-1', identifier: 'ORB-1', title: 'Ship it', description: '', statusId: 'todo',
  position: 0, priority: 'none', assigneeIds: [], creatorId: 'user-1', projectId: 'project-1',
  labels: [], attachments: [], dueAt: null, createdAt: '', updatedAt: '', comments: [], activity: [], version: 1,
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
            <TaskDetail task={task} project={undefined} state={state} onBack={() => {}} />
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
