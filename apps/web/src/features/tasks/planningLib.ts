import { addDays } from 'date-fns'
import type { Cycle, Milestone } from '@/features/tasks/api/models'
import { shortDate } from '@/lib/format'

/** The quick picks of the estimate picker (Fibonacci-like points); any whole number up to `MAX_ESTIMATE` can be typed. */
export const ESTIMATE_OPTIONS = [0, 1, 2, 3, 5, 8, 13] as const
export const MAX_ESTIMATE = 1000

/** "1 point", "3 points". */
export function estimateLabel(points: number): string {
  return `${points} ${points === 1 ? 'point' : 'points'}`
}

/** "1 pt", "3 pts": the list and board chip. */
export function estimateShortLabel(points: number): string {
  return `${points} ${points === 1 ? 'pt' : 'pts'}`
}

/** A typed estimate: a whole number from 0 to `MAX_ESTIMATE`, else null. */
export function parseEstimate(text: string): number | null {
  const trimmed = text.trim()
  if (!/^\d+$/.test(trimmed)) return null
  const points = Number(trimmed)
  return points <= MAX_ESTIMATE ? points : null
}

/** The cycle's name, or "Cycle 3" when it has none. */
export function cycleName(cycle: Pick<Cycle, 'name' | 'number'>): string {
  return cycle.name?.trim() || `Cycle ${cycle.number}`
}

export type CycleTiming = 'current' | 'upcoming' | 'past'

/** `ends_at` is exclusive. */
export function cycleTiming(cycle: Pick<Cycle, 'starts_at' | 'ends_at'>, now: Date = new Date()): CycleTiming {
  const time = now.getTime()
  if (Date.parse(cycle.ends_at) <= time) return 'past'
  return Date.parse(cycle.starts_at) <= time ? 'current' : 'upcoming'
}

/** "Nov 3 – Nov 16": the first and the last day (`ends_at` is exclusive). */
export function cycleDates(cycle: Pick<Cycle, 'starts_at' | 'ends_at'>): string {
  return `${shortDate(cycle.starts_at)} – ${shortDate(new Date(Date.parse(cycle.ends_at) - 1).toISOString())}`
}

/** The picker's order: the current cycle and the upcoming ones by date, then the past ones, newest first. */
export function cyclePickerGroups(cycles: Cycle[], now: Date = new Date()): { open: Cycle[]; past: Cycle[] } {
  const byStart = [...cycles].sort((a, b) => a.starts_at.localeCompare(b.starts_at))
  return {
    open: byStart.filter((cycle) => cycleTiming(cycle, now) !== 'past'),
    past: byStart.filter((cycle) => cycleTiming(cycle, now) === 'past').reverse(),
  }
}

const DAY_MS = 86_400_000

/** `YYYY-MM-DD` of an instant in the local time zone (a date input's value). */
export function localDateInput(iso: string): string {
  const date = new Date(iso)
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`
}

/** Local midnight of a date input's day, as an instant. */
export function localMidnight(day: string): string {
  const [year, month, date] = day.split('-').map(Number)
  return new Date(year!, month! - 1, date!).toISOString()
}

type CycleSpan = Pick<Cycle, 'starts_at' | 'ends_at'>

/** A cycle's first and last day as date-input values; the stored end is exclusive, so the last day is the one before it. */
export function cycleDayRange(span: CycleSpan): { start: string; end: string } {
  return { start: localDateInput(span.starts_at), end: localDateInput(new Date(Date.parse(span.ends_at) - 1).toISOString()) }
}

/**
 * The instants a cycle form changed: a newly picked first day becomes its local midnight, a newly picked last day the
 * midnight after it. A day left as shown is left out, so its stored instant (another time zone's midnight, an
 * imported time) is kept.
 */
export function changedCycleSpan(span: CycleSpan, start: string, end: string): Partial<CycleSpan> {
  const shown = cycleDayRange(span)
  const [year, month, date] = end.split('-').map(Number)
  return {
    ...(start === shown.start ? {} : { starts_at: localMidnight(start) }),
    ...(end === shown.end ? {} : { ends_at: new Date(year!, month! - 1, date! + 1).toISOString() }),
  }
}

/**
 * A new cycle's span: it starts at the instant the last cycle ends and lasts as many days (calendar days, so a DST
 * change does not move the last day); the first cycle starts today and lasts two weeks.
 */
export function nextCycleSpan(cycles: Cycle[], now: Date = new Date()): CycleSpan {
  const last = [...cycles].sort((a, b) => a.ends_at.localeCompare(b.ends_at)).at(-1)
  if (!last) {
    const today = localMidnight(localDateInput(now.toISOString()))
    return { starts_at: today, ends_at: addDays(today, 14).toISOString() }
  }
  const days = Math.max(1, Math.round((Date.parse(last.ends_at) - Date.parse(last.starts_at)) / DAY_MS))
  return { starts_at: last.ends_at, ends_at: addDays(last.ends_at, days).toISOString() }
}

/** A milestone's target day is stored at noon UTC, so it reads as the same date in every time zone. */
export function targetDateValue(day: string): string | null {
  return day ? `${day}T12:00:00.000Z` : null
}

export function targetDateInput(iso: string | null): string {
  return iso ? iso.slice(0, 10) : ''
}

/** Local midnight of a milestone's target day: its UTC day, so it is the same date in every time zone. */
export function targetDay(targetAt: string): Date {
  const [year, month, date] = targetAt.slice(0, 10).split('-').map(Number)
  return new Date(year!, month! - 1, date!)
}

/** "Nov 20": a milestone's target day. */
export function targetLabel(targetAt: string): string {
  return targetDay(targetAt).toLocaleDateString(undefined, { month: 'short', day: 'numeric' })
}

/** Past its target day (before today) with tasks still unfinished. */
export function isMilestoneLate(milestone: Pick<Milestone, 'target_at' | 'task_count' | 'completed_count'>, now: Date = new Date()): boolean {
  if (!milestone.target_at || milestone.completed_count >= milestone.task_count) return false
  return targetDay(milestone.target_at) < new Date(now.getFullYear(), now.getMonth(), now.getDate())
}

type PlanningCounts = { task_count: number; completed_count: number; estimate_total: number; estimate_completed: number }

/** "3/8 tasks", plus "· 5/13 pts" once a task in it is estimated. Cancelled and duplicate tasks are not counted. */
export function planningProgress(counts: PlanningCounts): string {
  const tasks = `${counts.completed_count}/${counts.task_count} ${counts.task_count === 1 ? 'task' : 'tasks'}`
  return counts.estimate_total > 0 ? `${tasks} · ${counts.estimate_completed}/${counts.estimate_total} pts` : tasks
}
