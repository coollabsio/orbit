import { expect, test } from 'bun:test'
import { cycleDatesLabel, cycleName, cycleProgress, estimateLabel, estimateOptions, groupPoints, moveBoundary, openCycles, pointsText } from './cyclesLib'

test('an estimate shows as its T-shirt size only where the scale has one for the value', () => {
  expect(estimateLabel(3, 'tshirt')).toBe('M')
  expect(estimateLabel(13, 'tshirt')).toBe('13')
  expect(estimateLabel(3, 'fibonacci')).toBe('3')
  expect(estimateLabel(3, null)).toBe('3')
})

test('each scale offers its values; a project with estimates off offers none', () => {
  expect(estimateOptions('fibonacci').map((option) => option.points)).toEqual([1, 2, 3, 5, 8, 13])
  expect(estimateOptions('linear').map((option) => option.label)).toEqual(['1', '2', '3', '4', '5'])
  expect(estimateOptions('tshirt')).toEqual([
    { points: 1, label: 'XS' }, { points: 2, label: 'S' }, { points: 3, label: 'M' }, { points: 5, label: 'L' }, { points: 8, label: 'XL' },
  ])
  expect(estimateOptions(null)).toEqual([])
  expect(pointsText(1)).toBe('1 point')
  expect(pointsText(5)).toBe('5 points')
})

test('a cycle has a default name and shows its first and last day', () => {
  expect(cycleName({ name: null, number: 4 })).toBe('Cycle 4')
  expect(cycleName({ name: ' Launch ', number: 4 })).toBe('Launch')
  // the cycle ends at 00:01 of the day after its last day
  const cycle = { starts_at: new Date(2026, 9, 5, 0, 1).toISOString(), ends_at: new Date(2026, 9, 19, 0, 1).toISOString() }
  expect(cycleDatesLabel(cycle, new Date(2026, 9, 8))).toBe('Oct 5 – Oct 18')
  expect(cycleDatesLabel(cycle, new Date(2027, 0, 1))).toBe('Oct 5, 2026 – Oct 18, 2026')
})

test('open cycles are the current and the future ones in time order', () => {
  const cycles = [
    { id: 'c', state: 'future', starts_at: '2026-10-19T00:01:00Z' },
    { id: 'a', state: 'completed', starts_at: '2026-09-21T00:01:00Z' },
    { id: 'b', state: 'current', starts_at: '2026-10-05T00:01:00Z' },
  ]
  expect(openCycles(cycles).map((cycle) => cycle.id)).toEqual(['b', 'c'])
})

test('progress uses points when the scope has points, else tasks', () => {
  expect(cycleProgress({ scope_count: 4, scope_points: 10, done_count: 1, done_points: 5 })).toBe(0.5)
  expect(cycleProgress({ scope_count: 4, scope_points: 0, done_count: 1, done_points: 0 })).toBe(0.25)
  expect(cycleProgress({ scope_count: 0, scope_points: 0, done_count: 0, done_points: 0 })).toBe(0)
})

test('the points of a group count leaf tasks of projects that have estimates on', () => {
  const projects = [{ id: 'p1', estimate_scale: 'fibonacci' }, { id: 'p2', estimate_scale: null }]
  const tasks = [
    { projectId: 'p1', estimate: 3 },
    { projectId: 'p1', estimate: 13, subIssueCount: 2 },
    { projectId: 'p1', estimate: null },
    { projectId: 'p2', estimate: 8 },
  ]
  expect(groupPoints(tasks, projects)).toBe(3)
  expect(groupPoints([{ projectId: 'p2', estimate: 8 }], projects)).toBeNull()
  expect(groupPoints([], projects)).toBeNull()
})

test('a cycle boundary moves by whole days and keeps its time of day', () => {
  const start = '2026-10-19T22:01:00.000Z'
  expect(moveBoundary(start, '2026-10-20', '2026-10-22')).toBe('2026-10-21T22:01:00.000Z')
  expect(moveBoundary(start, '2026-10-20', '2026-10-13')).toBe('2026-10-12T22:01:00.000Z')
  expect(moveBoundary(start, '2026-10-20', '2026-10-20')).toBe(start)
})
