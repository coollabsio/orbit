import type { TaskField, TaskFieldKind, TaskFieldOption } from './generated/types.gen'

/** Custom task field values (issue #87), shared by the web and mobile apps. */

export const FIELD_KINDS: { kind: TaskFieldKind; label: string }[] = [
  { kind: 'text', label: 'Text' },
  { kind: 'number', label: 'Number' },
  { kind: 'date', label: 'Date' },
  { kind: 'select', label: 'Select' },
  { kind: 'multi_select', label: 'Multi-select' },
  { kind: 'checkbox', label: 'Checkbox' },
  { kind: 'url', label: 'URL' },
]

export function fieldKindLabel(kind: TaskFieldKind): string {
  return FIELD_KINDS.find((item) => item.kind === kind)?.label ?? kind
}

export function hasOptions(kind: TaskFieldKind): boolean {
  return kind === 'select' || kind === 'multi_select'
}

/** The options a stored value points at, in the field's order (removed options are gone). */
export function selectedOptions(field: Pick<TaskField, 'kind' | 'options'>, value: unknown): TaskFieldOption[] {
  const ids = field.kind === 'select' && typeof value === 'string' ? [value] : field.kind === 'multi_select' && Array.isArray(value) ? value : []
  return field.options.filter((option) => ids.includes(option.id))
}

/** A short text of the value, for a chip or a screen reader; empty when the task has none. */
export function fieldValueText(field: Pick<TaskField, 'kind' | 'options'>, value: unknown): string {
  if (value === undefined || value === null) return ''
  switch (field.kind) {
    case 'select':
    case 'multi_select':
      return selectedOptions(field, value).map((option) => option.name).join(', ')
    case 'checkbox':
      return value === true ? 'Yes' : ''
    case 'number':
      return typeof value === 'number' ? value.toLocaleString() : ''
    case 'date':
      return typeof value === 'string' ? dateLabel(value) : ''
    default:
      return typeof value === 'string' ? value : ''
  }
}

/** `YYYY-MM-DD` as a local calendar day ("Mar 4, 2026"), without a time-zone shift. */
function dateLabel(day: string): string {
  const [year, month, date] = day.split('-').map(Number)
  if (!year || !month || !date) return day
  return new Date(year, month - 1, date).toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' })
}

/**
 * The value to send for a text input of a text, number, date or URL field: `null` clears it, `undefined` means the
 * text is not a valid value (the input keeps it and shows an error).
 */
export function parseFieldInput(kind: TaskFieldKind, text: string): unknown {
  const trimmed = text.trim()
  if (!trimmed) return null
  if (kind === 'number') {
    const number = Number(trimmed.replace(/,/g, ''))
    return Number.isFinite(number) && Math.abs(number) <= 1e15 ? number : undefined
  }
  if (kind === 'url') {
    try {
      const url = new URL(trimmed)
      return url.protocol === 'http:' || url.protocol === 'https:' ? trimmed : undefined
    } catch {
      return undefined
    }
  }
  return trimmed
}

/** The activity sentence of a `task.field_changed` event: it names the field as it was called then. */
export function fieldActivityText(action: string, metadata: Record<string, unknown>): string | null {
  if (action !== 'task.field_changed') return null
  return typeof metadata.field_name === 'string' ? `changed ${metadata.field_name}` : 'changed a field'
}
