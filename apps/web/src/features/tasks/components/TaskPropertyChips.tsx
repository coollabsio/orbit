import { Calendar, Refresh2, Signpost } from 'reicon-react'
import { cn } from 'cn'
import type { Project, Task, TaskStatusDef } from '@/features/tasks/api/models'
import { isOverdue } from '@/features/tasks/timeline/timelineLib'
import { fullDate, shortDate } from '@/lib/format'
import { ColorDot } from '@/components/common/ColorDot'
import { useCycles } from '@/features/tasks/api/cycles'
import { useMilestones } from '@/features/tasks/api/milestones'
import { cycleName, estimateLabel, groupPoints, pointsText } from '@/features/tasks/cyclesLib'
import { useWorkspace } from '@/features/workspaces/workspaceContext'

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
      <ColorDot color={project.color} />
      <span className="truncate">{project.name}</span>
    </span>
  )
}

/** The task's milestone, truncated; nothing for a task with none. The name comes from the shared milestones query. */
export function MilestoneChip({ task, className }: { task: Pick<Task, 'milestoneId'>; className?: string }) {
  const { workspace } = useWorkspace()
  const milestone = useMilestones(workspace.id, Boolean(task.milestoneId)).data?.find((item) => item.id === task.milestoneId)
  if (!milestone) return null
  return (
    <span data-property="milestone" title={`Milestone: ${milestone.name}`} className={cn('inline-flex max-w-[140px] min-w-0 shrink-0 items-center gap-1 text-muted-foreground', className)}>
      <Signpost aria-hidden className="size-[1.1em] shrink-0" />
      <span className="truncate">{milestone.name}</span>
    </span>
  )
}

/** The task's cycle, truncated; nothing for a task with none. The name comes from the project's cycles query. */
export function CycleChip({ task, className }: { task: Pick<Task, 'cycleId' | 'projectId'>; className?: string }) {
  const { workspace } = useWorkspace()
  const cycle = useCycles(workspace.id, task.cycleId ? task.projectId : undefined).data?.find((item) => item.id === task.cycleId)
  if (!cycle) return null
  return (
    <span data-property="cycle" title={`Cycle: ${cycleName(cycle)}`} className={cn('inline-flex max-w-[120px] min-w-0 shrink-0 items-center gap-1 text-muted-foreground', className)}>
      <Refresh2 aria-hidden className="size-[1.1em] shrink-0" />
      <span className="truncate">{cycleName(cycle)}</span>
    </span>
  )
}

/** The estimate in the project's scale. A task with sub-issues shows the sum of their estimates instead of its own. */
export function EstimateChip({ task, project, className }: { task: Pick<Task, 'estimate' | 'subIssueEstimate' | 'subIssueCount'>; project: Project | undefined; className?: string }) {
  if (!project?.estimate_scale) return null
  const parent = (task.subIssueCount ?? 0) > 0
  const points = parent ? task.subIssueEstimate : task.estimate
  if (points == null) return null
  return (
    <span data-property="estimate" title={parent ? `Estimate of the sub-issues: ${points}` : `Estimate: ${points}`} className={cn('inline-flex shrink-0 items-center rounded border px-1 text-muted-foreground tabular-nums', className)}>
      {parent ? String(points) : estimateLabel(points, project.estimate_scale)}
    </span>
  )
}

/** The points of a group or a board column, after its task count; nothing where estimates are off. */
export function GroupPoints({ tasks, projects }: { tasks: Task[]; projects: Project[] }) {
  const points = groupPoints(tasks, projects)
  if (points === null) return null
  return <span data-slot="group-points" title={pointsText(points)} className="font-normal text-muted-foreground/70 tabular-nums">· {points} pt{points === 1 ? '' : 's'}</span>
}

/** Created/updated date; the full date and which one it is live in the tooltip. */
export function DateStamp({ property, iso, className }: { property: 'created' | 'updated'; iso: string; className?: string }) {
  return (
    <span data-property={property} title={`${property === 'created' ? 'Created' : 'Updated'} ${fullDate(iso)}`} className={cn('shrink-0 whitespace-nowrap tabular-nums', className)}>
      {shortDate(iso)}
    </span>
  )
}
