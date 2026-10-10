import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, waitFor, within } from '@testing-library/react'
import { useState } from 'react'
import { render } from '@/test/render'
import type { Task } from '@/features/tasks/api/models'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import { testWorkspace } from '@/test/workspace'
import { TaskCalendar } from './TaskCalendar'
import type { CalendarMode } from './calendarLib'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

const iso = (value: string, hour = 9) => new Date(`${value}T${String(hour).padStart(2, '0')}:00:00`).toISOString()
function task(id: string, due: string | null, start: string | null = null): Task {
  return {
    id, identifier: id.toUpperCase(), title: `Task ${id}`, description: '', statusId: 'todo', position: 0, priority: 'none', assigneeIds: [], projectId: 'p1',
    labels: [], attachments: [], dueStartAt: start ? iso(start, 0) : null, dueAt: due ? iso(due) : null, createdAt: '', updatedAt: '', comments: [], activity: [], version: 4,
  }
}

function setup(tasks: Task[]) {
  const writes: Array<{ path: string; body: unknown }> = []
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const request = new Request(input, init)
    if (request.method === 'PATCH') writes.push({ path: new URL(request.url).pathname, body: await request.json() })
    return Response.json({})
  }) as unknown as typeof fetch
  const opened: string[] = []
  const created: string[] = []
  const workspace = testWorkspace('member')
  function Page() {
    const [mode, setMode] = useState<CalendarMode>('month')
    return <TaskCalendar tasks={tasks} projects={[]} statuses={[]} mode={mode} onModeChange={setMode} onOpen={(id) => opened.push(id)} onCreate={(due) => created.push(due)} today={new Date('2026-10-08T12:00:00')} weekStart={1} />
  }
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const view = render(<QueryClientProvider client={client}><WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}><Page /></WorkspaceContext.Provider></QueryClientProvider>)
  return { view, writes, opened, created, workspace }
}

const dataTransfer = { setData: () => {}, getData: () => '', effectAllowed: 'all', dropEffect: 'none' }
const cell = (view: ReturnType<typeof setup>['view'], day: string) => view.container.querySelector(`[data-day="${day}"]`) as HTMLElement
const bars = (view: ReturnType<typeof setup>['view'], id: string) => [...view.container.querySelectorAll(`[data-calendar-bar="${id}"]`)] as HTMLElement[]

test('the month grid shows dated tasks only, a range in each of its weeks, and the controls change the range', () => {
  const { view, opened } = setup([task('due', '2026-10-07'), task('range', '2026-10-13', '2026-10-09'), task('undated', null)])
  expect(view.getByRole('heading', { name: 'October 2026' })).toBeTruthy()
  expect(view.container.querySelectorAll('[data-calendar-week]').length).toBe(5)
  expect(bars(view, 'due').length).toBe(1)
  // Fri Oct 9 to Tue Oct 13 goes over a week boundary: one bar in each week
  expect(bars(view, 'range').map((bar) => bar.style.gridColumn)).toEqual(['5 / 8', '1 / 3'])
  expect(bars(view, 'undated').length).toBe(0)
  fireEvent.click(bars(view, 'due')[0]!)
  expect(opened).toEqual(['due'])

  fireEvent.click(view.getByRole('button', { name: 'Next month' }))
  expect(view.getByRole('heading', { name: 'November 2026' })).toBeTruthy()
  expect(bars(view, 'due').length).toBe(0)
  fireEvent.click(view.getByRole('button', { name: 'Today' }))
  fireEvent.click(view.getByRole('button', { name: 'Week' }))
  expect(view.getByRole('heading', { name: 'Oct 5 – 11, 2026' })).toBeTruthy()
  expect(view.container.querySelectorAll('[data-calendar-week]').length).toBe(1)
  expect(bars(view, 'range').map((bar) => bar.style.gridColumn)).toEqual(['5 / 8'])
})

test('a click on an empty day asks for a new task due that day', () => {
  const { view, created } = setup([])
  fireEvent.click(within(cell(view, '2026-10-15')).getByRole('button', { name: /New task due Thursday, October 15/ }))
  expect(created).toEqual([iso('2026-10-15')])
})

test('a drag to a different day keeps the bar mounted and saves the new due date', async () => {
  const { view, writes, workspace } = setup([task('due', '2026-10-07')])
  const bar = bars(view, 'due')[0]!
  fireEvent.dragStart(bar, { dataTransfer })
  // the drag source stays in the document, only faded
  expect(bar.isConnected).toBe(true)
  expect(bar.getAttribute('data-dragging')).toBe('true')
  fireEvent.dragOver(cell(view, '2026-10-10'), { dataTransfer })
  expect(cell(view, '2026-10-10').getAttribute('data-drop-target')).toBe('true')
  fireEvent.drop(cell(view, '2026-10-10'), { dataTransfer })
  await waitFor(() => expect(writes.length).toBe(1))
  expect(writes[0]).toEqual({ path: `/api/v1/workspaces/${workspace.id}/tasks/due`, body: { due_start_at: null, due_at: iso('2026-10-10'), expected_version: 4 } })
  expect(bars(view, 'due')[0]!.getAttribute('data-dragging')).toBeNull()
})

test('a drop on the same day saves nothing, and a day with more tasks than lanes shows "+N more" with the list', async () => {
  const many = ['a', 'b', 'c', 'd', 'e'].map((id) => task(id, '2026-10-07'))
  const { view, writes, opened } = setup(many)
  const bar = bars(view, 'a')[0]!
  fireEvent.dragStart(bar, { dataTransfer })
  fireEvent.drop(cell(view, '2026-10-07'), { dataTransfer })
  expect(writes.length).toBe(0)
  // three lanes in the month grid
  expect(view.container.querySelectorAll('[data-calendar-bar]').length).toBe(3)
  const more = within(cell(view, '2026-10-07')).getByRole('button', { name: '+2 more' })
  fireEvent.click(more)
  const hidden = await view.findByRole('button', { name: /Task e/ })
  fireEvent.click(hidden)
  expect(opened).toEqual(['e'])
})
