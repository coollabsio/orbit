import type { Database, Property, Row, ViewContext } from './types'

/** A relation's config (spec P3 11.2): the target database, the link limit and the two-way pair. */
export type RelationConfig = {
  database_id: string
  limit: 'none' | 'one'
  /** The paired property in the target database (two-way); `null` for a one-way relation. */
  synced_property_id: string | null
  /** The side the links are stored under; a synced (non-primary) side may only change `limit`. */
  primary: boolean
}

/** Links a write may give one relation cell (spec P3 11.3); `limit: "one"` allows one. */
export const MAX_RELATION_LINKS = 100

export const ROLLUP_FUNCTIONS = [
  'show_original',
  'show_unique',
  'count_all',
  'count_values',
  'count_unique',
  'count_empty',
  'count_not_empty',
  'percent_empty',
  'percent_not_empty',
  'sum',
  'average',
  'median',
  'min',
  'max',
  'range',
  'earliest_date',
  'latest_date',
  'date_range',
  'checked',
  'unchecked',
  'percent_checked',
  'percent_unchecked',
] as const
export type RollupFunction = (typeof ROLLUP_FUNCTIONS)[number]

export type RollupConfig = { relation_property_id: string; target_property_id: string; function: RollupFunction }

export type RelationProperty = Property & { type: 'relation' }
export type RollupProperty = Property & { type: 'rollup' }

export const ROLLUP_FUNCTION_LABELS: Record<RollupFunction, string> = {
  show_original: 'Show original',
  show_unique: 'Show unique values',
  count_all: 'Count all',
  count_values: 'Count values',
  count_unique: 'Count unique values',
  count_empty: 'Count empty',
  count_not_empty: 'Count not empty',
  percent_empty: 'Percent empty',
  percent_not_empty: 'Percent not empty',
  sum: 'Sum',
  average: 'Average',
  median: 'Median',
  min: 'Min',
  max: 'Max',
  range: 'Range',
  earliest_date: 'Earliest date',
  latest_date: 'Latest date',
  date_range: 'Date range',
  checked: 'Checked',
  unchecked: 'Unchecked',
  percent_checked: 'Percent checked',
  percent_unchecked: 'Percent unchecked',
}

/** What a rollup's value is: a number (a percent is a fraction 0..1), a date value, or a list of display texts. */
export type RollupResult = 'number' | 'percent' | 'date' | 'list'

export function rollupResult(fn: RollupFunction): RollupResult {
  switch (fn) {
    case 'show_original':
    case 'show_unique':
      return 'list'
    case 'percent_empty':
    case 'percent_not_empty':
    case 'percent_checked':
    case 'percent_unchecked':
      return 'percent'
    case 'earliest_date':
    case 'latest_date':
    case 'date_range':
      return 'date'
    default:
      return 'number'
  }
}

type TargetClass = 'number' | 'date' | 'checkbox' | 'other' | 'any'

/** As the server's `class_of`: a rollup target counts as its result, a formula fits every function. */
function targetClass(target: Property): TargetClass {
  switch (target.type) {
    case 'number':
    case 'unique_id':
      return 'number'
    case 'date':
    case 'created_time':
    case 'last_edited_time':
      return 'date'
    case 'checkbox':
      return 'checkbox'
    case 'formula':
      return 'any'
    case 'rollup': {
      const result = rollupResult(target.config.function)
      return result === 'date' ? 'date' : result === 'list' ? 'other' : 'number'
    }
    default:
      return 'other'
  }
}

/** Whether `fn` can roll up `target` (the server's `RollupFunction::fits`, else 422 `config.function`). */
export function rollupFits(fn: RollupFunction, target: Property): boolean {
  const needs: TargetClass | null = ['sum', 'average', 'median', 'min', 'max', 'range'].includes(fn)
    ? 'number'
    : ['earliest_date', 'latest_date', 'date_range'].includes(fn)
      ? 'date'
      : ['checked', 'unchecked', 'percent_checked', 'percent_unchecked'].includes(fn)
        ? 'checkbox'
        : null
  if (needs === null) return true
  const actual = targetClass(target)
  return actual === needs || actual === 'any'
}

/** The functions a target property can take, in menu order. */
export function rollupFunctionsFor(target: Property): RollupFunction[] {
  return ROLLUP_FUNCTIONS.filter((fn) => rollupFits(fn, target))
}

/** Whether a rollup can target the property: buttons have no value. */
export function rollupTargetable(property: Property): boolean {
  return property.type !== 'button'
}

export function parseRelationConfig(config: Record<string, unknown>): RelationConfig {
  return {
    database_id: typeof config.database_id === 'string' ? config.database_id : '',
    limit: config.limit === 'one' ? 'one' : 'none',
    synced_property_id: typeof config.synced_property_id === 'string' ? config.synced_property_id : null,
    primary: config.primary !== false,
  }
}

export function parseRollupConfig(config: Record<string, unknown>): RollupConfig {
  return {
    relation_property_id: typeof config.relation_property_id === 'string' ? config.relation_property_id : '',
    target_property_id: typeof config.target_property_id === 'string' ? config.target_property_id : '',
    function: ROLLUP_FUNCTIONS.includes(config.function as RollupFunction) ? (config.function as RollupFunction) : 'show_original',
  }
}

/**
 * The stored-type property a rollup's result reads as, with the rollup's id, as a formula's base: number results
 * format, filter, sort and calculate as a number (percent results in percent format), date results as a date, lists
 * as text over their display texts.
 */
export function rollupBase(property: RollupProperty): Property {
  const { config, ...rest } = property
  switch (rollupResult(config.function)) {
    case 'number':
      return { ...rest, type: 'number', config: { format: 'number' } }
    case 'percent':
      return { ...rest, type: 'number', config: { format: 'percent' } }
    case 'date':
      return { ...rest, type: 'date', config: { date_format: 'short', time_format: '24h' } }
    case 'list':
      return { ...rest, type: 'text', config: {} }
  }
}

/** The type whose filter operators and calculations fit a rollup; a list keeps `rollup` (contains and empty). */
export function rollupFilterType(property: RollupProperty): 'number' | 'date' | 'rollup' {
  const result = rollupResult(property.config.function)
  return result === 'list' ? 'rollup' : result === 'date' ? 'date' : 'number'
}

/** A context whose display texts and filters know the related pages' titles (`related_pages` of the response). */
export function withRelatedPages(ctx: ViewContext, database: Pick<Database, 'related_pages'> | undefined): ViewContext {
  const relatedPages = database?.related_pages
  return relatedPages && relatedPages !== ctx.relatedPages ? { ...ctx, relatedPages } : ctx
}

/** A related page's title as cells show it; `null` for a page the context does not know (just linked). */
export function relatedTitle(ctx: ViewContext, pageId: string): string | null {
  const page = ctx.relatedPages?.[pageId]
  return page ? page.title || 'Untitled' : null
}

/** Links of a row's relation cell to pages the viewer cannot read ("+N without access"). */
export function hiddenLinks(row: Row, relationPropertyId: string): number {
  return row.hidden_links?.[relationPropertyId] ?? 0
}

/** Whether a rollup of the row leaves out related pages the viewer cannot read ("Some pages are hidden"). */
export function rollupHidesPages(row: Row, property: RollupProperty): boolean {
  return hiddenLinks(row, property.config.relation_property_id) > 0
}
