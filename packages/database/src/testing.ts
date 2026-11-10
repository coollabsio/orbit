import type { Database, DatabaseProperty, DatabaseRow, Property, ViewContext } from './types'
import { parseDatabase } from './values'

export const ctx: ViewContext = {
  userId: 'u-me',
  now: Date.parse('2026-11-09T10:00:00Z'),
  timeZone: 'America/New_York',
  locale: 'en-US',
  members: new Map([
    ['u-me', 'Ada Lovelace'],
    ['u-bob', 'Bob Stone'],
    ['u-cy', 'Cy Young'],
  ]),
}

export const properties: DatabaseProperty[] = [
  { id: 'p-title', name: 'Name', type: 'title', config: {}, position: 0 },
  { id: 'p-text', name: 'Notes', type: 'text', config: {}, position: 1 },
  { id: 'p-num', name: 'Score', type: 'number', config: { format: 'number' }, position: 2 },
  {
    id: 'p-sel',
    name: 'Kind',
    type: 'select',
    config: {
      options: [
        { id: 'o-a', name: 'Zeta', color: 'red' },
        { id: 'o-b', name: 'Alpha', color: 'blue' },
        { id: 'o-c', name: 'Mid', color: 'green' },
      ],
    },
    position: 3,
  },
  {
    id: 'p-multi',
    name: 'Tags',
    type: 'multi_select',
    config: {
      options: [
        { id: 't-x', name: 'x-ray', color: 'default' },
        { id: 't-y', name: 'yankee', color: 'default' },
        { id: 't-z', name: 'zulu', color: 'default' },
      ],
    },
    position: 4,
  },
  {
    id: 'p-status',
    name: 'Status',
    type: 'status',
    config: {
      options: [
        { id: 's-todo', name: 'Not started', color: 'default', group: 'todo' },
        { id: 's-prog', name: 'In progress', color: 'blue', group: 'in_progress' },
        { id: 's-done', name: 'Done', color: 'green', group: 'done' },
      ],
    },
    position: 5,
  },
  { id: 'p-date', name: 'Due', type: 'date', config: { date_format: 'short', time_format: '24h' }, position: 6 },
  { id: 'p-person', name: 'Owner', type: 'person', config: {}, position: 7 },
  { id: 'p-check', name: 'Done?', type: 'checkbox', config: {}, position: 8 },
  { id: 'p-url', name: 'Link', type: 'url', config: {}, position: 9 },
  { id: 'p-ct', name: 'Created', type: 'created_time', config: {}, position: 10 },
  { id: 'p-cb', name: 'Created by', type: 'created_by', config: {}, position: 11 },
  { id: 'p-lt', name: 'Edited', type: 'last_edited_time', config: {}, position: 12 },
  { id: 'p-lb', name: 'Edited by', type: 'last_edited_by', config: {}, position: 13 },
]

export function row(
  id: string,
  position: number,
  title: string,
  values: Record<string, unknown>,
  page: Partial<DatabaseRow> = {},
): DatabaseRow {
  return {
    id,
    position,
    title,
    values,
    icon: null,
    version: 0,
    has_content: false,
    created_at: '2026-11-01T12:00:00.000Z',
    created_by: 'u-me',
    updated_at: '2026-11-01T12:00:00.000Z',
    updated_by: 'u-me',
    ...page,
  }
}

export const rows: DatabaseRow[] = [
  row(
    'r1',
    0,
    'Apple pie',
    {
      'p-text': 'Sweet Crust',
      'p-num': 10,
      'p-sel': 'o-a',
      'p-multi': ['t-x', 't-y'],
      'p-status': 's-done',
      'p-date': { start: '2026-11-09', end: null },
      'p-person': ['u-me'],
      'p-check': true,
      'p-url': 'https://example.com/a',
    },
    { updated_at: '2026-11-09T04:30:00.000Z', updated_by: 'u-bob' },
  ),
  row(
    'r2',
    1,
    'banana',
    {
      'p-text': 'crust',
      'p-num': -2.5,
      'p-sel': 'o-b',
      'p-multi': ['t-z'],
      'p-status': 's-prog',
      'p-date': { start: '2026-11-10T03:00:00Z', end: null },
      'p-person': ['u-bob', 'u-cy'],
      'p-url': 'http://Example.org',
    },
    {
      created_at: '2026-10-09T12:00:00.000Z',
      created_by: 'u-bob',
      updated_at: '2026-11-09T15:00:00.000Z',
      updated_by: 'u-me',
    },
  ),
  row(
    'r3',
    2,
    '',
    {},
    {
      created_at: '2026-11-08T12:00:00.000Z',
      created_by: 'u-cy',
      updated_at: '2026-11-08T12:00:00.000Z',
      updated_by: 'u-cy',
    },
  ),
  row(
    'r4',
    3,
    'Cherry',
    { 'p-text': 'ÉCLAIR', 'p-num': 0, 'p-sel': 'o-c', 'p-date': { start: '2026-11-02', end: '2026-11-12' } },
    { created_at: '2026-11-02T12:00:00.000Z', updated_at: '2026-11-02T12:00:00.000Z', updated_by: 'u-bob' },
  ),
]

export const database: Database = parseDatabase({
  database: { id: 'db', title: 'Research', icon: null, locked: false, row_count: rows.length },
  properties,
  views: [{ id: 'v1', name: 'Table', type: 'table', config: {}, position: 0 }],
  rows,
})

export function property(id: string): Property {
  const found = database.properties.find((candidate) => candidate.id === id)
  if (!found) throw new Error(`no property ${id}`)
  return found
}
