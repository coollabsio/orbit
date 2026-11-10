import { describe, expect, test } from 'bun:test'

import type { PageFavorite } from '@orbit/api/generated/types.gen'

import { absoluteUrl, blockId, descendantsOf, docSpaces, favoritePages, iconOf, moveRequest, pageTitle, paragraphBlocks, searchHint, spaceRequest, subPagesOf, trashPrompt, validCoverUrl, visibleThreads } from './docs'
import type { PageSummary, Teamspace } from './orbit'

const page = (id: string, fields: Partial<PageSummary>): PageSummary => ({
  id,
  title: id,
  icon: null,
  kind: 'page',
  parent_id: null,
  position: 0,
  private: false,
  teamspace_id: 'ts-general',
  updated_at: '2026-10-15T10:00:00Z',
  version: 1,
  ...fields,
})

const teamspace = (id: string, name: string, position: number): Teamspace => ({
  id,
  name,
  position,
  icon: null,
  is_default: position === 0,
  version: 0,
  workspace_id: 'w',
  created_at: '2026-10-15T10:00:00Z',
  updated_at: '2026-10-15T10:00:00Z',
})

describe('docSpaces', () => {
  const pages = [
    page('roadmap', { position: 2 }),
    page('notes', { position: 1 }),
    page('q4', { parent_id: 'roadmap', position: 0 }),
    page('design-doc', { teamspace_id: 'ts-design' }),
    page('diary', { private: true, teamspace_id: null }),
  ]
  const teamspaces = [teamspace('ts-design', 'Design', 1), teamspace('ts-general', 'General', 0)]

  test('teamspaces by position, pages depth first by position, private pages apart', () => {
    const spaces = docSpaces(pages, teamspaces, new Set())
    expect(spaces.teamspaces.map((space) => [space.name, space.rows.map((row) => `${'-'.repeat(row.depth)}${row.page.id}${row.hasChildren ? '+' : ''}`)])).toEqual([
      ['General', ['notes', 'roadmap+', '-q4']],
      ['Design', ['design-doc']],
    ])
    expect(spaces.private.rows.map((row) => row.page.id)).toEqual(['diary'])
  })

  test('a collapsed page hides its children', () => {
    expect(docSpaces(pages, teamspaces, new Set(['roadmap'])).teamspaces[0]!.rows.map((row) => row.page.id)).toEqual(['notes', 'roadmap'])
  })
})

describe('Move to', () => {
  const tree = [
    page('a', { position: 0 }),
    page('b', { position: 1 }),
    page('a1', { parent_id: 'a' }),
    page('db', { kind: 'database', parent_id: 'a', position: 1 }),
    page('diary', { private: true, teamspace_id: null }),
  ]

  test('a page or database destination puts the page last among its children, the moving page not counted', () => {
    expect(moveRequest(tree, 'b', { kind: 'page', page: tree[0]! })).toEqual({ parent_id: 'a', position: 2 })
    expect(moveRequest(tree, 'a1', { kind: 'page', page: tree[0]! })).toEqual({ parent_id: 'a', position: 1 })
    expect(moveRequest(tree, 'row', { kind: 'page', page: tree[3]! })).toEqual({ parent_id: 'db', position: 0 })
  })

  test('a space destination puts the page last among its root pages', () => {
    expect(moveRequest(tree, 'a1', { kind: 'space', space: 'teamspace:ts-general' })).toEqual({ parent_id: null, position: 2, teamspace_id: 'ts-general' })
    expect(moveRequest(tree, 'b', { kind: 'space', space: 'teamspace:ts-general' })).toEqual({ parent_id: null, position: 1, teamspace_id: 'ts-general' })
    expect(moveRequest(tree, 'a1', { kind: 'space', space: 'private' })).toEqual({ parent_id: null, position: 1, private: true })
  })
})

test('a search result names its database, or the space of a page', () => {
  const spaces = [teamspace('ts-general', 'General', 0)]
  expect(searchHint({ database_title: 'Research', private: false, teamspace_id: 'ts-general' }, spaces)).toBe('Row · Research')
  expect(searchHint({ database_title: '', private: false, teamspace_id: 'ts-general' }, spaces)).toBe('Row · Untitled')
  expect(searchHint({ database_title: null, private: false, teamspace_id: 'ts-general' }, spaces)).toBe('Page · General')
  expect(searchHint({ database_title: null, private: true, teamspace_id: null }, spaces)).toBe('Page · Private')
  expect(searchHint({ database_title: null, private: false, teamspace_id: 'gone' }, spaces)).toBe('Page · Teamspace')
})

test('an empty title reads Untitled', () => {
  expect(pageTitle({ title: '  ' })).toBe('Untitled')
})

describe('writing docs', () => {
  const tree = [page('a', {}), page('b', { parent_id: 'a' }), page('c', { parent_id: 'b' }), page('d', {})]

  test('a new page goes to the default teamspace, a teamspace, or Private', () => {
    expect(spaceRequest(null)).toEqual({})
    expect(spaceRequest('private')).toEqual({ private: true })
    expect(spaceRequest('teamspace:ts-1')).toEqual({ teamspace_id: 'ts-1' })
  })

  test('trashing a page says how many sub-pages go with it', () => {
    expect(descendantsOf(tree, 'a').map((item) => item.id)).toEqual(['b', 'c'])
    expect(trashPrompt(tree[0]!, 2)).toEqual({ title: 'Move “a” to the trash?', description: 'Its 2 sub-pages move with it. You can restore them from Trash.' })
    expect(trashPrompt(tree[3]!, 0).description).toBe('You can restore it from Trash.')
  })

  test('sub-pages come from the tree, or from the server for a page outside it (a database row)', async () => {
    const asked: string[] = []
    const server = async (id: string) => {
      asked.push(id)
      return [page('notes', { parent_id: id }), page('deep', { parent_id: 'notes' })]
    }
    expect((await subPagesOf(tree, 'a', server)).map((item) => item.id)).toEqual(['b', 'c'])
    expect(asked).toEqual([])
    expect((await subPagesOf(tree, 'row', server)).map((item) => item.id)).toEqual(['notes', 'deep'])
    expect(asked).toEqual(['row'])
  })

  test('favorites list in their order from the server entries; tree pages show their tree summary', () => {
    const entry = (page_id: string, position: number, patch: Partial<PageFavorite> = {}): PageFavorite => ({
      page_id, position, title: page_id, icon: null, kind: 'page', database_id: null, database_title: null, ...patch,
    })
    const renamed = [...tree.slice(0, 3), page('d', { title: 'D renamed' })]
    expect(
      favoritePages(renamed, [entry('d', 0), entry('row', 1, { title: 'Paper', icon: '📄', database_id: 'db', database_title: 'Research' }), entry('b', 2)]),
    ).toEqual([
      { id: 'd', title: 'D renamed', icon: null, kind: 'page', hint: null, page: renamed[3] },
      { id: 'row', title: 'Paper', icon: '📄', kind: 'page', hint: 'Research', page: null },
      { id: 'b', title: 'b', icon: null, kind: 'page', hint: null, page: renamed[1] },
    ])
  })

  test('typed text becomes one BlockNote paragraph per line', () => {
    let next = 0
    expect(paragraphBlocks('First line\n\n  \nSecond  ', () => `id-${++next}`)).toEqual([
      { id: 'id-1', type: 'paragraph', props: { backgroundColor: 'default', textAlignment: 'left', textColor: 'default' }, content: [{ type: 'text', text: 'First line', styles: {} }], children: [] },
      { id: 'id-2', type: 'paragraph', props: { backgroundColor: 'default', textAlignment: 'left', textColor: 'default' }, content: [{ type: 'text', text: 'Second', styles: {} }], children: [] },
    ])
  })

  test('block ids are v4 UUIDs', () => {
    expect(blockId(() => 0)).toBe('00000000-0000-4000-8000-000000000000')
    expect(blockId()).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/)
  })

  test('icons and covers', () => {
    expect([iconOf('  '), iconOf('🚀 launch'), iconOf('👩‍💻x')]).toEqual([null, '🚀', '👩‍💻'])
    expect([validCoverUrl(' https://img.example.com/a.png '), validCoverUrl('javascript:alert(1)'), validCoverUrl('')]).toEqual(['https://img.example.com/a.png', null, null])
    expect([absoluteUrl('https://orbit.test', '/api/v1/files/1'), absoluteUrl('https://orbit.test', 'https://cdn.test/x.png')]).toEqual(['https://orbit.test/api/v1/files/1', 'https://cdn.test/x.png'])
  })

  test('comment threads: only those with a comment left, open first, latest first', () => {
    const thread = (id: string, resolved: boolean, updated: string, deleted = false) => ({ id, resolved, updated_at: updated, comments: [{ deleted_at: deleted ? updated : null }] })
    const list = [thread('old', false, '2026-10-01'), thread('done', true, '2026-10-09'), thread('new', false, '2026-10-05'), thread('empty', false, '2026-10-10', true)]
    expect(visibleThreads(list).map((item) => item.id)).toEqual(['new', 'old', 'done'])
  })
})
