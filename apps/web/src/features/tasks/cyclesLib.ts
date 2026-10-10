import { format } from 'date-fns'
import type { CycleRecord } from '@/api/generated/types.gen'

export type EstimateScale = 'fibonacci' | 'linear' | 'tshirt'

export const ESTIMATE_SCALES: readonly EstimateScale[] = ['fibonacci', 'linear', 'tshirt']
export const SCALE_LABEL: Record<EstimateScale, string> = {
  fibonacci: 'Fibonacci (1, 2, 3, 5, 8, 13)',
  linear: 'Linear (1, 2, 3, 4, 5)',
  tshirt: 'T-shirt (XS, S, M, L, XL)',
}

/** The stored value is always points; a scale only decides which values are offered and how they show. */
const SCALE_POINTS: Record<EstimateScale, readonly number[]> = {
  fibonacci: [1, 2, 3, 5, 8, 13],
  linear: [1, 2, 3, 4, 5],
  tshirt: [1, 2, 3, 5, 8],
}
const TSHIRT: Record<number, string> = { 1: 'XS', 2: 'S', 3: 'M', 5: 'L', 8: 'XL' }

export function isEstimateScale(value: string | null | undefined): value is EstimateScale {
  return value === 'fibonacci' || value === 'linear' || value === 'tshirt'
}

/** How an estimate shows in a project: the T-shirt size where the scale has one for the value, else the number. */
export function estimateLabel(points: number, scale: string | null | undefined): string {
  return scale === 'tshirt' ? TSHIRT[points] ?? String(points) : String(points)
}

/** The values the estimate picker offers; none while estimates are off. */
export function estimateOptions(scale: string | null | undefined): Array<{ points: number; label: string }> {
  return isEstimateScale(scale) ? SCALE_POINTS[scale].map((points) => ({ points, label: estimateLabel(points, scale) })) : []
}

/** "3 points", "1 point". */
export function pointsText(points: number): string {
  return `${points} ${points === 1 ? 'point' : 'points'}`
}

type CycleName = Pick<CycleRecord, 'name' | 'number'>
type CycleDates = Pick<CycleRecord, 'starts_at' | 'ends_at'>

/** The name of a cycle; "Cycle {number}" while it has none. */
export function cycleName(cycle: CycleName): string {
  return cycle.name?.trim() || `Cycle ${cycle.number}`
}

/** The last day of a cycle. `ends_at` is the first instant after the cycle (00:01 of the day after). */
export function cycleLastDay(cycle: CycleDates): Date {
  return new Date(new Date(cycle.ends_at).getTime() - 2 * 60_000)
}

/** "Oct 5 – Oct 18"; the year shows when it is not the year of `now`. */
export function cycleDatesLabel(cycle: CycleDates, now: Date = new Date()): string {
  const start = new Date(cycle.starts_at)
  const end = cycleLastDay(cycle)
  const pattern = (date: Date) => (date.getFullYear() === now.getFullYear() ? 'MMM d' : 'MMM d, yyyy')
  return `${format(start, pattern(start))} – ${format(end, pattern(end))}`
}

export const STATE_LABEL: Record<string, string> = { current: 'Current', future: 'Upcoming', completed: 'Completed' }

/** Sunday first, as the server counts (0 = Sunday). */
export const WEEKDAYS = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'] as const

/** The cycles a task can be put in: the current one and the future ones, in time order. */
export function openCycles<T extends Pick<CycleRecord, 'state' | 'starts_at'>>(cycles: readonly T[]): T[] {
  return cycles.filter((cycle) => cycle.state !== 'completed').sort((a, b) => a.starts_at.localeCompare(b.starts_at))
}

/** Share of the scope that is done, 0 to 1, in points when the scope has points, else in tasks. */
export function cycleProgress(cycle: Pick<CycleRecord, 'scope_count' | 'scope_points' | 'done_count' | 'done_points'>): number {
  if (cycle.scope_points > 0) return cycle.done_points / cycle.scope_points
  return cycle.scope_count > 0 ? cycle.done_count / cycle.scope_count : 0
}

/**
 * The points of a group of tasks: the estimates of the tasks that have no sub-issues, in projects that have
 * estimates on. Null when none of the tasks is in such a project (the group then shows no points).
 */
export function groupPoints(
  tasks: ReadonlyArray<{ projectId: string; estimate?: number | null; subIssueCount?: number }>,
  projects: ReadonlyArray<{ id: string; estimate_scale?: string | null }>,
): number | null {
  const estimated = new Set(projects.filter((project) => project.estimate_scale).map((project) => project.id))
  const counted = tasks.filter((task) => estimated.has(task.projectId))
  if (counted.length === 0) return null
  return counted.reduce((sum, task) => sum + ((task.subIssueCount ?? 0) > 0 ? 0 : task.estimate ?? 0), 0)
}

const DAY_MS = 86_400_000

/**
 * A cycle boundary moved to another calendar day, for the date edit of a future cycle. The time of day stays, so
 * the boundary keeps the 00:01 of the project timezone (a daylight saving change between the two days can move it
 * by one hour). `day` is `YYYY-MM-DD` in the viewer's timezone; `shown` is the day the input showed before.
 */
export function moveBoundary(iso: string, shown: string, day: string): string {
  const days = Math.round((Date.parse(`${day}T00:00:00Z`) - Date.parse(`${shown}T00:00:00Z`)) / DAY_MS)
  return new Date(new Date(iso).getTime() + days * DAY_MS).toISOString()
}

/** `YYYY-MM-DD` of a date in the viewer's timezone, for a date input. */
export function dayInput(date: Date): string {
  return format(date, 'yyyy-MM-dd')
}
