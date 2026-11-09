import {
  ArrowCircleDown,
  Calendar,
  CheckSquare,
  Clock,
  Hashtag,
  Link,
  List,
  RecordCircle,
  Text,
  TextalignJustifyleft,
  User,
  UserCircle,
  type IconComponent,
} from 'reicon-react'
import type { DatabaseFilterOperator, DatabasePropertyType, OptionColor } from '@orbit/database/types'

export const PROPERTY_TYPES: Record<DatabasePropertyType, { label: string; icon: IconComponent }> = {
  title: { label: 'Title', icon: Text },
  text: { label: 'Text', icon: TextalignJustifyleft },
  number: { label: 'Number', icon: Hashtag },
  select: { label: 'Select', icon: ArrowCircleDown },
  multi_select: { label: 'Multi-select', icon: List },
  status: { label: 'Status', icon: RecordCircle },
  date: { label: 'Date', icon: Calendar },
  person: { label: 'Person', icon: User },
  checkbox: { label: 'Checkbox', icon: CheckSquare },
  url: { label: 'URL', icon: Link },
  created_time: { label: 'Created time', icon: Clock },
  created_by: { label: 'Created by', icon: UserCircle },
  last_edited_time: { label: 'Last edited time', icon: Clock },
  last_edited_by: { label: 'Last edited by', icon: UserCircle },
}

/** The types a user can give a property, in menu order (a database has exactly one title). */
export const CREATABLE_TYPES: DatabasePropertyType[] = [
  'text',
  'number',
  'select',
  'multi_select',
  'status',
  'date',
  'person',
  'checkbox',
  'url',
  'created_time',
  'created_by',
  'last_edited_time',
  'last_edited_by',
]

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
