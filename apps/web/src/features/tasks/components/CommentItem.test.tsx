import { afterEach, expect, mock, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render } from '@testing-library/react'
import type { ReactNode } from 'react'
import { MemoryRouter } from 'react-router'
import type { WorkspaceRecord } from '../../../api/generated/types.gen'
import { WorkspaceContext } from '../../workspaces/workspaceContext'
import type { TaskComment, TaskViewState, User } from '../api/models'
import { CommentItem } from './CommentItem'

const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
})

const workspace: WorkspaceRecord = { id: 'workspace-1', name: 'Orbit', role: 'owner', version: 1 }
const author: User = {
  id: 'user-1', membershipId: 'm1', name: 'Orbit Developers', handle: 'orbit', email: 'dev@orbit.test',
  role: 'Owner', color: '#4ade80', online: true, title: '', roleIds: [], version: 1,
}

function wrapper() {
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
  return { id: 'c1', authorId: 'user-1', bodyJson: { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text: 'hello' }] }] }, bodyText: 'hello', createdAt: new Date().toISOString(), version: 1, ...overrides }
}

function state(overrides: Partial<TaskViewState> = {}): TaskViewState {
  return { currentUserId: 'user-1', users: [author], statuses: [], labels: [], tasks: [], ...overrides }
}

test('comment header keeps author name and timestamp as separate text', () => {
  const view = render(<CommentItem state={state()} taskId="task-1" comment={comment()} workspaceId="workspace-1" statuses={[]} />, { wrapper: wrapper() })

  expect(view.getByText('Orbit Developers')).toBeTruthy()
  expect(view.getByText('just now')).toBeTruthy()
  expect(view.container.textContent).not.toContain('Orbit Developersjust now')
})

test('author comments expose compact copy, edit, and delete actions', () => {
  const view = render(<CommentItem state={state()} taskId="task-1" comment={comment()} workspaceId="workspace-1" statuses={[]} />, { wrapper: wrapper() })

  expect(view.getByRole('button', { name: 'Copy text' })).toBeTruthy()
  expect(view.getByRole('button', { name: 'Edit comment' })).toBeTruthy()
  expect(view.getByRole('button', { name: 'Delete comment' })).toBeTruthy()
})

test('other people can copy a comment but cannot edit or delete it', () => {
  const view = render(
    <CommentItem state={state({ currentUserId: 'user-2' })} taskId="task-1" comment={comment()} workspaceId="workspace-1" statuses={[]} />,
    { wrapper: wrapper() },
  )

  expect(view.getByRole('button', { name: 'Copy text' })).toBeTruthy()
  expect(view.queryByRole('button', { name: 'Edit comment' })).toBeNull()
  expect(view.queryByRole('button', { name: 'Delete comment' })).toBeNull()
})

test('edit mode replaces the body with a save/cancel rich text editor', () => {
  const view = render(<CommentItem state={state()} taskId="task-1" comment={comment()} workspaceId="workspace-1" statuses={[]} />, { wrapper: wrapper() })

  fireEvent.click(view.getByRole('button', { name: 'Edit comment' }))
  expect(view.getByLabelText('Edit comment')).toBeTruthy()
  expect(view.getByText(/escape to/)).toBeTruthy()
  expect(view.queryByRole('button', { name: 'Copy text' })).toBeNull()
})

test('a comment body renders as rich text, not as a markdown string', () => {
  // The chip resolves through the batch endpoint; nothing needs to come back.
  globalThis.fetch = (async () => Response.json({ items: [], next_cursor: null })) as unknown as typeof fetch
  const view = render(
    <CommentItem
      state={state()}
      taskId="task-1"
      workspaceId="workspace-1"
      statuses={[]}
      comment={comment({
        bodyJson: {
          type: 'doc',
          content: [
            { type: 'heading', attrs: { level: 3 }, content: [{ type: 'text', text: 'Findings' }] },
            {
              type: 'paragraph',
              content: [
                { type: 'text', text: 'see ' },
                { type: 'taskMention', attrs: { id: 'task-2', identifier: 'ORB-13' } },
              ],
            },
          ],
        },
        bodyText: 'Findings\nsee ORB-13',
      })}
    />,
    { wrapper: wrapper() },
  )

  expect(view.getByRole('heading', { level: 3 }).textContent).toBe('Findings')
  expect(view.getByText('ORB-13')).toBeTruthy()
  expect(view.container.querySelector('.editor-view')).toBeTruthy()
})

test('copy text copies the derived plain text, not the JSON', () => {
  const written: string[] = []
  Object.defineProperty(navigator, 'clipboard', {
    configurable: true,
    value: { writeText: async (value: string) => void written.push(value) },
  })
  const view = render(
    <CommentItem
      state={state()}
      taskId="task-1"
      workspaceId="workspace-1"
      statuses={[]}
      comment={comment({ bodyText: 'Findings\nsee ORB-13' })}
    />,
    { wrapper: wrapper() },
  )

  fireEvent.click(view.getByRole('button', { name: 'Copy text' }))

  expect(written).toEqual(['Findings\nsee ORB-13'])
})

test('an attachment-only comment renders no empty text block', () => {
  const view = render(
    <CommentItem state={state()} taskId="task-1" workspaceId="workspace-1" statuses={[]} comment={comment({ bodyJson: { type: 'doc', content: [] }, bodyText: '' })} />,
    { wrapper: wrapper() },
  )

  expect(view.container.querySelector('.tasks-comment-text')).toBeNull()
})
