import { expect, mock, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render } from '@testing-library/react'
import type { ReactNode } from 'react'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import type { Task, TaskViewState } from '@/features/tasks/api/models'
import { ActivityFeed } from './ActivityFeed'
import { testWorkspace } from '@/test/workspace'
import { NO_MEMBER_ABILITIES } from '@/features/workspaces/models'

const workspace = testWorkspace()

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
  activity: Array.from({ length: 7 }, (_, index) => ({
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
    title: '', roleIds: [], can: NO_MEMBER_ABILITIES, version: 1,
  }],
  statuses: [], labels: [], tasks: [task],
}

test('shows the first and the latest activities, with the hidden ones behind a row between them', () => {
  const view = render(<ActivityFeed task={task} state={state} />, { wrapper: Wrapper })

  expect(view.getByText(/Activity 1/)).toBeTruthy()
  expect(view.queryAllByText(/Activity [23]\b/)).toHaveLength(0)
  expect(view.getByText(/Activity 4/)).toBeTruthy()
  expect(view.getByText(/Activity 7/)).toBeTruthy()

  const toggle = view.getByRole('button', { name: 'Show 2 more' })
  expect(toggle.getAttribute('aria-expanded')).toBe('false')
  fireEvent.click(toggle)

  expect(view.getByText(/Activity 2/)).toBeTruthy()
  // the row sits right after the first activity
  expect(view.getAllByRole('listitem').map((row) => row.textContent?.includes('Show less')).indexOf(true)).toBe(1)
  expect(view.getByRole('button', { name: 'Show less' }).getAttribute('aria-expanded')).toBe('true')
})

test('shows a service account as the activity actor', () => {
  const serviceTask = {
    ...task,
    activity: [{
      id: 'service-activity', actorId: '', actorName: 'Discord', actorServiceAccountId: 'service-1',
      text: 'created task', createdAt: '2026-09-17T10:00:00.000Z',
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
      id: 'relation-activity', actorId: 'user-1', text: 'added blocker ORB-77AA',
      related: { taskId: 'task-77aa', identifier: 'ORB-77AA' }, createdAt: '2026-09-17T10:00:00.000Z',
    }],
  }
  const view = render(<ActivityFeed task={relationTask} state={{ ...state, tasks: [relationTask] }} onOpenTask={(id) => opened.push(id)} />, { wrapper: Wrapper })
  expect(view.getByRole('listitem').textContent).toContain('added blocker ORB-77AA')
  fireEvent.click(view.getByRole('button', { name: 'ORB-77AA' }))
  expect(opened).toEqual(['task-77aa'])
})
