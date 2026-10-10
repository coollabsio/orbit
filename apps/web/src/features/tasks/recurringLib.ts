import { format } from 'date-fns'
import type { RecurringTaskRecord } from '@/api/generated/types.gen'

export type RecurringMode = 'schedule' | 'after_completion'
export type IntervalUnit = 'day' | 'week' | 'month'

export const INTERVAL_UNITS: readonly IntervalUnit[] = ['day', 'week', 'month']
export const MODE_LABEL: Record<RecurringMode, string> = {
  schedule: 'On a schedule',
  after_completion: 'After completion',
}

/** "Every day", "Every 2 weeks", "1 month after completion". */
export function intervalText(count: number, unit: string, mode: string): string {
  const units = `${unit}${count === 1 ? '' : 's'}`
  if (mode === 'after_completion') return `${count} ${units} after completion`
  return count === 1 ? `Every ${unit}` : `Every ${count} ${units}`
}

/** When the routine makes its next task: "Paused", "After the last task is closed", "Now", or "Oct 12, 9:00 AM". */
export function nextRunText(routine: Pick<RecurringTaskRecord, 'paused' | 'next_run_at'>, now: Date = new Date()): string {
  if (routine.paused) return 'Paused'
  if (!routine.next_run_at) return 'After the last task is closed'
  const next = new Date(routine.next_run_at)
  // the job runs each minute
  if (next.getTime() <= now.getTime()) return 'Now'
  return format(next, next.getFullYear() === now.getFullYear() ? 'MMM d, p' : 'MMM d, yyyy, p')
}

/** A whole number from 1 to 1000, or null. */
export function parseEveryCount(text: string): number | null {
  if (!/^\d{1,4}$/.test(text.trim())) return null
  const count = Number(text.trim())
  return count >= 1 && count <= 1000 ? count : null
}
