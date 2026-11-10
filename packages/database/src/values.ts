import {
  addDays,
  dateValue,
  localDay,
  offsetMs,
  parseDatePoint,
  parseDateText,
  pointText,
} from './dates'
import { parseButtonConfig } from './buttons'
import {
  DATE_FORMATS,
  NUMBER_FORMATS,
  OPTION_COLORS,
  STATUS_GROUPS,
  TIME_FORMATS,
  type CellValue,
  type Database,
  type DatabaseDetail,
  type DatabaseProperty,
  type DatabaseRow,
  type DateConfig,
  type DatePoint,
  type NumberConfig,
  type Property,
  type Row,
  type SelectOption,
  type StatusOption,
  type ViewContext,
} from './types'

const MAX_TEXT_CHARS = 10_000
const MAX_URL_CHARS = 2_000
const DEFAULT_DATE_CONFIG: DateConfig = { date_format: 'short', time_format: '24h' }

export function parseDatabase(detail: DatabaseDetail): Database {
  const properties = detail.properties.map(parseProperty).sort((a, b) => a.position - b.position)
  return { ...detail, properties, rows: detail.rows.map((row) => parseRow(row, properties)) }
}

/** Reads a property's config; a missing or malformed field gets the server's default. */
export function parseProperty(property: DatabaseProperty): Property {
  const { config, ...rest } = property
  switch (rest.type) {
    case 'number':
      return {
        ...rest,
        type: rest.type,
        config: {
          format: oneOf(NUMBER_FORMATS, config.format, 'number'),
          ...(typeof config.currency === 'string' ? { currency: config.currency } : {}),
        },
      }
    case 'select':
    case 'multi_select':
      return {
        ...rest,
        type: rest.type,
        config: { options: parseOptions(config.options).map(({ id, name, color }) => ({ id, name, color })) },
      }
    case 'status':
      return { ...rest, type: rest.type, config: { options: parseOptions(config.options) } }
    case 'date':
      return {
        ...rest,
        type: rest.type,
        config: {
          date_format: oneOf(DATE_FORMATS, config.date_format, 'short'),
          time_format: oneOf(TIME_FORMATS, config.time_format, '24h'),
        },
      }
    case 'title':
    case 'text':
    case 'person':
    case 'checkbox':
    case 'url':
    case 'created_time':
    case 'created_by':
    case 'last_edited_time':
    case 'last_edited_by':
      return { ...rest, type: rest.type, config: {} }
    case 'button':
      return { ...rest, type: rest.type, config: parseButtonConfig(config) }
    default:
      return unreachable(rest.type)
  }
}

export function parseRow(row: DatabaseRow, properties: readonly Property[]): Row {
  const values: Record<string, CellValue> = {}
  for (const property of properties) {
    const value = parseValue(property, row.values[property.id])
    if (value) values[property.id] = value
  }
  return { ...row, values }
}

/**
 * Parses a stored value, as the server's `parse_value`: `null`, `""`, `[]` and `false` are empty,
 * and so is anything that does not fit the property (an id of a deleted option, a wrong shape).
 */
export function parseValue(property: Property, raw: unknown): CellValue | null {
  if (raw === null || raw === undefined) return null
  switch (property.type) {
    case 'text':
      return typeof raw === 'string' && raw !== '' ? { kind: 'text', text: raw } : null
    case 'number':
      return typeof raw === 'number' && Number.isFinite(raw) ? { kind: 'number', number: raw } : null
    case 'select':
    case 'status':
      return typeof raw === 'string' && hasOption(property, raw) ? { kind: 'choice', id: raw } : null
    case 'multi_select': {
      const ids = uniqueStrings(raw)
      return ids && ids.length > 0 && ids.every((id) => hasOption(property, id))
        ? { kind: 'choices', ids }
        : null
    }
    case 'date': {
      if (typeof raw !== 'object') return null
      const { start, end } = raw as { start?: unknown; end?: unknown }
      const startPoint = typeof start === 'string' ? parseDatePoint(start) : null
      const endPoint = typeof end === 'string' ? parseDatePoint(end) : null
      if (!startPoint || (end != null && !endPoint)) return null
      const date = dateValue(startPoint, endPoint)
      return date && { kind: 'date', date }
    }
    case 'person': {
      const ids = uniqueStrings(raw)
      return ids && ids.length > 0 ? { kind: 'people', ids } : null
    }
    case 'checkbox':
      return raw === true ? { kind: 'checked' } : null
    case 'url':
      return typeof raw === 'string' && validUrl(raw) ? { kind: 'url', url: raw } : null
    case 'title':
    case 'created_time':
    case 'created_by':
    case 'last_edited_time':
    case 'last_edited_by':
    case 'button':
      return null
    default:
      return unreachable(property)
  }
}

/**
 * The value a row shows for a property: computed types read the row's page, and a status
 * without a value is its first `todo` option.
 */
export function cellOf(row: Row, property: Property): CellValue | null {
  switch (property.type) {
    case 'title':
      return row.title === '' ? null : { kind: 'text', text: row.title }
    case 'created_time':
      return moment(row.created_at)
    case 'last_edited_time':
      return moment(row.updated_at)
    case 'created_by':
      return { kind: 'people', ids: [row.created_by] }
    case 'last_edited_by':
      return { kind: 'people', ids: [row.updated_by] }
    case 'status': {
      const stored = row.values[property.id]
      if (stored) return stored
      const fallback = property.config.options.find((option) => option.group === 'todo')
      return fallback ? { kind: 'choice', id: fallback.id } : null
    }
    case 'text':
    case 'number':
    case 'select':
    case 'multi_select':
    case 'date':
    case 'person':
    case 'checkbox':
    case 'url':
      return row.values[property.id] ?? null
    case 'button':
      return null
    default:
      return unreachable(property)
  }
}

/** The JSON the API takes for a value; `null` clears it. */
export function apiValue(value: CellValue | null): unknown {
  if (!value) return null
  switch (value.kind) {
    case 'text':
      return value.text
    case 'number':
      return value.number
    case 'choice':
      return value.id
    case 'choices':
    case 'people':
      return value.ids
    case 'date':
      return {
        start: pointText(value.date.start),
        end: value.date.end ? pointText(value.date.end) : null,
      }
    case 'checked':
      return true
    case 'url':
      return value.url
    default:
      return unreachable(value)
  }
}

/** The text a cell shows, formatted by the property's config, in the viewer's zone and locale. */
export function displayText(property: Property, value: CellValue | null, ctx: ViewContext): string {
  if (!value) return ''
  switch (value.kind) {
    case 'text':
      return value.text
    case 'url':
      return value.url
    case 'number':
      return property.type === 'number' ? formatNumber(value.number, property.config, ctx.locale) : String(value.number)
    case 'choice':
      return optionName(property, value.id)
    case 'choices':
      return value.ids.map((id) => optionName(property, id)).join(', ')
    case 'people':
      return value.ids.map((id) => ctx.members.get(id) ?? '').filter((name) => name !== '').join(', ')
    case 'checked':
      return 'true'
    case 'date': {
      const config = property.type === 'date' ? property.config : DEFAULT_DATE_CONFIG
      const start = formatPoint(value.date.start, config, ctx)
      return value.date.end ? `${start} → ${formatPoint(value.date.end, config, ctx)}` : start
    }
    default:
      return unreachable(value)
  }
}

/**
 * A value from typed or pasted text, with the server's text conversion rules (spec 4.2). Selects
 * match option names and people match member names, ignoring case; unknown names are dropped.
 * `null` means the text gives no value.
 */
export function valueFromText(property: Property, input: string, ctx: ViewContext): CellValue | null {
  const text = input.trim()
  switch (property.type) {
    case 'text': {
      const clipped = Array.from(text).slice(0, MAX_TEXT_CHARS).join('')
      return clipped === '' ? null : { kind: 'text', text: clipped }
    }
    case 'number': {
      const number = parseNumber(text)
      return number === null ? null : { kind: 'number', number }
    }
    case 'url':
      return validUrl(text) ? { kind: 'url', url: text } : null
    case 'checkbox':
      return ['true', 'yes', '1', 'x'].includes(text.toLowerCase()) ? { kind: 'checked' } : null
    case 'date': {
      const date = parseDateText(text)
      return date && { kind: 'date', date }
    }
    case 'select':
    case 'status': {
      const id = optionByName(property.config.options, text)
      return id ? { kind: 'choice', id } : null
    }
    case 'multi_select': {
      const ids = unique(splitNames(text).map((name) => optionByName(property.config.options, name)))
      return ids.length > 0 ? { kind: 'choices', ids } : null
    }
    case 'person': {
      const byName = new Map<string, string>()
      for (const [id, name] of ctx.members) byName.set(name.toLowerCase(), id)
      const ids = unique(splitNames(text).map((name) => byName.get(name.toLowerCase())))
      return ids.length > 0 ? { kind: 'people', ids } : null
    }
    case 'title':
    case 'created_time':
    case 'created_by':
    case 'last_edited_time':
    case 'last_edited_by':
    case 'button':
      return null
    default:
      return unreachable(property)
  }
}

/**
 * The text a number's edit field shows. A percent shows the percentage the cell shows, so a
 * stored 0.125 edits as "12.5".
 */
export function numberEditText(property: Property, number: number): string {
  return String(isPercent(property) ? rounded(number * 100) : number)
}

/**
 * The number an edit field's text stores, the inverse of `numberEditText`: in a percent property
 * "50" and "50%" store 0.5. `null` when the text is not a number.
 */
export function numberFromEditText(property: Property, input: string): number | null {
  const text = input.trim()
  if (!isPercent(property)) return parseNumber(text)
  const number = parseNumber(text.endsWith('%') ? text.slice(0, -1).trim() : text)
  return number === null ? null : rounded(number / 100)
}

/** Stored values with a patch applied: `null` removes a value, as the server does. */
export function patchValues(values: DatabaseRow['values'], patch: Record<string, unknown>): DatabaseRow['values'] {
  const next = { ...values }
  for (const [propertyId, value] of Object.entries(patch)) {
    if (value === null) delete next[propertyId]
    else next[propertyId] = value
  }
  return next
}

export function optionName(property: Property, id: string): string {
  return options(property).find((option) => option.id === id)?.name ?? ''
}

/** The options in sort order; empty for types without options. */
export function options(property: Property): readonly SelectOption[] {
  return property.type === 'select' || property.type === 'multi_select' || property.type === 'status'
    ? property.config.options
    : []
}

export function unreachable(value: never): never {
  throw new Error(`Unexpected value: ${JSON.stringify(value)}`)
}

function moment(iso: string): CellValue | null {
  const ms = Date.parse(iso)
  return Number.isNaN(ms) ? null : { kind: 'date', date: { start: { kind: 'moment', iso, ms }, end: null } }
}

function isPercent(property: Property): boolean {
  return property.type === 'number' && property.config.format === 'percent'
}

/** Drops the binary noise of a scale by 100 (0.07 * 100 is 7.000000000000001). */
function rounded(number: number): number {
  return Number(number.toPrecision(15))
}

/** Rust's `str::parse::<f64>` for finite numbers: no commas, no hex, no inner spaces. */
function parseNumber(text: string): number | null {
  if (!/^[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$/.test(text)) return null
  const number = Number(text)
  return Number.isFinite(number) ? number : null
}

function validUrl(url: string): boolean {
  if (url === '' || Array.from(url).length > MAX_URL_CHARS || /\s/u.test(url)) return false
  try {
    const parsed = new URL(url)
    return (parsed.protocol === 'https:' || parsed.protocol === 'http:') && parsed.hostname !== ''
  } catch {
    return false
  }
}

const numberFormats = new Map<string, Intl.NumberFormat>()

function formatNumber(number: number, config: NumberConfig, locale: string | undefined): string {
  if (config.format === 'number') return String(number)
  const key = `${locale ?? ''}|${config.format}|${config.currency ?? ''}`
  let format = numberFormats.get(key)
  if (!format) {
    format = new Intl.NumberFormat(
      locale,
      config.format === 'currency'
        ? { style: 'currency', currency: config.currency ?? 'USD' }
        : { style: config.format === 'percent' ? 'percent' : 'decimal', maximumFractionDigits: 10 },
    )
    numberFormats.set(key, format)
  }
  return format.format(number)
}

const MINUTE_MS = 60_000
const DAY_MS = 86_400_000
const dateFormats = new Map<string, Intl.DateTimeFormat>()
/** Formatted days and times by locale: a few hundred days and minutes repeat across thousands of cells. */
const formatted = new Map<string, string>()

function format(locale: string | undefined, style: 'day' | DateConfig['time_format'], ms: number): string {
  const key = `${locale ?? ''}|${style}|${ms}`
  let text = formatted.get(key)
  if (text === undefined) {
    const formatKey = `${locale ?? ''}|${style}`
    let formatter = dateFormats.get(formatKey)
    if (!formatter) {
      formatter = new Intl.DateTimeFormat(
        locale,
        style === 'day'
          ? { timeZone: 'UTC', year: 'numeric', month: 'short', day: 'numeric' }
          : { timeZone: 'UTC', hour: 'numeric', minute: '2-digit', hourCycle: style === '12h' ? 'h12' : 'h23' },
      )
      dateFormats.set(formatKey, formatter)
    }
    text = formatter.format(ms)
    formatted.set(key, text)
  }
  return text
}

function formatPoint(point: DatePoint, config: DateConfig, ctx: ViewContext): string {
  if (point.kind === 'day') return formatDay(point.day, config, ctx)
  const local = point.ms + offsetMs(point.ms, ctx.timeZone)
  const minute = Math.floor((((local % DAY_MS) + DAY_MS) % DAY_MS) / MINUTE_MS) * MINUTE_MS
  const day = new Date(local).toISOString().slice(0, 10)
  return `${formatDay(day, config, ctx)} ${format(ctx.locale, config.time_format, minute)}`
}

function formatDay(day: string, config: DateConfig, ctx: ViewContext): string {
  switch (config.date_format) {
    case 'iso':
      return day
    case 'relative': {
      const near = nearDays(ctx)
      if (day === near.today) return 'Today'
      if (day === near.tomorrow) return 'Tomorrow'
      if (day === near.yesterday) return 'Yesterday'
      return shortDay(day, ctx.locale)
    }
    case 'short':
      return shortDay(day, ctx.locale)
    default:
      return unreachable(config.date_format)
  }
}

let near = { key: '', today: '', tomorrow: '', yesterday: '' }

function nearDays(ctx: ViewContext): typeof near {
  const key = `${ctx.timeZone}|${ctx.now}`
  if (near.key !== key) {
    const today = localDay(ctx.now, ctx.timeZone)
    near = { key, today, tomorrow: addDays(today, 1), yesterday: addDays(today, -1) }
  }
  return near
}

function shortDay(day: string, locale: string | undefined): string {
  return format(locale, 'day', Date.parse(`${day}T00:00:00Z`))
}

function parseOptions(raw: unknown): StatusOption[] {
  if (!Array.isArray(raw)) return []
  return raw.flatMap((option: unknown) => {
    if (typeof option !== 'object' || option === null) return []
    const { id, name, color, group } = option as Record<string, unknown>
    if (typeof id !== 'string' || typeof name !== 'string') return []
    return [{ id, name, color: oneOf(OPTION_COLORS, color, 'default'), group: oneOf(STATUS_GROUPS, group, 'todo') }]
  })
}

function hasOption(property: Property, id: string): boolean {
  return options(property).some((option) => option.id === id)
}

function optionByName(list: readonly SelectOption[], name: string): string | undefined {
  const lower = name.toLowerCase()
  return list.find((option) => option.name.toLowerCase() === lower)?.id
}

function splitNames(text: string): string[] {
  return text.split(',').map((name) => name.trim()).filter((name) => name !== '')
}

function unique(ids: (string | undefined)[]): string[] {
  return [...new Set(ids.filter((id): id is string => id !== undefined))]
}

function uniqueStrings(raw: unknown): string[] | null {
  if (!Array.isArray(raw) || !raw.every((item) => typeof item === 'string')) return null
  return new Set(raw).size === raw.length ? (raw as string[]) : null
}

function oneOf<T extends string>(allowed: readonly T[], value: unknown, fallback: T): T {
  return allowed.includes(value as T) ? (value as T) : fallback
}
