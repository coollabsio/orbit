import { expect, mock, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render } from '@testing-library/react'
import type { ReactNode } from 'react'
import { MemoryRouter } from 'react-router'
import type { WorkspaceRecord } from '../../../api/generated/types.gen'
import { WorkspaceContext } from '../../workspaces/workspaceContext'
import type { Task, TaskViewState } from '../api/models'
import { ActivityFeed } from './ActivityFeed'

const workspace: WorkspaceRecord = { id: 'workspace-1', name: 'Orbit', role: 'owner', version: 1 }

function Wrapper({ children }: { children: ReactNode }) {
  return (
    <QueryClientProvider client={new QueryClient()}>
      <MemoryRouter>
        <WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: mock(() => {}) }}>
          {children}
        </WorkspaceContext.Provider>
      </MemoryRouter>
    </QueryClientProvider>
  )
}

const task: Task = {
  id: 'task-1', identifier: 'ORB-1', title: 'Activity test', descriptionJson: { type: 'doc', content: [] }, descriptionText: '', statusId: 'todo',
  position: 0, priority: 'none', assigneeIds: [], creatorId: 'user-1', projectId: 'project-1',
  labels: [], attachments: [], dueAt: null, parentId: null, subIssueTotal: 0, subIssueDone: 0, duplicateOfTaskId: null, duplicateIds: [], referencedBy: [], createdAt: '', updatedAt: '', comments: [], version: 1,
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

test('the reply composer is a rich text surface with the @ menu, like the top-level composer', () => {
  const threaded: Task = {
    ...task,
    activity: [],
    comments: [{
      id: 'comment-1', authorId: 'user-1', createdAt: '2026-09-17T10:00:00.000Z', version: 0,
      bodyJson: { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text: 'Root comment' }] }] },
      bodyText: 'Root comment',
    }],
  }
  const view = render(<ActivityFeed task={threaded} state={{ ...state, tasks: [threaded] }} />, { wrapper: Wrapper })

  expect(view.getByText('Root comment')).toBeTruthy()
  const composer = view.container.querySelector('.tasks-thread-composer')
  expect(composer?.querySelector('.editor-shell')).toBeTruthy()
  expect(composer?.querySelector('textarea')).toBeNull()
  expect(view.getAllByLabelText('Leave a reply…').length).toBeGreaterThan(0)
})

test('the reply composer is handed the workspace members for mentions', async () => {
  const source = await Bun.file(new URL('./ActivityFeed.tsx', import.meta.url)).text()
  const reply = source.slice(source.indexOf('placeholder="Leave a reply…"'))

  expect(reply.slice(0, reply.indexOf('/>'))).toContain('members={state.users}')
})
