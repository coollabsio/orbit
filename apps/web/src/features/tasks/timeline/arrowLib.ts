import { differenceInCalendarDays } from 'date-fns'
import { xOf, type TimeRange } from './timelineLib'

/** A task row that is in the DOM: `top` is the row's offset in the rows layer, in px. */
export interface ArrowRow {
  taskId: string
  top: number
  span: { start: Date; end: Date }
}

export interface BlockArrow {
  key: string
  blockerId: string
  blockedId: string
  /** The line from the end of the blocker's bar to the start of the blocked task's bar. */
  path: string
  /** The arrowhead at the blocked task. */
  head: string
  /** The blocked task starts before the blocker ends: the plan cannot hold. */
  conflict: boolean
}

/** The stub that leaves the blocker and the one that enters the blocked task, in px. */
const STUB = 8
const HEAD = 5

/**
 * One arrow for each "blocks" relation whose two tasks have a bar among `rows` (the mounted rows: with row
 * virtualisation, an arrow shows only while both ends are in the DOM). `blocking` gives, for a task, the tasks it blocks.
 * A task in several groups has several rows; the first one counts.
 */
export function blockArrows(input: { rows: ArrowRow[]; blocking: (taskId: string) => string[]; range: TimeRange; pxPerDay: number; rowHeight: number }): BlockArrow[] {
  const { rows, blocking, range, pxPerDay, rowHeight } = input
  const rowOf = new Map<string, ArrowRow>()
  for (const row of rows) if (!rowOf.has(row.taskId)) rowOf.set(row.taskId, row)
  const arrows: BlockArrow[] = []
  for (const blocker of rowOf.values()) {
    for (const blockedId of blocking(blocker.taskId)) {
      const blocked = rowOf.get(blockedId)
      if (!blocked || blocked === blocker) continue
      // a bar covers its last day, so it ends at the start of the next day
      const x1 = xOf(range, blocker.span.end, pxPerDay) + pxPerDay
      const x2 = xOf(range, blocked.span.start, pxPerDay)
      const y1 = blocker.top + rowHeight / 2
      const y2 = blocked.top + rowHeight / 2
      // enough room between the bars: out, down (or up), in. If not, the line goes back between the two rows.
      const direct = x2 - x1 >= 2 * STUB
      const between = y1 + (y2 >= y1 ? rowHeight / 2 : -rowHeight / 2)
      const path = direct
        ? `M${x1} ${y1}H${x1 + STUB}V${y2}H${x2}`
        : `M${x1} ${y1}H${x1 + STUB}V${between}H${x2 - STUB}V${y2}H${x2}`
      arrows.push({
        key: `${blocker.taskId}>${blockedId}`,
        blockerId: blocker.taskId,
        blockedId,
        path,
        head: `M${x2} ${y2}l${-HEAD} ${-HEAD * 0.7}v${HEAD * 1.4}z`,
        conflict: differenceInCalendarDays(blocked.span.start, blocker.span.end) <= 0,
      })
    }
  }
  return arrows
}
