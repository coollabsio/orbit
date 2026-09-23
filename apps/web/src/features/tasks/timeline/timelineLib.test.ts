import { afterAll, describe, expect, test } from 'bun:test'
import type { Project, Task, TaskStatusDef } from '@/features/tasks/api/models'
import {
  MAX_ZOOM, MIN_ZOOM, ZOOM_PRESETS, anchoredScrollLeft, applyDrag, buildTimelineRows, clampZoom, computeRange,
  dayAt, dayIndex, dayIndexAtX, drawRange, isOverdue, isSameEdit, monthMarks, presetOf, rowDates, spanLabel,
  taskSpan, tickMarks, xOf, zoomFromWheel,
} from './timelineLib'

// set before any fixture below is built, so every local date is a Berlin date (DST-bearing zone)
const originalTz = process.env.TZ
process.env.TZ = 'Europe/Berlin'
afterAll(() => {
  if (originalTz === undefined) delete process.env.TZ
  else process.env.TZ = originalTz
})

const local = (y: number, m: number, d: number, h = 0, min = 0) => new Date(y, m - 1, d, h, min)
const iso = (date: Date) => date.toISOString()

function task(id: string, overrides: Partial<Task> = {}): Task {
  return {
    id, identifier: `ORB-${id}`, title: id, description: '', statusId: 'todo', position: 0, priority: 'none',
    assigneeIds: [], projectId: 'p1', labels: [], attachments: [], dueStartAt: null, dueAt: null,
    createdAt: '', updatedAt: '', comments: [], activity: [], version: 1, ...overrides,
  }
}

const statuses: TaskStatusDef[] = [
  { id: 'todo', projectId: 'p1', name: 'Todo', description: '', color: '#888', category: 'unstarted', position: 0, version: 1 },
  { id: 'done', projectId: 'p1', name: 'Done', description: '', color: '#4cb782', category: 'completed', position: 1, version: 1 },
  { id: 'gone', projectId: 'p1', name: 'Cancelled', description: '', color: '#888', category: 'cancelled', position: 2, version: 1 },
  { id: 'todo2', projectId: 'p2', name: 'Todo', description: '', color: '#888', category: 'unstarted', position: 0, version: 1 },
]
const projects = [
  { id: 'p1', name: 'Web', key: 'WEB', color: '#e0457b', version: 1 },
  { id: 'p2', name: 'API', key: 'API', color: '#26b5ce', version: 1 },
] as Project[]

describe('day scale', () => {
  const range = { start: local(2026, 3, 1), days: 60 }

  test('dayIndex counts calendar days across the March DST change', () => {
    // 2026-03-29 is the Berlin spring-forward day (23 hours long)
    expect(dayIndex(range, local(2026, 3, 29))).toBe(28)
    expect(dayIndex(range, local(2026, 3, 30))).toBe(29)
    expect(dayIndex(range, local(2026, 3, 30, 23, 30))).toBe(29)
  })

  test('dayAt is local midnight and round-trips through dayIndex', () => {
    const day = dayAt(range, 29)
    expect(day.getHours()).toBe(0)
    expect(day.getDate()).toBe(30)
    expect(dayIndex(range, day)).toBe(29)
  })

  test('x and day index convert at a given zoom', () => {
    expect(xOf(range, local(2026, 3, 3), 16)).toBe(32)
    expect(dayIndexAtX(32, 16)).toBe(2)
    expect(dayIndexAtX(47.9, 16)).toBe(2)
    expect(dayIndexAtX(-1, 16)).toBe(-1)
  })
})

describe('computeRange', () => {
  const today = local(2026, 9, 23)

  test('empty input spans today ± 6 months, starting on a month start', () => {
    const range = computeRange([], today)
    expect(range.start).toEqual(local(2026, 3, 1))
    expect(dayAt(range, range.days - 1)).toEqual(local(2027, 3, 23))
  })

  test('data outside the minimum window adds 3 months of padding', () => {
    const range = computeRange([local(2025, 1, 10), local(2028, 1, 5)], today)
    expect(range.start).toEqual(local(2024, 10, 1))
    expect(dayAt(range, range.days - 1)).toEqual(local(2028, 4, 5))
  })
})

describe('zoom', () => {
  test('clamps to the supported range', () => {
    expect(clampZoom(1)).toBe(MIN_ZOOM)
    expect(clampZoom(500)).toBe(MAX_ZOOM)
    expect(clampZoom(20)).toBe(20)
  })

  test('recognises presets exactly', () => {
    expect(presetOf(ZOOM_PRESETS.month)).toBe('month')
    expect(presetOf(17)).toBeNull()
  })

  test('wheel up zooms in, wheel down zooms out, both clamped', () => {
    expect(zoomFromWheel(16, -100)).toBeGreaterThan(16)
    expect(zoomFromWheel(16, 100)).toBeLessThan(16)
    expect(zoomFromWheel(MAX_ZOOM, -1000)).toBe(MAX_ZOOM)
  })

  test('anchored zoom keeps the date under the anchor fixed', () => {
    // anchor 200px into the viewport, scrolled 1000px, 10px/day → day 120 under the anchor
    const next = anchoredScrollLeft(1000, 200, 10, 20)
    expect((next + 200) / 20).toBe(120)
  })

  test('anchored zoom never scrolls to a negative position', () => {
    expect(anchoredScrollLeft(0, 50, 20, 5)).toBe(0)
  })
})

describe('marks', () => {
  const range = { start: local(2026, 3, 1), days: 61 }

  test('month marks cover each month with its width', () => {
    const marks = monthMarks(range, 10)
    expect(marks[0]).toEqual({ x: 0, width: 310, label: 'March 2026' })
    expect(marks[1]!.x).toBe(310)
    expect(marks[1]!.label).toBe('April')
  })

  test('daily ticks at week zoom, Monday ticks at month zoom, none below 5px', () => {
    expect(tickMarks(range, 44)).toHaveLength(61)
    const weekly = tickMarks(range, 16)
    expect(weekly[0]).toEqual({ x: 16 * 1, label: '2' }) // Mon 2 March 2026
    expect(weekly.every((tick) => (tick.x / 16) % 7 === 1)).toBe(true)
    expect(tickMarks(range, 4)).toHaveLength(0)
  })
})

describe('taskSpan', () => {
  test('null without a due date', () => {
    expect(taskSpan(task('a'))).toBeNull()
  })

  test('due-only is a point on the due day', () => {
    expect(taskSpan(task('a', { dueAt: iso(local(2026, 3, 10, 9)) }))).toEqual({ start: local(2026, 3, 10), end: local(2026, 3, 10), point: true })
  })

  test('range uses local days', () => {
    expect(taskSpan(task('a', { dueStartAt: iso(local(2026, 3, 2)), dueAt: iso(local(2026, 3, 10, 9)) })))
      .toEqual({ start: local(2026, 3, 2), end: local(2026, 3, 10), point: false })
  })

  test('start after end (legacy data) falls back to a point at the due day', () => {
    expect(taskSpan(task('a', { dueStartAt: iso(local(2026, 3, 12)), dueAt: iso(local(2026, 3, 10, 9)) }))!.point).toBe(true)
  })
})

describe('isOverdue', () => {
  const today = local(2026, 3, 20, 15)
  const due = { dueAt: iso(local(2026, 3, 19, 9)) }
  test('open task past its due day is overdue', () => {
    expect(isOverdue(task('a', due), 'started', today)).toBe(true)
  })
  test('due today is not overdue', () => {
    expect(isOverdue(task('a', { dueAt: iso(local(2026, 3, 20, 9)) }), 'unstarted', today)).toBe(false)
  })
  test('completed and cancelled tasks are never overdue', () => {
    expect(isOverdue(task('a', due), 'completed', today)).toBe(false)
    expect(isOverdue(task('a', due), 'cancelled', today)).toBe(false)
  })
})

describe('applyDrag', () => {
  const ranged = task('a', { dueStartAt: iso(local(2026, 3, 2)), dueAt: iso(local(2026, 3, 10, 14, 30)) })

  test('move shifts both dates and keeps the end time of day', () => {
    expect(applyDrag(ranged, 'move', 3)).toEqual({ dueStartAt: iso(local(2026, 3, 5)), dueAt: iso(local(2026, 3, 13, 14, 30)) })
  })

  test('move across the DST change keeps local midnight and local time', () => {
    const edit = applyDrag(ranged, 'move', 27)!
    expect(new Date(edit.dueStartAt!).getHours()).toBe(0)
    expect(new Date(edit.dueAt).getHours()).toBe(14)
    expect(new Date(edit.dueAt).getDate()).toBe(6) // 10 March + 27 days = 6 April
  })

  test('resizing the start cannot pass the end', () => {
    expect(applyDrag(ranged, 'start', 20)!.dueStartAt).toBe(iso(local(2026, 3, 10)))
  })

  test('resizing the end cannot pass the start', () => {
    expect(applyDrag(ranged, 'end', -20)!.dueAt).toBe(iso(local(2026, 3, 2, 14, 30)))
  })

  test('point move keeps no start; the start handle adds one', () => {
    const point = task('a', { dueAt: iso(local(2026, 3, 10, 9)) })
    expect(applyDrag(point, 'move', 1)).toEqual({ dueStartAt: null, dueAt: iso(local(2026, 3, 11, 9)) })
    expect(applyDrag(point, 'start', -4)).toEqual({ dueStartAt: iso(local(2026, 3, 6)), dueAt: iso(local(2026, 3, 10, 9)) })
    expect(applyDrag(point, 'end', 2)).toEqual({ dueStartAt: null, dueAt: iso(local(2026, 3, 12, 9)) })
  })

  test('legacy start-after-end moves as a point and clears the bad start', () => {
    const legacy = task('a', { dueStartAt: iso(local(2026, 3, 12)), dueAt: iso(local(2026, 3, 10, 9)) })
    expect(applyDrag(legacy, 'move', -1)).toEqual({ dueStartAt: null, dueAt: iso(local(2026, 3, 9, 9)) })
  })

  test('undated task has nothing to drag', () => {
    expect(applyDrag(task('a'), 'move', 1)).toBeNull()
  })
})

describe('drawRange and isSameEdit', () => {
  test('one day sets only the due date at 09:00', () => {
    expect(drawRange(local(2026, 3, 4), local(2026, 3, 4))).toEqual({ dueStartAt: null, dueAt: iso(local(2026, 3, 4, 9)) })
  })

  test('a range is ordered regardless of drag direction', () => {
    expect(drawRange(local(2026, 3, 9), local(2026, 3, 4))).toEqual({ dueStartAt: iso(local(2026, 3, 4)), dueAt: iso(local(2026, 3, 9, 9)) })
  })

  test('same edit compares instants, not strings', () => {
    const t = task('a', { dueStartAt: '2026-03-04T00:00:00+01:00', dueAt: iso(local(2026, 3, 9, 9)) })
    expect(isSameEdit(t, { dueStartAt: iso(local(2026, 3, 4)), dueAt: iso(local(2026, 3, 9, 9)) })).toBe(true)
    expect(isSameEdit(t, { dueStartAt: null, dueAt: iso(local(2026, 3, 9, 9)) })).toBe(false)
  })
})

describe('spanLabel', () => {
  test('range and single day', () => {
    expect(spanLabel({ start: local(2026, 3, 3), end: local(2026, 3, 17) })).toBe('Mar 3 → Mar 17 · 15 days')
    expect(spanLabel({ start: local(2026, 3, 3), end: local(2026, 3, 3) })).toBe('Mar 3 · 1 day')
  })
})

describe('buildTimelineRows', () => {
  const dated = task('dated', { position: 5, dueStartAt: iso(local(2026, 3, 2)), dueAt: iso(local(2026, 3, 10, 9)) })
  const earlier = task('earlier', { position: 9, dueAt: iso(local(2026, 3, 1, 9)) })
  const sameStartShorter = task('shorter', { position: 1, dueStartAt: iso(local(2026, 3, 2)), dueAt: iso(local(2026, 3, 4, 9)) })
  const undatedA = task('undatedA', { position: 2 })
  const undatedB = task('undatedB', { position: 1 })
  const doneTask = task('doneTask', { statusId: 'done', dueAt: iso(local(2026, 3, 20, 9)) })
  const cancelled = task('cancelled', { statusId: 'gone', position: 3 })
  const other = task('other', { projectId: 'p2', statusId: 'todo2', dueAt: iso(local(2026, 4, 1, 9)) })
  const all = [dated, earlier, sameStartShorter, undatedA, undatedB, doneTask, cancelled, other]

  test('grouped: project header, dated rows by start/end/position, collapsed "No dates"', () => {
    const rows = buildTimelineRows({ tasks: all, projects, statuses, grouped: true, overrides: {} })
    expect(rows.map((row) => row.key)).toEqual([
      'group:p1', 'task:earlier', 'task:shorter', 'task:dated', 'task:doneTask', 'undated:p1',
      'group:p2', 'task:other',
    ])
    const header = rows[0]!
    expect(header).toMatchObject({ kind: 'group', done: 1, total: 6, open: true, span: { start: local(2026, 3, 1), end: local(2026, 3, 20) } })
    expect(rows[5]).toMatchObject({ kind: 'undated', count: 3, open: false })
  })

  test('overrides open "No dates" (sorted by position) and collapse groups', () => {
    const rows = buildTimelineRows({ tasks: all, projects, statuses, grouped: true, overrides: { 'undated:p1': true, 'group:p2': false } })
    expect(rows.map((row) => row.key).slice(5)).toEqual(['undated:p1', 'task:undatedB', 'task:undatedA', 'task:cancelled', 'group:p2'])
  })

  test('flat (single project) has no group rows', () => {
    const rows = buildTimelineRows({ tasks: [dated, undatedA], projects, statuses, grouped: false, overrides: {} })
    expect(rows.map((row) => row.key)).toEqual(['task:dated', 'undated:all'])
  })

  test('groups without tasks are dropped and empty input gives no rows', () => {
    expect(buildTimelineRows({ tasks: [], projects, statuses, grouped: true, overrides: {} })).toEqual([])
  })

  test('rowDates lists span bounds for the range', () => {
    expect(rowDates([dated, undatedA])).toEqual([local(2026, 3, 2), local(2026, 3, 10)])
  })
})
