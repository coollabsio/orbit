import type { InfiniteData, QueryClient } from '@tanstack/react-query'
import { queryKeys } from '@/api/queryKeys'
import { quoteOf } from '../lib/quote'
import { sortFollowedThreads } from '../lib/sidebar'
import type { MessageCursor } from './client'
import { chatKeys } from './keys'
import type { ChatEvent, Conversation, ConversationState, FollowedThread, Message, MessagePage, ReplyQuote, ThreadPage } from './types'

export type MessagePages = InfiniteData<MessagePage, MessageCursor>
export type ThreadPages = InfiniteData<ThreadPage, MessageCursor>

type Update = (message: Message) => Message | null

const byId = (a: Message, b: Message) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0)

/** Applies `update` to every item of a list; `null` removes the item. Returns the same array when nothing changed. */
function patchList(items: Message[], messageId: string, update: Update): Message[] {
  if (!items.some((item) => item.id === messageId)) return items
  return items.flatMap((item) => {
    if (item.id !== messageId) return [item]
    const next = update(item)
    return next ? [next] : []
  })
}

/** Applies `patch` to the items of every page. Returns the same data when no page changed. */
function mapPages<T extends MessagePage>(data: InfiniteData<T, MessageCursor> | undefined, patch: (items: Message[]) => Message[]) {
  if (!data) return data
  let changed = false
  const pages = data.pages.map((page) => {
    const items = patch(page.items)
    if (items === page.items) return page
    changed = true
    return { ...page, items }
  })
  return changed ? { ...data, pages } : data
}

function patchPages<T extends MessagePage>(data: InfiniteData<T, MessageCursor> | undefined, messageId: string, update: Update) {
  return mapPages(data, (items) => patchList(items, messageId, update))
}

/**
 * The messages that the server has not confirmed: sending or failed. They are kept apart from the lists, which hold
 * only what the server has, so a refetch, a resync or a list that is not at its newest page cannot lose one. They
 * live as long as the tab.
 */
export function updateOutbox(queryClient: QueryClient, workspaceId: string, update: (messages: Message[]) => Message[]) {
  const key = chatKeys.outbox(workspaceId)
  queryClient.setQueryDefaults(key, { gcTime: Infinity })
  queryClient.setQueryData<Message[]>(key, (messages) => update(messages ?? []))
}

/**
 * Puts a confirmed message into a paged list: over the row with the same id, else at the end, and only when the last
 * page is the live tail (`after === null`).
 */
function upsertPages<T extends MessagePage>(data: InfiniteData<T, MessageCursor> | undefined, message: Message) {
  if (!data || data.pages.length === 0) return data
  const matches = (item: Message) => item.id === message.id
  const last = data.pages.length - 1
  const found = data.pages.some((page) => page.items.some(matches))
  if (!found && data.pages[last].after !== null) return data
  const pages = data.pages.map((page, index) => {
    if (found ? !page.items.some(matches) : index !== last) return page
    return { ...page, items: [...page.items.filter((item) => !matches(item)), message].sort(byId) }
  })
  return { ...data, pages }
}

/** The lists a message shows in: the conversation's main list, its thread, or both for an "also in channel" reply. */
function upsertMessage(queryClient: QueryClient, workspaceId: string, message: Message) {
  if (message.threadRootId === null || message.alsoInChannel) {
    queryClient.setQueriesData<MessagePages>(
      { queryKey: chatKeys.messagesOf(workspaceId, message.conversationId) },
      (data) => upsertPages(data, message),
    )
  }
  if (message.threadRootId !== null) {
    queryClient.setQueryData<ThreadPages>(chatKeys.thread(workspaceId, message.threadRootId), (data) =>
      upsertPages(data, message),
    )
  }
}

/** Changes (or with `null` removes) one message in every cache that can hold it. */
export function patchMessage(
  queryClient: QueryClient,
  workspaceId: string,
  target: { conversationId: string; messageId: string; threadRootId: string | null },
  update: Update,
) {
  const { conversationId, messageId, threadRootId } = target
  queryClient.setQueriesData<MessagePages>({ queryKey: chatKeys.messagesOf(workspaceId, conversationId) }, (data) =>
    patchPages(data, messageId, update),
  )
  queryClient.setQueryData<ThreadPages>(chatKeys.thread(workspaceId, threadRootId ?? messageId), (data) => {
    const patched = patchPages(data, messageId, update)
    if (!patched || threadRootId !== null) return patched
    // The message is the root: every page carries a copy. A removed root keeps its last copy until the query is dropped.
    return { ...patched, pages: patched.pages.map((page) => ({ ...page, root: update(page.root) ?? page.root })) }
  })
  for (const key of [
    chatKeys.threads(workspaceId, conversationId),
    chatKeys.pins(workspaceId, conversationId),
    chatKeys.files(workspaceId, conversationId),
  ]) {
    queryClient.setQueryData<Message[]>(key, (items) => items && patchList(items, messageId, update))
  }
  queryClient.setQueryData<FollowedThread[]>(chatKeys.followedThreads(workspaceId), (threads) => {
    if (!threads?.some((thread) => thread.root.id === messageId || thread.lastReply?.id === messageId)) return threads
    return threads.flatMap((thread) => {
      if (thread.root.id === messageId) {
        const root = update(thread.root)
        return root ? [{ ...thread, root }] : []
      }
      if (thread.lastReply?.id === messageId) return [{ ...thread, lastReply: update(thread.lastReply) }]
      return [thread]
    })
  })
}

/**
 * Gives the replies that quote a message its new quote; `null` when the message is gone. The server sends no event
 * to the replies, so the client does it: in the conversation's lists, in the thread of the message, and in the outbox.
 */
function patchQuotes(
  queryClient: QueryClient,
  workspaceId: string,
  target: { conversationId: string; messageId: string; threadRootId: string | null },
  replyTo: ReplyQuote | null,
) {
  const { conversationId, messageId, threadRootId } = target
  // A reaction or a pin updates the message too: a quote that says the same stays as it is.
  const stale = (item: Message) => item.replyToId === messageId && (item.replyTo?.body ?? null) !== (replyTo?.body ?? null)
  const requote = (items: Message[]) => (items.some(stale) ? items.map((item) => (stale(item) ? { ...item, replyTo } : item)) : items)
  queryClient.setQueriesData<MessagePages>({ queryKey: chatKeys.messagesOf(workspaceId, conversationId) }, (data) => mapPages(data, requote))
  queryClient.setQueryData<ThreadPages>(chatKeys.thread(workspaceId, threadRootId ?? messageId), (data) => mapPages(data, requote))
  if (queryClient.getQueryData<Message[]>(chatKeys.outbox(workspaceId))) updateOutbox(queryClient, workspaceId, requote)
}

/** The Threads list's order, as the server sends it: newest reply first. */
const byLastReply = (a: Message, b: Message) => (b.lastReplyAt ?? b.createdAt) - (a.lastReplyAt ?? a.createdAt) || byId(b, a)

/**
 * Puts a root into its conversation's Threads list, in order, once it has replies (the first reply makes a new
 * thread); a root without replies leaves the list.
 */
function placeThreadRoot(queryClient: QueryClient, workspaceId: string, root: Message) {
  queryClient.setQueryData<Message[]>(chatKeys.threads(workspaceId, root.conversationId), (roots) => {
    if (!roots) return roots
    const others = roots.filter((item) => item.id !== root.id)
    if (root.replyCount === 0) return others.length === roots.length ? roots : others
    return [...others, root].sort(byLastReply)
  })
}

/** The newest copy of a root in the cache: its thread's, else the one in a message list. */
function cachedRoot(queryClient: QueryClient, workspaceId: string, conversationId: string, rootId: string): Message | undefined {
  const thread = queryClient.getQueryData<ThreadPages>(chatKeys.thread(workspaceId, rootId))
  if (thread?.pages[0]) return thread.pages[0].root
  for (const [, data] of queryClient.getQueriesData<MessagePages>({ queryKey: chatKeys.messagesOf(workspaceId, conversationId) })) {
    const root = data?.pages.flatMap((page) => page.items).find((item) => item.id === rootId)
    if (root) return root
  }
  return undefined
}

/**
 * The Threads view lists followed threads with their root and last reply, which events do not carry. When the user
 * follows a thread (its open thread says so) that the cached list lacks, the list is asked again.
 */
function refetchFollowedIfMissing(queryClient: QueryClient, workspaceId: string, rootId: string) {
  const followed = queryClient.getQueryData<FollowedThread[]>(chatKeys.followedThreads(workspaceId))
  if (!followed || followed.some((thread) => thread.root.id === rootId)) return
  const thread = queryClient.getQueryData<ThreadPages>(chatKeys.thread(workspaceId, rootId))
  if (thread?.pages[0]?.state?.following) void queryClient.invalidateQueries({ queryKey: chatKeys.followedThreads(workspaceId) })
}

function upsertById<T>(items: T[] | undefined, item: T, idOf: (item: T) => string): T[] | undefined {
  if (!items) return items
  return items.some((current) => idOf(current) === idOf(item))
    ? items.map((current) => (idOf(current) === idOf(item) ? item : current))
    : [...items, item]
}

function messageCreated(queryClient: QueryClient, workspaceId: string, message: Message) {
  const { conversationId, threadRootId } = message
  // The confirmed message takes the place of the row that waited for it.
  if (message.nonce !== null) updateOutbox(queryClient, workspaceId, (waiting) => waiting.filter((item) => item.nonce !== message.nonce))
  upsertMessage(queryClient, workspaceId, message)

  if (threadRootId === null || message.alsoInChannel) {
    queryClient.setQueryData<Conversation[]>(chatKeys.conversations(workspaceId), (conversations) =>
      conversations?.map((conversation) =>
        conversation.id === conversationId && (conversation.lastMessageAt ?? 0) < message.createdAt
          ? { ...conversation, lastMessageAt: message.createdAt }
          : conversation,
      ),
    )
  }
  if (message.attachments.length > 0) {
    void queryClient.invalidateQueries({ queryKey: chatKeys.files(workspaceId, conversationId) })
  }
  if (threadRootId === null) return

  // A reply changes its root's summary wherever the root shows. The time guard keeps this from counting twice when
  // the root's own `message.updated` arrived first.
  patchMessage(queryClient, workspaceId, { conversationId, messageId: threadRootId, threadRootId: null }, (root) =>
    (root.lastReplyAt ?? 0) >= message.createdAt
      ? root
      : {
          ...root,
          replyCount: root.replyCount + 1,
          lastReplyAt: message.createdAt,
          replyUserIds: root.replyUserIds.includes(message.authorId) ? root.replyUserIds : [...root.replyUserIds, message.authorId],
          lastReply: { authorId: message.authorId, body: message.body, sticker: message.stickerId != null, createdAt: message.createdAt },
        },
  )
  queryClient.setQueryData<FollowedThread[]>(chatKeys.followedThreads(workspaceId), (threads) =>
    threads?.some((thread) => thread.root.id === threadRootId)
      ? sortFollowedThreads(threads.map((thread) => (thread.root.id === threadRootId ? { ...thread, lastReply: message } : thread)))
      : threads,
  )
  refetchFollowedIfMissing(queryClient, workspaceId, threadRootId)
  // The first reply makes a new thread. Without a copy of its root the Threads list has to be asked again.
  const root = cachedRoot(queryClient, workspaceId, conversationId, threadRootId)
  if (root && root.replyCount > 0) placeThreadRoot(queryClient, workspaceId, root)
  else void queryClient.invalidateQueries({ queryKey: chatKeys.threads(workspaceId, conversationId) })
}

function messageUpdated(queryClient: QueryClient, workspaceId: string, message: Message) {
  const { conversationId, threadRootId } = message
  patchMessage(queryClient, workspaceId, { conversationId, messageId: message.id, threadRootId }, () => message)
  // An edit changes the quote; a deleted root that stays for its replies has nothing left to quote.
  patchQuotes(queryClient, workspaceId, { conversationId, messageId: message.id, threadRootId }, message.deleted ? null : quoteOf(message))
  if (threadRootId === null && message.kind === 'message') {
    placeThreadRoot(queryClient, workspaceId, message)
    if (message.replyCount > 0) refetchFollowedIfMissing(queryClient, workspaceId, message.id)
  }
  queryClient.setQueryData<Message[]>(chatKeys.pins(workspaceId, conversationId), (pins) => {
    if (!pins) return pins
    const others = pins.filter((pin) => pin.id !== message.id)
    return message.pinned ? [...others, message].sort((a, b) => byId(b, a)) : others
  })
}

/**
 * An event for a query that is loading can be lost: the query has no data to change yet, or the response that
 * comes was read before the event and takes its place. The event is applied once more when the load ends: every
 * event gives the same result when it is applied a second time.
 */
function applyAgainAfterFetch(queryClient: QueryClient, workspaceId: string, event: ChatEvent) {
  const fetching = changedKeys(workspaceId, event).flatMap((queryKey) => queryClient.getQueryCache().findAll({ queryKey, fetchStatus: 'fetching' }))
  if (fetching.length === 0) return
  const again = () => applyChatEvent(queryClient, workspaceId, event)
  // The events of one load are applied again in the order in which they came.
  void Promise.allSettled(fetching.map((query) => query.promise)).then(again)
}

/** The queries that an event changes in place. */
function changedKeys(workspaceId: string, event: ChatEvent): (readonly unknown[])[] {
  switch (event.type) {
    case 'message.created':
    case 'message.updated':
      return [
        chatKeys.messagesOf(workspaceId, event.message.conversationId),
        chatKeys.thread(workspaceId, event.message.threadRootId ?? event.message.id),
        chatKeys.threads(workspaceId, event.message.conversationId),
      ]
    case 'message.deleted':
      return [chatKeys.messagesOf(workspaceId, event.conversationId), chatKeys.thread(workspaceId, event.threadRootId ?? event.messageId)]
    case 'conversation.changed':
    case 'conversation.removed':
      return [chatKeys.conversations(workspaceId)]
    case 'state.changed':
      return [chatKeys.states(workspaceId)]
    case 'thread.changed':
      return [chatKeys.thread(workspaceId, event.state.rootId), chatKeys.followedThreads(workspaceId)]
    default:
      return []
  }
}

/**
 * Writes one client event into the React Query cache. Typing, presence and connection events are not cached: the
 * provider sends those to the live stores.
 */
export function applyChatEvent(queryClient: QueryClient, workspaceId: string, event: ChatEvent) {
  applyAgainAfterFetch(queryClient, workspaceId, event)
  switch (event.type) {
    case 'message.created':
      messageCreated(queryClient, workspaceId, event.message)
      break
    case 'message.updated':
      messageUpdated(queryClient, workspaceId, event.message)
      break
    case 'message.deleted': {
      const followed = queryClient.getQueryData<FollowedThread[]>(chatKeys.followedThreads(workspaceId))
      // The thread's last reply is gone and the cache does not hold the one before it.
      if (followed?.some((thread) => thread.lastReply?.id === event.messageId)) {
        void queryClient.invalidateQueries({ queryKey: chatKeys.followedThreads(workspaceId) })
      }
      patchMessage(queryClient, workspaceId, event, () => null)
      patchQuotes(queryClient, workspaceId, event, null)
      if (event.threadRootId === null) queryClient.removeQueries({ queryKey: chatKeys.thread(workspaceId, event.messageId) })
      else void queryClient.invalidateQueries({ queryKey: chatKeys.threads(workspaceId, event.conversationId) })
      break
    }
    case 'conversation.changed':
      queryClient.setQueryData<Conversation[]>(chatKeys.conversations(workspaceId), (conversations) =>
        upsertById(conversations, event.conversation, (conversation) => conversation.id),
      )
      break
    case 'conversation.removed':
      queryClient.setQueryData<Conversation[]>(chatKeys.conversations(workspaceId), (conversations) =>
        conversations?.filter((conversation) => conversation.id !== event.conversationId),
      )
      queryClient.setQueryData<ConversationState[]>(chatKeys.states(workspaceId), (states) =>
        states?.filter((state) => state.conversationId !== event.conversationId),
      )
      queryClient.setQueryData<FollowedThread[]>(chatKeys.followedThreads(workspaceId), (threads) =>
        threads?.filter((thread) => thread.conversationId !== event.conversationId),
      )
      for (const key of [
        chatKeys.messagesOf(workspaceId, event.conversationId),
        chatKeys.threads(workspaceId, event.conversationId),
        chatKeys.pins(workspaceId, event.conversationId),
        chatKeys.files(workspaceId, event.conversationId),
      ]) {
        queryClient.removeQueries({ queryKey: key })
      }
      queryClient.removeQueries({
        queryKey: chatKeys.threadPages(workspaceId),
        predicate: (query) => (query.state.data as ThreadPages | undefined)?.pages[0]?.root.conversationId === event.conversationId,
      })
      break
    case 'categories.changed':
      queryClient.setQueryData(chatKeys.categories(workspaceId), event.categories)
      break
    case 'state.changed':
      queryClient.setQueryData<ConversationState[]>(chatKeys.states(workspaceId), (states) =>
        upsertById(states, event.state, (state) => state.conversationId),
      )
      break
    case 'thread.changed': {
      const { state } = event
      queryClient.setQueryData<ThreadPages>(
        chatKeys.thread(workspaceId, state.rootId),
        (data) => data && { ...data, pages: data.pages.map((page) => ({ ...page, state })) },
      )
      const followed = queryClient.getQueryData<FollowedThread[]>(chatKeys.followedThreads(workspaceId))
      if (!followed) break
      if (followed.some((thread) => thread.root.id === state.rootId)) {
        queryClient.setQueryData<FollowedThread[]>(
          chatKeys.followedThreads(workspaceId),
          sortFollowedThreads(
            followed.flatMap((thread) =>
              thread.root.id !== state.rootId ? [thread] : state.following ? [{ ...thread, state }] : [],
            ),
          ),
        )
      } else if (state.following) {
        // A newly followed thread: the list needs its root and last reply, which the event does not carry.
        void queryClient.invalidateQueries({ queryKey: chatKeys.followedThreads(workspaceId) })
      }
      break
    }
    case 'emoji.changed':
      void queryClient.invalidateQueries({ queryKey: queryKeys.customEmoji(workspaceId) })
      break
    case 'stickers.changed':
      void queryClient.invalidateQueries({ queryKey: queryKeys.customStickers(workspaceId) })
      break
    case 'resync':
      void queryClient.invalidateQueries({ queryKey: chatKeys.all(workspaceId) })
      // the custom emoji and stickers are the workspace's (outside the chat prefix), and their events may be among the missed ones
      void queryClient.invalidateQueries({ queryKey: queryKeys.customEmoji(workspaceId) })
      void queryClient.invalidateQueries({ queryKey: queryKeys.customStickers(workspaceId) })
      break
    case 'typing':
    case 'presence':
    case 'self.changed':
    case 'connection':
      break
  }
}
