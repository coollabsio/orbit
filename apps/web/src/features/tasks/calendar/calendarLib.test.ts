import { expect, test } from 'bun:test'
import { format } from 'date-fns'
import type { Task } from '@/features/tasks/api/models'
import { calendarTitle, calendarWeeks, columnAt, dayTasks, dueAtFor, hiddenCount, localeWeekStart, moveToDay, shiftCursor, weekSegments } from './calendarLib'

const day = (value: string) => new Date(`${value}T00:00:00`)
const iso = (value: string, hour = 9) => new Date(`${value}T${String(hour).padStart(2, '0')}:00:00`).toISOString()
const ymd = (date: Date) => format(date, 'yyyy-MM-dd')
function task(id: string, due: string | null, start: string | null = null, position = 0): Task {
  return {
    id, identifier: id, title: id, description: '', statusId: 'todo', position, priority: 'none', assigneeIds: [], projectId: 'p1',
    labels: [], attachments: [], dueStartAt: start ? iso(start, 0) : null, dueAt: due ? iso(due) : null, createdAt: '', updatedAt: '', comments: [], activity: [], version: 1,
  }
}

test('the week starts on the day of the locale', () => {
  expect(localeWeekStart('en-US')).toBe(0)
  expect(localeWeekStart('en-GB')).toBe(1)
  expect(localeWeekStart('not a locale')).toBe(1)
})

test('a month grid has full weeks with the days of the months next to it', () => {
  const weeks = calendarWeeks(day('2026-10-08'), 'month', 1)
  expect(weeks.length).toBe(5)
  expect(weeks.every((week) => week.length === 7)).toBe(true)
  expect(ymd(weeks[0]![0]!)).toBe('2026-09-28')
  expect(ymd(weeks[4]![6]!)).toBe('2026-11-01')
  // Sunday start: October 2026 starts on a Thursday
  expect(ymd(calendarWeeks(day('2026-10-08'), 'month', 0)[0]![0]!)).toBe('2026-09-27')
  // a month that needs six weeks
  expect(calendarWeeks(day('2026-08-15'), 'month', 1).length).toBe(6)
})

test('a week grid is the week of the cursor, and the controls step by one week or one month', () => {
  const [week] = calendarWeeks(day('2026-10-08'), 'week', 1)
  expect(week!.map(ymd)).toEqual(['2026-10-05', '2026-10-06', '2026-10-07', '2026-10-08', '2026-10-09', '2026-10-10', '2026-10-11'])
  expect(ymd(shiftCursor(day('2026-10-08'), 'week', 1))).toBe('2026-10-15')
  expect(ymd(shiftCursor(day('2026-10-31'), 'month', 1))).toBe('2026-11-01')
  expect(ymd(shiftCursor(day('2026-10-08'), 'month', -1))).toBe('2026-09-01')
  expect(calendarTitle(day('2026-10-08'), 'month', 1)).toBe('October 2026')
  expect(calendarTitle(day('2026-10-08'), 'week', 1)).toBe('Oct 5 – 11, 2026')
  expect(calendarTitle(day('2026-10-01'), 'week', 1)).toBe('Sep 28 – Oct 4, 2026')
})

test('a due date is one day, a range is a bar across its days, and a task with no due date is not on the calendar', () => {
  const week = calendarWeeks(day('2026-10-08'), 'week', 1)[0]!
  const segments = weekSegments([
    task('due', '2026-10-07'),
    task('range', '2026-10-09', '2026-10-06'),
    task('none', null),
    task('long', '2026-10-20', '2026-09-30'),
    task('other-week', '2026-10-14'),
    task('same-day', '2026-10-07', null, 1),
  ], week)
  expect(segments.map((segment) => [segment.task.id, segment.startCol, segment.endCol, segment.lane, segment.continuesBefore, segment.continuesAfter])).toEqual([
    ['long', 0, 6, 0, true, true],
    ['range', 1, 4, 1, false, false],
    ['due', 2, 2, 2, false, false],
    ['same-day', 2, 2, 3, false, false],
  ])
  expect(dayTasks(segments, 2).map((item) => item.id)).toEqual(['long', 'range', 'due', 'same-day'])
  expect(dayTasks(segments, 5).map((item) => item.id)).toEqual(['long'])
  // three lanes have room: the fourth task of Wednesday is behind "+1 more"
  expect(hiddenCount(segments, 2, 3)).toBe(1)
  expect(hiddenCount(segments, 1, 3)).toBe(0)
})

test('a lane is used again when it is free', () => {
  const week = calendarWeeks(day('2026-10-08'), 'week', 1)[0]!
  const segments = weekSegments([task('a', '2026-10-06', '2026-10-05'), task('b', '2026-10-08', '2026-10-07'), task('c', '2026-10-07', '2026-10-06')], week)
  expect(segments.map((segment) => [segment.task.id, segment.lane])).toEqual([['a', 0], ['c', 1], ['b', 0]])
})

test('a drag moves the due date by the days between the two days and a range keeps its length', () => {
  const due = task('due', '2026-10-07')
  expect(moveToDay(due, day('2026-10-07'), day('2026-10-07'))).toBeNull()
  expect(moveToDay(due, day('2026-10-07'), day('2026-10-10'))).toEqual({ dueStartAt: null, dueAt: iso('2026-10-10') })
  const range = task('range', '2026-10-09', '2026-10-06')
  // grabbed on its second day, dropped two days earlier
  expect(moveToDay(range, day('2026-10-07'), day('2026-10-05'))).toEqual({ dueStartAt: iso('2026-10-04', 0), dueAt: iso('2026-10-07') })
})

test('a new task from a day is due that day, and the pointer column falls back with no layout', () => {
  expect(dueAtFor(day('2026-10-08'))).toBe(iso('2026-10-08'))
  expect(columnAt(350, { left: 0, width: 700 }, 0)).toBe(3)
  expect(columnAt(9999, { left: 0, width: 700 }, 0)).toBe(6)
  expect(columnAt(350, { left: 0, width: 0 }, 2)).toBe(2)
})
