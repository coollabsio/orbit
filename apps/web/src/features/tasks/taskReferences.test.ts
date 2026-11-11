import { expect, test } from 'bun:test'
import { looksLikeTaskId, rankTaskReferences } from './taskReferences'

test('looksLikeTaskId accepts identifiers, their start and numbers, not words', () => {
  for (const text of ['ENG-12', 'eng-1', 'eng-', '#12', '12', ' ENG-3 ']) expect(looksLikeTaskId(text)).toBe(true)
  for (const text of ['eng', 'login', 'fix-login', '12-3', '', 'ENG 12']) expect(looksLikeTaskId(text)).toBe(false)
})

test('rankTaskReferences puts the exact ID first, then IDs that start with the query, then title hits', () => {
  const refs = [
    { identifier: 'ENG-120', title: 'Other' },
    { identifier: 'OPS-4', title: 'Mentions ENG-12 in text' },
    { identifier: 'ENG-12', title: 'The one' },
  ]
  expect(rankTaskReferences(refs, 'eng-12').map((ref) => ref.identifier)).toEqual(['ENG-12', 'ENG-120', 'OPS-4'])
  expect(rankTaskReferences(refs, '#12')[0]!.identifier).toBe('ENG-12')
})

test('rankTaskReferences prefers titles that start with the query, and keeps the server order otherwise', () => {
  const refs = [
    { identifier: 'A-1', title: 'Description hit' },
    { identifier: 'A-2', title: 'Fix the login' },
    { identifier: 'A-3', title: 'Login redirect' },
  ]
  expect(rankTaskReferences(refs, 'login').map((ref) => ref.identifier)).toEqual(['A-3', 'A-2', 'A-1'])
  expect(rankTaskReferences(refs, '').map((ref) => ref.identifier)).toEqual(['A-1', 'A-2', 'A-3'])
})

test('searchTaskReferences always lists the task a full ID names, even when the latest hits leave it out', async () => {
  const { QueryClient } = await import('@tanstack/react-query')
  const { searchTaskReferences } = await import('./taskReferences')
  const record = (id: string, number: number, title: string) => ({
    id, number, title, workspace_id: 'w1', project_id: 'p1', status_id: 's1', description: '', position: 0, priority: 'none',
    assignee_ids: [], creator_id: 'u1', label_ids: [], created_at: '2026-11-01T00:00:00Z', updated_at: '2026-11-01T00:00:00Z', version: 1,
  })
  const original = globalThis.fetch
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const url = new URL(String(input instanceof Request ? input.url : input))
    if (url.pathname.endsWith('/statuses')) return Response.json({ items: [], next_cursor: null })
    if (url.pathname.endsWith('/projects')) return Response.json({ items: [{ id: 'p1', key: 'ENG', name: 'Engineering', color: '#888', version: 1 }], next_cursor: null })
    if (url.pathname.endsWith('/tasks/ENG-12')) return Response.json(record('t12', 12, 'The one'))
    // the latest hits mention "ENG-12" in their text but are other tasks
    return Response.json({ items: Array.from({ length: 8 }, (_, index) => record(`t${index}`, 100 + index, `About ENG-12 ${index}`)), next_cursor: null })
  }) as unknown as typeof fetch
  try {
    const references = await searchTaskReferences(new QueryClient(), 'w1', 'ENG-12')
    expect(references[0]!.identifier).toBe('ENG-12')
    expect(references).toHaveLength(8)
  } finally {
    globalThis.fetch = original
  }
})
