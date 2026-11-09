import type { DatabaseProperty, DatabaseRow, DatabaseView } from './types'
import { parseDatabase } from './values'
import { applyView } from './view'

const ROWS = 5_000
const RUNS = 25
const DAY = 86_400_000
const now = Date.parse('2026-11-09T10:00:00Z')

let seed = 42
function random(): number {
  seed = (seed * 1_103_515_245 + 12_345) % 2_147_483_648
  return seed / 2_147_483_648
}
const pick = <T>(list: readonly T[]): T => list[Math.floor(random() * list.length)]!

const words = ['alpha', 'bravo', 'charlie', 'delta', 'echo', 'foxtrot', 'golf', 'hotel', 'india', 'juliet']
const options = (prefix: string, count: number) =>
  Array.from({ length: count }, (_, index) => ({ id: `${prefix}${index}`, name: `${prefix} ${words[index % 10]} ${index}`, color: 'default' }))
const members = Array.from({ length: 30 }, (_, index) => `user-${index}`)

const properties: DatabaseProperty[] = [
  { id: 'title', name: 'Name', type: 'title', config: {}, position: 0 },
  ...(['text1', 'text2', 'text3'] as const).map((id) => ({ id, name: id, type: 'text' as const, config: {} })),
  { id: 'num1', name: 'num1', type: 'number', config: { format: 'number_with_commas' } },
  { id: 'num2', name: 'num2', type: 'number', config: { format: 'currency', currency: 'USD' } },
  { id: 'sel1', name: 'sel1', type: 'select', config: { options: options('s', 12) } },
  { id: 'sel2', name: 'sel2', type: 'select', config: { options: options('k', 40) } },
  { id: 'multi1', name: 'multi1', type: 'multi_select', config: { options: options('m', 20) } },
  {
    id: 'status',
    name: 'status',
    type: 'status',
    config: {
      options: [
        { id: 'todo', name: 'Not started', color: 'default', group: 'todo' },
        { id: 'prog', name: 'In progress', color: 'blue', group: 'in_progress' },
        { id: 'done', name: 'Done', color: 'green', group: 'done' },
      ],
    },
  },
  { id: 'date1', name: 'date1', type: 'date', config: { date_format: 'relative', time_format: '12h' } },
  { id: 'date2', name: 'date2', type: 'date', config: { date_format: 'short', time_format: '24h' } },
  { id: 'person', name: 'person', type: 'person', config: {} },
  { id: 'check', name: 'check', type: 'checkbox', config: {} },
  { id: 'url', name: 'url', type: 'url', config: {} },
  { id: 'ct', name: 'ct', type: 'created_time', config: {} },
  { id: 'cb', name: 'cb', type: 'created_by', config: {} },
  { id: 'lt', name: 'lt', type: 'last_edited_time', config: {} },
  { id: 'lb', name: 'lb', type: 'last_edited_by', config: {} },
  { id: 'text4', name: 'text4', type: 'text', config: {} },
].map((property, position) => ({ ...property, position }) as DatabaseProperty)

const sentence = () => Array.from({ length: 3 + Math.floor(random() * 6) }, () => pick(words)).join(' ')
const iso = (ms: number) => new Date(ms).toISOString()

const rows: DatabaseRow[] = Array.from({ length: ROWS }, (_, index) => {
  const values: Record<string, unknown> = {}
  const maybe = (id: string, value: () => unknown) => {
    if (random() < 0.85) values[id] = value()
  }
  maybe('text1', sentence)
  maybe('text2', sentence)
  maybe('text3', sentence)
  maybe('text4', sentence)
  maybe('num1', () => Math.round(random() * 100_000) / 100)
  maybe('num2', () => Math.round(random() * 1_000))
  maybe('sel1', () => `s${Math.floor(random() * 12)}`)
  maybe('sel2', () => `k${Math.floor(random() * 40)}`)
  maybe('multi1', () => [...new Set(Array.from({ length: 1 + Math.floor(random() * 3) }, () => `m${Math.floor(random() * 20)}`))])
  maybe('status', () => pick(['todo', 'prog', 'done']))
  maybe('date1', () => ({ start: iso(now + Math.round((random() - 0.5) * 120) * DAY).slice(0, 10), end: null }))
  maybe('date2', () => ({ start: iso(now + Math.round((random() - 0.5) * 1e10)).replace(/\.\d+Z$/, 'Z'), end: null }))
  maybe('person', () => [pick(members)])
  if (random() < 0.5) values.check = true
  maybe('url', () => `https://example.com/${pick(words)}/${index}`)
  return {
    id: `row-${String(index).padStart(5, '0')}`,
    title: `${pick(words)} ${pick(words)} ${index}`,
    icon: null,
    position: index,
    version: 0,
    has_content: false,
    created_at: iso(now - Math.floor(random() * 365 * DAY)),
    created_by: pick(members),
    updated_at: iso(now - Math.floor(random() * 30 * DAY)),
    updated_by: pick(members),
    values,
  }
})

const database = parseDatabase({
  database: { id: 'db', title: 'Bench', icon: null, locked: false, row_count: ROWS },
  properties,
  views: [],
  rows,
})

const view: DatabaseView = {
  id: 'v',
  name: 'Table',
  type: 'table',
  position: 0,
  config: {
    sorts: [
      { property_id: 'status', direction: 'asc' },
      { property_id: 'date2', direction: 'desc' },
      { property_id: 'text1', direction: 'asc' },
    ],
    filter: {
      op: 'and',
      children: [
        { property_id: 'text2', operator: 'not_contains', value: 'zulu' },
        { property_id: 'num1', operator: '>=', value: 50 },
        { property_id: 'status', operator: 'is_not', value: ['done'] },
        { property_id: 'lt', operator: 'on_or_after', value: 'one_month_ago' },
        { property_id: 'person', operator: 'not_contains', value: ['me'] },
      ],
    },
  },
}

const ctx = {
  userId: 'user-0',
  now,
  timeZone: 'Europe/Budapest',
  locale: 'en-US',
  members: new Map(members.map((id, index) => [id, `Member ${words[index % 10]} ${index}`])),
}

function median(search: string): { ms: number; rows: number } {
  const times: number[] = []
  let count = 0
  for (let run = 0; run < RUNS; run++) {
    const start = performance.now()
    count = applyView(database, view, search, ctx).length
    times.push(performance.now() - start)
  }
  times.sort((a, b) => a - b)
  return { ms: times[Math.floor(RUNS / 2)]!, rows: count }
}

applyView(database, view, '', ctx)
for (const search of ['', 'echo', 'qqq']) {
  const { ms, rows: count } = median(search)
  console.log(`search ${JSON.stringify(search).padEnd(7)} median ${ms.toFixed(2)} ms over ${RUNS} runs, ${count} rows shown`)
}
