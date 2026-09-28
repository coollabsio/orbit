import { cn } from 'cn'
import type { TaskStatusDef } from '@/features/tasks/api/models'
import { projectStatuses } from '@/features/tasks/taskMeta'

const RADIUS = 5.25
const CIRCUMFERENCE = 2 * Math.PI * RADIUS
/** The default "Done" colour (DEFAULT_STATUS_TEMPLATES), for projects without a completed status. */
const DONE_FALLBACK = '#4cb782'

export function completedStatusColor(statuses: TaskStatusDef[], projectId: string): string {
  return projectStatuses(statuses, projectId).find((status) => status.category === 'completed')?.color ?? DONE_FALLBACK
}

/** `◔ 2/5`: closed/total direct sub-issues. The arc eases to its new length (200ms strong ease-out); reduced motion jumps. */
export function SubIssueProgress({ closed, total, color, className }: { closed: number; total: number; color: string; className?: string }) {
  const ratio = total > 0 ? Math.min(closed, total) / total : 0
  return (
    <span
      role="img"
      aria-label={`${closed} of ${total} sub-issues closed`}
      className={cn('inline-flex h-[22px] shrink-0 items-center gap-1 rounded-full border border-border px-1.5 text-xs leading-none font-medium text-muted-foreground tabular-nums', className)}
    >
      <svg aria-hidden width="14" height="14" viewBox="0 0 14 14" fill="none" className="-rotate-90">
        {/* muted-foreground/35, not --border: the border token is too close to white for an empty ring in light mode */}
        <circle data-slot="progress-track" cx="7" cy="7" r={RADIUS} strokeWidth="1.5" className="stroke-muted-foreground/35" />
        <circle
          data-slot="progress-arc"
          cx="7" cy="7" r={RADIUS} stroke={color} strokeWidth="1.5" strokeLinecap={ratio > 0 ? 'round' : 'butt'}
          strokeDasharray={CIRCUMFERENCE} strokeDashoffset={CIRCUMFERENCE * (1 - ratio)}
          className="transition-[stroke-dashoffset] duration-200 ease-[cubic-bezier(0.23,1,0.32,1)] motion-reduce:transition-none"
        />
      </svg>
      {closed}/{total}
    </span>
  )
}
