import type { KeyboardEvent, PointerEvent } from 'react'
import { UserAvatar } from '@/components/common/UserAvatar'
import type { Task, TaskStatusDef } from '@/features/tasks/api/models'
import type { User } from '@/features/workspaces/models'
import { TaskStatusIcon } from '../components/TaskStatusIcon'
import { dayIndex, spanLabel, xOf, type DragMode, type TaskSpan, type TimeRange } from './timelineLib'

/** Below this width the label moves outside, to the right of the bar. */
const LABEL_MIN_WIDTH = 120
const HANDLE = 'absolute inset-y-0 w-1.5 cursor-ew-resize'

export interface TimelineBarProps {
  task: Task
  span: TaskSpan
  range: TimeRange
  pxPerDay: number
  color: string
  status: TaskStatusDef | undefined
  assignee: User | undefined
  overdueDays: number
  dragging: boolean
  onPointerDown: (event: PointerEvent, mode: DragMode) => void
  onClick: () => void
  onKeyDown: (event: KeyboardEvent) => void
}

/** A task on the timeline: a bar for a date range, a diamond for a due date only, plus an overdue tail. */
export function TimelineBar({ task, span, range, pxPerDay, color, status, assignee, overdueDays, dragging, onPointerDown, onClick, onKeyDown }: TimelineBarProps) {
  const closed = status?.category === 'completed' || status?.category === 'cancelled'
  const left = xOf(range, span.start, pxPerDay)
  const width = (dayIndex(range, span.end) - dayIndex(range, span.start) + 1) * pxPerDay
  const label = (
    <>
      <TaskStatusIcon status={status} size={13} />
      <span className="truncate">{task.title || 'Untitled'}</span>
    </>
  )
  const common = {
    'data-timeline-bar': task.id,
    'data-dragging': dragging || undefined,
    role: 'button',
    tabIndex: 0,
    'aria-label': `${task.identifier} ${task.title || 'Untitled'}, ${spanLabel(span)}`,
    onClick,
    onKeyDown,
  }
  const tail = overdueDays > 0 ? (
    <div
      data-overdue-tail
      aria-hidden="true"
      className="pointer-events-none absolute top-1.5 bottom-1.5 rounded-r-md border border-l-0 border-dashed border-destructive/50"
      style={{
        left: xOf(range, span.end, pxPerDay) + pxPerDay,
        width: overdueDays * pxPerDay,
        background: 'repeating-linear-gradient(135deg, color-mix(in oklch, var(--destructive) 28%, transparent) 0 4px, transparent 4px 8px)',
      }}
    />
  ) : null

  if (span.point) {
    const center = left + pxPerDay / 2
    return (
      <>
        {tail}
        <div
          {...common}
          className={`group/bar absolute top-1 flex h-6 cursor-grab items-center gap-1.5 text-xs whitespace-nowrap text-foreground outline-none select-none focus-visible:ring-1 focus-visible:ring-primary data-[dragging]:cursor-grabbing ${closed ? 'opacity-50' : ''}`}
          style={{ left: center - 6 }}
          onPointerDown={(event) => onPointerDown(event, 'move')}
        >
          {/* drag this handle left to give a due-only task a start date */}
          <span
            className="-ml-2 hidden h-4 w-2 cursor-ew-resize rounded-sm bg-foreground/30 group-hover/bar:block"
            aria-hidden="true"
            onPointerDown={(event) => { event.stopPropagation(); onPointerDown(event, 'start') }}
          />
          <span className="size-3 shrink-0 rotate-45 rounded-[2px] border border-background" style={{ background: color }} />
          {label}
        </div>
      </>
    )
  }

  const narrow = width < LABEL_MIN_WIDTH
  const barWidth = Math.max(width, 8)
  return (
    <>
      {tail}
      <div
        {...common}
        className={`absolute top-1 flex h-6 cursor-grab items-center gap-1.5 rounded-md border border-border bg-muted pr-1.5 pl-2 text-xs text-foreground shadow-xs outline-none select-none hover:border-foreground/25 focus-visible:ring-1 focus-visible:ring-primary data-[dragging]:cursor-grabbing data-[dragging]:shadow-md ${closed ? 'opacity-50' : ''}`}
        style={{ left, width: barWidth, borderLeft: `3px solid ${color}` }}
        onPointerDown={(event) => onPointerDown(event, 'move')}
      >
        <span className={`${HANDLE} left-0`} aria-hidden="true" onPointerDown={(event) => { event.stopPropagation(); onPointerDown(event, 'start') }} />
        {narrow ? null : (
          <>
            {/* the label follows the visible edge when the bar starts left of the view */}
            <span className="sticky left-[calc(var(--timeline-left)+8px)] flex min-w-0 items-center gap-1.5">{label}</span>
            <span className="flex-1" />
            {assignee ? <UserAvatar user={assignee} size={16} /> : null}
          </>
        )}
        <span className={`${HANDLE} right-0`} aria-hidden="true" onPointerDown={(event) => { event.stopPropagation(); onPointerDown(event, 'end') }} />
      </div>
      {narrow ? (
        <span className={`pointer-events-none absolute top-1 flex h-6 items-center gap-1.5 text-xs whitespace-nowrap text-foreground ${closed ? 'opacity-50' : ''}`} style={{ left: left + barWidth + 6 }}>
          {label}
        </span>
      ) : null}
    </>
  )
}
