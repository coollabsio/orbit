import { useState, type DragEvent } from 'react'
import { cn } from 'cn'
import { format, isSameDay, isSameMonth } from 'date-fns'
import { ArrowLeft2, ArrowRight2 } from 'reicon-react'
import { toast } from 'sonner'
import { ColorDot } from '@/components/common/ColorDot'
import { Button } from '@/components/ui/button'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { isTaskVersionConflict } from '@/features/tasks/api/conflicts'
import type { Project, Task, TaskStatusDef } from '@/features/tasks/api/models'
import { useUpdateTask } from '@/features/tasks/api/tasks'
import { isClosedCategory } from '@/features/tasks/taskMeta'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import {
  calendarTitle, calendarWeeks, columnAt, dayTasks, dueAtFor, hiddenCount, localeWeekStart, moveToDay, shiftCursor, weekSegments,
  type CalendarMode, type WeekSegment,
} from './calendarLib'

/** Lanes with room in a day cell; the other tasks of the day are behind "+N more". */
const MAX_LANES: Record<CalendarMode, number> = { month: 3, week: 14 }
const LANE_HEIGHT = 24

export interface TaskCalendarProps {
  tasks: Task[]
  projects: Project[]
  statuses: TaskStatusDef[]
  mode: CalendarMode
  onModeChange: (mode: CalendarMode) => void
  onOpen: (taskId: string) => void
  /** A click on an empty day: opens the new-task dialog with that due date. */
  onCreate: (dueAt: string) => void
  /** Injected for tests; defaults to now and to the locale of the browser. */
  today?: Date
  weekStart?: ReturnType<typeof localeWeekStart>
}

/**
 * Month and week grids of the tasks by due date. A due date shows on its day and a date range as a bar across its
 * days; a task with no due date is not here (the list and the board show it). A drag to a different day moves the
 * dates. The dragged bar stays mounted: the browser ends a drag when its source leaves the DOM.
 */
export function TaskCalendar({ tasks, projects, statuses, mode, onModeChange, onOpen, onCreate, today: todayProp, weekStart: weekStartProp }: TaskCalendarProps) {
  const [today] = useState(() => todayProp ?? new Date())
  const [weekStart] = useState(() => weekStartProp ?? localeWeekStart())
  const [cursor, setCursor] = useState(today)
  const [drag, setDrag] = useState<{ taskId: string; grabbed: Date } | null>(null)
  const [overDay, setOverDay] = useState<number | null>(null)
  const { workspace } = useWorkspace()
  const updateTask = useUpdateTask(workspace.id)
  const weeks = calendarWeeks(cursor, mode, weekStart)
  const projectById = new Map(projects.map((project) => [project.id, project]))
  const statusById = new Map(statuses.map((status) => [status.id, status]))
  const maxLanes = MAX_LANES[mode]

  const endDrag = () => {
    setDrag(null)
    setOverDay(null)
  }
  const drop = (day: Date) => {
    const task = tasks.find((item) => item.id === drag?.taskId)
    const edit = task && drag ? moveToDay(task, drag.grabbed, day) : null
    endDrag()
    if (!task || !edit) return
    updateTask
      .mutateAsync({ taskId: task.id, body: { due_start_at: edit.dueStartAt, due_at: edit.dueAt, expected_version: task.version } })
      // useUpdateTask already rolls back and handles 409 with "Refresh task?"
      .catch((error: unknown) => { if (!isTaskVersionConflict(error)) toast.error('Could not save the new dates.') })
  }
  /** The day of the week row under the pointer; `fallback` is the column of the element when there is no layout. */
  const dayUnder = (event: DragEvent<HTMLElement>, week: Date[], fallback: number) => {
    const row = event.currentTarget.closest('[data-calendar-week]')
    return week[row ? columnAt(event.clientX, row.getBoundingClientRect(), fallback) : fallback]!
  }

  return (
    <div data-calendar={mode} className="flex h-full min-h-0 flex-col">
      <div className="flex shrink-0 items-center gap-2 border-b px-3 py-2">
        <Button variant="outline" size="sm" onClick={() => setCursor(today)}>Today</Button>
        <Button variant="ghost" size="icon-sm" aria-label={mode === 'month' ? 'Previous month' : 'Previous week'} onClick={() => setCursor(shiftCursor(cursor, mode, -1))}><ArrowLeft2 className="size-4" /></Button>
        <Button variant="ghost" size="icon-sm" aria-label={mode === 'month' ? 'Next month' : 'Next week'} onClick={() => setCursor(shiftCursor(cursor, mode, 1))}><ArrowRight2 className="size-4" /></Button>
        <h2 aria-live="polite" className="min-w-0 flex-1 truncate text-sm font-semibold">{calendarTitle(cursor, mode, weekStart)}</h2>
        <ToggleGroup
          aria-label="Calendar range"
          variant="outline"
          size="sm"
          value={[mode]}
          onValueChange={(value: string[]) => {
            const next = value[0]
            if (next === 'month' || next === 'week') onModeChange(next)
          }}
        >
          <ToggleGroupItem value="month" className="font-normal text-muted-foreground aria-pressed:text-foreground">Month</ToggleGroupItem>
          <ToggleGroupItem value="week" className="font-normal text-muted-foreground aria-pressed:text-foreground">Week</ToggleGroupItem>
        </ToggleGroup>
      </div>
      <div className="grid shrink-0 grid-cols-7 border-b text-[11px] font-medium text-muted-foreground" aria-hidden="true">
        {weeks[0]!.map((day) => <div key={day.getDay()} className="truncate px-2 py-1">{format(day, 'EEE')}</div>)}
      </div>
      <div role="grid" aria-label={calendarTitle(cursor, mode, weekStart)} className="flex min-h-0 flex-1 flex-col overflow-y-auto">
        {weeks.map((week) => {
          const segments = weekSegments(tasks, week)
          return (
            <div key={week[0]!.toISOString()} role="row" data-calendar-week className={cn('relative grid min-h-28 shrink-0 grid-cols-7 border-b', mode === 'week' ? 'flex-1' : 'flex-1 basis-0')}>
              {week.map((day, col) => {
                const hidden = hiddenCount(segments, col, maxLanes)
                const outside = mode === 'month' && !isSameMonth(day, cursor)
                return (
                  <div
                    key={col}
                    role="gridcell"
                    data-day={format(day, 'yyyy-MM-dd')}
                    data-drop-target={(drag && overDay === day.getTime()) || undefined}
                    className={cn('relative flex min-w-0 flex-col border-r last:border-r-0 data-[drop-target]:bg-primary/10', outside && 'bg-muted/30')}
                    onDragOver={(event) => {
                      if (!drag) return
                      event.preventDefault()
                      event.dataTransfer.dropEffect = 'move'
                      if (overDay !== day.getTime()) setOverDay(day.getTime())
                    }}
                    onDrop={(event) => {
                      event.preventDefault()
                      drop(day)
                    }}
                  >
                    {/* the whole cell behind the bars: a click on an empty day makes a task due that day */}
                    <button
                      type="button"
                      aria-label={`New task due ${format(day, 'EEEE, MMMM d')}`}
                      className="absolute inset-0 cursor-default outline-none hover:bg-foreground/[0.02] focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset"
                      onClick={() => onCreate(dueAtFor(day))}
                    />
                    <span className={cn('pointer-events-none relative mx-1.5 mt-1 inline-flex size-5 items-center justify-center self-start rounded-full text-[11px] tabular-nums', outside ? 'text-muted-foreground/60' : 'text-muted-foreground', isSameDay(day, today) && 'bg-primary font-semibold text-primary-foreground')}>
                      {day.getDate()}
                    </span>
                    {hidden > 0 ? (
                      <Popover>
                        <PopoverTrigger
                          render={
                            <Button variant="ghost" size="xs" className="relative mx-1 mt-auto mb-0.5 h-5 justify-start px-1 text-[11px] font-normal text-muted-foreground" style={{ marginTop: maxLanes * LANE_HEIGHT + 2 }}>
                              +{hidden} more
                            </Button>
                          }
                        />
                        <PopoverContent align="start" className="w-64 gap-px p-1">
                          <div className="px-2 py-1 text-[11px] font-medium text-muted-foreground">{format(day, 'EEEE, MMMM d')}</div>
                          {dayTasks(segments, col).map((task) => (
                            <Button key={task.id} variant="ghost" className="w-full justify-start gap-2 font-normal" onClick={() => onOpen(task.id)}>
                              <ColorDot color={projectById.get(task.projectId)?.color} className="size-2" />
                              <span className="min-w-0 flex-1 truncate text-left">{task.title}</span>
                              <span className="shrink-0 text-[11px] text-muted-foreground tabular-nums">{task.identifier}</span>
                            </Button>
                          ))}
                        </PopoverContent>
                      </Popover>
                    ) : null}
                  </div>
                )
              })}
              {/* the bars, over the day cells: only the bars take the pointer, so an empty part of a day stays a click target */}
              <div className="pointer-events-none absolute inset-x-0 top-7 grid grid-cols-7" style={{ gridAutoRows: LANE_HEIGHT }}>
                {segments.filter((segment) => segment.lane < maxLanes).map((segment) => (
                  <CalendarBar
                    key={segment.task.id}
                    segment={segment}
                    color={projectById.get(segment.task.projectId)?.color}
                    closed={isClosedCategory(statusById.get(segment.task.statusId)?.category)}
                    dragging={drag?.taskId === segment.task.id}
                    onOpen={() => onOpen(segment.task.id)}
                    onDragStart={(event) => {
                      setDrag({ taskId: segment.task.id, grabbed: dayUnder(event, week, segment.startCol) })
                      event.dataTransfer.effectAllowed = 'move'
                      event.dataTransfer.setData('text/plain', segment.task.id)
                    }}
                    // a bar of a different task covers the day below it: the drop goes to the day under the pointer
                    onDragOver={(event) => {
                      if (!drag) return
                      event.preventDefault()
                      const day = dayUnder(event, week, segment.startCol).getTime()
                      if (overDay !== day) setOverDay(day)
                    }}
                    onDrop={(event) => {
                      event.preventDefault()
                      drop(dayUnder(event, week, segment.startCol))
                    }}
                    onDragEnd={endDrag}
                  />
                ))}
              </div>
            </div>
          )
        })}
      </div>
    </div>
  )
}

function CalendarBar({ segment, color, closed, dragging, onOpen, ...drag }: {
  segment: WeekSegment
  color: string | undefined
  closed: boolean
  dragging: boolean
  onOpen: () => void
  onDragStart: (event: DragEvent<HTMLButtonElement>) => void
  onDragOver: (event: DragEvent<HTMLButtonElement>) => void
  onDrop: (event: DragEvent<HTMLButtonElement>) => void
  onDragEnd: () => void
}) {
  const { task, startCol, endCol, lane, continuesBefore, continuesAfter } = segment
  return (
    <button
      type="button"
      draggable
      // a right-click opens the task menu (TaskContextMenu)
      data-task-menu={task.id}
      data-calendar-bar={task.id}
      data-dragging={dragging || undefined}
      title={`${task.identifier} ${task.title}`}
      style={{ gridColumn: `${startCol + 1} / ${endCol + 2}`, gridRow: lane + 1 }}
      className={cn(
        'pointer-events-auto mx-1 my-px flex min-w-0 items-center gap-1.5 rounded-md border bg-card px-1.5 text-left text-xs outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring data-[dragging]:opacity-40',
        continuesBefore && 'ml-0 rounded-l-none border-l-0',
        continuesAfter && 'mr-0 rounded-r-none border-r-0',
        closed && 'text-muted-foreground line-through',
      )}
      onClick={onOpen}
      {...drag}
    >
      <ColorDot color={color} className="size-2" />
      <span className="min-w-0 flex-1 truncate">{task.title}</span>
    </button>
  )
}
