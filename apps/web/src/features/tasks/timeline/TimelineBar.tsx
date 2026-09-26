import type { CSSProperties, KeyboardEvent, PointerEvent } from 'react'
import { UserAvatar } from '@/components/common/UserAvatar'
import type { Task, TaskStatusDef } from '@/features/tasks/api/models'
import { isClosedCategory } from '@/features/tasks/taskMeta'
import type { User } from '@/features/workspaces/models'
import { TaskStatusIcon } from '../components/TaskStatusIcon'
import { BlockedIndicator } from '../components/BlockedIndicator'
import { dayIndex, spanLabel, xOf, type DragMode, type TaskSpan, type TimeRange } from './timelineLib'

/** Below this width the label moves outside, to the right of the bar. */
const LABEL_MIN_WIDTH = 120
const DIAMOND = 12
const GAP = 6

/** Edge hit zone with a grip that shows on hover, so resizing is discoverable. */
function ResizeHandle({ side, onPointerDown }: { side: 'start' | 'end'; onPointerDown: (event: PointerEvent) => void }) {
  return (
    <span
      aria-hidden="true"
      className={`absolute inset-y-0 z-10 flex w-2 cursor-ew-resize items-center justify-center ${side === 'start' ? '-left-px' : '-right-px'}`}
      onPointerDown={(event) => { event.stopPropagation(); onPointerDown(event) }}
    >
      <span className="h-3 w-0.5 rounded-full bg-foreground/50 opacity-0 transition-opacity duration-150 group-hover/bar:opacity-100" />
    </span>
  )
}

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
  const closed = isClosedCategory(status?.category)
  const left = xOf(range, span.start, pxPerDay)
  const width = (dayIndex(range, span.end) - dayIndex(range, span.start) + 1) * pxPerDay
  const tailStart = xOf(range, span.end, pxPerDay) + pxPerDay
  const tailEnd = tailStart + overdueDays * pxPerDay
  const label = (
    <>
      <TaskStatusIcon status={status} size={13} />
      <span className="truncate">{task.title || 'Untitled'}</span>
      {task.blocked ? <BlockedIndicator /> : null}
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
  // day-snapped moves glide instead of jumping while the pointer drags
  const motion = 'data-[dragging]:transition-[left,width] data-[dragging]:duration-100 data-[dragging]:ease-out'
  const tail = overdueDays > 0 ? (
    <div
      data-overdue-tail
      aria-hidden="true"
      className="pointer-events-none absolute top-2 bottom-2 rounded-r-sm"
      style={{
        left: tailStart,
        width: overdueDays * pxPerDay,
        background: 'repeating-linear-gradient(135deg, color-mix(in oklch, var(--destructive) 30%, transparent) 0 3px, transparent 3px 7px)',
      }}
    />
  ) : null

  if (span.point) {
    const boxLeft = left + pxPerDay / 2 - DIAMOND / 2
    // an overdue diamond's label sits after its tail, where it stays readable
    const labelShift = overdueDays > 0 ? Math.max(0, tailEnd + GAP - (boxLeft + DIAMOND + GAP)) : 0
    return (
      <>
        {tail}
        <div
          {...common}
          className={`group/bar absolute top-1 flex h-6 cursor-grab items-center text-xs whitespace-nowrap text-foreground outline-none select-none focus-visible:[&>[data-diamond]]:ring-2 focus-visible:[&>[data-diamond]]:ring-primary data-[dragging]:cursor-grabbing ${motion} ${closed ? 'opacity-50' : ''}`}
          style={{ left: boxLeft }}
          onPointerDown={(event) => onPointerDown(event, 'move')}
        >
          {/* drag this grip left to give a due-only task a start date */}
          <span
            aria-hidden="true"
            className="absolute top-1/2 -left-3 flex h-4 w-2.5 -translate-y-1/2 cursor-ew-resize items-center justify-center opacity-0 transition-opacity duration-150 group-hover/bar:opacity-100"
            onPointerDown={(event) => { event.stopPropagation(); onPointerDown(event, 'start') }}
          >
            <span className="h-3 w-0.5 rounded-full bg-foreground/50" />
          </span>
          <span data-diamond className="size-3 shrink-0 rotate-45 rounded-[3px] shadow-[0_0_0_2px_var(--background)]" style={{ background: color }} />
          <span data-bar-label className="flex items-center gap-1.5 pl-1.5" style={{ marginLeft: labelShift }}>{label}</span>
        </div>
      </>
    )
  }

  const narrow = width < LABEL_MIN_WIDTH
  const barWidth = Math.max(width, 8)
  // tint from the project colour; started work reads a step stronger than work not begun
  const tint = { '--bar': color } as CSSProperties
  return (
    <>
      {tail}
      <div
        {...common}
        data-started={status?.category === 'started' || undefined}
        className={`group/bar absolute top-1 flex h-6 cursor-grab items-center gap-1.5 rounded-md border border-[color-mix(in_oklab,var(--bar)_38%,transparent)] bg-[color-mix(in_oklab,var(--bar)_12%,var(--background))] px-2 text-xs font-medium text-foreground outline-none select-none hover:border-[color-mix(in_oklab,var(--bar)_65%,transparent)] focus-visible:ring-2 focus-visible:ring-primary/60 data-[dragging]:cursor-grabbing data-[dragging]:shadow-lg data-[started]:bg-[color-mix(in_oklab,var(--bar)_22%,var(--background))] ${motion} ${closed ? 'opacity-50' : ''}`}
        style={{ ...tint, left, width: barWidth }}
        onPointerDown={(event) => onPointerDown(event, 'move')}
      >
        <ResizeHandle side="start" onPointerDown={(event) => onPointerDown(event, 'start')} />
        {narrow ? null : (
          <>
            {/* the label follows the visible edge when the bar starts left of the view */}
            <span className="sticky left-[calc(var(--timeline-left)+8px)] flex min-w-0 items-center gap-1.5">{label}</span>
            <span className="flex-1" />
            {assignee ? <UserAvatar user={assignee} size={16} /> : null}
          </>
        )}
        <ResizeHandle side="end" onPointerDown={(event) => onPointerDown(event, 'end')} />
      </div>
      {narrow ? (
        <span className={`pointer-events-none absolute top-1 flex h-6 items-center gap-1.5 text-xs whitespace-nowrap text-foreground ${closed ? 'opacity-50' : ''}`} style={{ left: Math.max(left + barWidth, overdueDays > 0 ? tailEnd : 0) + GAP }}>
          {label}
        </span>
      ) : null}
    </>
  )
}
