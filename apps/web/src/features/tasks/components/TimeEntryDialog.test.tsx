import { afterEach, expect, mock, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import { testWorkspace } from '@/test/workspace'
import { todayLocalDate } from '@/features/tasks/timeLib'
import { TimeEntryDialog } from './TimeEntryDialog'

const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
})

function renderDialog(onClose = mock(() => {})) {
  const workspace = testWorkspace('member')
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  return render(
    <MemoryRouter>
      <QueryClientProvider client={client}>
        <WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: mock(() => {}) }}>
          <TimeEntryDialog task={{ id: 'task-1', projectId: 'project-1' }} users={[]} currentUserId="user-1" onClose={onClose} />
        </WorkspaceContext.Provider>
      </QueryClientProvider>
    </MemoryRouter>,
  )
}

test('logs the typed duration for today, for the caller', async () => {
  const posted: unknown[] = []
  globalThis.fetch = mock(async (input: RequestInfo | URL, init?: RequestInit) => {
    const request = new Request(input, init)
    if (request.method === 'POST') {
      posted.push([new URL(request.url).pathname, await request.json()])
      return Response.json({}, { status: 201 })
    }
    return Response.json({ items: [], next_cursor: null })
  }) as unknown as typeof fetch
  const onClose = mock(() => {})
  const view = renderDialog(onClose)

  // a member without `time.manage` logs only their own time
  expect(view.queryAllByRole('button', { name: /^Person/ })).toHaveLength(0)
  fireEvent.change(view.getByLabelText(/Duration/), { target: { value: '1h 30m' } })
  fireEvent.click(view.getByRole('button', { name: 'Log time' }))

  await waitFor(() => expect(onClose).toHaveBeenCalled())
  expect(posted).toEqual([['/api/v1/workspaces/workspace-1/tasks/task-1/time-entries', { minutes: 90, worked_on: todayLocalDate(), note: '', billable: false }]])
})

test('refuses a duration it cannot read and sends nothing', () => {
  const methods: string[] = []
  globalThis.fetch = mock(async (input: RequestInfo | URL, init?: RequestInit) => {
    methods.push(new Request(input, init).method)
    return Response.json({ items: [], next_cursor: null })
  }) as unknown as typeof fetch
  const view = renderDialog()

  fireEvent.change(view.getByLabelText(/Duration/), { target: { value: '2d' } })
  fireEvent.click(view.getByRole('button', { name: 'Log time' }))

  expect(view.getByText('Enter a time such as 1h 30m, up to 24h.')).toBeTruthy()
  expect(methods.filter((method) => method !== 'GET')).toEqual([])
})
