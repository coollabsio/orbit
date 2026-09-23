import { useImperativeHandle, useLayoutEffect, useRef, useState, type Ref } from 'react'
import { differenceInCalendarDays } from 'date-fns'
import { TaskSquare as SquareCheck } from 'reicon-react'
import { EmptyState } from '@/components/common/EmptyState'
import type { Project, Task, TaskStatusDef } from '@/features/tasks/api/models'
import type { User } from '@/features/workspaces/models'
import { TimelineBar } from './TimelineBar'
import { TimelineHeader } from './TimelineHeader'
import { TimelineRowLabel } from './TimelineRowLabel'
import {
  buildTimelineRows, computeRange, dayAt, dayIndex, isOverdue, monthMarks, rowDates, showsDailyTicks, showsWeeklyLines, xOf,
} from './timelineLib'

export const LEFT_PANE = 280
export const ROW_HEIGHT = 32
export const HEADER_HEIGHT = 52

export interface TimelineHandle {
  scrollToToday: () => void
}

export interface TaskTimelineProps {
  tasks: Task[]
  projects: Project[]
  statuses: TaskStatusDef[]
  users: User[]
  grouped: boolean
  pxPerDay: number
  onZoomChange: (px: number) => void
  onOpen: (taskId: string) => void
  /** Injected for tests; defaults to now. */
  today?: Date
}

/** Roadmap timeline: one row per task, bars span due_start_at → due_at. */
export function TaskTimeline({ tasks, projects, statuses, users, grouped, pxPerDay, onOpen, today: todayProp, ref }: TaskTimelineProps & { ref?: Ref<TimelineHandle> }) {
  const [today] = useState(() => todayProp ?? new Date())
  const [overrides, setOverrides] = useState<Record<string, boolean>>({})
  const scrollRef = useRef<HTMLDivElement>(null)
  const trackRef = useRef<HTMLDivElement>(null)

  const range = computeRange(rowDates(tasks), today)
  const rows = buildTimelineRows({ tasks, projects, statuses, grouped, overrides })
  const statusById = new Map(statuses.map((status) => [status.id, status]))
  const projectById = new Map(projects.map((project) => [project.id, project]))
  const userById = new Map(users.map((user) => [user.id, user]))
  const trackWidth = range.days * pxPerDay
  const todayX = xOf(range, today, pxPerDay) + pxPerDay / 2

  const scrollToToday = () => {
    const scroller = scrollRef.current
    const track = trackRef.current
    if (!scroller || !track) return
    const visible = scroller.clientWidth - track.offsetLeft
    scroller.scrollLeft = Math.max(0, todayX - visible / 2)
  }
  useImperativeHandle(ref, () => ({ scrollToToday }))

  // open centred on today
  useLayoutEffect(() => {
    scrollToToday()
    // mount only; later centring is explicit (Today button, `T`)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  if (tasks.length === 0) {
    return <EmptyState icon={SquareCheck} title="No tasks match these filters" description="Change the filters or add a task." />
  }

  // Monday lines and weekend shading are repeating gradients: no DOM per day
  const firstMonday = (8 - dayAt(range, 0).getDay()) % 7
  const weekLines = showsWeeklyLines(pxPerDay)
    ? `repeating-linear-gradient(to right, var(--border) 0 1px, transparent 1px ${7 * pxPerDay}px)`
    : null
  const weekends = showsDailyTicks(pxPerDay)
    ? `repeating-linear-gradient(to right, color-mix(in oklch, var(--muted) 55%, transparent) 0 ${2 * pxPerDay}px, transparent ${2 * pxPerDay}px ${7 * pxPerDay}px)`
    : null
  const saturdayOffset = ((firstMonday + 5) % 7) * pxPerDay
  const background = [weekLines, weekends].filter(Boolean).join(', ')
  const backgroundPosition = [weekLines ? `${firstMonday * pxPerDay}px 0` : null, weekends ? `${saturdayOffset}px 0` : null].filter(Boolean).join(', ')

  return (
    <div ref={scrollRef} data-timeline-scroller className="relative h-full overflow-auto overscroll-x-contain [--timeline-left:280px] max-[899px]:[--timeline-left:0px]">
      <div className="relative min-h-full" style={{ width: `calc(var(--timeline-left) + ${trackWidth}px)` }}>
        {/* grid layer behind the rows; its left edge is the track origin for pointer maths */}
        <div
          ref={trackRef}
          data-timeline-track
          aria-hidden="true"
          className="pointer-events-none absolute inset-y-0 left-[var(--timeline-left)]"
          style={{ width: trackWidth, backgroundImage: background || undefined, backgroundPosition: backgroundPosition || undefined }}
        >
          {monthMarks(range, pxPerDay).map((month) => <div key={month.x} className="absolute inset-y-0 w-px bg-border" style={{ left: month.x }} />)}
          <div data-today-line className="absolute inset-y-0 z-[5] w-px bg-primary" style={{ left: todayX }} />
        </div>

        <div className="sticky top-0 z-20 flex border-b border-border bg-background">
          <div className="sticky left-0 z-10 w-[280px] shrink-0 border-r border-border bg-background max-[899px]:hidden" />
          <TimelineHeader range={range} pxPerDay={pxPerDay} today={today} />
        </div>

        {rows.map((row) => {
          const status = row.kind === 'task' ? statusById.get(row.task.statusId) : undefined
          const assignee = row.kind === 'task' ? userById.get(row.task.assigneeIds[0] ?? '') : undefined
          return (
            <div key={row.key} className={`relative flex h-8 ${row.kind === 'group' ? 'bg-muted/30' : ''}`}>
              <TimelineRowLabel row={row} status={status} assignee={assignee} onToggle={(key, open) => setOverrides((prev) => ({ ...prev, [key]: open }))} onOpen={onOpen} />
              <div className="relative shrink-0" style={{ width: trackWidth }} data-row-track={row.kind === 'task' ? row.task.id : undefined}>
                {row.kind === 'group' && row.span ? (
                  <div
                    className="absolute top-1/2 h-1.5 -translate-y-1/2 rounded-full opacity-70"
                    style={{ left: xOf(range, row.span.start, pxPerDay), width: (differenceInCalendarDays(row.span.end, row.span.start) + 1) * pxPerDay, background: row.project.color }}
                  />
                ) : null}
                {row.kind === 'task' && row.span ? (
                  <TimelineBar
                    task={row.task}
                    span={row.span}
                    range={range}
                    pxPerDay={pxPerDay}
                    color={projectById.get(row.task.projectId)?.color ?? 'var(--muted-foreground)'}
                    status={status}
                    assignee={assignee}
                    overdueDays={isOverdue(row.task, status?.category, today) ? dayIndex(range, today) - dayIndex(range, row.span.end) : 0}
                    dragging={false}
                    onPointerDown={() => {}}
                    onClick={() => onOpen(row.task.id)}
                    onKeyDown={(event) => { if (event.key === 'Enter') onOpen(row.task.id) }}
                  />
                ) : null}
              </div>
            </div>
          )
        })}
      </div>
    </div>
  )
}
