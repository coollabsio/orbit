import { describe, expect, test } from 'bun:test'

import { absoluteUrl, blockId, descendantsOf, docSpaces, favoritePages, iconOf, pageTitle, paragraphBlocks, spaceRequest, trashPrompt, validCoverUrl, visibleThreads } from './docs'
import type { PageSummary, Teamspace } from './orbit'

const page = (id: string, fields: Partial<PageSummary>): PageSummary => ({
  id,
  title: id,
  icon: null,
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
    expect(trashPrompt(tree, tree[0])).toEqual({ title: 'Move “a” to the trash?', description: 'Its 2 sub-pages move with it. You can restore them from Trash.' })
    expect(trashPrompt(tree, tree[3]).description).toBe('You can restore it from Trash.')
  })

  test('favorites keep their order and drop pages that are gone', () => {
    expect(favoritePages(tree, ['d', 'gone', 'b']).map((item) => item.id)).toEqual(['d', 'b'])
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
