import { describe, expect, test } from 'bun:test'
import {
  contactHref,
  firstImageFile,
  isImageName,
  parseFiles,
  parseUniqueIdConfig,
  rowFileUrl,
  uniqueIdText,
  validEmail,
  validPhone,
  validPrefix,
} from './fields'
import { matches } from './filter'
import { isComputed, operatorsOf, sortable } from './meta'
import { sortRows } from './sort'
import { ctx, row } from './testing'
import type { CellValue, DatabaseFilterOperator, DatabaseProperty, Property } from './types'
import { apiValue, displayText, parseDatabase, parseValue } from './values'

const photo = (file_id: string, name: string) => ({ file_id, name })

const database = parseDatabase({
  database: { id: 'db', title: 'Contacts', icon: null, locked: false, row_count: 4 },
  properties: [
    { id: 'p-title', name: 'Name', type: 'title', config: {}, position: 0 },
    { id: 'p-email', name: 'Email', type: 'email', config: {}, position: 1 },
    { id: 'p-phone', name: 'Phone', type: 'phone', config: {}, position: 2 },
    { id: 'p-files', name: 'Files', type: 'files', config: {}, position: 3 },
    { id: 'p-id', name: 'ID', type: 'unique_id', config: { prefix: 'TASK', next: 5 }, position: 4 },
  ] satisfies DatabaseProperty[],
  views: [],
  rows: [
    row('r1', 0, 'Ada', { 'p-email': 'ada@example.com', 'p-phone': '+1 (555) 010-2000', 'p-files': [photo('f1', 'notes.pdf'), photo('f2', 'face.PNG')], 'p-id': 3 }),
    row('r2', 1, 'Bob', { 'p-email': 'Bob@Example.org', 'p-files': [photo('f3', 'a.txt')], 'p-id': 1 }),
    row('r3', 2, 'Cy', { 'p-email': 'not an email', 'p-phone': 'call me', 'p-id': 0 }),
    row('r4', 3, 'Di', { 'p-phone': '555.0100', 'p-files': [photo('f4', 'x.jpg'), photo('f5', 'y.gif'), photo('f6', 'z.doc')], 'p-id': 4 }),
  ],
})

function property(id: string): Property {
  return database.properties.find((candidate) => candidate.id === id)!
}

function ids(propertyId: string, operator: DatabaseFilterOperator, value?: unknown) {
  const rule = { property_id: propertyId, operator, value }
  return database.rows.filter((row) => matches(row, rule, property(propertyId), ctx)).map((row) => row.id)
}

function order(propertyId: string, direction: 'asc' | 'desc') {
  return sortRows(database.rows, [{ property_id: propertyId, direction }], database.properties, ctx).map((row) => row.id)
}

const cell = (id: string, rowId: string): CellValue | null => database.rows.find((row) => row.id === rowId)!.values[id] ?? null

describe('email and phone', () => {
  test.each([
    ['a@b', true],
    ['ada@example.com', true],
    ['@example.com', false],
    ['ada@', false],
    ['a@b@c', false],
    ['ada @example.com', false],
    [`${'a'.repeat(316)}@b.c`, true],
    [`${'a'.repeat(317)}@b.c`, false],
  ] as const)('validEmail(%p) is %p', (text, valid) => {
    expect(validEmail(text)).toBe(valid)
  })

  test.each([
    ['+1 (555) 010-2000', true],
    ['555.0100', true],
    ['+-()', false],
    ['555-CALL', false],
    ['1'.repeat(50), true],
    ['1'.repeat(51), false],
  ] as const)('validPhone(%p) is %p', (text, valid) => {
    expect(validPhone(text)).toBe(valid)
  })

  test('invalid stored values read as empty; valid ones as text', () => {
    expect(cell('p-email', 'r1')).toEqual({ kind: 'text', text: 'ada@example.com' })
    expect(cell('p-email', 'r3')).toBe(null)
    expect(cell('p-phone', 'r3')).toBe(null)
  })

  test('filter and sort as text', () => {
    expect(operatorsOf('email')).toEqual(operatorsOf('text'))
    expect(ids('p-email', 'contains', 'EXAMPLE')).toEqual(['r1', 'r2'])
    expect(ids('p-email', 'ends_with', '.org')).toEqual(['r2'])
    expect(ids('p-phone', 'starts_with', '555')).toEqual(['r4'])
    expect(ids('p-phone', 'is_empty')).toEqual(['r2', 'r3'])
    expect(order('p-email', 'asc')).toEqual(['r1', 'r2', 'r3', 'r4'])
    expect(order('p-email', 'desc')).toEqual(['r2', 'r1', 'r3', 'r4'])
    expect(order('p-phone', 'desc')).toEqual(['r4', 'r1', 'r2', 'r3'])
  })

  test('contactHref links by type; a phone link keeps digits and +', () => {
    expect(contactHref('email', 'ada@example.com')).toBe('mailto:ada@example.com')
    expect(contactHref('phone', '+1 (555) 010-2000')).toBe('tel:+15550102000')
  })
})

describe('files', () => {
  test('parseFiles keeps ids and names; duplicates, empty names and more than 20 files are no value', () => {
    expect(parseFiles([photo('f1', 'a.png')])).toEqual({ kind: 'files', files: [photo('f1', 'a.png')] })
    expect(parseFiles([{ ...photo('f1', 'a.png'), extra: 1 }])).toEqual({ kind: 'files', files: [photo('f1', 'a.png')] })
    expect(parseFiles([])).toBe(null)
    expect(parseFiles([photo('f1', 'a.png'), photo('f1', 'b.png')])).toBe(null)
    expect(parseFiles([photo('f1', '')])).toBe(null)
    expect(parseFiles([photo('f1', 'n'.repeat(256))])).toBe(null)
    expect(parseFiles(Array.from({ length: 20 }, (_, i) => photo(`f${i}`, 'a')))?.kind).toBe('files')
    expect(parseFiles(Array.from({ length: 21 }, (_, i) => photo(`f${i}`, 'a')))).toBe(null)
  })

  test('display and API value', () => {
    expect(displayText(property('p-files'), cell('p-files', 'r1'), ctx)).toBe('notes.pdf, face.PNG')
    expect(apiValue(cell('p-files', 'r2'))).toEqual([photo('f3', 'a.txt')])
  })

  test('filter only by empty rules; other rules match every row', () => {
    expect(operatorsOf('files')).toEqual(['is_not_empty', 'is_empty'])
    expect(ids('p-files', 'is_empty')).toEqual(['r3'])
    expect(ids('p-files', 'is_not_empty')).toEqual(['r1', 'r2', 'r4'])
    expect(ids('p-files', 'contains', 'notes')).toEqual(['r1', 'r2', 'r3', 'r4'])
  })

  test('sort by file count, empty last', () => {
    expect(order('p-files', 'asc')).toEqual(['r2', 'r1', 'r4', 'r3'])
    expect(order('p-files', 'desc')).toEqual(['r4', 'r1', 'r2', 'r3'])
  })

  test('firstImageFile picks the first image by name', () => {
    expect(firstImageFile(cell('p-files', 'r1'))).toEqual(photo('f2', 'face.PNG'))
    expect(firstImageFile(cell('p-files', 'r4'))).toEqual(photo('f4', 'x.jpg'))
    expect(firstImageFile(cell('p-files', 'r2'))).toBe(null)
    expect(firstImageFile(null)).toBe(null)
    expect(firstImageFile({ kind: 'text', text: 'a.png' })).toBe(null)
  })

  test.each([
    ['cover.webp', true],
    ['a.b.AVIF', true],
    ['photo.jpeg', true],
    ['scan.tiff', false],
    ['png', false],
    ['archive.png.zip', false],
  ] as const)('isImageName(%p) is %p', (name, image) => {
    expect(isImageName(name)).toBe(image)
  })

  test('rowFileUrl is the page file route', () => {
    expect(rowFileUrl('ws1', 'r1', 'f2')).toBe('/api/v1/workspaces/ws1/pages/r1/files/f2')
  })
})

describe('unique ID', () => {
  test('values are positive integers shown with the prefix', () => {
    expect(cell('p-id', 'r1')).toEqual({ kind: 'unique_id', number: 3 })
    expect(cell('p-id', 'r3')).toBe(null)
    expect(parseValue(property('p-id'), 2.5)).toBe(null)
    expect(displayText(property('p-id'), cell('p-id', 'r1'), ctx)).toBe('TASK-3')
    expect(uniqueIdText({ ...property('p-id'), config: { prefix: null, next: 1 } } as Property, 12)).toBe('12')
    expect(apiValue(cell('p-id', 'r4'))).toBe(4)
  })

  test('filters and sorts as a number', () => {
    expect(operatorsOf('unique_id')).toEqual(operatorsOf('number'))
    expect(ids('p-id', '>', 2)).toEqual(['r1', 'r4'])
    expect(ids('p-id', '=', 1)).toEqual(['r2'])
    expect(ids('p-id', '!=', 3)).toEqual(['r2', 'r3', 'r4'])
    expect(ids('p-id', 'is_empty')).toEqual(['r3'])
    expect(order('p-id', 'asc')).toEqual(['r2', 'r1', 'r4', 'r3'])
    expect(order('p-id', 'desc')).toEqual(['r4', 'r1', 'r2', 'r3'])
  })

  test('is computed and sortable', () => {
    expect(isComputed('unique_id')).toBe(true)
    expect(isComputed('email')).toBe(false)
    expect(sortable('unique_id')).toBe(true)
    expect(sortable('files')).toBe(true)
  })

  test('config: a valid prefix and a positive integer next, else defaults', () => {
    expect(parseUniqueIdConfig({ prefix: 'BUG_1', next: 9 })).toEqual({ prefix: 'BUG_1', next: 9 })
    expect(parseUniqueIdConfig({ prefix: 'TOO-LONG-PREFIX', next: 0 })).toEqual({ prefix: null, next: 1 })
    expect(parseUniqueIdConfig({ prefix: 'a b', next: 1.5 })).toEqual({ prefix: null, next: 1 })
    expect([validPrefix(''), validPrefix('A'.repeat(10)), validPrefix('A'.repeat(11)), validPrefix('é')]).toEqual([false, true, false, false])
  })
})
