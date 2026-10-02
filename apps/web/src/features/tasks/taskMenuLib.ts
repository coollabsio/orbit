/** The tasks a row's context menu acts on: the selection when the row is part of it, else the row alone. */
export function menuTargetIds(rowId: string, selectedIds: readonly string[]): string[] {
  return selectedIds.includes(rowId) ? [...selectedIds] : [rowId]
}

export interface DuePreset {
  label: string
  /** The due date: the end of that local day. */
  date: Date
}

/** The quick due dates of the context menu. The week ends on Sunday, as in the date picker; a day shows once. */
export function dueDatePresets(now: Date): DuePreset[] {
  const inDays = (days: number) => {
    const date = new Date(now)
    date.setDate(date.getDate() + days)
    date.setHours(23, 59, 0, 0)
    return date
  }
  const toSunday = (7 - now.getDay()) % 7
  const presets = [
    { label: 'Today', date: inDays(0) },
    { label: 'Tomorrow', date: inDays(1) },
    { label: 'End of this week', date: inDays(toSunday) },
    { label: 'In one week', date: inDays(7) },
  ]
  return presets.filter((preset, index) => presets.findIndex((other) => other.date.getTime() === preset.date.getTime()) === index)
}
