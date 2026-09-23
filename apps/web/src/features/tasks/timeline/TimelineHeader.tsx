import { format } from 'date-fns'
import { monthMarks, tickMarks, xOf, type TimeRange } from './timelineLib'

/** Sticky month row + day tick row with the pink "today" pill. */
export function TimelineHeader({ range, pxPerDay, today }: { range: TimeRange; pxPerDay: number; today: Date }) {
  const todayX = xOf(range, today, pxPerDay) + pxPerDay / 2
  return (
    <div className="relative h-[52px] shrink-0" style={{ width: range.days * pxPerDay }}>
      {monthMarks(range, pxPerDay).map((month) => (
        <div key={month.x} className="absolute top-0 h-7 border-l border-border/60 px-2 text-xs leading-7 font-medium text-foreground" style={{ left: month.x, width: month.width }}>
          <span className="sticky left-[calc(var(--timeline-left)+8px)] whitespace-nowrap">{month.label}</span>
        </div>
      ))}
      {tickMarks(range, pxPerDay).map((tick) => (
        <span key={tick.x} className="absolute top-7 h-6 text-center text-[11px] leading-6 text-muted-foreground tabular-nums" style={{ left: tick.x, width: pxPerDay }}>
          {tick.label}
        </span>
      ))}
      <span
        className="absolute top-7 mt-0.5 -translate-x-1/2 rounded-full bg-primary px-1.5 text-[11px] leading-5 font-semibold text-primary-foreground tabular-nums"
        style={{ left: todayX }}
        aria-label={`Today, ${format(today, 'MMM d')}`}
      >
        {today.getDate()}
      </span>
    </div>
  )
}
