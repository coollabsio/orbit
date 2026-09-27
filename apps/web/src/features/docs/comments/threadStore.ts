// BlockNote's comments UI talks to a ThreadStore; this one keeps Orbit's server as the source of truth. Threads come
// from the TanStack query of the page's threads (refetched on realtime events, so other viewers' comments arrive
// live); every change is a REST call whose answer is written into that query right away. The thread anchor is
// BlockNote's `comment` mark, which the editor sets on the selection inside the collaborative document.
import type { QueryClient } from '@tanstack/react-query'
import { QueryObserver } from '@tanstack/react-query'
import { ThreadStore, ThreadStoreAuth, type CommentData, type ThreadData } from '@blocknote/core/comments'
import type { PageComment, PageThread } from '@/api/generated/types.gen'
import type { CommentBlocks, CommentsApi } from './api'

type Listener = (threads: Map<string, ThreadData>) => void

/** `metadata` of Orbit threads. */
export interface OrbitThreadMetadata {
  createdBy: string
  quote: string
}

export function toCommentData(comment: PageComment): CommentData {
  const base = {
    type: 'comment' as const,
    id: comment.id,
    userId: comment.author_id,
    createdAt: new Date(comment.created_at),
    // BlockNote shows "(edited)" when these differ; only real edits move `edited_at`.
    updatedAt: new Date(comment.edited_at ?? comment.created_at),
    reactions: [],
    metadata: { mentionedUserIds: comment.mentioned_user_ids },
  }
  return comment.deleted_at
    ? { ...base, deletedAt: new Date(comment.deleted_at), body: undefined }
    : { ...base, body: comment.body }
}

export function toThreadData(thread: PageThread): ThreadData {
  const metadata: OrbitThreadMetadata = { createdBy: thread.created_by, quote: thread.quote }
  return {
    type: 'thread',
    id: thread.id,
    createdAt: new Date(thread.created_at),
    updatedAt: new Date(thread.updated_at),
    comments: thread.comments.map(toCommentData),
    resolved: thread.resolved,
    resolvedUpdatedAt: thread.resolved_at ? new Date(thread.resolved_at) : undefined,
    resolvedBy: thread.resolved_by ?? undefined,
    metadata,
  }
}

/**
 * Mirrors the server's rules so the UI only offers what will work: anyone who sees the page comments, replies,
 * resolves and reopens; authors edit and delete their own comments; a thread's creator deletes the thread.
 * No reactions.
 */
export class OrbitThreadStoreAuth extends ThreadStoreAuth {
  private readonly userId: () => string | null

  constructor(userId: () => string | null) {
    super()
    this.userId = userId
  }

  canCreateThread() {
    return this.userId() !== null
  }
  canAddComment() {
    return this.userId() !== null
  }
  canUpdateComment(comment: CommentData) {
    return !comment.deletedAt && comment.userId === this.userId()
  }
  canDeleteComment(comment: CommentData) {
    return !comment.deletedAt && comment.userId === this.userId()
  }
  canDeleteThread(thread: ThreadData) {
    return (thread.metadata as OrbitThreadMetadata | undefined)?.createdBy === this.userId()
  }
  canResolveThread() {
    return this.userId() !== null
  }
  canUnresolveThread() {
    return this.userId() !== null
  }
  canAddReaction() {
    return false
  }
  canDeleteReaction() {
    return false
  }
}

export interface OrbitThreadStoreOptions {
  api: CommentsApi
  queryClient: QueryClient
  /** `queryKey` + `queryFn` of the page's threads (`threadsQueryOptions`). */
  query: { queryKey: readonly unknown[]; queryFn: () => Promise<PageThread[]> }
  /** The signed-in user (null while unknown: nothing can be changed then). */
  userId: () => string | null
  /** The text a new thread is about (the editor's selection when the thread is created). */
  quote: () => string
  /** A request failed; the store rethrows so BlockNote keeps the composer/editor open. */
  onError?: (action: 'create' | 'reply' | 'edit' | 'delete' | 'resolve', error: unknown) => void
}

export class OrbitThreadStore extends ThreadStore {
  private readonly options: OrbitThreadStoreOptions
  private readonly listeners = new Set<Listener>()
  private readonly resultListeners = new Set<() => void>()
  private source: PageThread[] | undefined = undefined
  private threads = new Map<string, ThreadData>()
  private stopObserving: (() => void) | null = null

  constructor(options: OrbitThreadStoreOptions) {
    super(new OrbitThreadStoreAuth(options.userId))
    this.options = options
    this.sync(options.queryClient.getQueryData<PageThread[]>(options.query.queryKey))
  }

  // The comments extension sets the mark itself (locally, inside the collaborative document).
  addThreadToDocument = undefined

  /** Rebuilds the map when the query data changed (stable identity otherwise, for useSyncExternalStore). */
  private sync(data: PageThread[] | undefined) {
    if (data === undefined || data === this.source) return false
    this.source = data
    this.threads = new Map(data.map((thread) => [thread.id, toThreadData(thread)]))
    return true
  }

  private emit() {
    for (const listener of this.listeners) listener(this.threads)
  }

  private update(change: (threads: PageThread[]) => PageThread[]) {
    const { queryClient, query } = this.options
    queryClient.setQueryData<PageThread[]>(query.queryKey, (current) => change(current ?? []))
    if (this.sync(queryClient.getQueryData<PageThread[]>(query.queryKey))) this.emit()
  }

  private async run<T>(action: Parameters<NonNullable<OrbitThreadStoreOptions['onError']>>[0], request: () => Promise<T>): Promise<T> {
    try {
      return await request()
    } catch (error) {
      this.options.onError?.(action, error)
      // Whatever happened, show the server's state again.
      void this.options.queryClient.invalidateQueries({ queryKey: this.options.query.queryKey })
      throw error
    }
  }

  private replaceThread(thread: PageThread) {
    this.update((threads) =>
      threads.some((item) => item.id === thread.id) ? threads.map((item) => (item.id === thread.id ? thread : item)) : [...threads, thread],
    )
  }

  private replaceComment(comment: PageComment) {
    this.update((threads) =>
      threads.map((thread) =>
        thread.id !== comment.thread_id
          ? thread
          : {
              ...thread,
              comments: thread.comments.some((item) => item.id === comment.id)
                ? thread.comments.map((item) => (item.id === comment.id ? comment : item))
                : [...thread.comments, comment],
            },
      ),
    )
  }

  createThread = async (options: { initialComment: { body: CommentBlocks } }): Promise<ThreadData> => {
    const thread = await this.run('create', () => this.options.api.createThread(options.initialComment.body, this.options.quote()))
    this.replaceThread(thread)
    return toThreadData(thread)
  }

  addComment = async (options: { comment: { body: CommentBlocks }; threadId: string }): Promise<CommentData> => {
    const comment = await this.run('reply', () => this.options.api.addComment(options.threadId, options.comment.body))
    this.replaceComment(comment)
    return toCommentData(comment)
  }

  updateComment = async (options: { comment: { body: CommentBlocks }; threadId: string; commentId: string }) => {
    const comment = await this.run('edit', () => this.options.api.updateComment(options.threadId, options.commentId, options.comment.body))
    this.replaceComment(comment)
  }

  deleteComment = async (options: { threadId: string; commentId: string }) => {
    await this.run('delete', () => this.options.api.deleteComment(options.threadId, options.commentId))
    const now = new Date().toISOString()
    this.update((threads) =>
      threads
        .map((thread) =>
          thread.id !== options.threadId
            ? thread
            : {
                ...thread,
                comments: thread.comments.map((comment) =>
                  comment.id === options.commentId ? { ...comment, body: [], body_text: '', mentioned_user_ids: [], deleted_at: now } : comment,
                ),
              },
        )
        // The server drops a thread whose last live comment went.
        .filter((thread) => thread.id !== options.threadId || thread.comments.some((comment) => !comment.deleted_at)),
    )
  }

  deleteThread = async (options: { threadId: string }) => {
    await this.run('delete', () => this.options.api.deleteThread(options.threadId))
    this.update((threads) => threads.filter((thread) => thread.id !== options.threadId))
  }

  resolveThread = async (options: { threadId: string }) => {
    this.replaceThread(await this.run('resolve', () => this.options.api.resolveThread(options.threadId)))
  }

  unresolveThread = async (options: { threadId: string }) => {
    this.replaceThread(await this.run('resolve', () => this.options.api.reopenThread(options.threadId)))
  }

  addReaction = async () => {
    throw new Error('Reactions are not supported.')
  }

  deleteReaction = async () => {
    throw new Error('Reactions are not supported.')
  }

  getThread = (threadId: string): ThreadData => {
    const thread = this.threads.get(threadId)
    if (!thread) throw new Error(`Thread ${threadId} not found`)
    return thread
  }

  getThreads = (): Map<string, ThreadData> => this.threads

  /**
   * Whether the first load finished (with data or an error). The comments extension rewrites every comment mark's
   * `orphan` flag in the shared document from `getThreads()` as soon as the editor mounts, so the editor must not
   * mount before this: an empty map would orphan (and broadcast) every anchor on the page.
   */
  isSettled = (): boolean => {
    const state = this.options.queryClient.getQueryState(this.options.query.queryKey)
    return state !== undefined && (state.data !== undefined || state.status === 'error')
  }

  /** Loads the threads and calls `listener` on every change of the query (for `isSettled`). */
  subscribeSettled = (listener: () => void): (() => void) => this.subscribe(() => listener(), listener)

  /** Watches the threads query (fetching it) while BlockNote listens. */
  subscribe = (listener: Listener, onResult?: () => void): (() => void) => {
    this.listeners.add(listener)
    if (onResult) this.resultListeners.add(onResult)
    if (!this.stopObserving) {
      const observer = new QueryObserver<PageThread[]>(this.options.queryClient, {
        queryKey: this.options.query.queryKey,
        queryFn: this.options.query.queryFn,
      })
      this.stopObserving = observer.subscribe((result) => {
        if (this.sync(result.data)) this.emit()
        for (const notify of this.resultListeners) notify()
      })
      if (this.sync(observer.getCurrentResult().data)) this.emit()
    }
    return () => {
      this.listeners.delete(listener)
      if (onResult) this.resultListeners.delete(onResult)
      if (this.listeners.size === 0) {
        this.stopObserving?.()
        this.stopObserving = null
      }
    }
  }
}

/**
 * A store with no threads that refuses changes. Every editor bound to a collaborative document needs the comments
 * extension (y-prosemirror drops text carrying a mark the schema does not know), so this stands in when there is no
 * comments backend (tests, previews).
 */
export class NoCommentsThreadStore extends ThreadStore {
  private static readonly empty = new Map<string, ThreadData>()

  constructor() {
    super(new OrbitThreadStoreAuth(() => null))
  }

  addThreadToDocument = undefined
  createThread = async (): Promise<ThreadData> => Promise.reject(new Error('Comments are unavailable.'))
  addComment = async (): Promise<CommentData> => Promise.reject(new Error('Comments are unavailable.'))
  updateComment = async () => Promise.reject(new Error('Comments are unavailable.'))
  deleteComment = async () => Promise.reject(new Error('Comments are unavailable.'))
  deleteThread = async () => Promise.reject(new Error('Comments are unavailable.'))
  resolveThread = async () => Promise.reject(new Error('Comments are unavailable.'))
  unresolveThread = async () => Promise.reject(new Error('Comments are unavailable.'))
  addReaction = async () => Promise.reject(new Error('Comments are unavailable.'))
  deleteReaction = async () => Promise.reject(new Error('Comments are unavailable.'))
  getThread = (threadId: string): ThreadData => {
    throw new Error(`Thread ${threadId} not found`)
  }
  getThreads = () => NoCommentsThreadStore.empty
  subscribe = () => () => {}
  isSettled = () => true
  subscribeSettled = () => () => {}
}
