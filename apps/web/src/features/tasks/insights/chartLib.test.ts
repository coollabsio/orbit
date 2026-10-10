import { expect, test } from 'bun:test'
import { average, fillDays, idealLine, measureText, niceMax } from './chartLib'

test('the ideal line goes from the scope of the first day to zero on the last day', () => {
  expect(idealLine(12, 5)).toEqual([12, 9, 6, 3, 0])
  expect(idealLine(7, 1)).toEqual([7])
  expect(idealLine(7, 0)).toEqual([])
})

test('the average of the velocity is rounded to one decimal', () => {
  expect(average([3, 4, 6])).toBe(4.3)
  expect(average([5])).toBe(5)
  expect(average([])).toBe(0)
})

test('a day with no snapshot takes the values of the day before it', () => {
  const rows = [
    { day: '2026-10-05', done: 1 },
    { day: '2026-10-08', done: 4 },
    { day: '2026-10-06', done: 2 },
  ]
  expect(fillDays(rows)).toEqual([
    { day: '2026-10-05', done: 1 },
    { day: '2026-10-06', done: 2 },
    { day: '2026-10-07', done: 2 },
    { day: '2026-10-08', done: 4 },
  ])
  // across a month end
  expect(fillDays([{ day: '2026-10-30' }, { day: '2026-11-02' }]).map((row) => row.day)).toEqual(['2026-10-30', '2026-10-31', '2026-11-01', '2026-11-02'])
  expect(fillDays([])).toEqual([])
})

test('the value axis ends on a round number', () => {
  expect(niceMax(0)).toBe(4)
  expect(niceMax(7)).toBe(10)
  expect(niceMax(13)).toBe(20)
  expect(niceMax(42)).toBe(50)
  expect(niceMax(100)).toBe(100)
})

test('values name their unit', () => {
  expect(measureText(1, 'count')).toBe('1 task')
  expect(measureText(12, 'count')).toBe('12 tasks')
  expect(measureText(4.25, 'points')).toBe('4.3 points')
})
