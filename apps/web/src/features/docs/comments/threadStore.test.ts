import { describe, expect, mock, test } from 'bun:test'
import { QueryClient } from '@tanstack/react-query'
import type { PageComment, PageThread } from '@/api/generated/types.gen'
import type { CommentsApi } from './api'
import { threadCounts } from './api'
import { OrbitThreadStore, OrbitThreadStoreAuth, toThreadData } from './threadStore'

const KEY = ['workspace', 'w1', 'pages', 'threads', 'p1'] as const

function comment(overrides: Partial<PageComment> = {}): PageComment {
  return {
    id: 'c1',
    thread_id: 't1',
    author_id: 'ann',
    body: [{ type: 'paragraph', content: [{ type: 'text', text: 'Hi', styles: {} }] }],
    body_text: 'Hi',
    mentioned_user_ids: [],
    created_at: '2026-09-25T10:00:00Z',
    updated_at: '2026-09-25T10:00:00Z',
    edited_at: null,
    deleted_at: null,
    ...overrides,
  }
}

function thread(overrides: Partial<PageThread> = {}): PageThread {
  return {
    id: 't1',
    page_id: 'p1',
    created_by: 'ann',
    quote: 'selected',
    resolved: false,
    resolved_at: null,
    resolved_by: null,
    version: 0,
    created_at: '2026-09-25T10:00:00Z',
    updated_at: '2026-09-25T10:00:00Z',
    comments: [comment()],
    ...overrides,
  }
}

function fakeApi(overrides: Partial<CommentsApi> = {}): CommentsApi {
  return {
    createThread: mock(async (body: unknown[], quote: string) => thread({ id: 't2', quote, comments: [comment({ id: 'c2', thread_id: 't2', body: body as never })] })),
    addComment: mock(async (threadId: string) => comment({ id: 'c3', thread_id: threadId, author_id: 'bob' })),
    updateComment: mock(async (threadId: string, commentId: string) => comment({ id: commentId, thread_id: threadId, body_text: 'Edited', edited_at: '2026-09-25T11:00:00Z' })),
    deleteComment: mock(async () => {}),
    deleteThread: mock(async () => {}),
    resolveThread: mock(async (threadId: string) => thread({ id: threadId, resolved: true, resolved_by: 'bob', resolved_at: '2026-09-25T12:00:00Z' })),
    reopenThread: mock(async (threadId: string) => thread({ id: threadId })),
    ...overrides,
  }
}

function setup(options: { api?: CommentsApi; threads?: PageThread[]; userId?: string | null } = {}) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const queryFn = mock(async () => options.threads ?? [thread()])
  const onError = mock(() => {})
  const store = new OrbitThreadStore({
    api: options.api ?? fakeApi(),
    queryClient,
    query: { queryKey: KEY, queryFn },
    userId: () => (options.userId === undefined ? 'ann' : options.userId),
    quote: () => 'the selection',
    onError,
  })
  return { store, queryClient, queryFn, onError }
}

async function loaded(store: OrbitThreadStore) {
  const seen: number[] = []
  const stop = store.subscribe((threads) => seen.push(threads.size))
  for (let i = 0; i < 50 && store.getThreads().size === 0; i += 1) await new Promise((resolve) => setTimeout(resolve, 5))
  return { stop, seen }
}

describe('thread data', () => {
  test('maps server threads to BlockNote threads; deleted comments have no body', () => {
    const data = toThreadData(
      thread({
        resolved: true,
        resolved_by: 'bob',
        resolved_at: '2026-09-25T12:00:00Z',
        comments: [comment(), comment({ id: 'c9', deleted_at: '2026-09-25T13:00:00Z', body: [] })],
      }),
    )
    expect(data.type).toBe('thread')
    expect(data.resolved).toBe(true)
    expect(data.resolvedBy).toBe('bob')
    expect(data.metadata).toEqual({ createdBy: 'ann', quote: 'selected' })
    expect(data.comments[0].body).toEqual(comment().body)
    expect(data.comments[1].body).toBeUndefined()
    expect(data.comments[1].deletedAt).toBeInstanceOf(Date)
    // "(edited)" only after a real edit.
    expect(data.comments[0].updatedAt.getTime()).toBe(data.comments[0].createdAt.getTime())
  })

  test('counts open and resolved threads, ignoring threads without live comments', () => {
    expect(
      threadCounts([
        thread(),
        thread({ id: 't2', resolved: true }),
        thread({ id: 't3', comments: [comment({ deleted_at: '2026-09-25T13:00:00Z' })] }),
      ]),
    ).toEqual({ open: 1, resolved: 1 })
    expect(threadCounts(undefined)).toEqual({ open: 0, resolved: 0 })
  })
})

describe('OrbitThreadStore', () => {
  test('loads the page threads through the query while subscribed', async () => {
    const { store, queryFn } = setup()
    expect(store.getThreads().size).toBe(0)
    const { stop, seen } = await loaded(store)
    expect(queryFn).toHaveBeenCalledTimes(1)
    expect(store.getThread('t1').comments).toHaveLength(1)
    expect(seen.at(-1)).toBe(1)
    // Stable identity between changes (useSyncExternalStore).
    expect(store.getThreads()).toBe(store.getThreads())
    stop()
  })

  test('realtime refreshes (query refetches) reach subscribers', async () => {
    const { store, queryClient } = setup()
    const { stop } = await loaded(store)
    queryClient.setQueryData(KEY, [thread(), thread({ id: 't5' })])
    await new Promise((resolve) => setTimeout(resolve, 10))
    expect([...store.getThreads().keys()]).toEqual(['t1', 't5'])
    stop()
  })

  test('creates threads with the selected text and writes results into the query', async () => {
    const api = fakeApi()
    const { store, queryClient } = setup({ api })
    const { stop } = await loaded(store)
    const body = [{ type: 'paragraph', content: 'Hello' }]
    const created = await store.createThread({ initialComment: { body } })
    expect(api.createThread).toHaveBeenCalledWith(body, 'the selection')
    expect(created.id).toBe('t2')
    expect(store.getThreads().has('t2')).toBe(true)
    expect(queryClient.getQueryData<PageThread[]>(KEY)?.map((item) => item.id)).toEqual(['t1', 't2'])

    await store.addComment({ threadId: 't1', comment: { body } })
    expect(store.getThread('t1').comments.map((item) => item.id)).toEqual(['c1', 'c3'])
    await store.updateComment({ threadId: 't1', commentId: 'c1', comment: { body } })
    expect(queryClient.getQueryData<PageThread[]>(KEY)?.[0].comments[0].body_text).toBe('Edited')
    await store.resolveThread({ threadId: 't1' })
    expect(store.getThread('t1').resolved).toBe(true)
    await store.unresolveThread({ threadId: 't1' })
    expect(store.getThread('t1').resolved).toBe(false)
    stop()
  })

  test('deleting the last live comment drops the thread, like the server', async () => {
    const { store } = setup()
    const { stop } = await loaded(store)
    await store.deleteComment({ threadId: 't1', commentId: 'c1' })
    expect(store.getThreads().has('t1')).toBe(false)
    stop()
  })

  test('failed requests are reported and rethrown (the composer keeps the draft)', async () => {
    const failure = new Error('nope')
    const api = fakeApi({ addComment: mock(async () => Promise.reject(failure)) })
    const { store, onError } = setup({ api })
    const { stop } = await loaded(store)
    await expect(store.addComment({ threadId: 't1', comment: { body: [] } })).rejects.toBe(failure)
    expect(onError).toHaveBeenCalledWith('reply', failure)
    await expect(store.addReaction()).rejects.toThrow()
    stop()
  })
})

describe('OrbitThreadStoreAuth', () => {
  const data = toThreadData(thread({ comments: [comment(), comment({ id: 'c2', author_id: 'bob' })] }))

  test('authors edit and delete their own comments; everyone replies and resolves', () => {
    const auth = new OrbitThreadStoreAuth(() => 'ann')
    expect(auth.canUpdateComment(data.comments[0])).toBe(true)
    expect(auth.canDeleteComment(data.comments[0])).toBe(true)
    expect(auth.canUpdateComment(data.comments[1])).toBe(false)
    expect(auth.canDeleteComment(data.comments[1])).toBe(false)
    expect(auth.canDeleteThread(data)).toBe(true)
    const bob = new OrbitThreadStoreAuth(() => 'bob')
    expect(bob.canDeleteThread(data)).toBe(false)
    expect(bob.canAddComment()).toBe(true)
    expect(bob.canResolveThread()).toBe(true)
    expect(bob.canUnresolveThread()).toBe(true)
    expect(bob.canAddReaction()).toBe(false)
  })

  test('nothing is allowed before the user is known', () => {
    const auth = new OrbitThreadStoreAuth(() => null)
    expect(auth.canCreateThread()).toBe(false)
    expect(auth.canAddComment()).toBe(false)
    expect(auth.canUpdateComment(data.comments[0])).toBe(false)
  })
})
