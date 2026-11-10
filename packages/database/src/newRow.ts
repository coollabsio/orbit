import { addDays } from './dates'
import { isGroup, matches, ruleDay } from './filter'
import type { CellValue, DatabaseFilterGroup, DatabaseFilterRule, Property, Row, ViewContext } from './types'
import { apiValue, parseValue } from './values'

/** What `POST …/rows` needs so a new row passes the filter: its title and its values by property id. */
export type NewRowFill = { title: string; values: Record<string, unknown> }

/**
 * The title and values a new row needs to match the view's filter, as in Notion: `Status is
 * Reading` gives the status "Reading", `Tags contains AI` the tag "AI". Only rules every row must
 * pass count: those of an `and` root and of the `and` groups under it; an `or` group gives nothing.
 * Each property takes the first rule that gives a value, and keeps it only when it passes every
 * such rule on that property. Rules with no single fitting value (`is_not`, `is_not_empty`,
 * computed types) add nothing.
 */
export function newRowValues(filter: DatabaseFilterGroup, properties: readonly Property[], ctx: ViewContext): NewRowFill {
  const rules = requiredRules(filter)
  const byId = new Map(properties.map((property) => [property.id, property]))
  const fill: NewRowFill = { title: '', values: {} }
  const done = new Set<string>()
  for (const rule of rules) {
    const property = byId.get(rule.property_id)
    if (!property || done.has(property.id)) continue
    const candidate = valueFor(rule, property, ctx)
    if (!candidate) continue
    const probe = probeRow(property, candidate)
    const own = rules.filter((other) => other.property_id === property.id)
    if (!own.every((other) => matches(probe, other, property, ctx))) continue
    done.add(property.id)
    if (property.type === 'title') fill.title = candidate.kind === 'text' ? candidate.text : ''
    else fill.values[property.id] = apiValue(candidate)
  }
  return fill
}

function requiredRules(group: DatabaseFilterGroup): DatabaseFilterRule[] {
  if (group.op !== 'and') return []
  return group.children.flatMap((child) => (isGroup(child) ? requiredRules(child) : [child]))
}

function valueFor(rule: DatabaseFilterRule, property: Property, ctx: ViewContext): CellValue | null {
  const { operator, value } = rule
  switch (property.type) {
    case 'title':
    case 'text':
    case 'url':
    case 'email':
    case 'phone':
      if (typeof value !== 'string' || value === '') return null
      if (!['equals', 'contains', 'starts_with', 'ends_with'].includes(operator)) return null
      return property.type === 'title' || property.type === 'text' ? { kind: 'text', text: value } : parseValue(property, value)
    case 'number': {
      if (typeof value !== 'number') return null
      const number = { '=': value, '>=': value, '<=': value, '>': value + 1, '<': value - 1 }[operator as string]
      return number === undefined ? null : { kind: 'number', number }
    }
    case 'select':
    case 'status':
      return operator === 'is' ? parseValue(property, firstId(value, ctx)) : null
    case 'multi_select':
    case 'person': {
      const id = operator === 'contains' ? firstId(value, ctx) : null
      return id ? parseValue(property, [id]) : null
    }
    case 'checkbox':
      return operator === 'is' && value === true ? { kind: 'checked' } : null
    case 'date': {
      const day = typeof value === 'string' ? ruleDay(value, ctx) : null
      if (!day) return null
      const shift = { is: 0, on_or_before: 0, on_or_after: 0, before: -1, after: 1 }[operator as string]
      return shift === undefined ? null : { kind: 'date', date: { start: { kind: 'day', day: addDays(day, shift) }, end: null } }
    }
    case 'created_time':
    case 'created_by':
    case 'last_edited_time':
    case 'last_edited_by':
    case 'button':
    case 'files':
    case 'unique_id':
    case 'formula':
      return null
  }
}

function firstId(value: unknown, ctx: ViewContext): string | null {
  const first = Array.isArray(value) ? value[0] : value
  if (typeof first !== 'string' || first === '') return null
  return first === 'me' ? ctx.userId : first
}

function probeRow(property: Property, value: CellValue): Row {
  const now = new Date(0).toISOString()
  return {
    id: '',
    position: 0,
    version: 0,
    icon: null,
    has_content: false,
    created_at: now,
    created_by: '',
    updated_at: now,
    updated_by: '',
    title: property.type === 'title' && value.kind === 'text' ? value.text : '',
    values: property.type === 'title' ? {} : { [property.id]: value },
  }
}
