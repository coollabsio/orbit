import { addDays, addMonths, localDay, parseDatePoint, pointDay } from './dates'
import type { CellValue, DatabaseFilterGroup, DatabaseFilterNode, DatabaseFilterRule, Property, Row, ViewContext } from './types'
import { cellOf, displayText, unreachable } from './values'
import { formulaBase } from './formula/values'
import { rollupBase } from './relations'

const RELATIVE_DAYS: Record<string, (today: string) => string> = {
  today: (today) => today,
  tomorrow: (today) => addDays(today, 1),
  yesterday: (today) => addDays(today, -1),
  one_week_ago: (today) => addDays(today, -7),
  one_week_from_now: (today) => addDays(today, 7),
  one_month_ago: (today) => addMonths(today, -1),
  one_month_from_now: (today) => addMonths(today, 1),
}

/** Group levels, the root included (spec P2 5.2). */
export const MAX_FILTER_LEVELS = 3
export const MAX_GROUP_CHILDREN = 10
/** Rules across the whole filter. */
export const MAX_RULES = 20

/** A filter with no rules: it matches every row. */
export const EMPTY_FILTER: DatabaseFilterGroup = { op: 'and', children: [] }

export function isGroup(node: DatabaseFilterNode): node is DatabaseFilterGroup {
  return 'children' in node
}

/** Every rule of a filter, depth first in order. */
export function filterRules(group: DatabaseFilterGroup): DatabaseFilterRule[] {
  return group.children.flatMap((child) => (isGroup(child) ? filterRules(child) : [child]))
}

/**
 * Whether `row` passes a filter: an `and` group needs every child, an `or` group one; an empty group matches every row.
 * A rule whose property is not in `properties` matches too (repair drops it).
 */
export function matchesFilter(row: Row, group: DatabaseFilterGroup, properties: ReadonlyMap<string, Property>, ctx: ViewContext): boolean {
  if (group.children.length === 0) return true
  const passes = (child: DatabaseFilterNode) => {
    if (isGroup(child)) return matchesFilter(row, child, properties, ctx)
    const property = properties.get(child.property_id)
    return property === undefined || matches(row, child, property, ctx)
  }
  return group.op === 'and' ? group.children.every(passes) : group.children.some(passes)
}

/**
 * The filter with the rules `keep` accepts, as the server's `retain_rules`: a group left empty goes (a group with one
 * child stays); the root stays.
 */
export function retainRules(group: DatabaseFilterGroup, keep: (rule: DatabaseFilterRule) => boolean): DatabaseFilterGroup {
  const children = group.children.flatMap((child): DatabaseFilterNode[] => {
    if (!isGroup(child)) return keep(child) ? [child] : []
    const kept = retainRules(child, keep)
    return kept.children.length > 0 ? [kept] : []
  })
  return { op: group.op === 'or' ? 'or' : 'and', children }
}

/**
 * Whether `row` passes `rule` on `property` (spec 4.3). A rule without a value (`null`, `""`,
 * `[]`) matches every row. Text compares ignore case. Dates compare the viewer's local days: a
 * time counts as its day in `ctx.timeZone`, and a range counts by its start, except `is`, which
 * matches when the day falls inside the range.
 */
export function matches(row: Row, rule: DatabaseFilterRule, property: Property, ctx: ViewContext): boolean {
  const cell = cellOf(row, property)
  if (rule.operator === 'is_empty') return cell === null
  if (rule.operator === 'is_not_empty') return cell !== null
  const value = rule.value
  if (value === null || value === undefined || value === '' || (Array.isArray(value) && value.length === 0)) {
    return true
  }
  switch (property.type) {
    case 'title':
    case 'text':
    case 'url':
    case 'email':
    case 'phone':
      return typeof value === 'string' ? matchText(displayText(property, cell, ctx), rule, value) : true
    case 'number':
      return typeof value === 'number' ? matchNumber(cell?.kind === 'number' ? cell.number : null, rule, value) : true
    case 'unique_id':
      return typeof value === 'number' ? matchNumber(cell?.kind === 'unique_id' ? cell.number : null, rule, value) : true
    case 'files':
      return true
    case 'select':
    case 'status': {
      const ids = new Set(idList(value, ctx))
      const hit = cell?.kind === 'choice' && ids.has(cell.id)
      return rule.operator === 'is_not' ? !hit : hit
    }
    case 'multi_select':
    case 'person':
    case 'created_by':
    case 'last_edited_by': {
      const ids = new Set(idList(value, ctx))
      const held = cell?.kind === 'choices' || cell?.kind === 'people' ? cell.ids : []
      const hit = held.some((id) => ids.has(id))
      return rule.operator === 'not_contains' || rule.operator === 'is_not' ? !hit : hit
    }
    case 'checkbox':
      return typeof value === 'boolean' ? (cell !== null) === value : true
    case 'date':
    case 'created_time':
    case 'last_edited_time': {
      const target = typeof value === 'string' ? ruleDay(value, ctx) : null
      return target === null ? true : matchDate(cell, rule, target, ctx)
    }
    case 'button':
      return true
    case 'formula':
      return matches(row, rule, formulaBase(property), ctx)
    case 'relation':
      return typeof value === 'string' ? matchText(displayText(property, cell, ctx), rule, value) : true
    case 'rollup':
      return matches(row, rule, rollupBase(property), ctx)
    default:
      return unreachable(property)
  }
}

function matchText(cellText: string, rule: DatabaseFilterRule, value: string): boolean {
  const text = cellText.toLowerCase()
  const needle = value.toLowerCase()
  switch (rule.operator) {
    case 'equals':
      return text === needle
    case 'not_equals':
      return text !== needle
    case 'contains':
      return text.includes(needle)
    case 'not_contains':
      return !text.includes(needle)
    case 'starts_with':
      return text.startsWith(needle)
    case 'ends_with':
      return text.endsWith(needle)
    default:
      return true
  }
}

/** An empty number fails every comparison except `!=`. */
function matchNumber(number: number | null, rule: DatabaseFilterRule, value: number): boolean {
  if (number === null) return rule.operator === '!='
  switch (rule.operator) {
    case '=':
      return number === value
    case '!=':
      return number !== value
    case '>':
      return number > value
    case '<':
      return number < value
    case '>=':
      return number >= value
    case '<=':
      return number <= value
    default:
      return true
  }
}

function matchDate(cell: CellValue | null, rule: DatabaseFilterRule, target: string, ctx: ViewContext): boolean {
  if (cell?.kind !== 'date') return false
  const start = pointDay(cell.date.start, ctx.timeZone)
  switch (rule.operator) {
    case 'is': {
      const end = cell.date.end ? pointDay(cell.date.end, ctx.timeZone) : start
      return start <= target && target <= end
    }
    case 'before':
      return start < target
    case 'after':
      return start > target
    case 'on_or_before':
      return start <= target
    case 'on_or_after':
      return start >= target
    default:
      return true
  }
}

/** The local day a rule names: a relative value from `ctx.now`, or an absolute date or time. */
export function ruleDay(value: string, ctx: ViewContext): string | null {
  const relative = RELATIVE_DAYS[value]
  if (relative) return relative(localDay(ctx.now, ctx.timeZone))
  const point = parseDatePoint(value)
  return point && pointDay(point, ctx.timeZone)
}

/** Option or user ids of a rule, with `"me"` (alone or in the list) as the viewer. */
function idList(value: unknown, ctx: ViewContext): string[] {
  const list = Array.isArray(value) ? value : [value]
  return list.flatMap((id) => (typeof id !== 'string' ? [] : id === 'me' ? [ctx.userId] : [id]))
}
