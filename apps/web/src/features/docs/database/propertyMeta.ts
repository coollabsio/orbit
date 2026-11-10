import {
  ArrowCircleDown,
  Calendar,
  CheckSquare,
  Flash,
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
import { PROPERTY_TYPE_LABELS } from '@orbit/database/meta'
import type { DatabasePropertyType } from '@orbit/database/types'

export const PROPERTY_TYPES: Record<DatabasePropertyType, { label: string; icon: IconComponent }> = {
  title: { label: PROPERTY_TYPE_LABELS.title, icon: Text },
  text: { label: PROPERTY_TYPE_LABELS.text, icon: TextalignJustifyleft },
  number: { label: PROPERTY_TYPE_LABELS.number, icon: Hashtag },
  select: { label: PROPERTY_TYPE_LABELS.select, icon: ArrowCircleDown },
  multi_select: { label: PROPERTY_TYPE_LABELS.multi_select, icon: List },
  status: { label: PROPERTY_TYPE_LABELS.status, icon: RecordCircle },
  date: { label: PROPERTY_TYPE_LABELS.date, icon: Calendar },
  person: { label: PROPERTY_TYPE_LABELS.person, icon: User },
  checkbox: { label: PROPERTY_TYPE_LABELS.checkbox, icon: CheckSquare },
  url: { label: PROPERTY_TYPE_LABELS.url, icon: Link },
  created_time: { label: PROPERTY_TYPE_LABELS.created_time, icon: Clock },
  created_by: { label: PROPERTY_TYPE_LABELS.created_by, icon: UserCircle },
  last_edited_time: { label: PROPERTY_TYPE_LABELS.last_edited_time, icon: Clock },
  last_edited_by: { label: PROPERTY_TYPE_LABELS.last_edited_by, icon: UserCircle },
  button: { label: PROPERTY_TYPE_LABELS.button, icon: Flash },
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
  'button',
]
