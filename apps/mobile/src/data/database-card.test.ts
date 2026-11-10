import { expect, test } from 'bun:test'
import { ApiProblem } from '@orbit/api/problem'
import type { DatabaseDetail } from '@orbit/api/generated/types.gen'

import { databaseBlocks, databaseCard, databaseHref } from './database-card'

const own = { databaseId: 'db1', linkId: '' }
const linked = { databaseId: 'db1', linkId: 'ln1' }
const none = { trashed: new Map(), readable: new Set<string>() }
const problem = (status: number, code: string) => new ApiProblem({ type: 'about:blank', title: 'x', status, code, detail: 'x', instance: '/', request_id: 'r1' } as never)

const detail = {
  database: { id: 'db1', title: 'Tasks', icon: '📋', locked: false, row_count: 12 },
  properties: [],
  rows: [],
  views: [
    { id: 'v2', name: 'Board', position: 2, type: 'board', config: {} },
    { id: 'v1', name: 'All tasks', position: 1, type: 'table', config: {} },
  ],
} as unknown as DatabaseDetail

test('a loaded database shows its title, icon, row count and first view', () => {
  expect(databaseCard(linked, { data: detail, error: null }, none)).toEqual({ state: 'ready', title: 'Tasks', icon: '📋', rowCount: 12, view: { id: 'v1', name: 'All tasks' } })
})

test('a database in the trash says so, even though the server answers 404', () => {
  const pages = { trashed: new Map([['db1', { title: 'Old', icon: null }]]), readable: new Set<string>() }
  expect(databaseCard(own, { data: undefined, error: problem(404, 'page_not_found') }, pages)).toEqual({ state: 'trashed', title: 'Old', icon: null })
})

test('a 404 is a gone link, no access, or a page that is not a database', () => {
  expect(databaseCard(linked, { data: undefined, error: problem(404, 'database_link_not_found') }, none)).toEqual({ state: 'link-gone' })
  expect(databaseCard(own, { data: undefined, error: problem(404, 'page_not_found') }, none)).toEqual({ state: 'no-access' })
  expect(databaseCard(own, { data: undefined, error: problem(404, 'page_not_found') }, { ...none, readable: new Set(['db1']) })).toEqual({ state: 'missing' })
})

test('a block with no database is missing; a pending fetch loads; another failure is a failure', () => {
  expect(databaseCard({ databaseId: '', linkId: '' }, undefined, none)).toEqual({ state: 'missing' })
  expect(databaseCard(own, { data: undefined, error: null }, none)).toEqual({ state: 'loading' })
  expect(databaseCard(own, { data: undefined, error: problem(500, 'internal') }, none)).toEqual({ state: 'failed' })
  expect(databaseCard(own, { data: undefined, error: new TypeError('Network request failed') }, none)).toEqual({ state: 'failed' })
})

test('the document lists each database block once, nested ones too, and skips empty ones', () => {
  const blocks = [
    { type: 'paragraph', children: [{ type: 'database', props: { databaseId: 'db2', linkId: '' } }] },
    { type: 'database', props: { databaseId: 'db1', linkId: 'ln1' } },
    { type: 'database', props: { databaseId: 'db1', linkId: 'ln1' } },
    { type: 'database', props: { databaseId: 'db1', linkId: '' } },
    { type: 'database', props: { databaseId: '', linkId: '' } },
  ]
  expect(databaseBlocks(blocks)).toEqual([
    { databaseId: 'db2', linkId: '' },
    { databaseId: 'db1', linkId: 'ln1' },
    { databaseId: 'db1', linkId: '' },
  ])
})

test('a card opens the database screen on its link and view', () => {
  expect(databaseHref(linked, 'v1')).toBe('/docs/db1?link=ln1&view=v1')
  expect(databaseHref(own, 'v1')).toBe('/docs/db1?view=v1')
  expect(databaseHref(own, null)).toBe('/docs/db1')
})
