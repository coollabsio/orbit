import { ChevronDown } from 'reicon-react'
import { UserAvatar } from '@/components/common/UserAvatar'
import type { TaskStatusDef } from '@/features/tasks/api/models'
import type { User } from '@/features/workspaces/models'
import { TaskStatusIcon } from '../components/TaskStatusIcon'
import type { TimelineRow } from './timelineLib'

const CELL = 'sticky left-0 z-10 flex h-8 w-[280px] shrink-0 items-center gap-2 border-r border-border bg-background px-3 text-[13px] max-[899px]:hidden'

/** Left-pane cell for a timeline row: project header, "No dates" toggle, or task. */
export function TimelineRowLabel({ row, status, assignee, onToggle, onOpen }: {
  row: TimelineRow
  status?: TaskStatusDef
  assignee?: User
  onToggle: (key: string, open: boolean) => void
  onOpen: (taskId: string) => void
}) {
  if (row.kind === 'group') {
    return (
      <button type="button" className={`${CELL} font-semibold text-foreground`} aria-expanded={row.open} onClick={() => onToggle(row.key, !row.open)}>
        <ChevronDown aria-hidden="true" className={`size-3.5 text-muted-foreground transition-transform duration-150 ${row.open ? '' : '-rotate-90'}`} />
        <span className="size-2 shrink-0 rounded-full" style={{ background: row.project.color }} />
        <span className="min-w-0 flex-1 truncate text-left">{row.project.name}</span>
        <span className="text-xs font-normal text-muted-foreground tabular-nums">{row.done}/{row.total}</span>
      </button>
    )
  }
  if (row.kind === 'undated') {
    return (
      <button type="button" className={`${CELL} pl-6 text-xs text-muted-foreground`} aria-expanded={row.open} onClick={() => onToggle(row.key, !row.open)}>
        <ChevronDown aria-hidden="true" className={`size-3 transition-transform duration-150 ${row.open ? '' : '-rotate-90'}`} />
        No dates ({row.count})
      </button>
    )
  }
  return (
    <button type="button" className={`${CELL} pl-6 text-left text-foreground hover:bg-accent`} onClick={() => onOpen(row.task.id)}>
      <TaskStatusIcon status={status} />
      <span className="shrink-0 text-xs text-muted-foreground tabular-nums">{row.task.identifier}</span>
      <span className="min-w-0 flex-1 truncate">{row.task.title || 'Untitled'}</span>
      {assignee ? <UserAvatar user={assignee} size={18} /> : null}
    </button>
  )
}
