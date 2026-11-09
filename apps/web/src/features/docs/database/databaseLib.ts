import type { DatabaseFilterRule, Property, Row, ViewConfig, ViewContext } from '@orbit/database/types'
import { options } from '@orbit/database/values'
import { OPERATOR_LABELS, RELATIVE_DATES, operatorsOf } from './propertyMeta'

/** `config` with the column of `propertyId` moved next to `targetId` (`side` of it). */
export function placeColumn(config: ViewConfig, propertyId: string, targetId: string, side: 'left' | 'right'): ViewConfig {
  const moving = config.columns.find((column) => column.property_id === propertyId) ?? { property_id: propertyId, width: 200, hidden: false }
  const rest = config.columns.filter((column) => column.property_id !== propertyId)
  const at = rest.findIndex((column) => column.property_id === targetId)
  if (at === -1) return { ...config, columns: [...rest, moving] }
  rest.splice(side === 'left' ? at : at + 1, 0, { ...moving, hidden: false })
  return { ...config, columns: rest }
}

/** The `after_row_id` that drops a row before or after `targetId` in the manual order of all rows (`null`: first). */
export function dropAfterRowId(rows: readonly Row[], movingId: string, targetId: string, side: 'before' | 'after'): string | null {
  if (side === 'after') return targetId
  const ordered = [...rows].sort((a, b) => a.position - b.position || a.id.localeCompare(b.id)).filter((row) => row.id !== movingId)
  const at = ordered.findIndex((row) => row.id === targetId)
  return at > 0 ? ordered[at - 1].id : null
}

export type Pin = { id: string; at: 'top' | 'bottom' }

/**
 * The view's rows with the rows added since the view last changed kept in place, at the top or the bottom, even when
 * they do not match the filter or sort elsewhere (spec 7.2: a new row does not jump away under the cursor).
 */
export function withPinnedRows(shown: readonly Row[], pins: readonly Pin[], all: readonly Row[]): Row[] {
  if (pins.length === 0) return [...shown]
  const byId = new Map(all.map((row) => [row.id, row]))
  const pinned = new Set(pins.map((pin) => pin.id))
  const pick = (at: Pin['at']) => pins.filter((pin) => pin.at === at).flatMap((pin) => byId.get(pin.id) ?? [])
  return [...pick('top'), ...shown.filter((row) => !pinned.has(row.id)), ...pick('bottom')]
}

/** A new filter rule on a property: its type's first operator and no value, which matches every row. */
export function newRule(property: Property): DatabaseFilterRule {
  return { property_id: property.id, operator: operatorsOf(property.type)[0], value: null }
}

/** The chip text after the property name (spec 7.1): `Reading`, `contains “abc”`, `is empty`. */
export function ruleSummary(rule: DatabaseFilterRule, property: Property, ctx: ViewContext): string {
  const operator = OPERATOR_LABELS[rule.operator]
  if (rule.operator === 'is_empty' || rule.operator === 'is_not_empty') return operator
  const value = rule.value
  if (value === null || value === undefined || value === '' || (Array.isArray(value) && value.length === 0)) return 'All'
  if (property.type === 'checkbox') return value === true ? 'Checked' : 'Unchecked'
  if (Array.isArray(value)) {
    const names = value.map((id) =>
      id === 'me' ? 'Me' : typeof id !== 'string' ? '' : options(property).find((option) => option.id === id)?.name ?? ctx.members.get(id) ?? '',
    )
    const listed = names.filter((name) => name !== '').join(', ')
    return rule.operator === 'is' || rule.operator === 'contains' ? listed : `${operator} ${listed}`
  }
  if (typeof value === 'string') {
    const relative = RELATIVE_DATES.find((date) => date.value === value)?.label
    if (relative || /^\d{4}-\d{2}-\d{2}/.test(value)) return `${operator} ${relative ?? value.slice(0, 10)}`
    return `${operator} “${value}”`
  }
  return `${operator} ${String(value)}`
}

/** The rows before and after `rowId` in the view's order (spec 7.5); null at an end, or when the view does not show it. */
export function neighborRows(order: readonly string[], rowId: string): { previous: string | null; next: string | null } {
  const at = order.indexOf(rowId)
  if (at === -1) return { previous: null, next: null }
  return { previous: order[at - 1] ?? null, next: order[at + 1] ?? null }
}
