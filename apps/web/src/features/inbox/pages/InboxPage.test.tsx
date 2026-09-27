import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, waitFor } from '@testing-library/react'
import { MemoryRouter, useLocation } from 'react-router'
import { queryKeys } from '@/api/queryKeys'
import { WorkspaceProvider } from '@/features/workspaces/WorkspaceProvider'
import { InboxPage } from './InboxPage'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

function Location() {
  const location = useLocation()
  return <output data-testid="location">{location.pathname}{location.search}</output>
}

test('inbox lists assignment notifications and opens the linked task', async () => {
  window.localStorage.clear()
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } })
  client.setQueryData(queryKeys.workspaces, [{ id: 'workspace-1', name: 'Alpha', role: 'owner', version: 1 }])
  client.setQueryData(queryKeys.members('workspace-1'), [{
    id: 'user-owner', membershipId: 'm1', name: 'Owner', handle: 'owner', email: 'o@x',
    role: 'Owner', color: '#000', online: false, title: '', roleIds: [], version: 1,
  }])
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const url = String(input instanceof Request ? input.url : input)
    if (url.includes('/notifications') && url.includes('unread=true')) return Response.json({ items: [{
      id: 'n1', workspace_id: 'workspace-1', recipient_user_id: 'me', actor_user_id: 'user-owner',
      kind: 'task_assigned', task_id: 'task-9', comment_id: null, read_at: null, created_at: '2026-09-06T10:00:00Z',
    }], next_cursor: null })
    if (url.includes('/notifications')) return Response.json({ items: [{
      id: 'n1', workspace_id: 'workspace-1', recipient_user_id: 'me', actor_user_id: 'user-owner',
      kind: 'task_assigned', task_id: 'task-9', comment_id: null, read_at: null, created_at: '2026-09-06T10:00:00Z',
    }], next_cursor: null })
    return new Response('missing', { status: 404 })
  }) as unknown as typeof fetch

  const view = render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={['/inbox?workspace=workspace-1']}>
        <WorkspaceProvider>
          <InboxPage />
          <Location />
        </WorkspaceProvider>
      </MemoryRouter>
    </QueryClientProvider>,
  )

  expect(await view.findByText('You were assigned a task')).toBeTruthy()
  let openedSidebar = false
  window.addEventListener('open-sidebar', () => { openedSidebar = true }, { once: true })
  fireEvent.click(view.getByRole('button', { name: 'Menu' }))
  expect(openedSidebar).toBe(true)
  fireEvent.click(view.getByText('You were assigned a task'))
  await waitFor(() => expect(view.getByTestId('location').textContent).toBe('/tasks/task-9?redirect=%2Finbox%3Fworkspace%3Dworkspace-1'))
})

test('page comment mentions name the page and open it with the thread', async () => {
  window.localStorage.clear()
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } })
  client.setQueryData(queryKeys.workspaces, [{ id: 'workspace-1', name: 'Alpha', role: 'owner', version: 1 }])
  client.setQueryData(queryKeys.members('workspace-1'), [{
    id: 'user-owner', membershipId: 'm1', name: 'Owner', handle: 'owner', email: 'o@x',
    role: 'Owner', color: '#000', online: false, title: '', roleIds: [], version: 1,
  }])
  client.setQueryData(queryKeys.pages.tree('workspace-1'), [{ id: 'page-7', title: 'Roadmap', parent_id: null, teamspace_id: 'ts', icon: null, position: 0, version: 1, updated_at: '2026-09-06T10:00:00Z' }])
  const mention = {
    id: 'n2', workspace_id: 'workspace-1', recipient_user_id: 'me', actor_user_id: 'user-owner',
    kind: 'page_comment_mentioned', task_id: null, comment_id: null, page_id: 'page-7', page_thread_id: 'thread-3',
    page_comment_id: 'comment-1', read_at: null, created_at: '2026-09-06T10:00:00Z',
  }
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const url = String(input instanceof Request ? input.url : input)
    if (url.includes('/notifications')) return Response.json({ items: [mention], next_cursor: null })
    return new Response('missing', { status: 404 })
  }) as unknown as typeof fetch

  const view = render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={['/inbox?workspace=workspace-1']}>
        <WorkspaceProvider>
          <InboxPage />
          <Location />
        </WorkspaceProvider>
      </MemoryRouter>
    </QueryClientProvider>,
  )

  expect(await view.findByText('You were mentioned in a comment')).toBeTruthy()
  expect(view.getByText(/Someone mentioned you in a comment on “Roadmap”/)).toBeTruthy()
  fireEvent.click(view.getByRole('tab', { name: 'Mentions' }))
  expect(view.getByText('You were mentioned in a comment')).toBeTruthy()
  fireEvent.click(view.getByText('You were mentioned in a comment'))
  await waitFor(() => expect(view.getByTestId('location').textContent).toBe('/docs/page-7?thread=thread-3'))
})

test('page body mentions name the editor and the page and open the page at the block', async () => {
  window.localStorage.clear()
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } })
  client.setQueryData(queryKeys.workspaces, [{ id: 'workspace-1', name: 'Alpha', role: 'owner', version: 1 }])
  client.setQueryData(queryKeys.members('workspace-1'), [{
    id: 'user-owner', membershipId: 'm1', name: 'Orbit Owner', handle: 'owner', email: 'o@x',
    role: 'Owner', color: '#000', online: false, title: '', roleIds: [], version: 1,
  }])
  client.setQueryData(queryKeys.pages.tree('workspace-1'), [{ id: 'page-7', title: '[mention-test]', parent_id: null, teamspace_id: 'ts', icon: null, position: 0, version: 1, updated_at: '2026-09-06T10:00:00Z' }])
  const mention = {
    id: 'n3', workspace_id: 'workspace-1', recipient_user_id: 'me', actor_user_id: 'user-owner',
    kind: 'page_mentioned', task_id: null, comment_id: null, page_id: 'page-7', page_thread_id: null,
    page_comment_id: null, page_block_id: 'block-1', read_at: null, created_at: '2026-09-06T10:00:00Z',
  }
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const url = String(input instanceof Request ? input.url : input)
    if (url.includes('/notifications')) return Response.json({ items: [mention], next_cursor: null })
    return new Response('missing', { status: 404 })
  }) as unknown as typeof fetch

  const view = render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={['/inbox?workspace=workspace-1']}>
        <WorkspaceProvider>
          <InboxPage />
          <Location />
        </WorkspaceProvider>
      </MemoryRouter>
    </QueryClientProvider>,
  )

  const title = 'Orbit Owner mentioned you in “[mention-test]”'
  expect(await view.findByText(title)).toBeTruthy()
  fireEvent.click(view.getByRole('tab', { name: 'Mentions' }))
  expect(view.getByText(title)).toBeTruthy()
  fireEvent.click(view.getByText(title))
  await waitFor(() => expect(view.getByTestId('location').textContent).toBe('/docs/page-7?block=block-1'))
})
