import type { DatabasePropertyType, Property } from '../types'

/** A formula's config: the expression with property ids in `prop("…")`, and the result type the server inferred. */
export type FormulaConfig = { expression: string; result_type: string }

export type FormulaProperty = Property & { type: 'formula' }

/** The formula types that filter, sort and show as a stored type; any other result (a list, a page) shows as text. */
const BASES = {
  number: 'number',
  text: 'text',
  boolean: 'checkbox',
  date: 'date',
  person: 'person',
  'list<person>': 'person',
} as const

export function parseFormulaConfig(config: Record<string, unknown>): FormulaConfig {
  return {
    expression: typeof config.expression === 'string' ? config.expression : '',
    result_type: typeof config.result_type === 'string' ? config.result_type : 'text',
  }
}

/**
 * The stored-type property a formula's result reads as, with the formula's id: its cells, sorts and
 * calculations follow that type. Formulas run only on the server (D-P3-3); clients read results.
 */
export function formulaBase(property: FormulaProperty): Property {
  const { config, ...rest } = property
  const base = BASES[config.result_type as keyof typeof BASES] ?? 'text'
  switch (base) {
    case 'number':
      return { ...rest, type: base, config: { format: 'number' } }
    case 'date':
      return { ...rest, type: base, config: { date_format: 'short', time_format: '24h' } }
    default:
      return { ...rest, type: base, config: {} }
  }
}

/** The type whose filter operators fit the formula, as the server's `filter_type`: lists, people and pages only filter on empty. */
export function formulaFilterType(property: FormulaProperty): 'number' | 'text' | 'checkbox' | 'date' | 'formula' {
  switch (property.config.result_type) {
    case 'number':
      return 'number'
    case 'text':
      return 'text'
    case 'boolean':
      return 'checkbox'
    case 'date':
      return 'date'
    default:
      return 'formula'
  }
}

/** The type whose filter operators and calculations fit a property: a formula's follows its result type. */
export function filterTypeOf(property: Property): DatabasePropertyType {
  return property.type === 'formula' ? formulaFilterType(property) : property.type
}

/** The property a filter rule edits and matches: a formula whose result filters as a stored type reads as that type. */
export function filterProperty(property: Property): Property {
  return property.type === 'formula' && formulaFilterType(property) !== 'formula' ? formulaBase(property) : property
}

/** The message of a failed cell (`{"error": "…"}`), or `null`. */
export function formulaError(raw: unknown): string | null {
  if (raw === null || typeof raw !== 'object' || Array.isArray(raw)) return null
  const message = (raw as { error?: unknown }).error
  return typeof message === 'string' ? message : null
}

/** A result in the stored shape of its base type: one person becomes a list of one, other lists their items as text. */
export function formulaRaw(property: FormulaProperty, raw: unknown): unknown {
  if (formulaError(raw) !== null) return null
  const base = formulaBase(property).type
  if (base !== 'text' || property.config.result_type === 'text') return base === 'person' && typeof raw === 'string' ? [raw] : raw
  return itemText(raw)
}

function itemText(raw: unknown): string {
  if (Array.isArray(raw)) return raw.map(itemText).filter((text) => text !== '').join(', ')
  if (raw && typeof raw === 'object') {
    const { start, end } = raw as { start?: unknown; end?: unknown }
    if (typeof start === 'string') return typeof end === 'string' ? `${start} → ${end}` : start
    return ''
  }
  return raw === null || raw === undefined ? '' : String(raw)
}
