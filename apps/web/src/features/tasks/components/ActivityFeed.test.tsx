import { expect, mock, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render } from '@testing-library/react'
import type { ReactNode } from 'react'
import type { WorkspaceRecord } from '@/api/generated/types.gen'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import type { Task, TaskViewState } from '@/features/tasks/api/models'
import { ActivityFeed } from './ActivityFeed'

const workspace: WorkspaceRecord = { id: 'workspace-1', name: 'Orbit', role: 'owner', version: 1 }

function Wrapper({ children }: { children: ReactNode }) {
  return (
    <QueryClientProvider client={new QueryClient()}>
      <WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: mock(() => {}) }}>
        {children}
      </WorkspaceContext.Provider>
    </QueryClientProvider>
  )
}

const task: Task = {
  id: 'task-1', identifier: 'ORB-1', title: 'Activity test', description: '', statusId: 'todo',
  position: 0, priority: 'none', assigneeIds: [], creatorId: 'user-1', projectId: 'project-1',
  labels: [], attachments: [], dueAt: null, createdAt: '', updatedAt: '', comments: [], version: 1,
  activity: Array.from({ length: 5 }, (_, index) => ({
    id: `activity-${index + 1}`,
    actorId: 'user-1',
    text: `Activity ${index + 1}`,
    createdAt: `2026-09-17T10:0${index}:00.000Z`,
  })),
}
const state: TaskViewState = {
  currentUserId: 'user-1',
  users: [{
    id: 'user-1', membershipId: 'membership-1', name: 'Andras', handle: 'andras',
    email: 'andras@example.com', role: 'Owner', color: '#16a34a', online: true,
    title: '', roleIds: [], version: 1,
  }],
  statuses: [], labels: [], tasks: [task],
}

test('shows the latest three activities and expands the full list', () => {
  const view = render(<ActivityFeed task={task} state={state} />, { wrapper: Wrapper })

  expect(view.queryByText(/Activity 1/)).toBeNull()
  expect(view.getByText(/Activity 3/)).toBeTruthy()
  expect(view.getByText(/Activity 5/)).toBeTruthy()

  const toggle = view.getByRole('button', { name: 'Show all activities' })
  expect(toggle.getAttribute('aria-expanded')).toBe('false')
  fireEvent.click(toggle)

  expect(view.getByText(/Activity 1/)).toBeTruthy()
  expect(view.getByRole('button', { name: 'Show fewer activities' }).getAttribute('aria-expanded')).toBe('true')
})

test('shows a service account as the activity actor', () => {
  const serviceTask = {
    ...task,
    activity: [{
      id: 'service-activity', actorId: '', actorName: 'Discord', actorServiceAccountId: 'service-1',
      text: 'Created task', createdAt: '2026-09-17T10:00:00.000Z',
    }],
  }
  const view = render(<ActivityFeed task={serviceTask} state={{ ...state, tasks: [serviceTask] }} />, { wrapper: Wrapper })

  expect(view.getByText('Discord')).toBeTruthy()
  expect(view.queryByText('Andras')).toBeNull()
})

test('relation activity links the other task', () => {
  const opened: string[] = []
  const relationTask: Task = {
    ...task,
    activity: [{
      id: 'relation-activity', actorId: 'user-1', text: 'Added blocker ORB-77AA',
      related: { taskId: 'task-77aa', identifier: 'ORB-77AA' }, createdAt: '2026-09-17T10:00:00.000Z',
    }],
  }
  const view = render(<ActivityFeed task={relationTask} state={{ ...state, tasks: [relationTask] }} onOpenTask={(id) => opened.push(id)} />, { wrapper: Wrapper })
  expect(view.getByRole('listitem').textContent).toContain('Added blocker ORB-77AA')
  fireEvent.click(view.getByRole('button', { name: 'ORB-77AA' }))
  expect(opened).toEqual(['task-77aa'])
})
