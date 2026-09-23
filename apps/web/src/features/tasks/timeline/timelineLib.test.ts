import { afterAll, describe, expect, test } from 'bun:test'
import {
  MAX_ZOOM, MIN_ZOOM, ZOOM_PRESETS, anchoredScrollLeft, clampZoom, computeRange, dayAt, dayIndex,
  dayIndexAtX, monthMarks, presetOf, tickMarks, xOf, zoomFromWheel,
} from './timelineLib'

// set before any fixture below is built, so every local date is a Berlin date (DST-bearing zone)
const originalTz = process.env.TZ
process.env.TZ = 'Europe/Berlin'
afterAll(() => {
  if (originalTz === undefined) delete process.env.TZ
  else process.env.TZ = originalTz
})

const local = (y: number, m: number, d: number, h = 0, min = 0) => new Date(y, m - 1, d, h, min)

describe('day scale', () => {
  const range = { start: local(2026, 3, 1), days: 60 }

  test('dayIndex counts calendar days across the March DST change', () => {
    // 2026-03-29 is the Berlin spring-forward day (23 hours long)
    expect(dayIndex(range, local(2026, 3, 29))).toBe(28)
    expect(dayIndex(range, local(2026, 3, 30))).toBe(29)
    expect(dayIndex(range, local(2026, 3, 30, 23, 30))).toBe(29)
  })

  test('dayAt is local midnight and round-trips through dayIndex', () => {
    const day = dayAt(range, 29)
    expect(day.getHours()).toBe(0)
    expect(day.getDate()).toBe(30)
    expect(dayIndex(range, day)).toBe(29)
  })

  test('x and day index convert at a given zoom', () => {
    expect(xOf(range, local(2026, 3, 3), 16)).toBe(32)
    expect(dayIndexAtX(32, 16)).toBe(2)
    expect(dayIndexAtX(47.9, 16)).toBe(2)
    expect(dayIndexAtX(-1, 16)).toBe(-1)
  })
})

describe('computeRange', () => {
  const today = local(2026, 9, 23)

  test('empty input spans today ± 6 months, starting on a month start', () => {
    const range = computeRange([], today)
    expect(range.start).toEqual(local(2026, 3, 1))
    expect(dayAt(range, range.days - 1)).toEqual(local(2027, 3, 23))
  })

  test('data outside the minimum window adds 3 months of padding', () => {
    const range = computeRange([local(2025, 1, 10), local(2028, 1, 5)], today)
    expect(range.start).toEqual(local(2024, 10, 1))
    expect(dayAt(range, range.days - 1)).toEqual(local(2028, 4, 5))
  })
})

describe('zoom', () => {
  test('clamps to the supported range', () => {
    expect(clampZoom(1)).toBe(MIN_ZOOM)
    expect(clampZoom(500)).toBe(MAX_ZOOM)
    expect(clampZoom(20)).toBe(20)
  })

  test('recognises presets exactly', () => {
    expect(presetOf(ZOOM_PRESETS.month)).toBe('month')
    expect(presetOf(17)).toBeNull()
  })

  test('wheel up zooms in, wheel down zooms out, both clamped', () => {
    expect(zoomFromWheel(16, -100)).toBeGreaterThan(16)
    expect(zoomFromWheel(16, 100)).toBeLessThan(16)
    expect(zoomFromWheel(MAX_ZOOM, -1000)).toBe(MAX_ZOOM)
  })

  test('anchored zoom keeps the date under the anchor fixed', () => {
    // anchor 200px into the viewport, scrolled 1000px, 10px/day → day 120 under the anchor
    const next = anchoredScrollLeft(1000, 200, 10, 20)
    expect((next + 200) / 20).toBe(120)
  })

  test('anchored zoom never scrolls to a negative position', () => {
    expect(anchoredScrollLeft(0, 50, 20, 5)).toBe(0)
  })
})

describe('marks', () => {
  const range = { start: local(2026, 3, 1), days: 61 }

  test('month marks cover each month with its width', () => {
    const marks = monthMarks(range, 10)
    expect(marks[0]).toEqual({ x: 0, width: 310, label: 'March 2026' })
    expect(marks[1]!.x).toBe(310)
    expect(marks[1]!.label).toBe('April')
  })

  test('daily ticks at week zoom, Monday ticks at month zoom, none below 5px', () => {
    expect(tickMarks(range, 44)).toHaveLength(61)
    const weekly = tickMarks(range, 16)
    expect(weekly[0]).toEqual({ x: 16 * 1, label: '2' }) // Mon 2 March 2026
    expect(weekly.every((tick) => (tick.x / 16) % 7 === 1)).toBe(true)
    expect(tickMarks(range, 4)).toHaveLength(0)
  })
})
