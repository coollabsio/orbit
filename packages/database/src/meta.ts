import type { DatabaseFilterOperator, DatabaseFilterRule, DatabasePropertyType, OptionColor, Property, ViewContext } from './types'
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
  created_time: 'Created time',
  created_by: 'Created by',
  last_edited_time: 'Last edited time',
  last_edited_by: 'Last edited by',
}

/** Types whose value comes from the row page: they cannot be edited, cleared or pasted into. */
export function isComputed(type: DatabasePropertyType): boolean {
  return type === 'created_time' || type === 'created_by' || type === 'last_edited_time' || type === 'last_edited_by'
}

/** The filter operators of each type (spec 4.3), the first one being the default. */
export function operatorsOf(type: DatabasePropertyType): DatabaseFilterOperator[] {
  switch (type) {
    case 'title':
    case 'text':
    case 'url':
      return ['contains', 'not_contains', 'equals', 'not_equals', 'starts_with', 'ends_with', 'is_empty', 'is_not_empty']
    case 'number':
      return ['=', '!=', '>', '<', '>=', '<=', 'is_empty', 'is_not_empty']
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
