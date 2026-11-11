import { dayStartMs } from './dates'
import type { CellValue, DatabaseCalculation, DatabasePropertyType, DatePoint, Property, Row, ViewContext } from './types'
import { apiValue, cellOf, displayText } from './values'

const MINUTE_MS = 60_000
const HOUR_MS = 3_600_000
const DAY_MS = 86_400_000

/** The menu name and the footer label of each function (spec P2 4.3). */
export const CALCULATION_LABELS: Record<DatabaseCalculation, { name: string; short: string }> = {
  count_all: { name: 'Count all', short: 'Count' },
  count_values: { name: 'Count values', short: 'Values' },
  count_unique: { name: 'Count unique values', short: 'Unique' },
  count_empty: { name: 'Count empty', short: 'Empty' },
  count_not_empty: { name: 'Count not empty', short: 'Not empty' },
  percent_empty: { name: 'Percent empty', short: 'Empty' },
  percent_not_empty: { name: 'Percent not empty', short: 'Not empty' },
  sum: { name: 'Sum', short: 'Sum' },
  average: { name: 'Average', short: 'Average' },
  median: { name: 'Median', short: 'Median' },
  min: { name: 'Min', short: 'Min' },
  max: { name: 'Max', short: 'Max' },
  range: { name: 'Range', short: 'Range' },
  earliest_date: { name: 'Earliest date', short: 'Earliest' },
  latest_date: { name: 'Latest date', short: 'Latest' },
  date_range: { name: 'Date range', short: 'Range' },
  checked: { name: 'Checked', short: 'Checked' },
  unchecked: { name: 'Unchecked', short: 'Unchecked' },
  percent_checked: { name: 'Percent checked', short: 'Checked' },
  percent_unchecked: { name: 'Percent unchecked', short: 'Unchecked' },
}

const COUNTS: DatabaseCalculation[] = ['count_all', 'count_values', 'count_unique', 'count_empty', 'count_not_empty', 'percent_empty', 'percent_not_empty']
const NUMBERS: DatabaseCalculation[] = ['sum', 'average', 'median', 'min', 'max', 'range']
const DATES: DatabaseCalculation[] = ['earliest_date', 'latest_date', 'date_range']
const CHECKBOXES: DatabaseCalculation[] = ['count_all', 'checked', 'unchecked', 'percent_checked', 'percent_unchecked']

/** The functions of a type, in menu order; the server's `Calculation::fits`. */
export function calculationsFor(type: DatabasePropertyType): DatabaseCalculation[] {
  switch (type) {
    case 'checkbox':
      return CHECKBOXES
    case 'number':
      return [...COUNTS, ...NUMBERS]
    case 'date':
    case 'created_time':
    case 'last_edited_time':
      return [...COUNTS, ...DATES]
    default:
      return COUNTS
  }
}

/** A footer result: the text it shows, and the number behind it (`null` when there is none, as the average of no rows). */
export type Calculated = { value: string; raw: number | null }

const NONE: Calculated = { value: '—', raw: null }

/**
 * `fn` over the rows of a view, for one property. Empty follows the filter's rule (a status without a value counts as its
 * default option); a multi-select or person cell counts each item. Percentages have one decimal; dates show in the
 * property's format, a date range as "3 days" or "2 months".
 */
export function calculate(rows: readonly Row[], property: Property, fn: DatabaseCalculation, ctx: ViewContext): Calculated {
  const cells = rows.map((row) => cellOf(row, property))
  const filled = cells.filter((cell) => cell !== null)
  switch (fn) {
    case 'count_all':
      return count(rows.length)
    case 'count_values':
      return count(filled.reduce((sum, cell) => sum + items(cell).length, 0))
    case 'count_unique':
      return count(new Set(filled.flatMap(items)).size)
    case 'count_empty':
    case 'unchecked':
      return count(rows.length - filled.length)
    case 'count_not_empty':
    case 'checked':
      return count(filled.length)
    case 'percent_empty':
    case 'percent_unchecked':
      return percent(rows.length - filled.length, rows.length)
    case 'percent_not_empty':
    case 'percent_checked':
      return percent(filled.length, rows.length)
    case 'sum':
    case 'average':
    case 'median':
    case 'min':
    case 'max':
    case 'range': {
      const numbers = filled.flatMap((cell) => (cell.kind === 'number' ? [cell.number] : []))
      const raw = numeric(fn, numbers)
      return raw === null ? NONE : { value: displayText(property, { kind: 'number', number: raw }, ctx), raw }
    }
    case 'earliest_date':
    case 'latest_date':
    case 'date_range': {
      const points = filled.flatMap((cell) => (cell.kind === 'date' ? [cell.date] : []))
      if (points.length === 0) return NONE
      const ms = (point: DatePoint) => (point.kind === 'day' ? dayStartMs(point.day, ctx.timeZone) : point.ms)
      const earliest = points.map((date) => date.start).reduce((a, b) => (ms(b) < ms(a) ? b : a))
      const latest = points.map((date) => date.end ?? date.start).reduce((a, b) => (ms(b) > ms(a) ? b : a))
      const show = (point: DatePoint): Calculated => ({ value: displayText(property, { kind: 'date', date: { start: point, end: null } }, ctx), raw: ms(point) })
      if (fn === 'earliest_date') return show(earliest)
      if (fn === 'latest_date') return show(latest)
      const span = ms(latest) - ms(earliest)
      const daysOnly = points.every((date) => date.start.kind === 'day')
      return { value: duration(span, daysOnly), raw: span }
    }
  }
}

function count(n: number): Calculated {
  return { value: n.toLocaleString('en-US'), raw: n }
}

function percent(part: number, total: number): Calculated {
  const raw = total === 0 ? 0 : Math.round((part / total) * 1000) / 10
  return { value: `${raw}%`, raw }
}

/** The keys `count_values` and `count_unique` count: each item of a list (options, people, pages, rollup texts), else the value itself. */
function items(cell: CellValue): string[] {
  if (cell.kind === 'choices' || cell.kind === 'people' || cell.kind === 'pages') return cell.ids
  if (cell.kind === 'list') return cell.items
  return [JSON.stringify(apiValue(cell))]
}

function numeric(fn: 'sum' | 'average' | 'median' | 'min' | 'max' | 'range', numbers: number[]): number | null {
  if (fn === 'sum') return clean(numbers.reduce((sum, n) => sum + n, 0))
  if (numbers.length === 0) return null
  const sorted = [...numbers].sort((a, b) => a - b)
  switch (fn) {
    case 'average':
      return clean(Math.round((sorted.reduce((sum, n) => sum + n, 0) / sorted.length) * 10_000) / 10_000)
    case 'median': {
      const middle = Math.floor(sorted.length / 2)
      return clean(sorted.length % 2 === 1 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2)
    }
    case 'min':
      return sorted[0]
    case 'max':
      return sorted[sorted.length - 1]
    case 'range':
      return clean(sorted[sorted.length - 1] - sorted[0])
  }
}

/** Drops binary noise: 0.1 + 0.2 is 0.3. An average keeps 4 decimals (10 / 3 is 3.3333). */
function clean(number: number): number {
  return Number(number.toPrecision(15))
}

/** "45 minutes", "5 hours", "3 days", "2 months", "1 year"; a range of dates without times counts whole days. */
function duration(ms: number, daysOnly: boolean): string {
  if (!daysOnly && ms < DAY_MS) {
    const hours = Math.round(ms / HOUR_MS)
    return hours < 1 ? plural(Math.round(ms / MINUTE_MS), 'minute') : plural(hours, 'hour')
  }
  const days = Math.round(ms / DAY_MS)
  if (days < 31) return plural(days, 'day')
  if (days < 365) return plural(Math.floor(days / 30), 'month')
  return plural(Math.floor(days / 365), 'year')
}

function plural(n: number, unit: string): string {
  return `${n} ${unit}${n === 1 ? '' : 's'}`
}
