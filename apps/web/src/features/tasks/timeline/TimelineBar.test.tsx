import { expect, test } from 'bun:test'
import { render } from '@testing-library/react'
import type { Task, TaskStatusDef } from '@/features/tasks/api/models'
import { TimelineBar } from './TimelineBar'

const task: Task = {
  id: 'a', identifier: 'WEB-A', title: 'Timeline bar', description: '', statusId: 'dup', position: 0, priority: 'none',
  assigneeIds: [], projectId: 'p1', labels: [], attachments: [], dueStartAt: null, dueAt: null,
  createdAt: '', updatedAt: '', comments: [], activity: [], version: 1,
}
const duplicate: TaskStatusDef = { id: 'dup', projectId: 'p1', name: 'Duplicate', description: '', color: '#8b8f98', category: 'duplicate', position: 4, version: 1 }
const range = { start: new Date(2026, 8, 1), days: 30 }
const span = { start: new Date(2026, 8, 2), end: new Date(2026, 8, 20), point: false }

function renderBar(overrides: Partial<Task> = {}, status: TaskStatusDef = duplicate) {
  return render(
    <div className="relative">
      <TimelineBar task={{ ...task, ...overrides }} span={span} range={range} pxPerDay={10} color="#e0457b" status={status}
        assignee={undefined} overdueDays={0} dragging={false} onPointerDown={() => {}} onClick={() => {}} onKeyDown={() => {}} />
    </div>,
  )
}

test('a duplicate reads as closed work on the timeline', () => {
  const view = renderBar()
  expect(view.container.querySelector('[data-timeline-bar="a"]')!.className).toContain('opacity-50')
})

test('a blocked task shows the blocked icon in its bar label', () => {
  const view = renderBar({ blocked: true })
  expect(view.getByRole('img', { name: 'Blocked' })).toBeTruthy()
})
