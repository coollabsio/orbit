import { calculationsFor } from './calculate'
import { datable, defaultDateProperty } from './calendar'
import { matchesFilter, retainRules } from './filter'
import { sortRows } from './sort'
import type { Database, DatabaseView, GroupProperty, LaneProperty, Property, Row, ViewConfig, ViewContext } from './types'
import { cellOf, displayText } from './values'

const DEFAULT_WIDTH = 200
const TITLE_WIDTH = 280

/**
 * The view's config with defaults filled in. Like the server's `ViewConfig::repair`, it drops
 * columns, sorts, rules, card properties and calculations that name a deleted property (and filter groups left
 * empty), repeats,
 * calculations that do not fit their property's type, and board groups that are not groups of their
 * property; a board whose group property is gone groups by the first status, else the first select,
 * and a lane property that is gone or is the group property is dropped. A calendar's date property that is gone
 * or is not a date takes the first `date` property.
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
  const filter = config.filter ? retainRules(config.filter, (rule) => known.has(rule.property_id)) : { op: 'and' as const, children: [] }
  const kept = config.group_by != null && groupable(known.get(config.group_by)) ? config.group_by : null
  const group_by = kept ?? (view.type === 'board' ? defaultGroupBy(properties) : null)
  const groupProperty = group_by === null ? undefined : known.get(group_by)
  const groups = (list: readonly (string | null)[] | undefined) =>
    group_by !== kept || !groupProperty ? [] : [...new Set(list ?? [])].filter((id) => fitsGroup(groupProperty, id))
  const laneProperty = config.sub_group_by != null && config.sub_group_by !== group_by ? known.get(config.sub_group_by) : undefined
  const sub_group_by = laneable(laneProperty) ? laneProperty.id : null
  const lanes = (list: readonly (string | null)[] | undefined) =>
    sub_group_by === null || !laneProperty ? [] : [...new Set(list ?? [])].filter((id) => fitsGroup(laneProperty, id))
  const card_properties = [...new Set(config.card_properties ?? [])].filter((id) => {
    const property = known.get(id)
    return property !== undefined && property.type !== 'title'
  })
  const dateProperty = config.date_property_id == null ? undefined : known.get(config.date_property_id)
  const date_property_id = datable(dateProperty) ? dateProperty.id : view.type === 'calendar' ? defaultDateProperty(properties) : null
  const calculations = Object.fromEntries(
    Object.entries(config.calculations ?? {}).filter(([id, fn]) => {
      const property = known.get(id)
      return property !== undefined && calculationsFor(property.type).includes(fn)
    }),
  )
  return {
    columns,
    sorts,
    filter,
    wrap_cells: config.wrap_cells ?? false,
    group_by,
    group_order: groups(config.group_order),
    hidden_groups: groups(config.hidden_groups),
    sub_group_by,
    sub_group_order: lanes(config.sub_group_order),
    hidden_sub_groups: lanes(config.hidden_sub_groups),
    hide_empty_groups: config.hide_empty_groups ?? false,
    card_properties,
    calculations,
    ...(config.default_template_id ? { default_template_id: config.default_template_id } : {}),
    ...(date_property_id ? { date_property_id } : {}),
    ...(config.calendar_mode === 'week' ? { calendar_mode: 'week' as const } : {}),
  }
}

/** A board and a calendar show the title and `card_properties`; the other views their visible columns. */
export function showsCards(type: DatabaseView['type']): boolean {
  return type === 'board' || type === 'calendar'
}

/** A board's columns can group by a `select`, `status`, `multi_select` or `person` property. */
export function groupable(property: Property | undefined): property is GroupProperty {
  return property?.type === 'select' || property?.type === 'status' || property?.type === 'multi_select' || property?.type === 'person'
}

/** A board's lanes can group by a column property or a `checkbox`. */
export function laneable(property: Property | undefined): property is LaneProperty {
  return groupable(property) || property?.type === 'checkbox'
}

function defaultGroupBy(properties: readonly Property[]): string | null {
  return (properties.find((property) => property.type === 'status') ?? properties.find((property) => property.type === 'select'))?.id ?? null
}

/** As the server's `board::fits`: an option id or a user id, `null` for no value; a checkbox has `"true"` and `"false"`. */
function fitsGroup(property: Property, id: string | null): boolean {
  switch (property.type) {
    case 'checkbox':
      return id === 'true' || id === 'false'
    case 'select':
    case 'status':
    case 'multi_select':
      return id === null || property.config.options.some((option) => option.id === id)
    default:
      return true
  }
}

/**
 * The rows a view shows: those that pass the filter groups, then those whose title or the
 * display text of a visible column (a board: a card property) contains `search` (ignoring case),
 * in the view's sort order.
 */
export function applyView(database: Database, view: DatabaseView, search: string, ctx: ViewContext): Row[] {
  const config = repairConfig(view, database.properties)
  const byId = new Map(database.properties.map((property) => [property.id, property]))
  let rows = database.rows.filter((row) => matchesFilter(row, config.filter, byId, ctx))
  const term = search.trim().toLowerCase()
  if (term !== '') {
    const shown = (property: Property) =>
      showsCards(view.type)
        ? config.card_properties.includes(property.id)
        : config.columns.some((column) => column.property_id === property.id && !column.hidden)
    const searched = database.properties.filter((property) => property.type === 'title' || shown(property))
    rows = rows.filter((row) =>
      searched.some((property) => displayText(property, cellOf(row, property), ctx).toLowerCase().includes(term)),
    )
  }
  return sortRows(rows, config.sorts, database.properties, ctx)
}
