import { addDays, addMonths, differenceInCalendarDays, eachMonthOfInterval, format, max, min, startOfDay, startOfMonth } from 'date-fns'

export type ZoomPreset = 'week' | 'month' | 'quarter'
export const ZOOM_PRESETS: Record<ZoomPreset, number> = { week: 44, month: 16, quarter: 5 }
export const MIN_ZOOM = 3
export const MAX_ZOOM = 64

export function clampZoom(px: number): number {
  return Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, px))
}

export function presetOf(px: number): ZoomPreset | null {
  return (Object.keys(ZOOM_PRESETS) as ZoomPreset[]).find((key) => ZOOM_PRESETS[key] === px) ?? null
}

/** Ctrl+wheel and trackpad pinch (browsers send pinch as ctrlKey wheel events). Negative deltaY zooms in. */
export function zoomFromWheel(px: number, deltaY: number): number {
  return clampZoom(px * Math.exp(-deltaY * 0.01))
}

/** New scrollLeft that keeps the day under `anchorX` (px from the track's visible left edge) in place. */
export function anchoredScrollLeft(scrollLeft: number, anchorX: number, oldPx: number, newPx: number): number {
  const day = (scrollLeft + anchorX) / oldPx
  return Math.max(0, day * newPx - anchorX)
}

/** Timeline window: `start` is local midnight of the first day; `days` columns follow. */
export interface TimeRange {
  start: Date
  days: number
}

export function dayIndex(range: TimeRange, date: Date): number {
  return differenceInCalendarDays(date, range.start)
}

export function dayAt(range: TimeRange, index: number): Date {
  return addDays(range.start, index)
}

export function xOf(range: TimeRange, date: Date, px: number): number {
  return dayIndex(range, date) * px
}

export function dayIndexAtX(x: number, px: number): number {
  return Math.floor(x / px)
}

/** Data bounds ± 3 months, never less than today ± 6 months; starts on a month start. */
export function computeRange(dates: Date[], today: Date): TimeRange {
  const earliest = min([today, ...dates])
  const latest = max([today, ...dates])
  const start = startOfMonth(min([addMonths(earliest, -3), addMonths(today, -6)]))
  const end = startOfDay(max([addMonths(latest, 3), addMonths(today, 6)]))
  return { start, days: differenceInCalendarDays(end, start) + 1 }
}

export interface MonthMark {
  x: number
  width: number
  label: string
}

/** One mark per month; January and the first mark also show the year. */
export function monthMarks(range: TimeRange, px: number): MonthMark[] {
  const last = dayAt(range, range.days - 1)
  return eachMonthOfInterval({ start: range.start, end: last }).map((month, index) => {
    const from = Math.max(0, dayIndex(range, month))
    const to = Math.min(range.days, dayIndex(range, addMonths(month, 1)))
    const label = index === 0 || month.getMonth() === 0 ? format(month, 'MMMM yyyy') : format(month, 'MMMM')
    return { x: from * px, width: (to - from) * px, label }
  })
}

export interface TickMark {
  x: number
  label: string
}

export const showsDailyTicks = (px: number) => px >= 28
export const showsWeeklyLines = (px: number) => px >= 5

/** Day-of-month labels: every day when wide, Mondays when medium, none when narrow. */
export function tickMarks(range: TimeRange, px: number): TickMark[] {
  if (!showsWeeklyLines(px)) return []
  const ticks: TickMark[] = []
  for (let index = 0; index < range.days; index++) {
    const day = dayAt(range, index)
    if (showsDailyTicks(px) || day.getDay() === 1) ticks.push({ x: index * px, label: String(day.getDate()) })
  }
  return ticks
}
