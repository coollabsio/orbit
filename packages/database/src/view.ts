import { matches } from './filter'
import { sortRows } from './sort'
import type { Database, DatabaseView, Property, Row, ViewConfig, ViewContext } from './types'
import { cellOf, displayText } from './values'

const DEFAULT_WIDTH = 200
const TITLE_WIDTH = 280

/**
 * The view's config with defaults filled in. Like the server's `ViewConfig::repair`, it drops
 * columns, sorts, rules and card properties that name a deleted property, repeats, and groups that
 * are not options of the group property; a board whose group property is gone groups by the first
 * status, else the first select. Then it appends each property missing from `columns` as a visible
 * column, in property order.
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
  const kept = config.group_by != null && groupable(known.get(config.group_by)) ? config.group_by : null
  const group_by = kept ?? (view.type === 'board' ? defaultGroupBy(properties) : null)
  const groupProperty = group_by === null ? undefined : known.get(group_by)
  const groups = (list: readonly (string | null)[] | undefined) =>
    group_by !== kept || !groupProperty ? [] : [...new Set(list ?? [])].filter((id) => id === null || hasOption(groupProperty, id))
  const card_properties = [...new Set(config.card_properties ?? [])].filter((id) => {
    const property = known.get(id)
    return property !== undefined && property.type !== 'title'
  })
  return {
    columns,
    sorts,
    filter: { rules },
    wrap_cells: config.wrap_cells ?? false,
    group_by,
    group_order: groups(config.group_order),
    hidden_groups: groups(config.hidden_groups),
    card_properties,
  }
}

/** A board can group by a `select` or `status` property. */
export function groupable(property: Property | undefined): boolean {
  return property?.type === 'select' || property?.type === 'status'
}

function defaultGroupBy(properties: readonly Property[]): string | null {
  return (properties.find((property) => property.type === 'status') ?? properties.find((property) => property.type === 'select'))?.id ?? null
}

function hasOption(property: Property | undefined, id: string): boolean {
  return (property?.type === 'select' || property?.type === 'status') && property.config.options.some((option) => option.id === id)
}

/**
 * The rows a view shows: those that pass every filter rule, then those whose title or the
 * display text of a visible column (a board: a card property) contains `search` (ignoring case),
 * in the view's sort order.
 */
export function applyView(database: Database, view: DatabaseView, search: string, ctx: ViewContext): Row[] {
  const config = repairConfig(view, database.properties)
  const byId = new Map(database.properties.map((property) => [property.id, property]))
  const rules = config.filter.rules.map((rule) => ({ rule, property: byId.get(rule.property_id)! }))
  let rows = database.rows.filter((row) => rules.every(({ rule, property }) => matches(row, rule, property, ctx)))
  const term = search.trim().toLowerCase()
  if (term !== '') {
    const shown = (property: Property) =>
      view.type === 'board'
        ? config.card_properties.includes(property.id)
        : config.columns.some((column) => column.property_id === property.id && !column.hidden)
    const searched = database.properties.filter((property) => property.type === 'title' || shown(property))
    rows = rows.filter((row) =>
      searched.some((property) => displayText(property, cellOf(row, property), ctx).toLowerCase().includes(term)),
    )
  }
  return sortRows(rows, config.sorts, database.properties, ctx)
}
