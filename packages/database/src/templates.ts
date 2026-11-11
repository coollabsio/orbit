import type { DatabaseTemplate } from '@orbit/api/generated/types.gen'
import { localDay } from './dates'
import type { Database, Property, Row, ViewConfig, ViewContext } from './types'
import { parseRow } from './values'

/** In a template's person value: whoever makes the row (spec P2 7.3). */
export const ME = 'me'
/** A template's date value that becomes the day the row is made. */
export const TODAY = { start: 'today', end: null } as const

/**
 * A row template with its values parsed like a row's. A person value may hold {@link ME} among its ids; `today` lists
 * the date properties that take the day a row is made (their values are left out of `values`).
 */
export type Template = Row & { name: string; today: readonly string[] }

export function isToday(raw: unknown): boolean {
  return typeof raw === 'object' && raw !== null && (raw as { start?: unknown }).start === TODAY.start
}

export function parseTemplate(template: DatabaseTemplate, properties: readonly Property[]): Template {
  const today = properties.filter((property) => property.type === 'date' && isToday(template.values[property.id])).map((property) => property.id)
  return { ...parseRow(template, properties), name: template.name, today }
}

/** The database's templates in order. */
export function templatesOf(database: Pick<Database, 'properties'> & { templates?: readonly DatabaseTemplate[] }): Template[] {
  return [...(database.templates ?? [])].sort((a, b) => a.position - b.position).map((template) => parseTemplate(template, database.properties))
}

/** The view's default template, when it still exists. */
export function defaultTemplate(config: Pick<ViewConfig, 'default_template_id'>, templates: readonly Template[]): Template | null {
  return templates.find((template) => template.id === config.default_template_id) ?? null
}

/** What `POST …/rows` adds to fill a row from `templateId`: the template and the viewer's local day for "today". */
export function templateFill(templateId: string | null, ctx: Pick<ViewContext, 'now' | 'timeZone'>): { template_id?: string; today?: string } {
  return templateId ? { template_id: templateId, today: localDay(ctx.now, ctx.timeZone) } : {}
}
