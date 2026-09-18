import { afterEach, expect, mock, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { MemoryRouter } from 'react-router'
import { TaskChip } from './TaskChip'
import { useTaskChips } from './useTaskChips'

const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
})

test('an unresolved chip falls back to the identifier cached in the document', () => {
  const view = render(<TaskChip identifier="ORB-12" />, { wrapper: MemoryRouter })

  expect(view.getByText('ORB-12')).toBeTruthy()
  expect(view.container.querySelector('.editor-chip')?.getAttribute('data-resolved')).toBeNull()
})

test('a resolved chip shows the live title next to the identifier', () => {
  const view = render(
    <TaskChip identifier="ORB-12" resolved={{ title: 'Ship the editor', statusCategory: 'started', taskId: 'task-1' }} />,
    { wrapper: MemoryRouter },
  )

  expect(view.getByText('ORB-12')).toBeTruthy()
  expect(view.getByText('Ship the editor')).toBeTruthy()
  expect(view.container.querySelector('.editor-chip')?.getAttribute('data-resolved')).toBe('true')
})

test('a cancelled task strikes the chip through', () => {
  const view = render(
    <TaskChip identifier="ORB-12" resolved={{ title: 'Abandoned', statusCategory: 'cancelled', taskId: 'task-1' }} />,
    { wrapper: MemoryRouter },
  )

  expect(view.container.querySelector('.editor-chip')?.getAttribute('data-cancelled')).toBe('true')
})

test('a resolved chip links to the task and an unresolved one does not', () => {
  const linked = render(
    <TaskChip identifier="ORB-12" resolved={{ title: 'Ship', statusCategory: 'unstarted', taskId: 'task-1' }} />,
    { wrapper: MemoryRouter },
  )
  expect((linked.getByRole('link') as HTMLAnchorElement).getAttribute('href')).toBe('/tasks/task-1')
  // Queries are bound to document.body, so the linked chip must go before checking the bare one.
  linked.unmount()

  const bare = render(<TaskChip identifier="ORB-12" />, { wrapper: MemoryRouter })
  expect(bare.queryByRole('link')).toBeNull()
})

function queryWrapper() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return ({ children }: { children: ReactNode }) => <QueryClientProvider client={client}>{children}</QueryClientProvider>
}

const statuses = [
  { id: 'status-1', projectId: 'p', name: 'In Progress', description: '', color: '#f2c94c', category: 'started' as const, position: 2, version: 0 },
  { id: 'status-2', projectId: 'p', name: 'Cancelled', description: '', color: '#8b8f98', category: 'cancelled' as const, position: 4, version: 0 },
]

test('every chip on a page resolves in a single batch request', async () => {
  const bodies: unknown[] = []
  const urls: string[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    urls.push((input as Request).url)
    bodies.push(await (input as Request).json())
    return Response.json({
      items: [
        { id: 'task-1', identifier: 'ORB-12', title: 'Ship the editor', status_id: 'status-1' },
        { id: 'task-2', identifier: 'ACME-3', title: 'Other', status_id: 'status-2' },
      ],
      next_cursor: null,
    })
  }) as unknown as typeof fetch
  const identifiers = ['ORB-12', 'ACME-3', 'ORB-12']

  const { result } = renderHook(() => useTaskChips('workspace-1', identifiers, statuses), { wrapper: queryWrapper() })

  await waitFor(() => expect(result.current('ORB-12')).toBeTruthy())
  expect(bodies).toHaveLength(1)
  expect(urls[0]).toContain('/workspaces/workspace-1/tasks/resolve')
  expect(bodies[0]).toEqual({ identifiers: ['ORB-12', 'ACME-3'] })
  expect(result.current('ORB-12')).toEqual({
    title: 'Ship the editor', statusCategory: 'started', taskId: 'task-1', statusColor: '#f2c94c',
  })
  expect(result.current('ACME-3')?.statusCategory).toBe('cancelled')
  expect(result.current('NOPE-1')).toBeUndefined()
})

test('no identifiers means no request at all', async () => {
  const calls = mock(() => Response.json({ items: [], next_cursor: null }))
  globalThis.fetch = calls as unknown as typeof fetch
  const identifiers: string[] = []

  renderHook(() => useTaskChips('workspace-1', identifiers, []), { wrapper: queryWrapper() })

  await new Promise((resolve) => setTimeout(resolve, 10))
  expect(calls).not.toHaveBeenCalled()
})
