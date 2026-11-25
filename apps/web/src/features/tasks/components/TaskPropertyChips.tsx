import { Calendar, CheckSquare, Diamonds, RefreshCircle, Repeat, Triangle } from 'reicon-react'
import { cn } from 'cn'
import type { TaskField } from '@orbit/api/generated/types.gen'
import { FIELD_KIND_ICON, fieldValueText, selectedOptions } from '@/features/tasks/fieldsLib'
import { LabelPill } from './TaskLabels'
import type { Cycle, Milestone, Project, Task, TaskStatusDef } from '@/features/tasks/api/models'
import { cycleName, estimateLabel, estimateShortLabel, isMilestoneLate } from '@/features/tasks/planningLib'
import { isOverdue } from '@/features/tasks/timeline/timelineLib'
import { fullDate, shortDate } from '@/lib/format'
import { ProjectMark } from '@/components/common/ProjectMark'

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
      <ProjectMark project={project} iconClassName="size-3.5" />
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

/** `example.com` for a URL value (no `www.`); the text itself when it does not parse. */
function urlHost(value: string): string {
  try {
    return new URL(value).host.replace(/^www\./, '')
  } catch {
    return value
  }
}

/** Option chips show this many, then "+n". */
const MAX_OPTION_CHIPS = 2

/**
 * One custom field's value on a row or card, named in the tooltip: option chips in their colours, a checked box with
 * the field's name, or the kind's icon and a short value (number, date, URL host, text). Nothing without a value.
 */
export function CustomFieldChip({ field, value, className }: { field: TaskField; value: unknown; className?: string }) {
  const text = fieldValueText(field, value)
  if (!text) return null
  const shared = { 'data-property': 'custom_field', 'data-field-id': field.id, title: `${field.name}: ${text}` }
  if (field.kind === 'select' || field.kind === 'multi_select') {
    const chosen = selectedOptions(field, value)
    return (
      <span {...shared} className={cn('inline-flex max-w-[220px] min-w-0 shrink-0 items-center gap-1', className)}>
        {chosen.slice(0, MAX_OPTION_CHIPS).map((option) => <LabelPill key={option.id} label={option} className="min-w-0 truncate" />)}
        {chosen.length > MAX_OPTION_CHIPS ? <span className="shrink-0 text-muted-foreground tabular-nums">+{chosen.length - MAX_OPTION_CHIPS}</span> : null}
      </span>
    )
  }
  const Icon = field.kind === 'checkbox' ? CheckSquare : FIELD_KIND_ICON[field.kind]
  const shown = field.kind === 'checkbox'
    ? field.name
    : field.kind === 'date' && typeof value === 'string'
      ? shortDate(`${value}T00:00:00`)
      : field.kind === 'url' && typeof value === 'string' ? urlHost(value) : text
  return (
    <span {...shared} className={cn('inline-flex max-w-[140px] min-w-0 shrink-0 items-center gap-1 whitespace-nowrap text-muted-foreground', (field.kind === 'number' || field.kind === 'time_tracking') && 'tabular-nums', className)}>
      <Icon aria-hidden className="size-[1.1em] shrink-0" />
      <span className="truncate">{shown}</span>
    </span>
  )
}
