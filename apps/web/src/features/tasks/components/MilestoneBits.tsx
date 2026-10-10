import { cn } from 'cn'
import { HEALTH_LABEL, type Milestone } from '@/features/tasks/api/milestones'
import { isHealth } from '@/features/tasks/roadmap/roadmapLib'
import { SubIssueProgress } from './SubIssueProgress'

/** The default "Done" colour, as in `SubIssueProgress`. */
const DONE_COLOR = '#4cb782'

/** A coloured dot for the health of a milestone; a hollow dot while it has no update. */
export function HealthDot({ health, className }: { health: string | null | undefined; className?: string }) {
  const known = isHealth(health) ? health : null
  return (
    <span
      role="img"
      data-slot="health-dot"
      // dashes: Tailwind reads an underscore in a variant value as a space
      data-health={known?.replace('_', '-') ?? 'none'}
      aria-label={known ? HEALTH_LABEL[known] : 'No update'}
      title={known ? HEALTH_LABEL[known] : 'No update'}
      className={cn(
        'size-2 shrink-0 rounded-full data-[health=at-risk]:bg-amber-500 data-[health=none]:border data-[health=none]:border-muted-foreground/50 data-[health=off-track]:bg-destructive data-[health=on-track]:bg-emerald-500',
        className,
      )}
    />
  )
}

/** Health dot with its name, e.g. "● At risk". */
export function HealthLabel({ health, className }: { health: string | null | undefined; className?: string }) {
  return (
    <span className={cn('inline-flex items-center gap-1.5 text-xs text-muted-foreground', className)}>
      <HealthDot health={health} />
      {isHealth(health) ? HEALTH_LABEL[health] : 'No update'}
    </span>
  )
}

/** `◔ 2/5`: done and total tasks of a milestone or a project. */
export function TaskProgress({ done, total, className }: { done: number; total: number; className?: string }) {
  return <SubIssueProgress closed={done} total={total} color={DONE_COLOR} noun="tasks" className={className} />
}

export function MilestoneProgress({ milestone, className }: { milestone: Pick<Milestone, 'task_count' | 'task_done_count'>; className?: string }) {
  return <TaskProgress done={milestone.task_done_count} total={milestone.task_count} className={className} />
}
