import { isGroup } from './filter'
import type { DatabaseFilterGroup, DatabaseFilterOperator, DatabaseFilterRule, DatabasePropertyType, OptionColor, Property, ViewContext } from './types'
import { options } from './values'

/** The name of each property type, as the type menus show it. */
export const PROPERTY_TYPE_LABELS: Record<DatabasePropertyType, string> = {
  title: 'Title',
  text: 'Text',
  number: 'Number',
  select: 'Select',
  multi_select: 'Multi-select',
  status: 'Status',
  date: 'Date',
  person: 'Person',
  checkbox: 'Checkbox',
  url: 'URL',
  email: 'Email',
  phone: 'Phone',
  files: 'Files & media',
  unique_id: 'ID',
  created_time: 'Created time',
  created_by: 'Created by',
  last_edited_time: 'Last edited time',
  last_edited_by: 'Last edited by',
  button: 'Button',
}

/** Types whose value comes from the row page: they cannot be edited, cleared or pasted into. */
export function isComputed(type: DatabasePropertyType): boolean {
  return (
    type === 'created_time' ||
    type === 'created_by' ||
    type === 'last_edited_time' ||
    type === 'last_edited_by' ||
    type === 'unique_id'
  )
}

/** Whether views can filter and sort by the type (a button has no value). */
export function sortable(type: DatabasePropertyType): boolean {
  return type !== 'button'
}

/** The filter operators of each type (spec 4.3), the first one being the default. */
export function operatorsOf(type: DatabasePropertyType): DatabaseFilterOperator[] {
  switch (type) {
    case 'title':
    case 'text':
    case 'url':
    case 'email':
    case 'phone':
      return ['contains', 'not_contains', 'equals', 'not_equals', 'starts_with', 'ends_with', 'is_empty', 'is_not_empty']
    case 'number':
    case 'unique_id':
      return ['=', '!=', '>', '<', '>=', '<=', 'is_empty', 'is_not_empty']
    case 'files':
      return ['is_not_empty', 'is_empty']
    case 'select':
    case 'status':
      return ['is', 'is_not', 'is_empty', 'is_not_empty']
    case 'multi_select':
    case 'person':
      return ['contains', 'not_contains', 'is_empty', 'is_not_empty']
    case 'checkbox':
      return ['is']
    case 'date':
    case 'created_time':
    case 'last_edited_time':
      return ['is', 'before', 'after', 'on_or_before', 'on_or_after', 'is_empty', 'is_not_empty']
    case 'created_by':
    case 'last_edited_by':
      return ['is', 'is_not']
    case 'button':
      return []
  }
}

export const OPERATOR_LABELS: Record<DatabaseFilterOperator, string> = {
  equals: 'is',
  not_equals: 'is not',
  contains: 'contains',
  not_contains: 'does not contain',
  starts_with: 'starts with',
  ends_with: 'ends with',
  is_empty: 'is empty',
  is_not_empty: 'is not empty',
  '=': '=',
  '!=': '≠',
  '>': '>',
  '<': '<',
  '>=': '≥',
  '<=': '≤',
  is: 'is',
  is_not: 'is not',
  before: 'is before',
  after: 'is after',
  on_or_before: 'is on or before',
  on_or_after: 'is on or after',
}

export const RELATIVE_DATES: { value: string; label: string }[] = [
  { value: 'today', label: 'Today' },
  { value: 'tomorrow', label: 'Tomorrow' },
  { value: 'yesterday', label: 'Yesterday' },
  { value: 'one_week_ago', label: 'One week ago' },
  { value: 'one_week_from_now', label: 'One week from now' },
  { value: 'one_month_ago', label: 'One month ago' },
  { value: 'one_month_from_now', label: 'One month from now' },
]

export const OPTION_COLOR_LABELS: Record<OptionColor, string> = {
  default: 'Default',
  gray: 'Gray',
  brown: 'Brown',
  orange: 'Orange',
  yellow: 'Yellow',
  green: 'Green',
  blue: 'Blue',
  purple: 'Purple',
  pink: 'Pink',
  red: 'Red',
}

/**
 * The rules of a filter the bar shows as chips (spec P2 5.4): an `and` root with only rules. `null` for a filter with an
 * `or` or a nested group, which the advanced filter edits.
 */
export function simpleRules(filter: DatabaseFilterGroup): DatabaseFilterRule[] | null {
  if (filter.op !== 'and' && filter.children.length > 1) return null
  const rules = filter.children.flatMap((child) => (isGroup(child) ? [] : [child]))
  return rules.length === filter.children.length ? rules : null
}

/** The filter of a rule list: every rule must match. */
export function allOf(rules: DatabaseFilterRule[]): DatabaseFilterGroup {
  return { op: 'and', children: rules }
}

/** A new filter rule on a property: its type's first operator and no value, which matches every row. */
export function newRule(property: Property): DatabaseFilterRule {
  return { property_id: property.id, operator: operatorsOf(property.type)[0], value: null }
}

/** Whether a rule leaves rows out: an operator that needs no value, or a value. A rule without one matches every row. */
export function ruleNeedsValue(operator: DatabaseFilterOperator): boolean {
  return operator !== 'is_empty' && operator !== 'is_not_empty'
}

/** The chip text after the property name (spec 7.1): `Reading`, `contains “abc”`, `is empty`. */
export function ruleSummary(rule: DatabaseFilterRule, property: Property, ctx: ViewContext): string {
  const operator = OPERATOR_LABELS[rule.operator]
  if (!ruleNeedsValue(rule.operator)) return operator
  const value = ruleValueText(rule, property, ctx)
  if (value === null) return 'All'
  if (property.type === 'checkbox') return value
  if (Array.isArray(rule.value)) return rule.operator === 'is' || rule.operator === 'contains' ? value : `${operator} ${value}`
  return `${operator} ${typeof rule.value === 'string' && !isDateValue(rule.value) ? `“${value}”` : value}`
}

/** A rule's value as text: option and member names, `Checked`, a relative date's label or the date. `null` without a value. */
export function ruleValueText(rule: DatabaseFilterRule, property: Property, ctx: ViewContext): string | null {
  const value = rule.value
  if (value === null || value === undefined || value === '' || (Array.isArray(value) && value.length === 0)) return null
  if (property.type === 'checkbox') return value === true ? 'Checked' : 'Unchecked'
  if (Array.isArray(value)) {
    const names = value.map((id) =>
      id === 'me' ? 'Me' : typeof id !== 'string' ? '' : options(property).find((option) => option.id === id)?.name ?? ctx.members.get(id) ?? '',
    )
    return names.filter((name) => name !== '').join(', ')
  }
  if (typeof value === 'string') return RELATIVE_DATES.find((date) => date.value === value)?.label ?? (isDateValue(value) ? value.slice(0, 10) : value)
  return String(value)
}

function isDateValue(value: string): boolean {
  return RELATIVE_DATES.some((date) => date.value === value) || /^\d{4}-\d{2}-\d{2}/.test(value)
}
