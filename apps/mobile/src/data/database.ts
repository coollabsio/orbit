/**
 * What the database screens decide on a phone, as plain data: which view is open, which properties a row of the list
 * shows, and what a board card move sends. The view logic itself (filter, sort, groups) is `@orbit/database`.
 */
import { CALCULATION_LABELS, calculate } from '@orbit/database/calculate'
import type { DatabasePropertyType, DatabaseView, GroupProperty, OptionColor, Property, Row, ViewConfig, ViewContext } from '@orbit/database/types'
import { cellOf } from '@orbit/database/values'

import type { IconName } from '../ui'

/** The web's type icons (`propertyMeta.ts`). */
export const PROPERTY_ICONS: Record<DatabasePropertyType, IconName> = {
  title: 'text',
  text: 'textalignJustifyleft',
  number: 'hashtag',
  select: 'arrowCircleDown',
  multi_select: 'list',
  status: 'circle',
  date: 'calendar',
  person: 'user',
  checkbox: 'checkSquare',
  url: 'link',
  created_time: 'clock',
  created_by: 'userCircle',
  last_edited_time: 'clock',
  last_edited_by: 'userCircle',
}

export const VIEW_ICONS: Record<DatabaseView['type'], IconName> = { table: 'grid', board: 'kanban' }

/** Where the open view of a database, or of one linked view, is kept (a secure-store key: letters, digits, `.`, `-` and `_` only). */
export const viewStorageKey = (databaseId: string, linkId = '') => `orbit.database-view.${databaseId}${linkId ? `.${linkId}` : ''}`

/** The views in tab order. */
export function sortedViews(views: readonly DatabaseView[]): DatabaseView[] {
  return [...views].sort((a, b) => a.position - b.position)
}

/** The view to show: the chosen one while it exists, else the first. */
export function pickView(views: readonly DatabaseView[], chosen: string | null): DatabaseView | undefined {
  const sorted = sortedViews(views)
  return sorted.find((view) => view.id === chosen) ?? sorted[0]
}

/** How many properties a row of the phone list shows after its title (spec 8). */
export const LIST_PROPERTIES = 3

/** The first visible properties of a table view after the title, in column order: the list shows them as chips. */
export function listProperties(properties: readonly Property[], config: ViewConfig): Property[] {
  const byId = new Map(properties.map((property) => [property.id, property]))
  return config.columns
    .flatMap((column) => {
      const property = byId.get(column.property_id)
      return property && !column.hidden && property.type !== 'title' ? [property] : []
    })
    .slice(0, LIST_PROPERTIES)
}

/**
 * The web footer's calculations as read-only lines (spec P2 4.6): `<property> · <label> <value>`, in column order, for
 * the visible columns that have one, over the view's rows.
 */
export function summaryLines(properties: readonly Property[], config: ViewConfig, rows: readonly Row[], ctx: ViewContext): { id: string; text: string }[] {
  const byId = new Map(properties.map((property) => [property.id, property]))
  return config.columns.flatMap((column) => {
    const property = byId.get(column.property_id)
    const fn = config.calculations[column.property_id]
    if (!property || column.hidden || !fn) return []
    return [{ id: property.id, text: `${property.name} · ${CALCULATION_LABELS[fn].short} ${calculate(rows, property, fn, ctx).value}` }]
  })
}

/** Of `shown`, the properties the row has a value for: an empty one leaves no gap. */
export function filledProperties(row: Row, shown: readonly Property[]): Property[] {
  return shown.filter((property) => cellOf(row, property) !== null)
}

/** The values a card moved to another board column gets: the column's option, or none for "No <property>". */
export function groupValues(property: GroupProperty, groupId: string | null): Record<string, unknown> {
  return { [property.id]: groupId }
}

/** The web's option pill colors (`cells/Pill.tsx`), one class set per scheme: Uniwind's `dark:` does not override. */
export const PILL_COLORS: Record<'light' | 'dark', Record<OptionColor, { box: string; text: string; dot: string }>> = {
  light: {
    default: { box: 'bg-foreground/7', text: 'text-foreground', dot: 'bg-foreground' },
    gray: { box: 'bg-neutral-500/15', text: 'text-neutral-700', dot: 'bg-neutral-700' },
    brown: { box: 'bg-amber-900/15', text: 'text-amber-900', dot: 'bg-amber-900' },
    orange: { box: 'bg-orange-500/15', text: 'text-orange-700', dot: 'bg-orange-700' },
    yellow: { box: 'bg-yellow-500/20', text: 'text-yellow-800', dot: 'bg-yellow-800' },
    green: { box: 'bg-green-500/15', text: 'text-green-700', dot: 'bg-green-700' },
    blue: { box: 'bg-blue-500/15', text: 'text-blue-700', dot: 'bg-blue-700' },
    purple: { box: 'bg-purple-500/15', text: 'text-purple-700', dot: 'bg-purple-700' },
    pink: { box: 'bg-pink-500/15', text: 'text-pink-700', dot: 'bg-pink-700' },
    red: { box: 'bg-red-500/15', text: 'text-red-700', dot: 'bg-red-700' },
  },
  dark: {
    default: { box: 'bg-foreground/7', text: 'text-foreground', dot: 'bg-foreground' },
    gray: { box: 'bg-neutral-500/15', text: 'text-neutral-300', dot: 'bg-neutral-300' },
    brown: { box: 'bg-amber-700/20', text: 'text-amber-200', dot: 'bg-amber-200' },
    orange: { box: 'bg-orange-500/15', text: 'text-orange-300', dot: 'bg-orange-300' },
    yellow: { box: 'bg-yellow-500/20', text: 'text-yellow-200', dot: 'bg-yellow-200' },
    green: { box: 'bg-green-500/15', text: 'text-green-300', dot: 'bg-green-300' },
    blue: { box: 'bg-blue-500/15', text: 'text-blue-300', dot: 'bg-blue-300' },
    purple: { box: 'bg-purple-500/15', text: 'text-purple-300', dot: 'bg-purple-300' },
    pink: { box: 'bg-pink-500/15', text: 'text-pink-300', dot: 'bg-pink-300' },
    red: { box: 'bg-red-500/15', text: 'text-red-300', dot: 'bg-red-300' },
  },
}

/** A typed `HH:MM` (24 h), or `null`. */
export function parseTime(text: string): string | null {
  const match = /^(\d{1,2}):(\d{2})$/.exec(text.trim())
  if (!match) return null
  const hours = Number(match[1])
  const minutes = Number(match[2])
  return hours < 24 && minutes < 60 ? `${String(hours).padStart(2, '0')}:${match[2]}` : null
}
