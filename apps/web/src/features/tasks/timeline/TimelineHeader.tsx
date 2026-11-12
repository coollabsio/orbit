import { cn } from 'cn'
import { format } from 'date-fns'
import { Diamonds } from 'reicon-react'
import { Tip } from '@/components/common/Tip'
import { targetLabel } from '@/features/tasks/planningLib'
import { dayAt, monthMarks, tickMarks, xOf, type CycleBand, type MilestoneMark, type TimeRange } from './timelineLib'

/**
 * Sticky month row + day tick row with the pink "today" pill. A project's timeline adds a planning lane between
 * them: its cycles as bands and its milestones as diamonds on their target days.
 */
export function TimelineHeader({ range, pxPerDay, today, lane, milestones, cycles }: {
  range: TimeRange
  pxPerDay: number
  today: Date
  /** Shows the planning lane (20px) between the months and the days. */
  lane: boolean
  milestones: MilestoneMark[]
  cycles: CycleBand[]
}) {
  const todayX = xOf(range, today, pxPerDay) + pxPerDay / 2
  return (
    <div className={cn('relative shrink-0', lane ? 'h-[72px]' : 'h-[52px]')} style={{ width: range.days * pxPerDay }}>
      {monthMarks(range, pxPerDay).map((month) => (
        <div key={month.x} className="absolute top-0 h-7 border-l border-border/60 px-2 text-xs leading-7 font-medium text-foreground" style={{ left: month.x, width: month.width }}>
          <span className="sticky left-[calc(var(--timeline-left)+8px)] whitespace-nowrap">{month.label}</span>
        </div>
      ))}
      {cycles.map((cycle) => (
        <div
          key={cycle.id}
          data-cycle-band={cycle.id}
          data-current={cycle.current || undefined}
          className="absolute top-7 h-5 overflow-hidden border-l border-border/60 bg-foreground/[0.03] px-1.5 text-[0.6875rem] leading-5 text-muted-foreground data-current:border-primary/40 data-current:bg-primary/10 data-current:font-medium data-current:text-foreground"
          style={{ left: cycle.from * pxPerDay, width: (cycle.to - cycle.from) * pxPerDay }}
        >
          <span className="block truncate">{cycle.label}</span>
        </div>
      ))}
      {milestones.map((milestone) => (
        <Tip
          key={milestone.id}
          label={
            <span className="flex flex-col">
              <span className="font-medium">{milestone.name}</span>
              <span>Target {targetLabel(milestone.targetAt)} · {milestone.progress}</span>
            </span>
          }
        >
          <span
            data-milestone-mark={milestone.id}
            tabIndex={0}
            role="img"
            aria-label={`Milestone ${milestone.name}, target ${targetLabel(milestone.targetAt)}, ${milestone.progress}`}
            className="absolute top-7 z-[1] flex h-5 w-4 -translate-x-1/2 items-center justify-center rounded-sm text-foreground outline-none focus-visible:ring-1 focus-visible:ring-primary/70"
            style={{ left: milestone.day * pxPerDay + pxPerDay / 2 }}
          >
            <Diamonds aria-hidden className="size-3.5" />
          </span>
        </Tip>
      ))}
      {tickMarks(range, pxPerDay).map((tick) => (
        <span
          key={tick.x}
          // weekends step back so the working week reads first
          className={cn('absolute h-6 text-center text-[0.6875rem] leading-6 text-muted-foreground tabular-nums', lane ? 'top-12' : 'top-7', [0, 6].includes(dayAt(range, Math.round(tick.x / pxPerDay)).getDay()) && 'text-muted-foreground/50')}
          style={{ left: tick.x, width: pxPerDay }}
        >
          {tick.label}
        </span>
      ))}
      <span
        className={cn('absolute mt-0.5 -translate-x-1/2 rounded-full bg-primary px-1.5 text-[0.6875rem] leading-5 font-semibold text-primary-foreground tabular-nums', lane ? 'top-12' : 'top-7')}
        style={{ left: todayX }}
        aria-label={`Today, ${format(today, 'MMM d')}`}
      >
        {today.getDate()}
      </span>
    </div>
  )
}
