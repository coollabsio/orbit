import { addDays, format, startOfDay } from 'date-fns'
import type { Project } from '@/features/tasks/api/models'
import type { Milestone, MilestoneHealth, MilestoneStatus } from '@/features/tasks/api/milestones'
import type { DragMode } from '@/features/tasks/timeline/timelineLib'

type MilestoneDates = Pick<Milestone, 'start_at' | 'target_at'>

export interface MilestoneSpan {
  start: Date
  end: Date
  /** Only one of the two dates is set: drawn as a diamond on that day. */
  point: boolean
}

/** A milestone day is stored as the local midnight of that day. */
export function dayToIso(day: Date): string {
  return startOfDay(day).toISOString()
}

/** `yyyy-MM-dd` for a date input; empty for no date. */
export function isoToDayInput(iso: string | null | undefined): string {
  return iso ? format(new Date(iso), 'yyyy-MM-dd') : ''
}

/** The value of a date input as a stored day; null for an empty input. */
export function dayInputToIso(value: string): string | null {
  return value ? dayToIso(new Date(`${value}T00:00:00`)) : null
}

export function milestoneSpan(milestone: MilestoneDates): MilestoneSpan | null {
  const start = milestone.start_at ? startOfDay(new Date(milestone.start_at)) : null
  const end = milestone.target_at ? startOfDay(new Date(milestone.target_at)) : null
  if (start && end && start <= end) return { start, end, point: false }
  const only = end ?? start
  return only ? { start: only, end: only, point: true } : null
}

/**
 * The dates after a drag of `deltaDays`. A bar moves, or one edge moves and stops at the other edge. A diamond
 * moves its one date; a start grip pulled earlier on a target-only milestone (or an end grip pulled later on a
 * start-only one) adds the missing date and makes a bar. Null when the milestone has no dates.
 */
export function applyMilestoneDrag(milestone: MilestoneDates, mode: DragMode, deltaDays: number): MilestoneDates | null {
  const span = milestoneSpan(milestone)
  if (!span) return null
  const shifted = (day: Date) => dayToIso(addDays(day, deltaDays))
  if (span.point) {
    const targetOnly = Boolean(milestone.target_at)
    if (mode === 'start' && targetOnly) return deltaDays < 0 ? { start_at: shifted(span.end), target_at: milestone.target_at } : milestone
    if (mode === 'end' && !targetOnly) return deltaDays > 0 ? { start_at: milestone.start_at, target_at: shifted(span.start) } : milestone
    return targetOnly ? { start_at: null, target_at: shifted(span.end) } : { start_at: shifted(span.start), target_at: null }
  }
  if (mode === 'move') return { start_at: shifted(span.start), target_at: shifted(span.end) }
  if (mode === 'start') {
    const start = addDays(span.start, deltaDays)
    return { start_at: dayToIso(start > span.end ? span.end : start), target_at: milestone.target_at }
  }
  const end = addDays(span.end, deltaDays)
  return { start_at: milestone.start_at, target_at: dayToIso(end < span.start ? span.start : end) }
}

const instant = (iso: string | null | undefined) => (iso ? new Date(iso).getTime() : null)

export function sameDates(a: MilestoneDates, b: MilestoneDates): boolean {
  return instant(a.start_at) === instant(b.start_at) && instant(a.target_at) === instant(b.target_at)
}

export type RoadmapRow =
  | { kind: 'project'; key: string; project: Project; count: number }
  | { kind: 'milestone'; key: string; project: Project; milestone: Milestone; span: MilestoneSpan }
  /** The milestones of the project that have no dates: one row, a list of names. */
  | { kind: 'undated'; key: string; project: Project; milestones: Milestone[] }

/**
 * One group for each project that has milestones (projects by name): a header row, one row for each dated
 * milestone (by start, then target), and one row that lists the milestones with no dates.
 */
export function buildRoadmapRows(projects: Project[], milestones: Milestone[]): RoadmapRow[] {
  const rows: RoadmapRow[] = []
  for (const project of [...projects].sort((a, b) => a.name.localeCompare(b.name))) {
    const own = milestones.filter((milestone) => milestone.project_id === project.id)
    if (own.length === 0) continue
    rows.push({ kind: 'project', key: `project:${project.id}`, project, count: own.length })
    const dated = own
      .flatMap((milestone) => {
        const span = milestoneSpan(milestone)
        return span ? [{ milestone, span }] : []
      })
      .sort((a, b) => a.span.start.getTime() - b.span.start.getTime() || a.span.end.getTime() - b.span.end.getTime() || a.milestone.position - b.milestone.position)
    for (const { milestone, span } of dated) rows.push({ kind: 'milestone', key: milestone.id, project, milestone, span })
    const undated = own.filter((milestone) => milestoneSpan(milestone) === null)
    if (undated.length > 0) rows.push({ kind: 'undated', key: `undated:${project.id}`, project, milestones: undated })
  }
  return rows
}

/** Every date on the roadmap, for the time range. */
export function roadmapDates(milestones: Milestone[]): Date[] {
  return milestones.flatMap((milestone) => {
    const span = milestoneSpan(milestone)
    return span ? [span.start, span.end] : []
  })
}

const OPEN_STATUSES: MilestoneStatus[] = ['in_progress', 'planned']

/**
 * The milestone a project works on now: the first one in progress, else the next planned one (earliest target
 * date first, undated last). Undefined when every milestone is completed or cancelled.
 */
export function activeMilestone(milestones: Milestone[]): Milestone | undefined {
  const byTarget = (a: Milestone, b: Milestone) =>
    (instant(a.target_at) ?? Number.MAX_SAFE_INTEGER) - (instant(b.target_at) ?? Number.MAX_SAFE_INTEGER) || a.position - b.position
  for (const status of OPEN_STATUSES) {
    const [first] = milestones.filter((milestone) => milestone.status === status).sort(byTarget)
    if (first) return first
  }
  return undefined
}

/** Done and total tasks of a project: done is the completed category; total leaves out cancelled, duplicate and triage. */
export function projectProgress(counts: Record<string, number>): { done: number; total: number } {
  const total = Object.entries(counts).reduce((sum, [category, count]) => (category === 'cancelled' || category === 'duplicate' || category === 'triage' ? sum : sum + count), 0)
  return { done: counts.completed ?? 0, total }
}

export function isHealth(value: string | null | undefined): value is MilestoneHealth {
  return value === 'on_track' || value === 'at_risk' || value === 'off_track'
}

export function isMilestoneStatus(value: string): value is MilestoneStatus {
  return value === 'planned' || value === 'in_progress' || value === 'completed' || value === 'cancelled'
}

/** "Mar 3 – Apr 18", "Target Apr 18", "Starts Mar 3", or "No dates". */
export function milestoneDatesLabel(milestone: MilestoneDates): string {
  const day = (iso: string) => format(new Date(iso), 'MMM d')
  if (milestone.start_at && milestone.target_at) return `${day(milestone.start_at)} – ${day(milestone.target_at)}`
  if (milestone.target_at) return `Target ${day(milestone.target_at)}`
  if (milestone.start_at) return `Starts ${day(milestone.start_at)}`
  return 'No dates'
}
