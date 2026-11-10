import type {
  DatabaseCalculation,
  DatabaseCalendarMode,
  DatabaseCardPreview,
  DatabaseCardSize,
  DatabaseTimelineZoom,
  DatabaseDetail,
  DatabaseFilterGroup,
  DatabaseFormConfig,
  DatabaseProperty,
  DatabasePropertyType,
  DatabaseRow,
  DatabaseViewColumn,
  DatabaseViewSort,
} from '@orbit/api/generated/types.gen'
import type { ButtonConfig } from './buttons'
import type { FormulaConfig } from './formula/values'

export type {
  DatabaseCalculation,
  DatabaseDetail,
  DatabaseFilterGroup,
  DatabaseFilterNode,
  DatabaseFilterOperator,
  DatabaseFilterRule,
  DatabaseHead,
  DatabaseProperty,
  DatabasePropertyType,
  DatabaseRow,
  DatabaseSortDirection,
  DatabaseView,
  DatabaseViewColumn,
  DatabaseViewConfig,
  DatabaseViewSort,
} from '@orbit/api/generated/types.gen'

export const OPTION_COLORS = [
  'default',
  'gray',
  'brown',
  'orange',
  'yellow',
  'green',
  'blue',
  'purple',
  'pink',
  'red',
] as const
export type OptionColor = (typeof OPTION_COLORS)[number]

export const STATUS_GROUPS = ['todo', 'in_progress', 'done'] as const
export type StatusGroup = (typeof STATUS_GROUPS)[number]

export type SelectOption = { id: string; name: string; color: OptionColor }
export type StatusOption = SelectOption & { group: StatusGroup }

export const NUMBER_FORMATS = ['number', 'number_with_commas', 'percent', 'currency'] as const
export type NumberConfig = { format: (typeof NUMBER_FORMATS)[number]; currency?: string }

/** `next` is the number the next row gets; only the server changes it. */
export type UniqueIdConfig = { prefix: string | null; next: number }

/** A file of a files value: a file of the row page. */
export type FileRef = { file_id: string; name: string }

export const DATE_FORMATS = ['relative', 'short', 'iso'] as const
export const TIME_FORMATS = ['12h', '24h'] as const
export type DateConfig = {
  date_format: (typeof DATE_FORMATS)[number]
  time_format: (typeof TIME_FORMATS)[number]
}

/** The type with its config, as the server's `PropertyKind`. */
export type PropertyKind =
  | { type: 'number'; config: NumberConfig }
  | { type: 'select' | 'multi_select'; config: { options: SelectOption[] } }
  | { type: 'status'; config: { options: StatusOption[] } }
  | { type: 'date'; config: DateConfig }
  | { type: 'button'; config: ButtonConfig }
  | { type: 'unique_id'; config: UniqueIdConfig }
  | { type: 'formula'; config: FormulaConfig }
  | {
      type: Exclude<DatabasePropertyType, 'number' | 'select' | 'multi_select' | 'status' | 'date' | 'button' | 'unique_id' | 'formula'>
      config: Record<string, never>
    }

export type Property = Omit<DatabaseProperty, 'type' | 'config'> & PropertyKind

/** A property whose groups can be a board's columns (spec P2 8.2). */
export type GroupProperty = Property & { type: 'select' | 'status' | 'multi_select' | 'person' }

/** A property whose groups can be a board's lanes: a column property or a checkbox. */
export type LaneProperty = Property & { type: GroupProperty['type'] | 'checkbox' }

/** A date (`2026-11-09`) or a UTC time (`2026-11-09T14:00:00Z`, normalized as the server stores it). */
export type DatePoint = { kind: 'day'; day: string } | { kind: 'moment'; iso: string; ms: number }
export type DateValue = { start: DatePoint; end: DatePoint | null }

/** A non-empty value, as the server's `CellValue`. */
export type CellValue =
  | { kind: 'text'; text: string }
  | { kind: 'number'; number: number }
  /** A `select` or `status` option id. */
  | { kind: 'choice'; id: string }
  /** `multi_select` option ids. */
  | { kind: 'choices'; ids: string[] }
  | { kind: 'date'; date: DateValue }
  /** User ids (`person`, `created_by`, `last_edited_by`). */
  | { kind: 'people'; ids: string[] }
  | { kind: 'checked' }
  | { kind: 'url'; url: string }
  /** Files of the row page, at most 20. */
  | { kind: 'files'; files: FileRef[] }
  /** A unique ID number (the prefix is the property's). Email and phone values are `text`. */
  | { kind: 'unique_id'; number: number }

/** A row whose stored values are parsed; a property missing from `values` is empty. */
export type Row = Omit<DatabaseRow, 'values'> & {
  values: Readonly<Record<string, CellValue>>
  /** Formula cells that failed, by property id: the message (the cell has no value). */
  formulaErrors?: Readonly<Record<string, string>>
}

export type Database = Omit<DatabaseDetail, 'properties' | 'rows'> & {
  properties: Property[]
  rows: Row[]
}

/** A view config with every default filled in and only known properties. */
export type ViewConfig = {
  columns: Required<DatabaseViewColumn>[]
  sorts: DatabaseViewSort[]
  /** AND/OR groups of rules, the root group first (spec P2 5.2). */
  filter: DatabaseFilterGroup
  wrap_cells: boolean
  /** Form: the questions and texts (spec P3 18); only form views have one (see `formConfig`). */
  form?: DatabaseFormConfig
  /** Board: the property whose groups are the columns. */
  group_by: string | null
  /** Board: group ids (option or user ids) in column order; `null` is the "No <property>" column. */
  group_order: (string | null)[]
  hidden_groups: (string | null)[]
  /** Board: the property whose groups are the lanes (a checkbox's are `"true"` and `"false"`). */
  sub_group_by: string | null
  sub_group_order: (string | null)[]
  hidden_sub_groups: (string | null)[]
  /** Board: hides the columns and lanes that have no cards. */
  hide_empty_groups: boolean
  /** Board: the properties shown on each card, after the title. */
  card_properties: string[]
  /** Table: the footer function of each property that has one. */
  calculations: Readonly<Record<string, DatabaseCalculation>>
  /** The template "New" and "+" fill new rows from (absent: an empty row). The server drops a deleted one. */
  default_template_id?: string
  /** Calendar: the `date`, `created_time` or `last_edited_time` property that places the rows (absent: none yet). */
  date_property_id?: string
  /** Calendar: absent is `month`. */
  calendar_mode?: 'week'
  /** Gallery: what a card shows above its title (absent: `content`, the body's first image or text). */
  card_preview?: CardPreview
  /** Gallery: absent is `medium`. */
  card_size?: CardSize
  /** Gallery: the preview image fits its box instead of filling it. */
  fit_image?: boolean
  /** Timeline: the date property where a bar starts (absent: none yet). */
  start_property_id?: string
  /** Timeline: the date property where a bar ends (absent: a range's end, or one day). */
  end_property_id?: string
  /** Timeline: absent is `month`. */
  zoom?: DatabaseTimelineZoom
  /** Timeline: absent is `true` (the table pane shows). */
  show_table?: boolean
  /** Timeline: the table pane's properties after the title. */
  table_columns?: string[]
}

export type CalendarMode = DatabaseCalendarMode

/** What a gallery card shows above its title: nothing, the row's cover, its body, or a files property's first image. */
export type CardPreview = DatabaseCardPreview
export type CardSize = DatabaseCardSize

/** A property whose value can place a row on a calendar. */
export type DateProperty = Property & { type: 'date' | 'created_time' | 'last_edited_time' }

export type ViewContext = {
  /** The viewer, for `"me"`. */
  userId: string
  /** Epoch milliseconds, for relative dates. */
  now: number
  /** IANA zone of the viewer: date-only values and filters compare local days in it. */
  timeZone: string
  /** BCP 47 locale for display text; `undefined` is the runtime default. */
  locale?: string
  /** Workspace members: user id to display name. */
  members: ReadonlyMap<string, string>
}
