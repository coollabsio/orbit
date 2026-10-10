import { expect, test } from 'bun:test'
import { blockArrows, type ArrowRow } from './arrowLib'
import type { TimeRange } from './timelineLib'

const day = (value: string) => new Date(`${value}T00:00:00`)
const range: TimeRange = { start: day('2026-10-01'), days: 60 }
const row = (taskId: string, index: number, start: string, end: string): ArrowRow => ({ taskId, top: index * 32, span: { start: day(start), end: day(end) } })
const arrows = (rows: ArrowRow[], blocks: Record<string, string[]>) => blockArrows({ rows, blocking: (id) => blocks[id] ?? [], range, pxPerDay: 10, rowHeight: 32 })

test('an arrow goes from the end of the blocker to the start of the blocked task', () => {
  // blocker: Oct 2 to Oct 4 (ends at x = 40); blocked: starts Oct 8 (x = 70), two rows down
  const [arrow, ...rest] = arrows([row('a', 0, '2026-10-02', '2026-10-04'), row('b', 2, '2026-10-08', '2026-10-09')], { a: ['b'] })
  expect(rest).toEqual([])
  expect(arrow).toEqual({ key: 'a>b', blockerId: 'a', blockedId: 'b', path: 'M40 16H48V80H70', head: 'M70 80l-5 -3.5v7z', conflict: false })
})

test('the arrow is red when the blocked task starts before the blocker ends, and it goes back between the rows', () => {
  const [late] = arrows([row('a', 0, '2026-10-02', '2026-10-10'), row('b', 1, '2026-10-05', '2026-10-06')], { a: ['b'] })
  expect(late!.conflict).toBe(true)
  // out of the blocker at x = 100, back between the two rows (y = 32) to before the blocked bar (x = 40)
  expect(late!.path).toBe('M100 16H108V32H32V48H40')
  // the same day is a conflict too; the day after is not
  expect(arrows([row('a', 0, '2026-10-02', '2026-10-05'), row('b', 1, '2026-10-05', '2026-10-06')], { a: ['b'] })[0]!.conflict).toBe(true)
  expect(arrows([row('a', 0, '2026-10-02', '2026-10-05'), row('b', 1, '2026-10-06', '2026-10-06')], { a: ['b'] })[0]!.conflict).toBe(false)
  // a blocked task above its blocker: the line goes up
  expect(arrows([row('b', 0, '2026-10-05', '2026-10-06'), row('a', 1, '2026-10-02', '2026-10-10')], { a: ['b'] })[0]!.path).toBe('M100 48H108V32H32V16H40')
})

test('an arrow needs both rows: a relation to a task with no mounted bar draws nothing', () => {
  expect(arrows([row('a', 0, '2026-10-02', '2026-10-04')], { a: ['b'] })).toEqual([])
  // a task in two groups has two rows: the first one counts, and there is one arrow
  const twice = arrows([row('a', 0, '2026-10-02', '2026-10-04'), row('b', 1, '2026-10-08', '2026-10-09'), row('b', 5, '2026-10-08', '2026-10-09')], { a: ['b'] })
  expect(twice.map((arrow) => arrow.path)).toEqual(['M40 16H48V48H70'])
})
