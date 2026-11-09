import type { Property, Row, SelectOption, StatusOption, ViewConfig } from './types'
import { cellOf } from './values'

export type GroupProperty = Property & { type: 'select' | 'status' }

/** One board column: an option of the group property, or `id: null` for "No <property>". */
export type BoardGroup = {
  id: string | null
  option: SelectOption | StatusOption | null
  rows: Row[]
  hidden: boolean
}

/**
 * The board columns of `rows` (already filtered and sorted by `applyView`), grouped by `property`.
 * Columns follow `group_order`, then the remaining options in option order; the "No <property>"
 * column comes first unless `group_order` places it. A status is never empty (a row with no
 * status sits in its first `todo` option, as the table shows it), so a status board has no such
 * column. A value naming a deleted option counts as no value. Rows keep their order in each
 * column; hidden columns stay in the list with `hidden: true`.
 */
export function groupRows(rows: readonly Row[], property: GroupProperty, config: Pick<ViewConfig, 'group_order' | 'hidden_groups'>): BoardGroup[] {
  const options: readonly (SelectOption | StatusOption)[] = property.config.options
  const byId = new Map(options.map((option) => [option.id, option]))
  const placed = config.group_order.filter((id) => (id === null ? property.type === 'select' : byId.has(id)))
  const ids = [
    ...(property.type === 'select' && !placed.includes(null) ? [null] : []),
    ...placed,
    ...options.filter((option) => !placed.includes(option.id)).map((option) => option.id),
  ]
  const groups = new Map<string | null, BoardGroup>(
    ids.map((id) => [
      id,
      { id, option: id === null ? null : (byId.get(id) ?? null), rows: [], hidden: config.hidden_groups.includes(id) },
    ]),
  )
  for (const row of rows) {
    const value = cellOf(row, property)
    const id = value?.kind === 'choice' && byId.has(value.id) ? value.id : null
    groups.get(id)?.rows.push(row)
  }
  return [...groups.values()]
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
