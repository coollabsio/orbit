import { expect, test } from 'bun:test'
import { type InfiniteData, QueryClient } from '@tanstack/react-query'
import type { MailCounts, MailThreadDetail, MailThreadPage, MailThreadSummary } from '@orbit/api/generated/types.gen'
import { mailKeys } from './keys'
import { applyThreadChange, restoreMailSnapshot } from './optimistic'

const W = 'w1'

const summary = (id: string, overrides: Partial<MailThreadSummary> = {}): MailThreadSummary => ({
  id, mailbox_id: 'mb1', folder: 'inbox', from: [], has_attachments: false, labels: [], last_message_at: '2026-10-23T10:00:00Z',
  message_count: 1, snippet: '', starred: false, subject: id, unread: false, ...overrides,
})
const pages = (...items: MailThreadSummary[]): InfiniteData<MailThreadPage> => ({
  pages: [{ items, cursor: null }],
  pageParams: [undefined],
})

function setup() {
  const client = new QueryClient()
  client.setQueryData(mailKeys.threads(W, { folder: 'inbox' }), pages(summary('t1', { unread: true }), summary('t2')))
  client.setQueryData(mailKeys.threads(W, { starred: true }), pages(summary('t1', { unread: true, starred: true })))
  client.setQueryData(mailKeys.search(W, 'hello'), pages(summary('t1', { unread: true })))
  client.setQueryData<MailThreadDetail>(mailKeys.thread(W, 't1'), { thread: summary('t1', { unread: true }), messages: [] })
  client.setQueryData<MailCounts>(mailKeys.counts(W), {
    mailboxes: [{ mailbox_id: 'mb1', inbox: 1, archive: 0, spam: 0, trash: 0 }],
  })
  const ids = (key: readonly unknown[]) =>
    client.getQueryData<InfiniteData<MailThreadPage>>(key)!.pages.flatMap((page) => page.items.map((item) => item.id))
  return { client, ids }
}

test('a move leaves the lists it no longer fits, moves the unread count, and rolls back', () => {
  const { client, ids } = setup()
  const before = client.getQueryCache().getAll().map((query) => [query.queryKey, query.state.data])
  const snapshot = applyThreadChange(client, W, ['t1'], { folder: 'trash' })

  expect(ids(mailKeys.threads(W, { folder: 'inbox' }))).toEqual(['t2'])
  expect(ids(mailKeys.threads(W, { starred: true }))).toEqual([])
  // a search keeps its rows until it is read again
  expect(ids(mailKeys.search(W, 'hello'))).toEqual(['t1'])
  expect(client.getQueryData<MailThreadDetail>(mailKeys.thread(W, 't1'))!.thread.folder).toBe('trash')
  expect(client.getQueryData<MailCounts>(mailKeys.counts(W))!.mailboxes[0]).toMatchObject({ inbox: 0, trash: 1 })

  restoreMailSnapshot(client, snapshot)
  expect(client.getQueryCache().getAll().map((query) => [query.queryKey, query.state.data])).toEqual(before)
})

test('read, star and labels patch every copy of the thread', () => {
  const { client } = setup()
  applyThreadChange(client, W, ['t1'], { read: true, starred: true, add_labels: ['l1'] })
  const detail = client.getQueryData<MailThreadDetail>(mailKeys.thread(W, 't1'))!.thread
  expect(detail).toMatchObject({ unread: false, starred: true, labels: ['l1'] })
  const row = client.getQueryData<InfiniteData<MailThreadPage>>(mailKeys.search(W, 'hello'))!.pages[0].items[0]
  expect(row).toMatchObject({ unread: false, starred: true, labels: ['l1'] })
  expect(client.getQueryData<MailCounts>(mailKeys.counts(W))!.mailboxes[0].inbox).toBe(0)
})

test('unstarring leaves the starred list; a restore leaves the trash list', () => {
  const { client, ids } = setup()
  client.setQueryData(mailKeys.threads(W, { folder: 'trash' }), pages(summary('t3', { folder: 'trash' })))
  applyThreadChange(client, W, ['t1'], { starred: false })
  applyThreadChange(client, W, ['t3'], { restore: true })
  expect(ids(mailKeys.threads(W, { starred: true }))).toEqual([])
  expect(ids(mailKeys.threads(W, { folder: 'trash' }))).toEqual([])
})
