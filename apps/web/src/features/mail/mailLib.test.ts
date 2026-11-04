import { expect, test } from 'bun:test'
import { frameHeight, mailThreadPath, mailViewPath, parseMailPath, searchSuggestions, threadFilterOf, type MailView } from './mailLib'

test('a message frame takes only a finite height from its own window, clamped to 80–20,000 px', () => {
  const frame = { name: 'frame' }
  const other = { name: 'other page' }
  expect(frameHeight({ source: frame, data: { orbitMailHeight: 640.2 } }, frame)).toBe(641)
  expect(frameHeight({ source: frame, data: { orbitMailHeight: 12 } }, frame)).toBe(80)
  expect(frameHeight({ source: frame, data: { orbitMailHeight: 1e9 } }, frame)).toBe(20_000)
  // another window, no frame window yet, or no number
  expect(frameHeight({ source: other, data: { orbitMailHeight: 300 } }, frame)).toBeNull()
  expect(frameHeight({ source: null, data: { orbitMailHeight: 300 } }, null)).toBeNull()
  expect(frameHeight({ source: undefined, data: { orbitMailHeight: 300 } }, undefined)).toBeNull()
  for (const data of [{ orbitMailHeight: Number.NaN }, { orbitMailHeight: Infinity }, { orbitMailHeight: '300' }, 300, null, 'orbitMailHeight']) {
    expect(frameHeight({ source: frame, data }, frame)).toBeNull()
  }
})

test('mail paths name a view and an open thread, both ways', () => {
  const views: MailView[] = [
    { kind: 'folder', folder: 'inbox' },
    { kind: 'folder', folder: 'trash' },
    { kind: 'folder', folder: 'archive', mailboxId: 'mb1' },
    { kind: 'folder', folder: 'inbox', mailboxId: 'mb1' },
    { kind: 'label', labelId: 'lb1' },
    { kind: 'starred' },
    { kind: 'search' },
  ]
  for (const view of views) {
    expect(parseMailPath(mailViewPath(view).replace(/^\/mail\/?/, ''))).toEqual({ view, threadId: null })
    expect(parseMailPath(mailThreadPath(view, 'th1').replace(/^\/mail\/?/, ''))).toEqual({ view, threadId: 'th1' })
  }
  expect(mailViewPath({ kind: 'folder', folder: 'inbox' })).toBe('/mail')
  expect(mailThreadPath({ kind: 'folder', folder: 'inbox' }, 'th1')).toBe('/mail/t/th1')
  for (const bad of ['sent', 'mb1/sent', 'label', 'starred/extra', 'a/b/c', 't']) expect(parseMailPath(bad)).toBeNull()

  expect(threadFilterOf({ kind: 'folder', folder: 'spam', mailboxId: 'mb1' })).toEqual({ mailboxId: 'mb1', folder: 'spam' })
  expect(threadFilterOf({ kind: 'label', labelId: 'lb1' })).toEqual({ labelId: 'lb1' })
  expect(threadFilterOf({ kind: 'starred' })).toEqual({ starred: true })
  expect(threadFilterOf({ kind: 'search' })).toBeNull()
})

test('search completes the word being typed and keeps the rest of the query', () => {
  const known = { labels: ['Billing', 'Two words'], people: ['ana@example.com', 'bob@example.com'] }
  const queries = (q: string) => searchSuggestions(q, known).map((suggestion) => suggestion.query)
  expect(queries('')).toEqual([])
  expect(queries('invoice ')).toEqual([])
  expect(queries('invoice f')).toEqual(['invoice from:'])
  expect(queries('invoice in:ar')).toEqual(['invoice in:archive '])
  expect(queries('is:')).toEqual(['is:unread ', 'is:starred '])
  expect(queries('HAS:a')).toEqual(['has:attachment '])
  expect(queries('label:t')).toEqual(['label:"Two words" '])
  expect(queries('label:"two w')).toEqual(['label:"Two words" '])
  expect(queries('from:b')).toEqual(['from:bob@example.com '])
  // a finished value, an unknown operator and the date operators have no values to offer
  expect(queries('in:archive')).toEqual([])
  expect(queries('size:big')).toEqual([])
  expect(queries('before:2026')).toEqual([])
  // a quoted phrase is one word
  expect(queries('subject:"big news" i')).toEqual(['subject:"big news" in:', 'subject:"big news" is:'])
})
