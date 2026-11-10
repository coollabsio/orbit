import type { GroupProperty, LaneProperty, Row, SelectOption, StatusOption, ViewConfig } from './types'
import { cellOf } from './values'

/**
 * One board column or lane: a group of the property (an option id, a user id, `"true"` or `"false"`), or
 * `id: null` for "No <property>".
 */
export type BoardGroup = {
  id: string | null
  /** The option of a select, status or multi-select group; `null` for a person, a checkbox and "No <property>". */
  option: SelectOption | StatusOption | null
  rows: Row[]
  hidden: boolean
}

/** A lane: its group, and the board's columns with only the lane's rows. */
export type BoardLane = BoardGroup & { columns: BoardGroup[] }

type GroupConfig = Pick<ViewConfig, 'group_order' | 'hidden_groups'> & Partial<Pick<ViewConfig, 'hide_empty_groups'>>
type Members = ReadonlyMap<string, string>
const NO_MEMBERS: Members = new Map()
const CHECKBOX_GROUPS = ['false', 'true']

/**
 * The groups a row belongs to. A select or status has one (a row with no status sits in its first `todo` option,
 * as the table shows it); a multi-select or person has one per value, so the card shows in each column. A value
 * naming a deleted option counts as no value. A checkbox is `"true"` or `"false"`.
 */
export function groupIdsOf(row: Row, property: LaneProperty): (string | null)[] {
  const value = cellOf(row, property)
  switch (property.type) {
    case 'checkbox':
      return [value === null ? 'false' : 'true']
    case 'person':
      return value?.kind === 'people' && value.ids.length > 0 ? value.ids : [null]
    case 'select':
    case 'status':
    case 'multi_select': {
      const ids = value?.kind === 'choice' ? [value.id] : value?.kind === 'choices' ? value.ids : []
      const known = ids.filter((id) => property.config.options.some((option) => option.id === id))
      return known.length > 0 ? known : [null]
    }
  }
}

/**
 * The groups of `property` in order: `order` first, then the rest. A select or multi-select follows its option
 * order, with "No <property>" first; a status has no such group. A person lists the other people the rows name, by
 * name (`members`: user id to name; unknown users last), then "No <property>". A checkbox is unchecked, then checked.
 */
function groupsOf(rows: readonly Row[], property: LaneProperty, order: readonly (string | null)[], hidden: readonly (string | null)[], hideEmpty: boolean, members: Members): BoardGroup[] {
  const options: readonly (SelectOption | StatusOption)[] = property.type === 'select' || property.type === 'status' || property.type === 'multi_select' ? property.config.options : []
  const byId = new Map(options.map((option) => [option.id, option]))
  const rest = (() => {
    switch (property.type) {
      case 'checkbox':
        return CHECKBOX_GROUPS
      case 'person': {
        const named = new Set(rows.flatMap((row) => groupIdsOf(row, property)).filter((id): id is string => id !== null))
        const name = (id: string) => members.get(id)
        const people = [...named].sort((a, b) => Number(name(a) === undefined) - Number(name(b) === undefined) || (name(a) ?? '').localeCompare(name(b) ?? '') || a.localeCompare(b))
        return [...people, null]
      }
      case 'status':
        return options.map((option) => option.id)
      case 'select':
      case 'multi_select':
        return [null, ...options.map((option) => option.id)]
    }
  })()
  const valid = new Set(rest)
  const placed = order.filter((id) => valid.has(id) || (property.type === 'person' && id !== null))
  const ids = [...new Set([...placed, ...rest])]
  const groups = new Map<string | null, BoardGroup>(
    ids.map((id) => [id, { id, option: id === null ? null : (byId.get(id) ?? null), rows: [], hidden: hidden.includes(id) }]),
  )
  for (const row of rows) {
    for (const id of groupIdsOf(row, property)) groups.get(id)?.rows.push(row)
  }
  const all = [...groups.values()]
  return hideEmpty ? all.filter((group) => group.rows.length > 0) : all
}

/**
 * The board columns of `rows` (already filtered and sorted by `applyView`), grouped by `property` in `group_order`,
 * then the property's other groups (see `groupsOf`). A row with several values is a card in each of their columns,
 * and a column's rows are its cards. Rows keep their order in each column; hidden columns stay in the list with
 * `hidden: true`; `hide_empty_groups` leaves out the columns with no cards.
 */
export function groupRows(rows: readonly Row[], property: GroupProperty, config: GroupConfig, members: Members = NO_MEMBERS): BoardGroup[] {
  return groupsOf(rows, property, config.group_order, config.hidden_groups, config.hide_empty_groups ?? false, members)
}

/**
 * The lanes of a board with sub-groups: the groups of `laneProperty` in `sub_group_order` (as `groupRows`), each with
 * the board's columns (`groupRows` of all rows, so every lane has the same columns) holding only the lane's cards.
 * `hide_empty_groups` leaves out the lanes with no cards.
 */
export function laneRows(
  rows: readonly Row[],
  property: GroupProperty,
  laneProperty: LaneProperty,
  config: GroupConfig & Pick<ViewConfig, 'sub_group_order' | 'hidden_sub_groups'>,
  members: Members = NO_MEMBERS,
): BoardLane[] {
  const columns = groupRows(rows, property, config, members)
  return groupsOf(rows, laneProperty, config.sub_group_order, config.hidden_sub_groups, config.hide_empty_groups ?? false, members).map((lane) => {
    const inLane = new Set(lane.rows.map((row) => row.id))
    return { ...lane, columns: columns.map((column) => ({ ...column, rows: column.rows.filter((row) => inLane.has(row.id)) })) }
  })
}

/**
 * The value patch that moves the card of `row` from group `from` to group `to` of `property` (spec P2 8.3), for
 * `PATCH …/values` or the `values` of `POST …/rows/{id}/move`. A multi-select or person swaps `from` for `to` and
 * keeps its other values; a drop on "No <property>" clears the value. The same group is no patch.
 */
export function moveValues(row: Row, property: LaneProperty, from: string | null, to: string | null): Record<string, unknown> {
  if (from === to) return {}
  switch (property.type) {
    case 'checkbox':
      return { [property.id]: to === 'true' }
    case 'select':
    case 'status':
      return { [property.id]: to }
    case 'multi_select':
    case 'person': {
      if (to === null) return { [property.id]: null }
      const kept = groupIdsOf(row, property).filter((id): id is string => id !== null && id !== from)
      return { [property.id]: kept.includes(to) ? kept : [...kept, to] }
    }
  }
}

/** The values of a new card in group `id` of `property` (a column, or a lane). */
export function groupValues(property: LaneProperty, id: string | null): Record<string, unknown> {
  switch (property.type) {
    case 'checkbox':
      return { [property.id]: id === 'true' }
    case 'select':
    case 'status':
      return { [property.id]: id }
    case 'multi_select':
    case 'person':
      return { [property.id]: id === null ? null : [id] }
  }
}

/** The name of a group: its option, its member (`members`: user id to name), "Checked" or "Unchecked", or "No <property>". */
export function groupName(property: LaneProperty, group: Pick<BoardGroup, 'id' | 'option'>, members: Members): string {
  if (group.id === null) return `No ${property.name}`
  switch (property.type) {
    case 'checkbox':
      return group.id === 'true' ? 'Checked' : 'Unchecked'
    case 'person':
      return members.get(group.id) ?? 'Former member'
    default:
      return group.option?.name ?? ''
  }
}

/**
 * The `after_row_id` of `POST …/rows/{id}/move` that puts the card `cardId` at `index` in a
 * column. `ordered` is every row in manual order; `column` is the target column's rows without
 * the card. At the top of a column the card goes right before the column's first row; in an
 * empty column it keeps its place.
 */
export function cardAfter(ordered: readonly Row[], column: readonly Row[], index: number, cardId: string): string | null {
  const before = column[index - 1]
  if (before) return before.id
  const others = ordered.filter((row) => row.id !== cardId)
  const anchor = column[0]
  const at = anchor ? others.findIndex((row) => row.id === anchor.id) : ordered.findIndex((row) => row.id === cardId)
  return others[at - 1]?.id ?? null
}
