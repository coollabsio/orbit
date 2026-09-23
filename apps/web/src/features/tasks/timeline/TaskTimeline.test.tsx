import { afterAll, afterEach, expect, spyOn, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { toast } from 'sonner'
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

function captureFetch() {
  const requests: Array<{ url: string; method: string; body: Record<string, unknown> }> = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    const body = await request.json().catch(() => ({}))
    requests.push({ url: request.url, method: request.method, body })
    return Response.json({ ...body, id: 'a' })
  }) as unknown as typeof fetch
  return requests
}

const pointer = { button: 0, pointerId: 1, pointerType: 'mouse' }
const flush = () => new Promise((resolve) => setTimeout(resolve, 0))

test('dragging a bar 3 days right saves both shifted dates once', async () => {
  const requests = captureFetch()
  const view = renderTimeline([ranged])
  const bar = view.container.querySelector<HTMLElement>('[data-timeline-bar="a"]')!
  fireEvent.pointerDown(bar, { ...pointer, clientX: 100 })
  fireEvent.pointerMove(window, { ...pointer, clientX: 131 })
  fireEvent.pointerUp(window, { ...pointer, clientX: 131 })
  fireEvent.click(bar) // the browser's click after a drag must not open the task
  await flush()
  expect(requests).toHaveLength(1)
  expect(requests[0]!.url).toContain('/tasks/a')
  expect(requests[0]!.body).toEqual({
    due_start_at: local(2026, 9, 4).toISOString(),
    due_at: local(2026, 9, 13, 9).toISOString(),
    expected_version: 3,
  })
})

test('Escape cancels a drag without saving', async () => {
  const requests = captureFetch()
  const view = renderTimeline([ranged])
  const bar = view.container.querySelector<HTMLElement>('[data-timeline-bar="a"]')!
  fireEvent.pointerDown(bar, { ...pointer, clientX: 100 })
  fireEvent.pointerMove(window, { ...pointer, clientX: 160 })
  fireEvent.keyDown(window, { key: 'Escape' })
  fireEvent.pointerUp(window, { ...pointer, clientX: 160 })
  await flush()
  expect(requests).toHaveLength(0)
})

test('a click (≤ 4px) opens the task and does not save', async () => {
  const requests = captureFetch()
  const opened: string[] = []
  const view = renderTimeline([ranged], { onOpen: (id) => opened.push(id) })
  const bar = view.container.querySelector<HTMLElement>('[data-timeline-bar="a"]')!
  fireEvent.pointerDown(bar, { ...pointer, clientX: 100 })
  fireEvent.pointerMove(window, { ...pointer, clientX: 103 })
  fireEvent.pointerUp(window, { ...pointer, clientX: 103 })
  fireEvent.click(bar)
  await flush()
  expect(opened).toEqual(['a'])
  expect(requests).toHaveLength(0)
})

test('resizing the end edge changes only the due date', async () => {
  const requests = captureFetch()
  const view = renderTimeline([ranged])
  const endHandle = view.container.querySelector<HTMLElement>('[data-timeline-bar="a"] > span:last-child')!
  fireEvent.pointerDown(endHandle, { ...pointer, clientX: 200 })
  fireEvent.pointerMove(window, { ...pointer, clientX: 220 })
  fireEvent.pointerUp(window, { ...pointer, clientX: 220 })
  await flush()
  expect(requests[0]!.body.due_start_at).toBe(local(2026, 9, 1).toISOString())
  expect(requests[0]!.body.due_at).toBe(local(2026, 9, 12, 9).toISOString())
})

test('clicking an undated row track sets a due date on that day', async () => {
  const requests = captureFetch()
  const view = renderTimeline([ranged, task('b')])
  fireEvent.click(view.getByRole('button', { name: 'No dates (1)' }))
  const range = computeRange(rowDates([ranged]), today)
  const x = dayIndex(range, local(2026, 10, 5)) * 10 + 5
  const track = view.container.querySelector<HTMLElement>('[data-row-track="b"]')!
  fireEvent.pointerDown(track, { ...pointer, clientX: x })
  fireEvent.pointerUp(window, { ...pointer, clientX: x })
  await flush()
  expect(requests[0]!.body).toEqual({ due_start_at: null, due_at: local(2026, 10, 5, 9).toISOString(), expected_version: 3 })
})

test('a failed save shows a toast', async () => {
  const errorToast = spyOn(toast, 'error').mockImplementation(() => 0)
  globalThis.fetch = (async () => Response.json({ type: 'about:blank', title: 'Server error', status: 500, code: 'internal' }, { status: 500 })) as unknown as typeof fetch
  const view = renderTimeline([ranged])
  const bar = view.container.querySelector<HTMLElement>('[data-timeline-bar="a"]')!
  fireEvent.pointerDown(bar, { ...pointer, clientX: 100 })
  fireEvent.pointerMove(window, { ...pointer, clientX: 131 })
  fireEvent.pointerUp(window, { ...pointer, clientX: 131 })
  await waitFor(() => expect(errorToast).toHaveBeenCalledTimes(1))
  errorToast.mockRestore()
})

test('touch pointers do not start a drag', async () => {
  const requests = captureFetch()
  const view = renderTimeline([ranged])
  const bar = view.container.querySelector<HTMLElement>('[data-timeline-bar="a"]')!
  fireEvent.pointerDown(bar, { ...pointer, pointerType: 'touch', clientX: 100 })
  fireEvent.pointerMove(window, { ...pointer, pointerType: 'touch', clientX: 200 })
  fireEvent.pointerUp(window, { ...pointer, pointerType: 'touch', clientX: 200 })
  await flush()
  expect(requests).toHaveLength(0)
})

test('arrow keys move a focused bar; shift+arrow changes the end date; enter opens', async () => {
  const requests = captureFetch()
  const opened: string[] = []
  const view = renderTimeline([ranged], { onOpen: (id) => opened.push(id) })
  const bar = view.container.querySelector<HTMLElement>('[data-timeline-bar="a"]')!
  fireEvent.keyDown(bar, { key: 'ArrowRight' })
  await flush()
  expect(requests[0]!.body.due_start_at).toBe(local(2026, 9, 2).toISOString())
  expect(requests[0]!.body.due_at).toBe(local(2026, 9, 11, 9).toISOString())
  fireEvent.keyDown(bar, { key: 'ArrowLeft', shiftKey: true })
  await flush()
  expect(requests[1]!.body.due_at).toBe(local(2026, 9, 9, 9).toISOString())
  fireEvent.keyDown(bar, { key: 'Enter' })
  expect(opened).toEqual(['a'])
})

test('ctrl+wheel zooms in and plain wheel does not', () => {
  const zooms: number[] = []
  const workspace = { id: 'ws', name: 'Orbit', role: 'owner', version: 1 } as WorkspaceRecord
  const client = new QueryClient()
  const view = render(
    <QueryClientProvider client={client}>
      <WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}>
        <TaskTimeline tasks={[ranged]} projects={projects} statuses={statuses} users={[]} grouped pxPerDay={10} onZoomChange={(px) => zooms.push(px)} onOpen={() => {}} today={today} />
      </WorkspaceContext.Provider>
    </QueryClientProvider>,
  )
  const scroller = view.container.querySelector<HTMLElement>('[data-timeline-scroller]')!
  scroller.dispatchEvent(new WheelEvent('wheel', { deltaY: 40, bubbles: true, cancelable: true }))
  const zoom = new WheelEvent('wheel', { deltaY: -40, ctrlKey: true, bubbles: true, cancelable: true })
  // happy-dom's WheelEvent drops ctrlKey (browsers inherit it from MouseEvent)
  Object.defineProperty(zoom, 'ctrlKey', { value: true })
  scroller.dispatchEvent(zoom)
  expect(zooms).toHaveLength(1)
  expect(zooms[0]!).toBeGreaterThan(10)
  expect(zoom.defaultPrevented).toBe(true)
})

test('scroll position is saved and restored on the next mount', () => {
  const first = renderTimeline([ranged])
  const scroller = first.container.querySelector<HTMLElement>('[data-timeline-scroller]')!
  scroller.scrollLeft = 480
  scroller.scrollTop = 64
  fireEvent.scroll(scroller)
  first.unmount()
  const second = renderTimeline([ranged])
  const restored = second.container.querySelector<HTMLElement>('[data-timeline-scroller]')!
  expect(restored.scrollLeft).toBe(480)
  expect(restored.scrollTop).toBe(64)
})
