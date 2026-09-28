import { Calendar } from 'reicon-react'
import { cn } from 'cn'
import type { Project, Task, TaskStatusDef } from '@/features/tasks/api/models'
import { isOverdue } from '@/features/tasks/timeline/timelineLib'
import { fullDate, shortDate } from '@/lib/format'

/** Quiet due-date chip; open tasks past their day turn destructive. Font size comes from the caller. */
export function DueDateChip({ task, status, className }: { task: Task; status: TaskStatusDef | undefined; className?: string }) {
  if (!task.dueAt) return null
  const overdue = isOverdue(task, status?.category, new Date())
  return (
    <span
      data-property="due_date"
      title={`Due ${fullDate(task.dueAt)}`}
      className={cn('inline-flex shrink-0 items-center gap-1 whitespace-nowrap text-muted-foreground tabular-nums', overdue && 'text-destructive', className)}
    >
      <Calendar aria-hidden className="size-[1.1em]" />
      {shortDate(task.dueAt)}
    </span>
  )
}

/** Project colour dot + name, truncated. */
export function ProjectChip({ project, className }: { project: Project | undefined; className?: string }) {
  if (!project) return null
  return (
    <span data-property="project" title={project.name} className={cn('inline-flex max-w-[140px] min-w-0 shrink-0 items-center gap-1.5 text-muted-foreground', className)}>
      <span aria-hidden className="size-1.5 shrink-0 rounded-full" style={{ background: project.color }} />
      <span className="truncate">{project.name}</span>
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
