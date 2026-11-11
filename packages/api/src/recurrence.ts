import type { TaskRecurrence, TaskRecurrenceFrequency, TaskRecurrenceWeekday } from './generated/types.gen'

/** Monday first, as a rule lists them. */
export const RECURRENCE_WEEKDAYS: { value: TaskRecurrenceWeekday; label: string; letter: string }[] = [
  { value: 'mon', label: 'Mon', letter: 'M' },
  { value: 'tue', label: 'Tue', letter: 'T' },
  { value: 'wed', label: 'Wed', letter: 'W' },
  { value: 'thu', label: 'Thu', letter: 'T' },
  { value: 'fri', label: 'Fri', letter: 'F' },
  { value: 'sat', label: 'Sat', letter: 'S' },
  { value: 'sun', label: 'Sun', letter: 'S' },
]

export const RECURRENCE_FREQUENCIES: TaskRecurrenceFrequency[] = ['daily', 'weekly', 'monthly', 'yearly']

const UNITS: Record<TaskRecurrenceFrequency, [string, string]> = {
  daily: ['day', 'days'],
  weekly: ['week', 'weeks'],
  monthly: ['month', 'months'],
  yearly: ['year', 'years'],
}

export const MAX_RECURRENCE_INTERVAL = 365

/** "day" or "days" for an interval of `count`. */
export function recurrenceUnit(frequency: TaskRecurrenceFrequency, count: number): string {
  return UNITS[frequency][count === 1 ? 0 : 1]
}

/** The rule a task gets when someone turns repeating on: weekly, on the due date's weekday. */
export const DEFAULT_RECURRENCE: TaskRecurrence = { frequency: 'weekly', interval: 1, weekdays: [], base: 'due' }

/** A rule the server accepts: weekdays only for a weekly rule counted from the due date, interval 1 to 365. */
export function normalizeRecurrence(rule: TaskRecurrence): TaskRecurrence {
  const takesWeekdays = rule.frequency === 'weekly' && rule.base === 'due'
  const interval = Math.min(MAX_RECURRENCE_INTERVAL, Math.max(1, Math.round(rule.interval) || 1))
  const weekdays = takesWeekdays ? RECURRENCE_WEEKDAYS.map((day) => day.value).filter((day) => rule.weekdays.includes(day)) : []
  return { ...rule, interval, weekdays }
}

/** "Every week on Mon, Wed", "Every 2 months · from completion", "Does not repeat". */
export function recurrenceLabel(rule: TaskRecurrence | null | undefined): string {
  if (!rule) return 'Does not repeat'
  const unit = recurrenceUnit(rule.frequency, rule.interval)
  const every = rule.interval === 1 ? `Every ${unit}` : `Every ${rule.interval} ${unit}`
  const days = RECURRENCE_WEEKDAYS.filter((day) => rule.weekdays.includes(day.value))
  const weekdays = days.length === 5 && days.every((day) => day.value !== 'sat' && day.value !== 'sun') ? ' on weekdays' : days.length > 0 ? ` on ${days.map((day) => day.label).join(', ')}` : ''
  return `${every}${weekdays}${rule.base === 'completion' ? ' · from completion' : ''}`
}

/** The activity feed's sentence for a changed rule: "set it to repeat every week on Mon", "stopped repeating". */
export function recurrenceChangeText(rule: TaskRecurrence | null | undefined): string {
  if (!rule) return 'stopped repeating'
  const label = recurrenceLabel(rule)
  return `set it to repeat ${label.charAt(0).toLowerCase()}${label.slice(1)}`
}
