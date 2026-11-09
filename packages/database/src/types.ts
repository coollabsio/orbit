import type {
  DatabaseDetail,
  DatabaseFilterRule,
  DatabaseProperty,
  DatabasePropertyType,
  DatabaseRow,
  DatabaseViewColumn,
  DatabaseViewSort,
} from '@orbit/api/generated/types.gen'

export type {
  DatabaseDetail,
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
  | {
      type: Exclude<DatabasePropertyType, 'number' | 'select' | 'multi_select' | 'status' | 'date'>
      config: Record<string, never>
    }

export type Property = Omit<DatabaseProperty, 'type' | 'config'> & PropertyKind

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

/** A row whose stored values are parsed; a property missing from `values` is empty. */
export type Row = Omit<DatabaseRow, 'values'> & { values: Readonly<Record<string, CellValue>> }

export type Database = Omit<DatabaseDetail, 'properties' | 'rows'> & {
  properties: Property[]
  rows: Row[]
}

/** A view config with every default filled in and only known properties. */
export type ViewConfig = {
  columns: Required<DatabaseViewColumn>[]
  sorts: DatabaseViewSort[]
  filter: { rules: DatabaseFilterRule[] }
  wrap_cells: boolean
  /** Board: the `select` or `status` property whose options are the columns. */
  group_by: string | null
  /** Board: option ids in column order; `null` is the "No <property>" column. */
  group_order: (string | null)[]
  hidden_groups: (string | null)[]
  /** Board: the properties shown on each card, after the title. */
  card_properties: string[]
}

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
