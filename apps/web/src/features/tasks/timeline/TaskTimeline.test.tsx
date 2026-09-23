import { afterAll, afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render } from '@testing-library/react'
import type { ReactNode } from 'react'
import type { WorkspaceRecord } from '@/api/generated/types.gen'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import type { Project, Task, TaskStatusDef } from '@/features/tasks/api/models'
import { TaskTimeline } from './TaskTimeline'
import { computeRange, dayIndex, rowDates } from './timelineLib'

// set before any fixture below is built, so every local date is a Berlin date (DST-bearing zone)
const originalTz = process.env.TZ
process.env.TZ = 'Europe/Berlin'
afterAll(() => {
  if (originalTz === undefined) delete process.env.TZ
  else process.env.TZ = originalTz
})
const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch; sessionStorage.clear() })

const local = (y: number, m: number, d: number, h = 0) => new Date(y, m - 1, d, h)
const today = local(2026, 9, 23, 10)

function task(id: string, overrides: Partial<Task> = {}): Task {
  return {
    id, identifier: `WEB-${id}`, title: `Task ${id}`, description: '', statusId: 'todo', position: 0, priority: 'none',
    assigneeIds: [], projectId: 'p1', labels: [], attachments: [], dueStartAt: null, dueAt: null,
    createdAt: '', updatedAt: '', comments: [], activity: [], version: 3, ...overrides,
  }
}

const statuses: TaskStatusDef[] = [
  { id: 'todo', projectId: 'p1', name: 'Todo', description: '', color: '#888', category: 'unstarted', position: 0, version: 1 },
  { id: 'done', projectId: 'p1', name: 'Done', description: '', color: '#4cb782', category: 'completed', position: 1, version: 1 },
]
const projects = [{ id: 'p1', name: 'Web', key: 'WEB', color: '#e0457b', version: 1 }] as Project[]

function renderTimeline(tasks: Task[], extra: { onOpen?: (id: string) => void; grouped?: boolean; pxPerDay?: number } = {}) {
  const workspace: WorkspaceRecord = { id: 'ws', name: 'Orbit', role: 'owner', version: 1 }
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>
      <WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}>{children}</WorkspaceContext.Provider>
    </QueryClientProvider>
  )
  return render(
    <TaskTimeline tasks={tasks} projects={projects} statuses={statuses} users={[]} grouped={extra.grouped ?? true}
      pxPerDay={extra.pxPerDay ?? 10} onZoomChange={() => {}} onOpen={extra.onOpen ?? (() => {})} today={today} />,
    { wrapper },
  )
}

const ranged = task('a', { dueStartAt: local(2026, 9, 1).toISOString(), dueAt: local(2026, 9, 10, 9).toISOString() })

test('a ranged bar is positioned and sized from its local days', () => {
  const view = renderTimeline([ranged])
  const range = computeRange(rowDates([ranged]), today)
  const bar = view.container.querySelector<HTMLElement>('[data-timeline-bar="a"]')!
  expect(bar.style.left).toBe(`${dayIndex(range, local(2026, 9, 1)) * 10}px`)
  expect(bar.style.width).toBe('100px')
  expect(bar.getAttribute('aria-label')).toBe('WEB-a Task a, Sep 1 → Sep 10 · 10 days')
})

test('overdue tail only for open tasks past their due day', () => {
  const late = task('late', { dueAt: local(2026, 9, 20, 9).toISOString() })
  const doneLate = task('doneLate', { statusId: 'done', dueAt: local(2026, 9, 20, 9).toISOString() })
  const view = renderTimeline([late, doneLate])
  const tails = view.container.querySelectorAll<HTMLElement>('[data-overdue-tail]')
  expect(tails).toHaveLength(1)
  expect(tails[0]!.style.width).toBe('30px') // 21, 22, 23 September
})

test('group header shows the project with done/total and a collapsed "No dates" section', () => {
  const view = renderTimeline([ranged, task('b')])
  expect(view.getByText('Web')).toBeTruthy()
  expect(view.getByText('0/2')).toBeTruthy()
  const toggle = view.getByRole('button', { name: 'No dates (1)' })
  expect(toggle.getAttribute('aria-expanded')).toBe('false')
  fireEvent.click(toggle)
  expect(view.getByText('Task b')).toBeTruthy()
})

test('today line is drawn and the empty state shows without tasks', () => {
  expect(renderTimeline([ranged]).container.querySelector('[data-today-line]')).toBeTruthy()
  expect(renderTimeline([]).getByText('No tasks match these filters')).toBeTruthy()
})

test('only undated tasks still render the project and its "No dates" section', () => {
  const view = renderTimeline([task('b'), task('c')])
  expect(view.getByRole('button', { name: 'No dates (2)' })).toBeTruthy()
  expect(view.container.querySelector('[data-timeline-bar]')).toBeNull()
})
