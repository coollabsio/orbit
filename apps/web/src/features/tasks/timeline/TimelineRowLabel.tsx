import { ChevronDown, Gps } from 'reicon-react'
import { UserAvatar } from '@/components/common/UserAvatar'
import type { TaskStatusDef } from '@/features/tasks/api/models'
import type { User } from '@/features/workspaces/models'
import { TaskStatusIcon } from '../components/TaskStatusIcon'
import type { TimelineRow } from './timelineLib'

const CELL = 'sticky left-0 z-10 flex h-8 w-[280px] shrink-0 items-center gap-2 border-r border-border px-3 text-[13px] max-[899px]:hidden'
/** Sticky cells need an opaque fill; this one matches the row hover band. */
const ROW_FILL = 'bg-background group-hover/row:bg-[color-mix(in_oklch,var(--foreground)_3%,var(--background))]'
const GROUP_FILL = 'bg-[color-mix(in_oklch,var(--muted)_45%,var(--background))]'

/** Left-pane cell for a timeline row: project header, "No dates" toggle, or task. */
export function TimelineRowLabel({ row, status, assignee, onToggle, onOpen, onReveal }: {
  row: TimelineRow
  status?: TaskStatusDef
  assignee?: User
  onToggle: (key: string, open: boolean) => void
  onOpen: (taskId: string) => void
  /** Scrolls the timeline to this task's bar; only for tasks with dates. */
  onReveal?: () => void
}) {
  if (row.kind === 'group') {
    return (
      <button type="button" className={`${CELL} ${GROUP_FILL} font-semibold text-foreground`} aria-expanded={row.open} onClick={() => onToggle(row.key, !row.open)}>
        <ChevronDown aria-hidden="true" className={`size-3.5 text-muted-foreground transition-transform duration-150 ${row.open ? '' : '-rotate-90'}`} />
        <span className="size-2 shrink-0 rounded-full" style={{ background: row.project.color }} />
        <span className="min-w-0 flex-1 truncate text-left">{row.project.name}</span>
        <span className="text-xs font-normal text-muted-foreground tabular-nums">{row.done}/{row.total}</span>
      </button>
    )
  }
  if (row.kind === 'undated') {
    return (
      <button type="button" className={`${CELL} ${ROW_FILL} pl-6 text-xs text-muted-foreground hover:text-foreground`} aria-expanded={row.open} onClick={() => onToggle(row.key, !row.open)}>
        <ChevronDown aria-hidden="true" className={`size-3 transition-transform duration-150 ${row.open ? '' : '-rotate-90'}`} />
        No dates ({row.count})
      </button>
    )
  }
  return (
    <div className={`${CELL} ${ROW_FILL} p-0`}>
      <button type="button" className="flex h-full min-w-0 flex-1 items-center gap-2 pr-3 pl-6 text-left text-foreground outline-none focus-visible:bg-accent" onClick={() => onOpen(row.task.id)}>
        <TaskStatusIcon status={status} />
        <span className="shrink-0 text-xs text-muted-foreground tabular-nums">{row.task.identifier}</span>
        <span className="min-w-0 flex-1 truncate">{row.task.title || 'Untitled'}</span>
        {assignee ? <UserAvatar user={assignee} size={18} /> : null}
      </button>
      {onReveal ? (
        // overlays the row end on hover, so it never costs the title any width
        <button
          type="button"
          aria-label={`Show ${row.task.identifier} on the timeline`}
          title="Show on timeline"
          className="absolute right-2 flex size-6 items-center justify-center rounded-md bg-[color-mix(in_oklch,var(--foreground)_3%,var(--background))] text-muted-foreground opacity-0 shadow-[-12px_0_8px_-2px_color-mix(in_oklch,var(--foreground)_3%,var(--background))] transition-[opacity,transform] duration-150 ease-out group-hover/row:opacity-100 hover:bg-accent hover:text-foreground focus-visible:opacity-100 active:scale-[0.94]"
          onClick={onReveal}
        >
          <Gps aria-hidden="true" className="size-3.5" />
        </button>
      ) : null}
    </div>
  )
}
