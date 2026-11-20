import { expect, test } from 'bun:test'

import type { MailThreadSummary } from '@orbit/api/generated/types.gen'

import { mailKeys } from './keys'
import { changedThread, filterOfKey, groupThreads, mailEventKeys, mailRoute, parseTab, patchRows, shownTabs, splitTextQuote, threadActions, threadSenders, sentRecipients } from './mail'

const thread = (fields: Partial<MailThreadSummary> = {}): MailThreadSummary => ({
  id: 't1',
  folder: 'inbox',
  from: [{ address: 'ana@x.test', name: 'Ana Lee' }],
  has_attachments: false,
  labels: [],
  last_message_at: '2026-11-20T09:00:00Z',
  mailbox_id: 'm1',
  message_count: 1,
  sent_to: [],
  sent_to_more: 0,
  snippet: '',
  starred: false,
  subject: 'Plan',
  unread: true,
  ...fields,
})

test('the tabs follow the web phone layout; the Outbox only when it has something', () => {
  expect(shownTabs(false)).toEqual(['inbox', 'starred', 'drafts', 'sent', 'scheduled', 'archive', 'spam', 'trash'])
  expect(shownTabs(true)).toContain('outbox')
  expect(parseTab('archive')).toBe('archive')
  expect(parseTab('label')).toBe('inbox')
  expect(parseTab(undefined)).toBe('inbox')
})

test('a changed thread leaves a list it no longer fits; a search keeps it', () => {
  const rows = [thread(), thread({ id: 't2' })]
  const archive = (row: MailThreadSummary) => (row.id === 't1' ? changedThread(row, { folder: 'archive' }) : null)
  expect(patchRows(rows, { folder: 'inbox' }, archive).map((row) => row.id)).toEqual(['t2'])
  expect(patchRows(rows, null, archive).map((row) => row.folder)).toEqual(['archive', 'inbox'])
  const unstar = (row: MailThreadSummary) => changedThread(row, { starred: false })
  expect(patchRows([thread({ starred: true, folder: 'archive' })], { starred: true }, unstar)).toEqual([])
  // a row read in the Unread list stays until the list is read again, as on the web
  expect(patchRows(rows, { folder: 'inbox', unread: true }, (row) => changedThread(row, { read: true }))).toHaveLength(2)
  expect(filterOfKey(mailKeys.threads('w', { folder: 'spam' }))).toEqual({ folder: 'spam' })
  expect(filterOfKey(mailKeys.search('w', 'from:ana'))).toBe(null)
})

test('a change sets the folder, labels, read and star; a restore guesses the inbox', () => {
  expect(changedThread(thread({ labels: ['a', 'b'] }), { add_labels: ['c', 'a'], remove_labels: ['b'], read: true, starred: true })).toMatchObject({ labels: ['a', 'c'], unread: false, starred: true })
  expect(changedThread(thread({ folder: 'trash' }), { restore: true }).folder).toBe('inbox')
})

test('the thread actions by folder', () => {
  expect(threadActions('inbox')).toEqual({ archive: 'archive', spam: 'spam', trash: 'trash' })
  expect(threadActions('archive')).toEqual({ archive: 'unarchive', spam: 'spam', trash: 'trash' })
  expect(threadActions('spam')).toEqual({ archive: null, spam: 'not-spam', trash: 'trash' })
  expect(threadActions('trash')).toEqual({ archive: null, spam: null, trash: 'restore' })
})

test('senders and Sent recipients of a row', () => {
  expect(threadSenders(thread({ from: [{ address: 'ana@x.test', name: 'Ana Lee' }, { address: 'bob@x.test' }] }))).toBe('Ana, bob@x.test')
  expect(sentRecipients(thread({ sent_to: ['a@x.test', 'b@x.test'], sent_to_more: 3 }))).toBe('To: a@x.test, b@x.test +3')
})

test('a plain body folds its trailing quote', () => {
  expect(splitTextQuote('Thanks!\n\nOn Mon, Ana wrote:\n> Plan?\n> Yes\n')).toEqual({ body: 'Thanks!', quote: 'On Mon, Ana wrote:\n> Plan?\n> Yes' })
  expect(splitTextQuote('> only a quote')).toEqual({ body: '> only a quote', quote: null })
})

test('rows get date groups, a search none', () => {
  const now = new Date(2026, 10, 20, 12)
  const rows = [thread({ id: 'a', last_message_at: new Date(2026, 10, 20, 8).toISOString() }), thread({ id: 'b', last_message_at: new Date(2026, 10, 19, 8).toISOString() }), thread({ id: 'c', last_message_at: new Date(2026, 8, 3).toISOString() })]
  expect(groupThreads(rows, {}, now).map((entry) => (entry.kind === 'group' ? entry.label : entry.key))).toEqual(['Today', 'a', 'Yesterday', 'b', 'September', 'c'])
  expect(groupThreads(rows, { grouped: false }, now).map((entry) => entry.key)).toEqual(['a', 'b', 'c'])
})

test('a live mail event marks the lists, counts, send views and its thread stale', () => {
  expect(mailEventKeys({ type: 'thread.updated', mailbox_id: 'm', thread_id: 't9' }, 'w')).toEqual([mailKeys.lists('w'), mailKeys.counts('w'), mailKeys.outboundAll('w'), mailKeys.drafts('w'), mailKeys.thread('w', 't9')])
  expect(mailEventKeys({ type: 'outbound.updated', mailbox_id: 'm', outbound_id: 'o' }, 'w')).toHaveLength(4)
  expect(mailEventKeys({ type: 'thread.updated', mailbox_id: 'm' }, 'w')).toEqual([])
  expect(mailEventKeys('garbage', 'w')).toEqual([])
})

test('web mail paths open the thread or the tab', () => {
  expect(mailRoute([])).toBe('/mail')
  expect(mailRoute(['archive'])).toBe('/mail?tab=archive')
  expect(mailRoute(['m1', 'spam', 't', 'th 1'])).toBe('/mail/th%201')
  expect(mailRoute(['m1', 'sent'])).toBe('/mail?tab=sent')
  expect(mailRoute(['label', 'l1'])).toBe('/mail')
})
