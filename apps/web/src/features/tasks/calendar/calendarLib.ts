import { addDays, addMonths, addWeeks, differenceInCalendarDays, endOfMonth, format, startOfDay, startOfMonth, startOfWeek } from 'date-fns'
import type { Task } from '@/features/tasks/api/models'
import { DEFAULT_DUE_HOUR, applyDrag, taskSpan, type DateEdit } from '@/features/tasks/timeline/timelineLib'

export type CalendarMode = 'month' | 'week'
/** 0 = Sunday … 6 = Saturday, as `Date.getDay()`. */
export type WeekStart = 0 | 1 | 2 | 3 | 4 | 5 | 6

/** The first day of the week of the user's locale (`Intl.Locale` week info: 1 = Monday … 7 = Sunday); Monday when unknown. */
export function localeWeekStart(locale: string | undefined = globalThis.navigator?.language): WeekStart {
  try {
    const info = new Intl.Locale(locale ?? 'en-GB') as Intl.Locale & { getWeekInfo?: () => { firstDay?: number }; weekInfo?: { firstDay?: number } }
    const first = (info.getWeekInfo?.() ?? info.weekInfo)?.firstDay
    if (typeof first === 'number' && first >= 1 && first <= 7) return (first % 7) as WeekStart
  } catch {
    // an unknown locale tag: the default below
  }
  return 1
}

/** The weeks of the grid: each week of the cursor's month (with the days of the months next to it), or the cursor's week. */
export function calendarWeeks(cursor: Date, mode: CalendarMode, weekStart: WeekStart): Date[][] {
  const first = startOfWeek(mode === 'month' ? startOfMonth(cursor) : cursor, { weekStartsOn: weekStart })
  const last = mode === 'month' ? startOfDay(endOfMonth(cursor)) : addDays(first, 6)
  const weeks: Date[][] = []
  for (let day = first; day <= last; day = addDays(day, 7)) {
    weeks.push(Array.from({ length: 7 }, (_, index) => addDays(day, index)))
  }
  return weeks
}

/** The cursor one month or one week later (`step` 1) or earlier (`step` -1). */
export function shiftCursor(cursor: Date, mode: CalendarMode, step: number): Date {
  return mode === 'month' ? addMonths(startOfMonth(cursor), step) : addWeeks(cursor, step)
}

/** "October 2026" for a month; "Oct 5 – 11, 2026" (or "Sep 28 – Oct 4, 2026") for a week. */
export function calendarTitle(cursor: Date, mode: CalendarMode, weekStart: WeekStart): string {
  if (mode === 'month') return format(cursor, 'MMMM yyyy')
  const first = startOfWeek(cursor, { weekStartsOn: weekStart })
  const last = addDays(first, 6)
  const end = first.getMonth() === last.getMonth() ? format(last, 'd, yyyy') : format(last, 'MMM d, yyyy')
  return `${format(first, 'MMM d')} – ${end}`
}

/** The part of one task inside one week: a due date is one day, a date range is a bar across its days. */
export interface WeekSegment {
  task: Task
  /** Columns 0–6 of the week, both included. */
  startCol: number
  endCol: number
  /** The row of the segment in the week; segments of one lane do not overlap. */
  lane: number
  /** The range starts before this week / ends after it. */
  continuesBefore: boolean
  continuesAfter: boolean
}

/**
 * The segments of a week, in lanes. A task with no due date has no segment. Earlier starts and longer ranges get
 * the upper lanes, so a range keeps a straight bar.
 */
export function weekSegments(tasks: Task[], week: Date[]): WeekSegment[] {
  const weekStart = week[0]!
  const placed = tasks
    .flatMap((task) => {
      const span = taskSpan(task)
      if (!span) return []
      const start = differenceInCalendarDays(span.start, weekStart)
      const end = differenceInCalendarDays(span.end, weekStart)
      if (end < 0 || start > 6) return []
      return [{ task, start, end }]
    })
    .sort((a, b) => a.start - b.start || b.end - a.end || a.task.position - b.task.position || a.task.id.localeCompare(b.task.id))
  // the last column each lane has in use
  const laneEnds: number[] = []
  return placed.map(({ task, start, end }) => {
    const startCol = Math.max(0, start)
    const endCol = Math.min(6, end)
    let lane = laneEnds.findIndex((used) => used < startCol)
    if (lane === -1) lane = laneEnds.length
    laneEnds[lane] = endCol
    return { task, startCol, endCol, lane, continuesBefore: start < 0, continuesAfter: end > 6 }
  })
}

/** The tasks of one day of the week, upper lane first. */
export function dayTasks(segments: WeekSegment[], col: number): Task[] {
  return segments
    .filter((segment) => segment.startCol <= col && col <= segment.endCol)
    .sort((a, b) => a.lane - b.lane)
    .map((segment) => segment.task)
}

/** How many tasks of the day have no room in `maxLanes` lanes: the "+N more" count. */
export function hiddenCount(segments: WeekSegment[], col: number, maxLanes: number): number {
  return segments.filter((segment) => segment.startCol <= col && col <= segment.endCol && segment.lane >= maxLanes).length
}

/** The dates of `task` after a drag from the day `grabbed` to the day `dropped`: a range keeps its length. Null when nothing moves. */
export function moveToDay(task: Task, grabbed: Date, dropped: Date): DateEdit | null {
  const delta = differenceInCalendarDays(dropped, grabbed)
  return delta === 0 ? null : applyDrag(task, 'move', delta)
}

/** The due date of a task made by a click on a day: that day at the default due hour, local time. */
export function dueAtFor(day: Date): string {
  const due = startOfDay(day)
  due.setHours(DEFAULT_DUE_HOUR, 0, 0, 0)
  return due.toISOString()
}

/** The column (0–6) of a pointer position in a week row; `fallback` when the row has no width (no layout). */
export function columnAt(clientX: number, rect: { left: number; width: number }, fallback: number): number {
  if (!(rect.width > 0)) return fallback
  return Math.min(6, Math.max(0, Math.floor(((clientX - rect.left) / rect.width) * 7)))
}
