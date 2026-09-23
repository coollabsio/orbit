import { addDays, addMonths, differenceInCalendarDays, eachMonthOfInterval, format, max, min, startOfDay, startOfMonth } from 'date-fns'
import type { Project, StatusCategory, Task, TaskStatusDef } from '@/features/tasks/api/models'

export type ZoomPreset = 'week' | 'month' | 'quarter'
export const ZOOM_PRESETS: Record<ZoomPreset, number> = { week: 44, month: 16, quarter: 5 }
export const MIN_ZOOM = 3
export const MAX_ZOOM = 64

export function clampZoom(px: number): number {
  return Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, px))
}

export function presetOf(px: number): ZoomPreset | null {
  return (Object.keys(ZOOM_PRESETS) as ZoomPreset[]).find((key) => ZOOM_PRESETS[key] === px) ?? null
}

/** Ctrl+wheel and trackpad pinch (browsers send pinch as ctrlKey wheel events). Negative deltaY zooms in. */
export function zoomFromWheel(px: number, deltaY: number): number {
  return clampZoom(px * Math.exp(-deltaY * 0.01))
}

/** New scrollLeft that keeps the day under `anchorX` (px from the track's visible left edge) in place. */
export function anchoredScrollLeft(scrollLeft: number, anchorX: number, oldPx: number, newPx: number): number {
  const day = (scrollLeft + anchorX) / oldPx
  return Math.max(0, day * newPx - anchorX)
}

/** Timeline window: `start` is local midnight of the first day; `days` columns follow. */
export interface TimeRange {
  start: Date
  days: number
}

export function dayIndex(range: TimeRange, date: Date): number {
  return differenceInCalendarDays(date, range.start)
}

export function dayAt(range: TimeRange, index: number): Date {
  return addDays(range.start, index)
}

export function xOf(range: TimeRange, date: Date, px: number): number {
  return dayIndex(range, date) * px
}

export function dayIndexAtX(x: number, px: number): number {
  return Math.floor(x / px)
}

/** Data bounds ± 3 months, never less than today ± 6 months; starts on a month start. */
export function computeRange(dates: Date[], today: Date): TimeRange {
  const earliest = min([today, ...dates])
  const latest = max([today, ...dates])
  const start = startOfMonth(min([addMonths(earliest, -3), addMonths(today, -6)]))
  const end = startOfDay(max([addMonths(latest, 3), addMonths(today, 6)]))
  return { start, days: differenceInCalendarDays(end, start) + 1 }
}

export interface MonthMark {
  x: number
  width: number
  label: string
}

/** One mark per month; January and the first mark also show the year. */
export function monthMarks(range: TimeRange, px: number): MonthMark[] {
  const last = dayAt(range, range.days - 1)
  return eachMonthOfInterval({ start: range.start, end: last }).map((month, index) => {
    const from = Math.max(0, dayIndex(range, month))
    const to = Math.min(range.days, dayIndex(range, addMonths(month, 1)))
    const label = index === 0 || month.getMonth() === 0 ? format(month, 'MMMM yyyy') : format(month, 'MMMM')
    return { x: from * px, width: (to - from) * px, label }
  })
}

export interface TickMark {
  x: number
  label: string
}

export const showsDailyTicks = (px: number) => px >= 28
export const showsWeeklyLines = (px: number) => px >= 5

/** Day-of-month labels: every day when wide, Mondays when medium, none when narrow. */
export function tickMarks(range: TimeRange, px: number): TickMark[] {
  if (!showsWeeklyLines(px)) return []
  const ticks: TickMark[] = []
  for (let index = 0; index < range.days; index++) {
    const day = dayAt(range, index)
    if (showsDailyTicks(px) || day.getDay() === 1) ticks.push({ x: index * px, label: String(day.getDate()) })
  }
  return ticks
}

export const DEFAULT_DUE_HOUR = 9

export interface TaskSpan {
  start: Date
  end: Date
  /** Due date only (or legacy start after end): drawn as a diamond. */
  point: boolean
}

type TaskDates = Pick<Task, 'dueStartAt' | 'dueAt'>

export function taskSpan(task: TaskDates): TaskSpan | null {
  if (!task.dueAt) return null
  const end = startOfDay(new Date(task.dueAt))
  const start = task.dueStartAt ? startOfDay(new Date(task.dueStartAt)) : null
  if (!start || start > end) return { start: end, end, point: true }
  return { start, end, point: false }
}

export function isOverdue(task: Task, category: StatusCategory | undefined, today: Date): boolean {
  if (!task.dueAt || category === 'completed' || category === 'cancelled') return false
  return startOfDay(new Date(task.dueAt)) < startOfDay(today)
}

export type TimelineRow =
  | { kind: 'group'; key: string; project: Project; done: number; total: number; span: { start: Date; end: Date } | null; open: boolean }
  | { kind: 'task'; key: string; task: Task; span: TaskSpan | null }
  | { kind: 'undated'; key: string; count: number; open: boolean }

/** Project groups start open; "No dates" sections start collapsed. */
const isOpen = (overrides: Record<string, boolean>, key: string) => overrides[key] ?? !key.startsWith('undated:')

function sectionRows(tasks: Task[], undatedKey: string, overrides: Record<string, boolean>): TimelineRow[] {
  const withSpan = tasks.map((task) => ({ task, span: taskSpan(task) }))
  const dated = withSpan
    .filter((item): item is { task: Task; span: TaskSpan } => item.span !== null)
    .sort((a, b) => a.span.start.getTime() - b.span.start.getTime() || a.span.end.getTime() - b.span.end.getTime() || a.task.position - b.task.position)
  const undated = withSpan.filter((item) => item.span === null).sort((a, b) => a.task.position - b.task.position)
  const rows: TimelineRow[] = dated.map(({ task, span }) => ({ kind: 'task', key: `task:${task.id}`, task, span }))
  if (undated.length > 0) {
    const open = isOpen(overrides, undatedKey)
    rows.push({ kind: 'undated', key: undatedKey, count: undated.length, open })
    if (open) rows.push(...undated.map(({ task }) => ({ kind: 'task' as const, key: `task:${task.id}`, task, span: null })))
  }
  return rows
}

/** Grouped = one section per project (all-projects view); flat = one project, no sections. */
export function buildTimelineRows(input: {
  tasks: Task[]
  projects: Project[]
  statuses: TaskStatusDef[]
  grouped: boolean
  overrides: Record<string, boolean>
}): TimelineRow[] {
  const { tasks, projects, statuses, grouped, overrides } = input
  if (!grouped) return sectionRows(tasks, 'undated:all', overrides)
  const categoryOf = new Map(statuses.map((status) => [status.id, status.category]))
  return projects.flatMap((project) => {
    const own = tasks.filter((task) => task.projectId === project.id)
    if (own.length === 0) return []
    const key = `group:${project.id}`
    const open = isOpen(overrides, key)
    const spans = own.map(taskSpan).filter((span): span is TaskSpan => span !== null)
    const header: TimelineRow = {
      kind: 'group', key, project, open,
      done: own.filter((task) => categoryOf.get(task.statusId) === 'completed').length,
      total: own.filter((task) => categoryOf.get(task.statusId) !== 'cancelled').length,
      span: spans.length > 0 ? { start: min(spans.map((s) => s.start)), end: max(spans.map((s) => s.end)) } : null,
    }
    return open ? [header, ...sectionRows(own, `undated:${project.id}`, overrides)] : [header]
  })
}

export function rowDates(tasks: Task[]): Date[] {
  return tasks.flatMap((task) => {
    const span = taskSpan(task)
    return span ? [span.start, span.end] : []
  })
}

export type DragMode = 'move' | 'start' | 'end'

export interface DateEdit {
  dueStartAt: string | null
  dueAt: string
}

function atTime(day: Date, hours: number, minutes: number): Date {
  const next = startOfDay(day)
  next.setHours(hours, minutes, 0, 0)
  return next
}

/** New dates after dragging by whole days. Start stays local midnight; the end keeps its time of day. */
export function applyDrag(task: TaskDates, mode: DragMode, deltaDays: number): DateEdit | null {
  const span = taskSpan(task)
  if (!span || !task.dueAt) return null
  const due = new Date(task.dueAt)
  const endAt = (day: Date) => atTime(day, due.getHours(), due.getMinutes()).toISOString()
  if (span.point) {
    // the start grip only means something when pulled earlier; a nudge or a pull right is a no-op
    if (mode === 'start') return { dueStartAt: deltaDays < 0 ? addDays(span.end, deltaDays).toISOString() : null, dueAt: task.dueAt }
    return { dueStartAt: null, dueAt: endAt(addDays(span.end, deltaDays)) }
  }
  if (mode === 'move') return { dueStartAt: addDays(span.start, deltaDays).toISOString(), dueAt: endAt(addDays(span.end, deltaDays)) }
  if (mode === 'start') return { dueStartAt: min([addDays(span.start, deltaDays), span.end]).toISOString(), dueAt: task.dueAt }
  return { dueStartAt: span.start.toISOString(), dueAt: endAt(max([addDays(span.end, deltaDays), span.start])) }
}

/** Click (one day) or press-drag (range) on an undated row. */
export function drawRange(a: Date, b: Date): DateEdit {
  const [lo, hi] = a <= b ? [a, b] : [b, a]
  const dueAt = atTime(hi, DEFAULT_DUE_HOUR, 0).toISOString()
  if (differenceInCalendarDays(hi, lo) === 0) return { dueStartAt: null, dueAt }
  return { dueStartAt: startOfDay(lo).toISOString(), dueAt }
}

const instant = (value: string | null | undefined) => (value ? new Date(value).getTime() : null)

export function isSameEdit(task: TaskDates, edit: DateEdit): boolean {
  return instant(task.dueStartAt) === instant(edit.dueStartAt) && instant(task.dueAt) === instant(edit.dueAt)
}

export function spanLabel(span: { start: Date; end: Date }): string {
  const days = differenceInCalendarDays(span.end, span.start) + 1
  const dayText = `${days} ${days === 1 ? 'day' : 'days'}`
  if (days === 1) return `${format(span.start, 'MMM d')} · ${dayText}`
  return `${format(span.start, 'MMM d')} → ${format(span.end, 'MMM d')} · ${dayText}`
}
