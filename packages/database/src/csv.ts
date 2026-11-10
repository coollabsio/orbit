import { dateText, parseDateText } from './dates'
import type { Database, DatabaseView, Property, Row, ViewContext } from './types'
import { cellOf, optionName, unreachable } from './values'
import { repairConfig } from './view'

/** Spec P2 9.3 limits of an import. */
export const MAX_CSV_BYTES = 5 * 1024 * 1024
export const MAX_CSV_ROWS = 5_000
export const MAX_CSV_COLUMNS = 100

/** The property types an import can create for a column (title is the title column). */
export type ImportType = 'text' | 'number' | 'select' | 'multi_select' | 'status' | 'date' | 'person' | 'checkbox' | 'url'

const DELIMITERS = [',', ';', '\t'] as const

/**
 * The records of a CSV text (RFC 4180): quoted fields with `""` escapes and line breaks, CRLF,
 * LF or CR line ends, a leading BOM. The delimiter is `,`, `;` or tab, whichever the first line
 * holds most of outside quotes. Blank lines are dropped; rows keep their own length.
 */
export function parseCsv(input: string): string[][] {
  const text = input.startsWith('﻿') ? input.slice(1) : input
  const delimiter = detectDelimiter(text)
  const records: string[][] = []
  let record: string[] = []
  let field = ''
  let quoted = false
  let i = 0
  const endRecord = () => {
    record.push(field)
    if (record.length > 1 || record[0] !== '') records.push(record)
    record = []
    field = ''
  }
  while (i < text.length) {
    const char = text[i]
    if (quoted) {
      if (char === '"') {
        if (text[i + 1] === '"') {
          field += '"'
          i += 2
          continue
        }
        quoted = false
      } else {
        field += char
      }
      i += 1
      continue
    }
    if (char === '"' && field === '') {
      quoted = true
    } else if (char === delimiter) {
      record.push(field)
      field = ''
    } else if (char === '\r' || char === '\n') {
      endRecord()
      if (char === '\r' && text[i + 1] === '\n') i += 1
    } else {
      field += char
    }
    i += 1
  }
  if (field !== '' || record.length > 0) endRecord()
  return records
}

function detectDelimiter(text: string): string {
  const counts = new Map<string, number>(DELIMITERS.map((delimiter) => [delimiter, 0]))
  let quoted = false
  for (const char of text) {
    if (char === '"') quoted = !quoted
    else if (!quoted && (char === '\n' || char === '\r')) break
    else if (!quoted && counts.has(char)) counts.set(char, (counts.get(char) ?? 0) + 1)
  }
  let best: string = ','
  for (const delimiter of DELIMITERS) if ((counts.get(delimiter) ?? 0) > (counts.get(best) ?? 0)) best = delimiter
  return best
}

const CHECKBOX_WORDS = new Set(['true', 'false', 'yes', 'no', 'x'])
const NUMBER = /^[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$/
const MAX_SELECT_OPTIONS = 50
const MAX_SELECT_NAME = 40

/**
 * The property type a column's values suggest: checkbox (only true/false/yes/no/x), number,
 * date (ISO 8601), url, select (at most 50 distinct short values, some repeated), else text.
 * Empty cells do not count.
 */
export function guessType(values: readonly string[]): ImportType {
  const cells = values.map((value) => value.trim()).filter((value) => value !== '')
  if (cells.length === 0) return 'text'
  if (cells.every((cell) => CHECKBOX_WORDS.has(cell.toLowerCase()))) return 'checkbox'
  if (cells.every((cell) => NUMBER.test(cell) && Number.isFinite(Number(cell)))) return 'number'
  if (cells.every((cell) => parseDateText(cell) !== null)) return 'date'
  if (cells.every((cell) => /^https?:\/\/\S+$/i.test(cell))) return 'url'
  const distinct = new Set(cells)
  if (
    distinct.size <= MAX_SELECT_OPTIONS &&
    distinct.size < cells.length &&
    cells.every((cell) => Array.from(cell).length <= MAX_SELECT_NAME)
  )
    return 'select'
  return 'text'
}

/** The CSV column a title most likely sits in: one named "name" or "title", else the first. */
export function guessTitleColumn(header: readonly string[]): number {
  const index = header.findIndex((name) => ['name', 'title'].includes(name.trim().toLowerCase()))
  return Math.max(index, 0)
}

/** Undoes the formula guard of an exported cell (`'=1+1` → `=1+1`). */
export function unguard(cell: string): string {
  return /^'[=+\-@\t\r]/.test(cell) ? cell.slice(1) : cell
}

/**
 * A cell as a spreadsheet app cannot run it: text that starts with `=`, `+`, `-`, `@`, tab or CR
 * gets a leading `'`. Numbers are left as they are.
 */
export function guardCell(text: string, isNumber: boolean): string {
  return !isNumber && /^[=+\-@\t\r]/.test(text) ? `'${text}` : text
}

/**
 * A value as an export writes it, in a fixed format for machines: numbers without grouping (as
 * stored), dates as ISO 8601 (`start/end` for a range), names joined with ", ", a checkbox as
 * `true` or `false`.
 */
export function csvText(property: Property, row: Row, ctx: ViewContext): string {
  if (property.type === 'checkbox') return row.values[property.id] ? 'true' : 'false'
  const value = cellOf(row, property)
  if (!value) return ''
  switch (value.kind) {
    case 'text':
      return value.text
    case 'url':
      return value.url
    case 'number':
      return String(value.number)
    case 'choice':
      return optionName(property, value.id)
    case 'choices':
      return value.ids.map((id) => optionName(property, id)).join(', ')
    case 'people':
      return value.ids.map((id) => ctx.members.get(id) ?? '').filter((name) => name !== '').join(', ')
    case 'checked':
      return 'true'
    case 'date':
      return dateText(value.date)
    default:
      return unreachable(value)
  }
}

/** Rows as CSV text: UTF-8 BOM, CRLF line ends, fields quoted when they hold `"`, `,`, `;` or a line break. */
export function toCsv(records: readonly (readonly string[])[]): string {
  const field = (text: string) => (/[",;\r\n]/.test(text) ? `"${text.replaceAll('"', '""')}"` : text)
  return `﻿${records.map((record) => record.map(field).join(',')).join('\r\n')}\r\n`
}

/** The properties a view shows, in order: a table's visible columns, a board's title and card properties. */
export function viewProperties(database: Database, view: DatabaseView): Property[] {
  const config = repairConfig(view, database.properties)
  const byId = new Map(database.properties.map((property) => [property.id, property]))
  if (view.type === 'board') {
    const title = database.properties.find((property) => property.type === 'title')
    const cards = config.card_properties.flatMap((id) => byId.get(id) ?? [])
    return title ? [title, ...cards.filter((property) => property !== title)] : cards
  }
  return config.columns.flatMap((column) => {
    const property = byId.get(column.property_id)
    return property && !column.hidden ? [property] : []
  })
}

/** A view's rows (already filtered, searched and sorted) and its visible properties as CSV text. */
export function viewCsv(properties: readonly Property[], rows: readonly Row[], ctx: ViewContext): string {
  const header = properties.map((property) => guardCell(property.name, false))
  const lines = rows.map((row) =>
    properties.map((property) => guardCell(csvText(property, row, ctx), property.type === 'number')),
  )
  return toCsv([header, ...lines])
}

/** Where a CSV column goes: an existing property, a new property (`title` names a new database's title column), or nowhere. */
export type ColumnTarget = { kind: 'property'; propertyId: string } | { kind: 'new'; type: ImportType | 'title' } | { kind: 'skip' }

/** A parsed CSV as columns: the column names and the cell text of each row. */
export type CsvTable = { header: string[]; rows: string[][] }

/**
 * The table of a CSV's records. With `hasHeader` the first record names the columns (an empty name becomes
 * "Column N"), else every record is a row. Rows keep at most one cell per column, with exported formula guards
 * undone.
 */
export function csvTable(records: readonly string[][], hasHeader: boolean): CsvTable {
  const width = Math.max(0, ...records.map((record) => record.length))
  const names = hasHeader ? (records[0] ?? []) : []
  const header = Array.from({ length: width }, (_, index) => names[index]?.trim() || `Column ${index + 1}`)
  const rows = (hasHeader ? records.slice(1) : records).map((record) => record.slice(0, width).map(unguard))
  return { header, rows }
}

function columnValues(table: CsvTable, index: number): string[] {
  return table.rows.map((row) => row[index] ?? '')
}

/** A property a CSV column can fill: not computed, except the title. */
function fillable(property: Property): boolean {
  return !['created_time', 'created_by', 'last_edited_time', 'last_edited_by'].includes(property.type)
}

/**
 * The first mapping of a CSV. A new database (`properties` null): the title column by its name, every other column a
 * new property of its guessed type. A merge: the title property takes the column named like it (else the likely title
 * column), other columns go to the property of the same name (case ignored), else to a new property.
 */
export function guessTargets(table: CsvTable, properties: readonly Property[] | null): ColumnTarget[] {
  const guess = (index: number): ColumnTarget => ({ kind: 'new', type: guessType(columnValues(table, index)) })
  const lower = table.header.map((name) => name.toLowerCase())
  if (!properties) {
    const title = guessTitleColumn(table.header)
    return table.header.map((_, index) => (index === title ? { kind: 'new', type: 'title' } : guess(index)))
  }
  const titleProperty = properties.find((property) => property.type === 'title')
  const named = titleProperty ? lower.indexOf(titleProperty.name.toLowerCase()) : -1
  const titleColumn = named === -1 ? guessTitleColumn(table.header) : named
  const used = new Set<string>()
  return table.header.map((_, index): ColumnTarget => {
    if (index === titleColumn && titleProperty) {
      used.add(titleProperty.id)
      return { kind: 'property', propertyId: titleProperty.id }
    }
    const match = properties.find(
      (property) => property.type !== 'title' && fillable(property) && !used.has(property.id) && property.name.toLowerCase() === lower[index],
    )
    if (match) {
      used.add(match.id)
      return { kind: 'property', propertyId: match.id }
    }
    return guess(index)
  })
}

/** Whether a target fills the title. */
export function isTitleTarget(target: ColumnTarget, properties: readonly Property[] | null): boolean {
  if (target.kind === 'new') return target.type === 'title'
  return target.kind === 'property' && properties?.find((property) => property.id === target.propertyId)?.type === 'title'
}

/**
 * The mapping with column `index` set to `target`. A title target moves the title: the column that had it becomes a
 * new property of its guessed type. A property another column had goes back to a guessed new property there.
 */
export function retarget(table: CsvTable, targets: readonly ColumnTarget[], index: number, target: ColumnTarget, properties: readonly Property[] | null): ColumnTarget[] {
  const title = isTitleTarget(target, properties)
  return targets.map((current, at) => {
    if (at === index) return target
    const clash =
      (title && isTitleTarget(current, properties)) ||
      (target.kind === 'property' && current.kind === 'property' && current.propertyId === target.propertyId)
    return clash ? { kind: 'new', type: guessType(columnValues(table, at)) } : current
  })
}

/** Why the mapping cannot be imported, or `null`. */
export function mappingProblem(table: CsvTable, targets: readonly ColumnTarget[], properties: readonly Property[] | null): string | null {
  if (table.header.length === 0) return 'The file has no columns.'
  if (table.header.length > MAX_CSV_COLUMNS) return `A CSV can have at most ${MAX_CSV_COLUMNS} columns; this one has ${table.header.length}.`
  if (table.rows.length > MAX_CSV_ROWS) return `A CSV can have at most ${MAX_CSV_ROWS.toLocaleString('en-US')} rows; this one has ${table.rows.length.toLocaleString('en-US')}.`
  if (!targets.some((target) => isTitleTarget(target, properties))) return 'Choose the column that holds the titles.'
  return null
}

/** The `columns` of an import request. */
export function importColumns(table: CsvTable, targets: readonly ColumnTarget[]) {
  return table.header.map((name, index) => {
    const target = targets[index] ?? { kind: 'skip' }
    return {
      name,
      target:
        target.kind === 'property'
          ? { property_id: target.propertyId }
          : target.kind === 'new'
            ? { new: target.type }
            : ('skip' as const),
    }
  })
}

/** The file name of a view's CSV: "<database> - <view>.csv", without characters file systems refuse. */
export function viewCsvName(database: Database, view: DatabaseView): string {
  const title = database.database.title.trim() || 'Untitled'
  return `${title} - ${view.name}.csv`.replace(/[\\/:*?"<>|]/g, '_')
}
