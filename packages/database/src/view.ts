import { matches } from './filter'
import { sortRows } from './sort'
import type { Database, DatabaseView, Property, Row, ViewConfig, ViewContext } from './types'
import { cellOf, displayText } from './values'

const DEFAULT_WIDTH = 200
const TITLE_WIDTH = 280

/**
 * The view's config with defaults filled in. Like the server's `ViewConfig::retain_known`, it
 * drops columns, sorts and rules that name a deleted property, and repeated columns and sorts.
 * Then it appends each property missing from `columns` as a visible column, in property order.
 */
export function repairConfig(view: DatabaseView, properties: readonly Property[]): ViewConfig {
  const config = view.config
  const known = new Map(properties.map((property) => [property.id, property]))
  const seenColumns = new Set<string>()
  const columns = (config.columns ?? []).flatMap((column) => {
    if (!known.has(column.property_id) || seenColumns.has(column.property_id)) return []
    seenColumns.add(column.property_id)
    return [{ property_id: column.property_id, width: column.width ?? DEFAULT_WIDTH, hidden: column.hidden ?? false }]
  })
  for (const property of [...properties].sort((a, b) => a.position - b.position)) {
    if (seenColumns.has(property.id)) continue
    columns.push({
      property_id: property.id,
      width: property.type === 'title' ? TITLE_WIDTH : DEFAULT_WIDTH,
      hidden: false,
    })
  }
  const seenSorts = new Set<string>()
  const sorts = (config.sorts ?? []).filter((sort) => {
    if (!known.has(sort.property_id) || seenSorts.has(sort.property_id)) return false
    seenSorts.add(sort.property_id)
    return true
  })
  const rules = (config.filter?.rules ?? []).filter((rule) => known.has(rule.property_id))
  return { columns, sorts, filter: { rules }, wrap_cells: config.wrap_cells ?? false }
}

/**
 * The rows a view shows: those that pass every filter rule, then those whose title or the
 * display text of a visible column contains `search` (ignoring case), in the view's sort order.
 */
export function applyView(database: Database, view: DatabaseView, search: string, ctx: ViewContext): Row[] {
  const config = repairConfig(view, database.properties)
  const byId = new Map(database.properties.map((property) => [property.id, property]))
  const rules = config.filter.rules.map((rule) => ({ rule, property: byId.get(rule.property_id)! }))
  let rows = database.rows.filter((row) => rules.every(({ rule, property }) => matches(row, rule, property, ctx)))
  const term = search.trim().toLowerCase()
  if (term !== '') {
    const searched = database.properties.filter(
      (property) =>
        property.type === 'title' ||
        config.columns.some((column) => column.property_id === property.id && !column.hidden),
    )
    rows = rows.filter((row) =>
      searched.some((property) => displayText(property, cellOf(row, property), ctx).toLowerCase().includes(term)),
    )
  }
  return sortRows(rows, config.sorts, database.properties, ctx)
}
