import { expect, test } from 'bun:test'
import { ApiProblem } from '@orbit/api/problem'
import { applyDraftConflict, frameHeight, mailThreadPath, mailViewPath, parseMailPath, railFolders, searchSuggestions, sendingOutboundId, sentRecipients, threadFilterOf, type MailView } from './mailLib'

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
  for (const bad of ['outgoing', 'mb1/outgoing', 'label', 'starred/extra', 'a/b/c', 't']) expect(parseMailPath(bad)).toBeNull()

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

test('parses_new_folders', () => {
  for (const folder of ['drafts', 'sent', 'scheduled', 'outbox'] as const) {
    expect(parseMailPath(folder)).toEqual({ view: { kind: 'folder', folder }, threadId: null })
    expect(parseMailPath(`${folder}/t/th1`)).toEqual({ view: { kind: 'folder', folder }, threadId: 'th1' })
    expect(parseMailPath(`mb1/${folder}`)).toEqual({ view: { kind: 'folder', folder, mailboxId: 'mb1' }, threadId: null })
    expect(parseMailPath(`mb1/${folder}/t/th1`)).toEqual({ view: { kind: 'folder', folder, mailboxId: 'mb1' }, threadId: 'th1' })
    expect(mailViewPath({ kind: 'folder', folder })).toBe(`/mail/${folder}`)
    expect(mailViewPath({ kind: 'folder', folder, mailboxId: 'mb1' })).toBe(`/mail/mb1/${folder}`)
    // Sent is a thread list; the others are not
    if (folder !== 'sent') expect(threadFilterOf({ kind: 'folder', folder })).toBeNull()
  }
  expect(threadFilterOf({ kind: 'folder', folder: 'sent' })).toEqual({ sent: true })
  expect(threadFilterOf({ kind: 'folder', folder: 'sent', mailboxId: 'mb1' })).toEqual({ mailboxId: 'mb1', sent: true })
})

test('draft_conflict_keeps_local_text', () => {
  const server = { subject: 'Server subject', body_html: '<p>server</p>', to: ['a@x.test'], version: 5 }
  const local = { subject: 'Mine', body_html: '<p>mine</p>', to: ['b@x.test'], version: 3 }
  const result = applyDraftConflict(local, server)
  // the server copy replaces the draft, the unsaved text waits in the banner
  expect(result.draft).toEqual(server)
  expect(result.restore).toEqual({ subject: 'Mine', body_html: '<p>mine</p>', to: ['b@x.test'] })
  // nothing differs: no banner
  expect(applyDraftConflict({ ...server, version: 4 }, server).restore).toBeNull()
})

test('the rail shows the Outbox only while it has something', () => {
  const names = (showOutbox: boolean) => railFolders(showOutbox).map(({ folder }) => folder)
  expect(names(false)).toEqual(['drafts', 'sent', 'scheduled', 'archive', 'spam', 'trash'])
  expect(names(true)).toEqual(['drafts', 'sent', 'scheduled', 'outbox', 'archive', 'spam', 'trash'])
})

test('a Sent row names the recipients of the newest message sent', () => {
  const thread = (sent_to: string[], sent_to_more: number) => ({ sent_to, sent_to_more, from: [{ address: 'me@x.test', name: 'Me' }] })
  expect(sentRecipients(thread(['ana@x.test'], 0))).toBe('To: ana@x.test')
  expect(sentRecipients(thread(['ana@x.test', 'bob@x.test'], 3))).toBe('To: ana@x.test, bob@x.test +3')
  // nothing sent (or not known): the senders, as in other lists
  expect(sentRecipients(thread([], 0))).toBe('Me')
})

test('a reply being sent: the 409 names the message to undo', () => {
  const problem = (code: string, extra: Record<string, unknown> = {}) =>
    new ApiProblem({ type: 'about:blank', title: 'Conflict', status: 409, code, detail: 'x', instance: '/x', request_id: 'r', ...extra })
  expect(sendingOutboundId(problem('draft_sending', { outbound_id: 'ob1' }))).toBe('ob1')
  expect(sendingOutboundId(problem('draft_sending'))).toBeNull()
  expect(sendingOutboundId(problem('draft_conflict', { outbound_id: 'ob1' }))).toBeNull()
  expect(sendingOutboundId(new Error('x'))).toBeNull()
})
