import { useEffect, useImperativeHandle, useLayoutEffect, useRef, useState, type Ref } from 'react'
import { differenceInCalendarDays } from 'date-fns'
import { TaskSquare as SquareCheck } from 'reicon-react'
import { toast } from 'sonner'
import { EmptyState } from '@/components/common/EmptyState'
import { isTaskVersionConflict } from '@/features/tasks/api/conflicts'
import type { Project, Task, TaskStatusDef } from '@/features/tasks/api/models'
import { useUpdateTask } from '@/features/tasks/api/tasks'
import type { User } from '@/features/workspaces/models'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { TimelineBar } from './TimelineBar'
import { TimelineHeader } from './TimelineHeader'
import { TimelineRowLabel } from './TimelineRowLabel'
import { useTimelineDrag, type DragState } from './useTimelineDrag'
import {
  anchoredScrollLeft, applyDrag, buildTimelineRows, computeRange, dayAt, dayIndex, dayIndexAtX, drawRange, isOverdue, isSameEdit, monthMarks,
  rowDates, showsDailyTicks, showsWeeklyLines, spanLabel, taskSpan, xOf, zoomFromWheel, type DateEdit,
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
export function TaskTimeline({ tasks, projects, statuses, users, grouped, pxPerDay, onZoomChange, onOpen, today: todayProp, ref }: TaskTimelineProps & { ref?: Ref<TimelineHandle> }) {
  const [today] = useState(() => todayProp ?? new Date())
  const [overrides, setOverrides] = useState<Record<string, boolean>>({})
  const scrollRef = useRef<HTMLDivElement>(null)
  const trackRef = useRef<HTMLDivElement>(null)
  const [ghost, setGhost] = useState<{ taskId: string; day: number } | null>(null)
  const { workspace } = useWorkspace()
  const updateTask = useUpdateTask(workspace.id)
  const scrollKey = `orbit:timeline_scroll:${workspace.id}`
  const prevPx = useRef(pxPerDay)
  // last known scroll position; zoom anchoring must not read the live value, which the browser
  // has already lowered once a narrower track is laid out
  const scrollLeftRef = useRef(0)
  const pendingSaves = useRef(new Set<string>())
  const pendingAnchor = useRef<number | null>(null)
  const empty = tasks.length === 0

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
    setScrollLeft(scroller, Math.max(0, todayX - visible / 2))
  }

  const setScrollLeft = (scroller: HTMLElement, left: number) => {
    scroller.scrollLeft = left
    scrollLeftRef.current = left
  }
  useImperativeHandle(ref, () => ({ scrollToToday }))

  const save = (task: Task, edit: DateEdit) => {
    // one save per task at a time: the next edit needs the version the server returns
    if (isSameEdit(task, edit) || pendingSaves.current.has(task.id)) return
    pendingSaves.current.add(task.id)
    updateTask
      .mutateAsync({ taskId: task.id, body: { due_start_at: edit.dueStartAt, due_at: edit.dueAt, expected_version: task.version } })
      // useUpdateTask already rolls back and handles 409 with "Refresh task?"
      .catch((error: unknown) => { if (!isTaskVersionConflict(error)) toast.error('Could not save the new dates.') })
      .finally(() => pendingSaves.current.delete(task.id))
  }

  const editFor = (state: DragState, task: Task): DateEdit | null => {
    if (state.kind !== 'draw') return applyDrag(task, state.kind, state.deltaDays)
    const anchor = dayIndexAtX(state.originX, pxPerDay)
    return drawRange(dayAt(range, anchor), dayAt(range, anchor + state.deltaDays))
  }

  const { drag, begin, consumeClick } = useTimelineDrag({
    pxPerDay, trackRef, scrollRef,
    onCommit: (state) => {
      const task = tasks.find((item) => item.id === state.taskId)
      const edit = task ? editFor(state, task) : null
      if (task && edit) save(task, edit)
    },
  })

  // restore the last position (coming back from a task), else centre on today; again when
  // the scroller remounts after an empty result
  useLayoutEffect(() => {
    const scroller = scrollRef.current
    if (!scroller) return
    const saved = sessionStorage.getItem(scrollKey)
    if (saved) {
      // saved against its own range start, so a different range still shows the same dates
      const { start, left, top, px } = JSON.parse(saved) as { start: string; left: number; top: number; px: number }
      setScrollLeft(scroller, (left / px + dayIndex(range, new Date(start))) * pxPerDay)
      scroller.scrollTop = top
    } else scrollToToday()
    // later centring is explicit (Today button, `T`)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [empty])

  // keep the anchor date fixed when zoom changes (pointer for ctrl+wheel, centre for presets)
  useLayoutEffect(() => {
    const scroller = scrollRef.current
    const track = trackRef.current
    if (!scroller || !track || prevPx.current === pxPerDay) return
    const anchor = pendingAnchor.current ?? (scroller.clientWidth - track.offsetLeft) / 2
    setScrollLeft(scroller, anchoredScrollLeft(scrollLeftRef.current, anchor, prevPx.current, pxPerDay))
    pendingAnchor.current = null
    prevPx.current = pxPerDay
  }, [pxPerDay])

  // ctrl+wheel and trackpad pinch; React's onWheel is passive, so preventDefault needs a native listener
  useEffect(() => {
    const scroller = scrollRef.current
    if (!scroller) return
    const onWheel = (event: WheelEvent) => {
      if (!event.ctrlKey) return
      event.preventDefault()
      pendingAnchor.current = event.clientX - scroller.getBoundingClientRect().left - (trackRef.current?.offsetLeft ?? 0)
      onZoomChange(zoomFromWheel(prevPx.current, event.deltaY))
    }
    scroller.addEventListener('wheel', onWheel, { passive: false })
    return () => scroller.removeEventListener('wheel', onWheel)
  }, [onZoomChange, empty])

  // `T` jumps to today unless the user is typing
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key.toLowerCase() !== 't' || event.metaKey || event.ctrlKey || event.altKey) return
      const target = event.target as HTMLElement | null
      if (target?.closest('input, textarea, select, [contenteditable="true"]')) return
      scrollToToday()
    }
    document.addEventListener('keydown', onKey)
    return () => document.removeEventListener('keydown', onKey)
  })

  if (empty) {
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
    <div
      ref={scrollRef}
      data-timeline-scroller
      className="relative h-full overflow-auto overscroll-x-contain [--timeline-left:280px] max-[899px]:[--timeline-left:0px]"
      onScroll={(event) => {
        const { scrollLeft, scrollTop } = event.currentTarget
        scrollLeftRef.current = scrollLeft
        sessionStorage.setItem(scrollKey, JSON.stringify({ start: range.start.toISOString(), left: scrollLeft, top: scrollTop, px: pxPerDay }))
      }}
    >
      <div className="relative flex min-h-full flex-col" style={{ width: `calc(var(--timeline-left) + ${trackWidth}px)` }}>
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
          const undated = row.kind === 'task' && !row.span
          // live preview while dragging (also the drawn range on an undated row)
          const preview = row.kind === 'task' && drag?.taskId === row.task.id && (drag.moved || drag.kind === 'draw') ? editFor(drag, row.task) : null
          const span = row.kind === 'task' ? (preview ? taskSpan(preview) : row.span) : null
          return (
            <div key={row.key} className={`relative flex h-8 ${row.kind === 'group' ? 'bg-muted/30' : ''}`}>
              <TimelineRowLabel row={row} status={status} assignee={assignee} onToggle={(key, open) => setOverrides((prev) => ({ ...prev, [key]: open }))} onOpen={onOpen} />
              <div
                className={`relative shrink-0 ${undated ? 'cursor-crosshair' : ''}`}
                style={{ width: trackWidth }}
                data-row-track={row.kind === 'task' ? row.task.id : undefined}
                onPointerDown={undated ? (event) => begin(event, row.task.id, 'draw') : undefined}
                onPointerMove={undated ? (event) => {
                  if (event.pointerType === 'touch' || !trackRef.current) return
                  const day = dayIndexAtX(event.clientX - trackRef.current.getBoundingClientRect().left, pxPerDay)
                  if (ghost?.taskId !== row.task.id || ghost.day !== day) setGhost({ taskId: row.task.id, day })
                } : undefined}
                onPointerLeave={undated ? () => setGhost(null) : undefined}
              >
                {row.kind === 'group' && row.span ? (
                  <div
                    className="absolute top-1/2 h-1.5 -translate-y-1/2 rounded-full opacity-70"
                    style={{ left: xOf(range, row.span.start, pxPerDay), width: (differenceInCalendarDays(row.span.end, row.span.start) + 1) * pxPerDay, background: row.project.color }}
                  />
                ) : null}
                {undated && ghost?.taskId === row.task.id && !drag ? (
                  <div aria-hidden="true" className="pointer-events-none absolute top-1 h-6 rounded-md border border-dashed border-primary/50 bg-primary/10" style={{ left: ghost.day * pxPerDay, width: pxPerDay }} />
                ) : null}
                {preview && span ? (
                  <span className="pointer-events-none absolute -top-5 z-30 rounded bg-popover px-1.5 text-[11px] leading-5 whitespace-nowrap text-popover-foreground shadow-md tabular-nums" style={{ left: xOf(range, span.start, pxPerDay) }}>
                    {spanLabel(span)}
                  </span>
                ) : null}
                {row.kind === 'task' && span ? (
                  <TimelineBar
                    task={row.task}
                    span={span}
                    range={range}
                    pxPerDay={pxPerDay}
                    color={projectById.get(row.task.projectId)?.color ?? 'var(--muted-foreground)'}
                    status={status}
                    assignee={assignee}
                    overdueDays={!preview && isOverdue(row.task, status?.category, today) ? dayIndex(range, today) - dayIndex(range, span.end) : 0}
                    dragging={preview !== null}
                    onPointerDown={(event, mode) => begin(event, row.task.id, mode)}
                    onClick={() => { if (!consumeClick()) onOpen(row.task.id) }}
                    onKeyDown={(event) => {
                      if (event.key === 'Enter') { onOpen(row.task.id); return }
                      if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return
                      event.preventDefault()
                      if (event.repeat) return
                      const step = event.key === 'ArrowRight' ? 1 : -1
                      const edit = applyDrag(row.task, event.shiftKey ? 'end' : 'move', step)
                      if (edit) save(row.task, edit)
                    }}
                  />
                ) : null}
              </div>
            </div>
          )
        })}
        {/* keeps the left pane solid below the last row, so the grid never shows through it */}
        <div className="flex flex-1">
          <div className="sticky left-0 z-10 w-[280px] shrink-0 border-r border-border bg-background max-[899px]:hidden" />
        </div>
      </div>
    </div>
  )
}
