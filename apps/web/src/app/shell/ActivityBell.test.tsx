import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, fireEvent, render, waitFor } from '@testing-library/react'
import { type ReactNode, useEffect } from 'react'
import { MemoryRouter, type NavigateFunction, useLocation, useNavigate } from 'react-router'
import { queryKeys } from '@/api/queryKeys'
import { WorkspaceProvider } from '@/features/workspaces/WorkspaceProvider'
import { NO_MEMBER_ABILITIES } from '@/features/workspaces/models'
import { ActivityBell } from './ActivityBell'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

function Location() {
  const location = useLocation()
  return <output data-testid="location">{location.pathname}{location.search}</output>
}

/** Navigation from outside the popover (a shortcut, Back). */
const router: { go?: NavigateFunction } = {}
function Navigator() {
  const navigate = useNavigate()
  useEffect(() => { router.go = navigate }, [navigate])
  return null
}

const notification = (id: string, read: boolean) => ({
  id, workspace_id: 'workspace-1', recipient_user_id: 'me', actor_user_id: 'user-owner',
  kind: 'task_assigned', task_id: `task-${id}`, comment_id: null, read_at: (read ? '2026-09-06T11:00:00Z' : null) as string | null, created_at: '2026-09-06T10:00:00Z',
})

/** The bell inside a workspace, with the notifications API answering from `items`; returns the requests it got. */
function renderBell(element: ReactNode, items: ReturnType<typeof notification>[], { entry = '/tasks?workspace=workspace-1&project=project-r', readFails = false } = {}) {
  window.localStorage.clear()
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } })
  client.setQueryData(queryKeys.workspaces, [{ id: 'workspace-1', name: 'Alpha', role: 'owner', permissions: [], version: 1 }])
  client.setQueryData(queryKeys.members('workspace-1'), [{
    id: 'user-owner', membershipId: 'm1', name: 'Owner', handle: 'owner', email: 'o@x',
    role: 'Owner', color: '#000', title: '', roleIds: [], can: NO_MEMBER_ABILITIES, version: 1,
  }])
  const requests: string[] = []
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    requests.push(`${request.method} ${request.url}`)
    if (readFails && request.method === 'POST') return Response.json({ code: 'internal' }, { status: 500 })
    const read = request.method === 'POST' ? items.find((item) => request.url.includes(`/notifications/${item.id}/read`)) : undefined
    if (read) {
      read.read_at = '2026-09-06T12:00:00Z'
      return Response.json(read)
    }
    if (request.url.includes('/notifications')) {
      const unread = request.url.includes('unread=true')
      return Response.json({ items: items.filter((item) => !unread || !item.read_at), next_cursor: null })
    }
    return new Response('missing', { status: 404 })
  }) as unknown as typeof fetch
  const view = render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[entry]}>
        <WorkspaceProvider>
          {element}
          <Location />
          <Navigator />
        </WorkspaceProvider>
      </MemoryRouter>
    </QueryClientProvider>,
  )
  return { view, requests, client }
}

test('the phone bell opens Activity and keeps the selected project', async () => {
  const { view } = renderBell(<ActivityBell topbar />, [])
  const bell = await view.findByRole('link', { name: 'Activity' })
  expect(bell.dataset.active).toBe('false')

  fireEvent.click(bell)
  expect(view.getByTestId('location').textContent).toBe('/activity?project=project-r')
  expect(bell.getAttribute('aria-current')).toBe('page')
})

test('the bell shows a dot while a notification is unread', async () => {
  const { view } = renderBell(<ActivityBell unread />, [])
  expect((await view.findByRole('button', { name: 'Activity, unread' })).querySelectorAll('[data-slot="activity-bell-dot"]').length).toBe(1)
  view.unmount()

  const read = renderBell(<ActivityBell />, [])
  expect((await read.view.findByRole('button', { name: 'Activity' })).querySelectorAll('[data-slot="activity-bell-dot"]').length).toBe(0)
})

test('the sidebar bell opens the newest notifications, and a row opens its task', async () => {
  const { view, requests, client } = renderBell(<ActivityBell unread />, [notification('1', false), notification('2', true)])
  fireEvent.click(await view.findByRole('button', { name: 'Activity, unread' }))

  const rows = await view.findAllByText('You were assigned a task')
  expect(rows.length).toBe(2)
  // one page of the newest, not the whole list
  expect(requests.some((request) => request.includes('unread=false') && request.includes('limit=10') && !request.includes('cursor'))).toBe(true)
  expect(requests.some((request) => request.includes('unread=true') && request.includes('limit=100') && !request.includes('cursor'))).toBe(true)

  fireEvent.click(rows[0])
  await waitFor(() => expect(view.getByTestId('location').textContent).toBe(
    '/tasks/task-1?redirect=%2Ftasks%3Fworkspace%3Dworkspace-1%26project%3Dproject-r',
  ))
  await waitFor(() => expect(requests.some((request) => request.startsWith('POST') && request.includes('/notifications/1/read'))).toBe(true))
  await waitFor(() => expect(view.queryAllByText('View all activity').length).toBe(0))
  await waitFor(() => expect(client.isMutating() + client.isFetching()).toBe(0))
})

test('the Unread tab lists only unread notifications, and View all keeps the tab and the project', async () => {
  const { view } = renderBell(<ActivityBell unread />, [notification('1', false), notification('2', true)])
  fireEvent.click(await view.findByRole('button', { name: 'Activity, unread' }))
  expect((await view.findAllByText('You were assigned a task')).length).toBe(2)

  fireEvent.click(view.getByRole('tab', { name: /Unread/ }))
  await waitFor(() => expect(view.getAllByText('You were assigned a task').length).toBe(1))

  fireEvent.click(view.getByText('View all activity'))
  expect(view.getByTestId('location').textContent).toBe('/activity?project=project-r&tab=unread')
})

test('Mark as read clears a row without opening it', async () => {
  const { view, requests, client } = renderBell(<ActivityBell unread />, [notification('1', false)])
  fireEvent.click(await view.findByRole('button', { name: 'Activity, unread' }))
  fireEvent.click(await view.findByLabelText('Mark as read'))
  // the dot goes at once; the task does not open
  await waitFor(() => expect(view.queryAllByLabelText('Mark as read').length).toBe(0))
  // the read and the refresh after it end inside the test
  await waitFor(() => expect(client.isMutating() + client.isFetching()).toBe(0))
  expect(requests.some((request) => request.startsWith('POST') && request.includes('/notifications/1/read'))).toBe(true)
  expect(view.queryAllByLabelText('Mark as read').length).toBe(0)
  expect(view.getByTestId('location').textContent).toBe('/tasks?workspace=workspace-1&project=project-r')
})

test('a new notification shows in the open popover when the live socket refreshes the inbox', async () => {
  const items = [notification('1', true)]
  const { view, client } = renderBell(<ActivityBell />, items)
  fireEvent.click(await view.findByRole('button', { name: 'Activity' }))
  expect((await view.findAllByText('You were assigned a task')).length).toBe(1)

  // what ChatProvider's `inbox` signal and a workspace event both do
  items.unshift(notification('2', false))
  await client.invalidateQueries({ queryKey: [...queryKeys.workspace('workspace-1'), 'notifications'] })
  await waitFor(() => expect(view.getAllByText('You were assigned a task').length).toBe(2))
  expect(view.getAllByLabelText('Mark as read').length).toBe(1)
  await waitFor(() => expect(client.isFetching()).toBe(0))
})

test('a read that fails puts the dot back', async () => {
  const { view, client } = renderBell(<ActivityBell unread />, [notification('1', false)], { readFails: true })
  fireEvent.click(await view.findByRole('button', { name: 'Activity, unread' }))
  fireEvent.click(await view.findByLabelText('Mark as read'))
  await waitFor(() => expect(client.isMutating() + client.isFetching()).toBe(0))
  expect(view.getAllByLabelText('Mark as read').length).toBe(1)
})

test('navigating elsewhere closes the popover', async () => {
  const { view, client } = renderBell(<ActivityBell />, [notification('1', true)])
  fireEvent.click(await view.findByRole('button', { name: 'Activity' }))
  expect(await view.findByText('View all activity')).toBeTruthy()

  act(() => { void router.go?.('/activity') })
  await waitFor(() => expect(view.queryAllByText('View all activity').length).toBe(0))
  expect(view.getByRole('button', { name: 'Activity' }).getAttribute('aria-current')).toBe('page')
  await waitFor(() => expect(client.isFetching()).toBe(0))
})
