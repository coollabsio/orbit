/**
 * The task list's display of custom fields (issue #87), as the web's `TaskPropertyChips` / `DisplayPopover` /
 * `displayMeta`: which values a row shows, which fields the display sheet offers, and the orderings. No React.
 */
import type { TaskField, TaskFieldOption, WorkspaceRecord } from '@orbit/api/generated/types.gen'
import { hidesTaskKey } from '@orbit/api/taskProperties'
import { fieldValueText, selectedOptions, storesValues } from '@orbit/api/taskFields'

import type { IconName } from '../ui/Icon'
import { shortDate } from './format'
import type { TaskRecord } from './orbit'
import type { DisplayOptions } from './tasks-filter'

/** Option chips show this many, then "+n". */
export const MAX_OPTION_CHIPS = 2

/** One value on a row: option pills, or the kind's icon and a short text. `label` names it for a screen reader. */
export type FieldChip =
  | { fieldId: string; label: string; kind: 'options'; options: TaskFieldOption[]; more: number }
  | { fieldId: string; label: string; kind: 'text'; icon: IconName; text: string }

/** Each kind's icon, as the web's `FIELD_KIND_ICON`. */
export const FIELD_KIND_ICON: Record<TaskField['kind'], IconName> = {
  text: 'text',
  number: 'hashtag',
  date: 'calendar',
  select: 'arrowCircleDown',
  multi_select: 'list',
  checkbox: 'checkSquare',
  url: 'link',
  time_tracking: 'timer',
}

/** `example.com` for a URL value (no `www.`); the text itself when it does not parse. */
export function urlHost(value: string): string {
  try {
    return new URL(value).host.replace(/^www\./, '')
  } catch {
    return value
  }
}

/** The chip of one value; null when the task has none (an unchecked box, a removed option, …). */
export function fieldChip(field: TaskField, value: unknown): FieldChip | null {
  const text = fieldValueText(field, value)
  if (!text) return null
  const label = `${field.name}: ${text}`
  if (field.kind === 'select' || field.kind === 'multi_select') {
    const chosen = selectedOptions(field, value)
    return { fieldId: field.id, label, kind: 'options', options: chosen.slice(0, MAX_OPTION_CHIPS), more: Math.max(0, chosen.length - MAX_OPTION_CHIPS) }
  }
  const shown =
    field.kind === 'checkbox'
      ? field.name
      : field.kind === 'date' && typeof value === 'string'
        ? shortDate(`${value}T00:00:00`)
        : field.kind === 'url' && typeof value === 'string'
          ? urlHost(value)
          : text
  return { fieldId: field.id, label, kind: 'text', icon: FIELD_KIND_ICON[field.kind], text: shown }
}

/** The chips of a row: the display's fields (`custom_fields`, resolved against the gallery) in order, values only. */
export function taskFieldChips(task: Pick<TaskRecord, 'field_values'>, shownFields: readonly TaskField[]): FieldChip[] {
  if (shownFields.length === 0) return []
  const values = new Map((task.field_values ?? []).map((item) => [item.field_id, item.value]))
  return shownFields.flatMap((field) => fieldChip(field, values.get(field.id)) ?? [])
}

/** The shown custom fields as gallery records, in the display's order; a deleted field is skipped. */
export function shownCustomFields(gallery: readonly TaskField[], ids: readonly string[] | undefined): TaskField[] {
  if (!ids?.length) return []
  const byId = new Map(gallery.map((field) => [field.id, field]))
  return ids.flatMap((id) => byId.get(id) ?? [])
}

/**
 * The fields the display sheet offers: on a project page the fields that project shows (in its order), elsewhere the
 * whole gallery. A gallery field the display already shows or orders by stays listed, so it can be turned off.
 */
export function displayFields(gallery: readonly TaskField[], projectFields: readonly TaskField[] | null, display: Pick<DisplayOptions, 'custom_fields' | 'order_field_id'>): TaskField[] {
  if (!projectFields) return [...gallery]
  const used = new Set([...(display.custom_fields ?? []), ...(display.order_field_id ? [display.order_field_id] : [])])
  const listed = new Set(projectFields.map((field) => field.id))
  return [...projectFields, ...gallery.filter((field) => used.has(field.id) && !listed.has(field.id))]
}

/** Turns one field on (at the end) or off; a field that no longer exists drops out. */
export function toggleCustomField(display: DisplayOptions, fieldId: string, fields: readonly TaskField[]): Pick<DisplayOptions, 'custom_fields'> {
  const current = (display.custom_fields ?? []).filter((id) => fields.some((field) => field.id === id))
  return { custom_fields: current.includes(fieldId) ? current.filter((id) => id !== fieldId) : [...current, fieldId] }
}

type BuiltInOrder = Exclude<DisplayOptions['order_by'], 'custom_field'>

const ORDER_LABEL: Record<BuiltInOrder, string> = {
  manual: 'Manual',
  priority: 'Priority',
  created: 'Created',
  updated: 'Updated',
  title: 'Title',
  due_date: 'Due date',
  estimate: 'Estimate',
}
const BUILT_IN_ORDERS = Object.keys(ORDER_LABEL) as BuiltInOrder[]

const FIELD_PREFIX = 'field:'

/** The key of the display's ordering: a built-in name, or `field:<id>` for a custom field. */
export function orderKey(display: Pick<DisplayOptions, 'order_by' | 'order_field_id'>): string {
  return display.order_by === 'custom_field' && display.order_field_id ? `${FIELD_PREFIX}${display.order_field_id}` : display.order_by === 'custom_field' ? 'manual' : display.order_by
}

/**
 * The orderings to choose from: the built-in ones (without a property the workspace hides), then each field that
 * stores values; an ordering by a field not listed shows as "Unknown field".
 */
export function orderOptions(fields: readonly TaskField[], current: string, workspace?: Pick<WorkspaceRecord, 'hidden_task_properties'>): { key: string; label: string }[] {
  const items = [...BUILT_IN_ORDERS.filter((order) => !hidesTaskKey(workspace, order)).map((order) => ({ key: order as string, label: ORDER_LABEL[order] })), ...fields.filter((field) => storesValues(field.kind)).map((field) => ({ key: `${FIELD_PREFIX}${field.id}`, label: field.name }))]
  if (!items.some((item) => item.key === current)) items.push({ key: current, label: 'Unknown field' })
  return items
}

/** The display change of choosing an ordering key; `order_field_id` goes only with a custom-field ordering. */
export function orderPatch(key: string): Pick<DisplayOptions, 'order_by' | 'order_field_id'> {
  if (key.startsWith(FIELD_PREFIX)) return { order_by: 'custom_field', order_field_id: key.slice(FIELD_PREFIX.length) }
  return { order_by: key as BuiltInOrder, order_field_id: undefined }
}
