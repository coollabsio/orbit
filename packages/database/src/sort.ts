import { dayStartMs } from './dates'
import type { DatabaseViewSort, Property, Row, ViewContext } from './types'
import { cellOf, displayText, options, unreachable } from './values'

type SortKey = number | string | number[] | null

/**
 * Rows ordered by `sorts` (spec 6): empty values last in both directions, options by their order
 * in the config, people by display name, unticked checkboxes before ticked ones, dates by their
 * start. Ties fall back to the manual order (`position`, then `id`). Sorts on unknown properties
 * are skipped. Each row's keys are computed once, so a sort costs O(n log n) key comparisons.
 */
export function sortRows(
  rows: readonly Row[],
  sorts: readonly DatabaseViewSort[],
  properties: readonly Property[],
  ctx: ViewContext,
): Row[] {
  const byId = new Map(properties.map((property) => [property.id, property]))
  const active = sorts.flatMap((sort) => {
    const property = byId.get(sort.property_id)
    return property ? [{ property, sign: sort.direction === 'desc' ? -1 : 1 }] : []
  })
  const collator = new Intl.Collator(ctx.locale, { sensitivity: 'base', numeric: true })
  const keyed = rows.map((row) => ({ row, keys: active.map(({ property }) => sortKey(row, property, ctx)) }))
  keyed.sort((a, b) => {
    for (let index = 0; index < active.length; index++) {
      const left = a.keys[index]!
      const right = b.keys[index]!
      if (left === null || right === null) {
        if (left !== right) return left === null ? 1 : -1
        continue
      }
      const order = compareKeys(left, right, collator)
      if (order !== 0) return order * active[index]!.sign
    }
    return a.row.position - b.row.position || (a.row.id < b.row.id ? -1 : a.row.id > b.row.id ? 1 : 0)
  })
  return keyed.map(({ row }) => row)
}

function sortKey(row: Row, property: Property, ctx: ViewContext): SortKey {
  const cell = cellOf(row, property)
  switch (property.type) {
    case 'checkbox':
      return cell ? 1 : 0
    case 'title':
    case 'text':
    case 'url':
    case 'person':
    case 'created_by':
    case 'last_edited_by': {
      const text = displayText(property, cell, ctx)
      return text === '' ? null : text
    }
    case 'number':
      return cell?.kind === 'number' ? cell.number : null
    case 'select':
    case 'status':
    case 'multi_select': {
      const ids = cell?.kind === 'choice' ? [cell.id] : cell?.kind === 'choices' ? cell.ids : []
      const list = options(property)
      const order = ids.map((id) => list.findIndex((option) => option.id === id)).filter((at) => at !== -1)
      return order.length === 0 ? null : property.type === 'multi_select' ? order : order[0]!
    }
    case 'date':
    case 'created_time':
    case 'last_edited_time': {
      if (cell?.kind !== 'date') return null
      const start = cell.date.start
      return start.kind === 'moment' ? start.ms : dayStartMs(start.day, ctx.timeZone)
    }
    default:
      return unreachable(property)
  }
}

function compareKeys(left: Exclude<SortKey, null>, right: Exclude<SortKey, null>, collator: Intl.Collator): number {
  if (typeof left === 'number' && typeof right === 'number') return left - right
  if (typeof left === 'string' && typeof right === 'string') return collator.compare(left, right)
  if (Array.isArray(left) && Array.isArray(right)) {
    for (let index = 0; index < Math.min(left.length, right.length); index++) {
      const order = left[index]! - right[index]!
      if (order !== 0) return order
    }
    return left.length - right.length
  }
  return 0
}
