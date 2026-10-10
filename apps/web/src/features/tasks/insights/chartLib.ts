import { useState } from 'react'

/** What a chart counts: tasks, or the sum of their estimates. */
export type Measure = 'count' | 'points'

const MEASURE_KEY = 'orbit:insights_measure'

/** One setting for all charts, kept in local storage. Points are offered only where estimates are on. */
export function useMeasure(pointsAvailable: boolean): [Measure, (measure: Measure) => void] {
  const [measure, setMeasure] = useState<Measure>(() => (localStorage.getItem(MEASURE_KEY) === 'points' ? 'points' : 'count'))
  const update = (next: Measure) => {
    localStorage.setItem(MEASURE_KEY, next)
    setMeasure(next)
  }
  return [pointsAvailable ? measure : 'count', update]
}

/** The ideal line of a burndown: from the scope of the first day to zero on the last day. */
export function idealLine(scope: number, days: number): number[] {
  if (days <= 1) return days === 1 ? [scope] : []
  return Array.from({ length: days }, (_, index) => scope * (1 - index / (days - 1)))
}

/** The mean, rounded to one decimal; 0 for no values. */
export function average(values: number[]): number {
  if (values.length === 0) return 0
  return Math.round((values.reduce((sum, value) => sum + value, 0) / values.length) * 10) / 10
}

/** `YYYY-MM-DD` plus a number of days, in the calendar (no timezone). */
export function addDays(day: string, days: number): string {
  const date = new Date(`${day}T00:00:00Z`)
  date.setUTCDate(date.getUTCDate() + days)
  return date.toISOString().slice(0, 10)
}

/**
 * The days from the first row to the last one with no gap. The cycle job writes a row on each day it runs; a day
 * with no row (the server was down) takes the values of the day before it, so the lines stay flat there.
 */
export function fillDays<T extends { day: string }>(rows: T[]): T[] {
  const sorted = [...rows].sort((a, b) => a.day.localeCompare(b.day))
  const filled: T[] = []
  for (const row of sorted) {
    let last = filled.at(-1)
    while (last && addDays(last.day, 1) < row.day) {
      last = { ...last, day: addDays(last.day, 1) }
      filled.push(last)
    }
    filled.push(row)
  }
  return filled
}

/** A round upper limit for a value axis: 1, 2 or 5 times a power of ten, at least 4. */
export function niceMax(value: number): number {
  if (value <= 4) return 4
  const power = 10 ** Math.floor(Math.log10(value))
  const scaled = value / power
  return (scaled <= 1 ? 1 : scaled <= 2 ? 2 : scaled <= 5 ? 5 : 10) * power
}

/** "Oct 5" for a `YYYY-MM-DD` day. */
export function dayLabel(day: string): string {
  return new Date(`${day}T00:00:00Z`).toLocaleDateString('en-US', { month: 'short', day: 'numeric', timeZone: 'UTC' })
}

/** "12 tasks", "1 task", "8 points". */
export function measureText(value: number, measure: Measure): string {
  const rounded = Math.round(value * 10) / 10
  const unit = measure === 'points' ? 'point' : 'task'
  return `${rounded} ${unit}${rounded === 1 ? '' : 's'}`
}
