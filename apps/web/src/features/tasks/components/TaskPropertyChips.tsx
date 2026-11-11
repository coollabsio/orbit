import { Calendar, Diamonds, RefreshCircle, Repeat, Triangle } from 'reicon-react'
import { cn } from 'cn'
import type { Cycle, Milestone, Project, Task, TaskStatusDef } from '@/features/tasks/api/models'
import { cycleName, estimateLabel, estimateShortLabel, isMilestoneLate } from '@/features/tasks/planningLib'
import { isOverdue } from '@/features/tasks/timeline/timelineLib'
import { fullDate, shortDate } from '@/lib/format'
import { ColorDot } from '@/components/common/ColorDot'

/**
 * Quiet due-date chip; open tasks past their day turn destructive, and a repeating task shows a repeat glyph instead of
 * the calendar (the picker's tooltip names the rule). Font size comes from the caller.
 */
export function DueDateChip({ task, status, className }: { task: Task; status: TaskStatusDef | undefined; className?: string }) {
  if (!task.dueAt) return null
  const overdue = isOverdue(task, status?.category, new Date())
  return (
    <span
      data-property="due_date"
      title={task.recurrence ? undefined : `Due ${fullDate(task.dueAt)}`}
      className={cn('inline-flex shrink-0 items-center gap-1 whitespace-nowrap text-muted-foreground tabular-nums', overdue && 'text-destructive', className)}
    >
      {task.recurrence ? <Repeat aria-hidden className="size-[1.1em]" /> : <Calendar aria-hidden className="size-[1.1em]" />}
      {shortDate(task.dueAt)}
    </span>
  )
}

/** Project colour dot + name, truncated. */
export function ProjectChip({ project, className }: { project: Project | undefined; className?: string }) {
  if (!project) return null
  return (
    <span data-property="project" title={project.name} className={cn('inline-flex max-w-[140px] min-w-0 shrink-0 items-center gap-1.5 text-muted-foreground', className)}>
      <ColorDot color={project.color} />
      <span className="truncate">{project.name}</span>
    </span>
  )
}

/** Milestone flag + name, truncated. */
export function MilestoneChip({ milestone, className }: { milestone: Milestone | undefined; className?: string }) {
  if (!milestone) return null
  return (
    <span data-property="milestone" title={`Milestone: ${milestone.name}`} className={cn('inline-flex max-w-[140px] min-w-0 shrink-0 items-center gap-1 text-muted-foreground', className)}>
      <Diamonds aria-hidden className="size-[1.1em] shrink-0" />
      <span className="truncate">{milestone.name}</span>
    </span>
  )
}

/** "Late" beside a milestone group's name: its target day has passed and tasks are unfinished. */
export function MilestoneLate({ milestone }: { milestone: Milestone | undefined }) {
  if (!milestone || !isMilestoneLate(milestone)) return null
  return <span title="Past its target date with unfinished tasks" className="shrink-0 font-normal text-destructive">Late</span>
}

/** Cycle icon + name ("Cycle 3" when it has none), truncated. */
export function CycleChip({ cycle, className }: { cycle: Cycle | undefined; className?: string }) {
  if (!cycle) return null
  const name = cycleName(cycle)
  return (
    <span data-property="cycle" title={`Cycle: ${name}`} className={cn('inline-flex max-w-[140px] min-w-0 shrink-0 items-center gap-1 text-muted-foreground', className)}>
      <RefreshCircle aria-hidden className="size-[1.1em] shrink-0" />
      <span className="truncate">{name}</span>
    </span>
  )
}

/** "3 pts"; nothing for an unestimated task. */
export function EstimateChip({ task, className }: { task: Task; className?: string }) {
  if (task.estimate == null) return null
  return (
    <span data-property="estimate" title={`Estimate: ${estimateLabel(task.estimate)}`} className={cn('inline-flex shrink-0 items-center gap-1 whitespace-nowrap text-muted-foreground tabular-nums', className)}>
      <Triangle aria-hidden className="size-[1.1em]" />
      {estimateShortLabel(task.estimate)}
    </span>
  )
}

/** Created/updated date; the full date and which one it is live in the tooltip. */
export function DateStamp({ property, iso, className }: { property: 'created' | 'updated'; iso: string; className?: string }) {
  return (
    <span data-property={property} title={`${property === 'created' ? 'Created' : 'Updated'} ${fullDate(iso)}`} className={cn('shrink-0 whitespace-nowrap tabular-nums', className)}>
      {shortDate(iso)}
    </span>
  )
}
