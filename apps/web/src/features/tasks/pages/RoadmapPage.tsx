import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { Link, useNavigate } from 'react-router'
import { toast } from 'sonner'
import { Roadmap } from 'reicon-react'
import { ColorDot } from '@/components/common/ColorDot'
import { EmptyState } from '@/components/common/EmptyState'
import { Pane, PaneHeader, PaneTitle } from '@/components/common/Pane'
import { useMilestones, useUpdateMilestone, type Milestone } from '@/features/tasks/api/milestones'
import { useProjects } from '@/features/tasks/api/projects'
import { HealthDot, MilestoneProgress } from '@/features/tasks/components/MilestoneBits'
import { RoadmapBar } from '@/features/tasks/roadmap/RoadmapBar'
import { applyMilestoneDrag, buildRoadmapRows, milestoneDatesLabel, milestoneSpan, roadmapDates, sameDates } from '@/features/tasks/roadmap/roadmapLib'
import { TimelineControls } from '@/features/tasks/timeline/TimelineControls'
import { TimelineHeader } from '@/features/tasks/timeline/TimelineHeader'
import { anchoredScrollLeft, computeRange, dayAt, monthMarks, showsDailyTicks, showsWeeklyLines, xOf, zoomFromWheel } from '@/features/tasks/timeline/timelineLib'
import { useTimelineDrag, type DragState } from '@/features/tasks/timeline/useTimelineDrag'
import { useTimelineZoom } from '@/features/tasks/timeline/useTimelineZoom'
import { useWorkspace } from '@/features/workspaces/workspaceContext'

type Dates = Pick<Milestone, 'start_at' | 'target_at'>

/** The milestones of every project on one timeline: a bar from start to target, grouped by project. */
export function RoadmapPage() {
  const navigate = useNavigate()
  const { workspace } = useWorkspace()
  const projects = useProjects(workspace.id).data ?? []
  const milestonesQuery = useMilestones(workspace.id)
  const milestones = milestonesQuery.data ?? []
  const updateMilestone = useUpdateMilestone(workspace.id)
  const [pxPerDay, setPxPerDay] = useTimelineZoom()
  const [today] = useState(() => new Date())
  const scrollRef = useRef<HTMLDivElement>(null)
  const trackRef = useRef<HTMLDivElement>(null)
  // a dropped edit stays on screen until its save settles, so the bar never flashes back
  const [committed, setCommitted] = useState<{ id: string; dates: Dates } | null>(null)
  const pendingSaves = useRef(new Set<string>())

  const range = computeRange(roadmapDates(milestones), today)
  const rows = buildRoadmapRows(projects, milestones)
  const trackWidth = range.days * pxPerDay
  const todayX = xOf(range, today, pxPerDay) + pxPerDay / 2
  const empty = rows.length === 0

  const save = (milestone: Milestone, dates: Dates) => {
    // one save for each milestone at a time: the next one needs the version the server returns
    if (sameDates(milestone, dates) || pendingSaves.current.has(milestone.id)) return
    pendingSaves.current.add(milestone.id)
    setCommitted({ id: milestone.id, dates })
    updateMilestone
      .mutateAsync({ milestone, body: { expected_version: milestone.version, ...dates } })
      .catch(() => toast.error('Could not save the new dates.'))
      .finally(() => {
        pendingSaves.current.delete(milestone.id)
        setCommitted((current) => (current?.id === milestone.id ? null : current))
      })
  }

  const { drag, begin, consumeClick } = useTimelineDrag({
    pxPerDay,
    trackRef,
    scrollRef,
    onCommit: (state: DragState) => {
      const milestone = milestones.find((item) => item.id === state.taskId)
      if (!milestone || state.kind === 'draw') return
      const dates = applyMilestoneDrag(milestone, state.kind, state.deltaDays)
      if (dates) save(milestone, dates)
    },
  })

  const scrollToToday = () => {
    const scroller = scrollRef.current
    const track = trackRef.current
    if (!scroller || !track) return
    scroller.scrollLeft = Math.max(0, todayX - (scroller.clientWidth - track.offsetLeft) / 2)
  }

  // open on today; the range start changes when the first milestones load
  const rangeStart = range.start.getTime()
  useLayoutEffect(() => {
    if (!empty) scrollToToday()
    // eslint-disable-next-line react-hooks/exhaustive-deps -- only when the roadmap first has rows or its range moves
  }, [empty, rangeStart])

  // a zoom keeps the day in the middle of the view in place
  const prevPx = useRef(pxPerDay)
  useLayoutEffect(() => {
    const scroller = scrollRef.current
    const track = trackRef.current
    if (scroller && track && prevPx.current !== pxPerDay) {
      const anchor = (scroller.clientWidth - track.offsetLeft) / 2
      scroller.scrollLeft = anchoredScrollLeft(scroller.scrollLeft, anchor, prevPx.current, pxPerDay)
    }
    prevPx.current = pxPerDay
  }, [pxPerDay])

  // ctrl+wheel and trackpad pinch zoom; a non-passive listener, so the page itself does not zoom
  useEffect(() => {
    const scroller = scrollRef.current
    if (!scroller) return
    const onWheel = (event: WheelEvent) => {
      if (!event.ctrlKey && !event.metaKey) return
      event.preventDefault()
      setPxPerDay(zoomFromWheel(prevPx.current, event.deltaY))
    }
    scroller.addEventListener('wheel', onWheel, { passive: false })
    return () => scroller.removeEventListener('wheel', onWheel)
  }, [setPxPerDay, empty])

  // Monday lines and weekend shading are repeating gradients: no DOM for each day
  const firstMonday = (8 - dayAt(range, 0).getDay()) % 7
  const weekLines = showsWeeklyLines(pxPerDay)
    ? `repeating-linear-gradient(to right, color-mix(in oklch, var(--border) 55%, transparent) 0 1px, transparent 1px ${7 * pxPerDay}px)`
    : null
  const weekends = showsDailyTicks(pxPerDay)
    ? `repeating-linear-gradient(to right, color-mix(in oklch, var(--foreground) 2.5%, transparent) 0 ${2 * pxPerDay}px, transparent ${2 * pxPerDay}px ${7 * pxPerDay}px)`
    : null
  const background = [weekLines, weekends].filter(Boolean).join(', ')
  const backgroundPosition = [weekLines ? `${firstMonday * pxPerDay}px 0` : null, weekends ? `${((firstMonday + 5) % 7) * pxPerDay}px 0` : null].filter(Boolean).join(', ')

  const openMilestone = (milestone: Milestone) => navigate(`/tasks/projects/${milestone.project_id}/milestones/${milestone.id}`)

  return (
    <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden bg-background">
      <Pane>
        <PaneHeader>
          <PaneTitle render={<h1 />}>Roadmap</PaneTitle>
          {empty ? null : (
            <div className="ml-auto flex items-center gap-2">
              <TimelineControls pxPerDay={pxPerDay} onZoomChange={setPxPerDay} onToday={scrollToToday} />
            </div>
          )}
        </PaneHeader>
        {milestonesQuery.isPending ? null : empty ? (
          <div className="min-h-0 flex-1 overflow-y-auto">
            <EmptyState icon={Roadmap} title="No milestones" description="Add a milestone to a project to see it on the roadmap." />
          </div>
        ) : (
          <div ref={scrollRef} data-timeline-scroller className="relative min-h-0 flex-1 overflow-auto overscroll-x-contain [--timeline-left:280px] max-[899px]:[--timeline-left:0px]">
            <div className="relative flex min-h-full flex-col" style={{ width: `calc(var(--timeline-left) + ${trackWidth}px)` }}>
              {/* grid layer behind the rows; its left edge is the track origin for pointer maths */}
              <div
                ref={trackRef}
                aria-hidden="true"
                className="pointer-events-none absolute inset-y-0 left-[var(--timeline-left)]"
                style={{ width: trackWidth, backgroundImage: background || undefined, backgroundPosition: backgroundPosition || undefined }}
              >
                {monthMarks(range, pxPerDay).map((month) => <div key={month.x} className="absolute inset-y-0 w-px bg-border" style={{ left: month.x }} />)}
                <div className="absolute inset-y-0 z-[5] w-px bg-primary/70" style={{ left: todayX }} />
              </div>

              <div className="sticky top-0 z-20 flex border-b bg-background">
                <div className="sticky left-0 z-10 w-[280px] shrink-0 border-r bg-background max-[899px]:hidden" />
                <TimelineHeader range={range} pxPerDay={pxPerDay} today={today} />
              </div>

              {rows.map((row) => {
                if (row.kind === 'project') {
                  return (
                    <div key={row.key} className="flex h-8 bg-[color-mix(in_oklch,var(--muted)_45%,var(--background))]">
                      <div className="sticky left-0 z-10 flex w-[280px] shrink-0 items-center gap-2 border-r bg-[color-mix(in_oklch,var(--muted)_45%,var(--background))] px-3 text-xs font-medium max-[899px]:hidden">
                        <ColorDot color={row.project.color} />
                        <Link to={`/tasks/projects/${row.project.id}`} className="truncate outline-none hover:underline focus-visible:underline">{row.project.name}</Link>
                        <span className="ml-auto text-muted-foreground tabular-nums">{row.count}</span>
                      </div>
                      <div className="shrink-0" style={{ width: trackWidth }} />
                    </div>
                  )
                }
                if (row.kind === 'undated') {
                  return (
                    <div key={row.key} className="flex min-h-8">
                      <div className="sticky left-0 z-10 flex w-[280px] shrink-0 items-center border-r bg-background px-3 pl-7 text-xs text-muted-foreground max-[899px]:hidden">No dates</div>
                      {/* the names stay in view at any scroll position: they have no place on the time axis */}
                      <ul aria-label={`${row.project.name}: milestones with no dates`} className="sticky left-[var(--timeline-left)] flex flex-wrap items-center gap-x-4 gap-y-1 px-3 py-1.5 text-xs">
                        {row.milestones.map((milestone) => (
                          <li key={milestone.id} className="flex items-center gap-1.5">
                            <HealthDot health={milestone.health} />
                            <Link to={`/tasks/projects/${milestone.project_id}/milestones/${milestone.id}`} className="outline-none hover:underline focus-visible:underline">{milestone.name}</Link>
                          </li>
                        ))}
                      </ul>
                    </div>
                  )
                }
                const { milestone } = row
                // live preview while dragging, then the saving edit
                const live = drag?.taskId === milestone.id && drag.moved && drag.kind !== 'draw' ? applyMilestoneDrag(milestone, drag.kind, drag.deltaDays) : null
                const preview = live ?? (committed?.id === milestone.id ? committed.dates : null)
                const span = preview ? milestoneSpan(preview) ?? row.span : row.span
                return (
                  <div key={row.key} className="group/row flex h-8 transition-colors duration-150 ease-out hover:bg-foreground/[0.03]">
                    <div className="sticky left-0 z-10 flex w-[280px] shrink-0 items-center gap-2 border-r bg-background px-3 pl-7 text-[13px] max-[899px]:hidden">
                      <Link to={`/tasks/projects/${milestone.project_id}/milestones/${milestone.id}`} className="min-w-0 flex-1 truncate outline-none hover:underline focus-visible:underline">{milestone.name}</Link>
                      <MilestoneProgress milestone={milestone} className="border-0 px-0" />
                    </div>
                    <div className="relative shrink-0" style={{ width: trackWidth }}>
                      {live ? (
                        <span className="pointer-events-none absolute -top-5 z-30 rounded bg-popover px-1.5 text-[11px] leading-5 whitespace-nowrap text-popover-foreground shadow-md tabular-nums" style={{ left: xOf(range, span.start, pxPerDay) }}>
                          {milestoneDatesLabel(live)}
                        </span>
                      ) : null}
                      <RoadmapBar
                        milestone={milestone}
                        span={span}
                        range={range}
                        pxPerDay={pxPerDay}
                        color={row.project.color}
                        dragging={live !== null}
                        onPointerDown={(event, mode) => begin(event, milestone.id, mode)}
                        onClick={() => { if (!consumeClick()) openMilestone(milestone) }}
                        onKeyDown={(event) => {
                          if (event.key === 'Enter') { openMilestone(milestone); return }
                          if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return
                          event.preventDefault()
                          if (event.repeat) return
                          const dates = applyMilestoneDrag(milestone, event.shiftKey ? 'end' : 'move', event.key === 'ArrowRight' ? 1 : -1)
                          if (dates) save(milestone, dates)
                        }}
                      />
                    </div>
                  </div>
                )
              })}
              {/* keeps the left pane solid below the last row, so the grid never shows through it */}
              <div className="flex flex-1">
                <div className="sticky left-0 z-10 w-[280px] shrink-0 border-r bg-background max-[899px]:hidden" />
              </div>
            </div>
          </div>
        )}
      </Pane>
    </div>
  )
}
