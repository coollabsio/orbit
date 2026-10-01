import { expect, mock, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render } from '@testing-library/react'
import type { ReactNode } from 'react'
import { MemoryRouter } from 'react-router'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import type { TaskComment, TaskViewState } from '@/features/tasks/api/models'
import type { User } from '@/features/workspaces/models'
import { CommentItem } from './CommentItem'
import { testWorkspace } from '@/test/workspace'
import { NO_MEMBER_ABILITIES } from '@/features/workspaces/models'

const author: User = {
  id: 'user-1', membershipId: 'm1', name: 'Orbit Developers', handle: 'orbit', email: 'dev@orbit.test',
  role: 'Owner', color: '#4ade80', online: true, title: '', roleIds: [], can: NO_MEMBER_ABILITIES, version: 1,
}

function wrapper() {
  const workspace = testWorkspace()
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  return ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: mock(() => {}) }}>
          {children}
        </WorkspaceContext.Provider>
      </MemoryRouter>
    </QueryClientProvider>
  )
}

function comment(overrides: Partial<TaskComment> = {}): TaskComment {
  return { id: 'c1', authorId: 'user-1', body: 'hello', canEdit: true, canDelete: true, createdAt: new Date().toISOString(), version: 1, ...overrides }
}

function state(overrides: Partial<TaskViewState> = {}): TaskViewState {
  return { currentUserId: 'user-1', users: [author], statuses: [], labels: [], tasks: [], ...overrides }
}

test('comment header keeps author name and timestamp as separate text', () => {
  const view = render(<CommentItem state={state()} taskId="task-1" comment={comment()} mentionTokens={[]} />, { wrapper: wrapper() })

  expect(view.getByText('Orbit Developers')).toBeTruthy()
  expect(view.getByText('just now')).toBeTruthy()
  expect(view.container.textContent).not.toContain('Orbit Developersjust now')
})

test('author comments expose compact copy, edit, and delete actions', () => {
  const view = render(<CommentItem state={state()} taskId="task-1" comment={comment()} mentionTokens={[]} />, { wrapper: wrapper() })

  expect(view.getByRole('button', { name: 'Copy text' })).toBeTruthy()
  expect(view.getByRole('button', { name: 'Edit comment' })).toBeTruthy()
  expect(view.getByRole('button', { name: 'Delete comment' })).toBeTruthy()
})

test('a comment the server marks as delete-only shows Delete without Edit', () => {
  const view = render(
    <CommentItem state={state({ currentUserId: 'user-2' })} taskId="task-1" comment={comment({ canEdit: false })} mentionTokens={[]} />,
    { wrapper: wrapper() },
  )

  expect(view.queryAllByRole('button', { name: 'Edit comment' })).toHaveLength(0)
  expect(view.getByRole('button', { name: 'Delete comment' })).toBeTruthy()
})

test('a comment the caller may not change offers only Copy', () => {
  const view = render(
    <CommentItem state={state({ currentUserId: 'user-2' })} taskId="task-1" comment={comment({ canEdit: false, canDelete: false })} mentionTokens={[]} />,
    { wrapper: wrapper() },
  )

  expect(view.getByRole('button', { name: 'Copy text' })).toBeTruthy()
  expect(view.queryByRole('button', { name: 'Edit comment' })).toBeNull()
  expect(view.queryByRole('button', { name: 'Delete comment' })).toBeNull()
})

test('edit mode replaces the body with a save/cancel textarea', () => {
  const view = render(<CommentItem state={state()} taskId="task-1" comment={comment()} mentionTokens={[]} />, { wrapper: wrapper() })

  fireEvent.click(view.getByRole('button', { name: 'Edit comment' }))
  expect(view.getByLabelText('Edit comment')).toBeTruthy()
  expect(view.getByText('escape').tagName).toBe('KBD')
  expect(view.getByText('cancel')).toBeTruthy()
  expect(view.queryByRole('button', { name: 'Copy text' })).toBeNull()
})
