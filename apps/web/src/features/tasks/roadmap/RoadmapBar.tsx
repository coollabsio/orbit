import type { CSSProperties, KeyboardEvent, PointerEvent } from 'react'
import { differenceInCalendarDays } from 'date-fns'
import type { Milestone } from '@/features/tasks/api/milestones'
import { HealthDot } from '@/features/tasks/components/MilestoneBits'
import { xOf, type DragMode, type TimeRange } from '@/features/tasks/timeline/timelineLib'
import { milestoneDatesLabel, type MilestoneSpan } from './roadmapLib'

const DIAMOND = 12
const LABEL_MIN_WIDTH = 72
const GAP = 8

interface RoadmapBarProps {
  milestone: Milestone
  span: MilestoneSpan
  range: TimeRange
  pxPerDay: number
  /** The project colour. */
  color: string
  dragging: boolean
  onPointerDown: (event: PointerEvent, mode: DragMode) => void
  onClick: () => void
  onKeyDown: (event: KeyboardEvent) => void
}

function Grip({ side, onPointerDown }: { side: 'start' | 'end'; onPointerDown: (event: PointerEvent) => void }) {
  return (
    <span
      aria-hidden="true"
      data-side={side}
      className="absolute top-0 flex h-full w-2 cursor-ew-resize items-center justify-center opacity-0 transition-opacity duration-150 group-hover/bar:opacity-100 data-[side=end]:right-0 data-[side=start]:left-0"
      onPointerDown={(event) => {
        event.stopPropagation()
        onPointerDown(event)
      }}
    >
      <span className="h-3 w-0.5 rounded-full bg-foreground/50" />
    </span>
  )
}

/** One milestone on the roadmap: a bar from start to target in the project colour with a health dot, or a diamond when it has one date. */
export function RoadmapBar({ milestone, span, range, pxPerDay, color, dragging, onPointerDown, onClick, onKeyDown }: RoadmapBarProps) {
  const left = xOf(range, span.start, pxPerDay)
  const closed = milestone.status === 'completed' || milestone.status === 'cancelled'
  const common = {
    'data-roadmap-bar': milestone.id,
    'data-dragging': dragging || undefined,
    'data-closed': closed || undefined,
    role: 'button',
    tabIndex: 0,
    'aria-label': `${milestone.name}, ${milestoneDatesLabel({ start_at: span.point ? null : span.start.toISOString(), target_at: span.end.toISOString() })}`,
    onClick,
    onKeyDown,
  }
  const label = (
    <>
      <HealthDot health={milestone.health} />
      <span className="truncate">{milestone.name}</span>
    </>
  )

  if (span.point) {
    return (
      <div
        {...common}
        className="group/bar absolute top-1 flex h-6 cursor-grab items-center text-xs whitespace-nowrap text-foreground outline-none select-none data-closed:opacity-50 data-dragging:cursor-grabbing focus-visible:[&>[data-diamond]]:ring-2 focus-visible:[&>[data-diamond]]:ring-primary"
        style={{ left: left + pxPerDay / 2 - DIAMOND / 2 }}
        onPointerDown={(event) => onPointerDown(event, 'move')}
      >
        <span data-diamond className="size-3 shrink-0 rotate-45 rounded-[3px] shadow-[0_0_0_2px_var(--background)]" style={{ background: color }} />
        <span className="flex items-center gap-1.5 pl-2">{label}</span>
      </div>
    )
  }

  const width = Math.max((differenceInCalendarDays(span.end, span.start) + 1) * pxPerDay, 8)
  const narrow = width < LABEL_MIN_WIDTH
  return (
    <>
      <div
        {...common}
        className="group/bar absolute top-1 flex h-6 cursor-grab items-center gap-1.5 rounded-md border border-[color-mix(in_oklab,var(--bar)_38%,transparent)] bg-[color-mix(in_oklab,var(--bar)_18%,var(--background))] px-2 text-xs font-medium text-foreground outline-none select-none hover:border-[color-mix(in_oklab,var(--bar)_65%,transparent)] focus-visible:ring-2 focus-visible:ring-primary/60 data-closed:opacity-50 data-dragging:cursor-grabbing data-dragging:shadow-lg data-dragging:transition-[left,width] data-dragging:duration-100 data-dragging:ease-out"
        style={{ '--bar': color, left, width } as CSSProperties}
        onPointerDown={(event) => onPointerDown(event, 'move')}
      >
        <Grip side="start" onPointerDown={(event) => onPointerDown(event, 'start')} />
        {/* the label follows the visible edge when the bar starts left of the view */}
        {narrow ? null : <span className="sticky left-[calc(var(--timeline-left)+8px)] flex min-w-0 items-center gap-1.5">{label}</span>}
        <Grip side="end" onPointerDown={(event) => onPointerDown(event, 'end')} />
      </div>
      {narrow ? (
        <span className="pointer-events-none absolute top-1 flex h-6 items-center gap-1.5 text-xs whitespace-nowrap text-foreground" style={{ left: left + width + GAP }}>
          {label}
        </span>
      ) : null}
    </>
  )
}
