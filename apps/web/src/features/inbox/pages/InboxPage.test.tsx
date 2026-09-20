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
  return <output data-testid="location">{location.pathname}</output>
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
  fireEvent.click(view.getByText('You were assigned a task'))
  await waitFor(() => expect(view.getByTestId('location').textContent).toBe('/tasks/task-9'))
})
