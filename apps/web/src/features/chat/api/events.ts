import type { InfiniteData, QueryClient } from '@tanstack/react-query'
import { sortFollowedThreads } from '../lib/sidebar'
import type { MessageCursor } from './client'
import { chatKeys } from './keys'
import type { ChatEvent, Conversation, ConversationState, FollowedThread, Message, MessagePage, ThreadPage } from './types'

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

function patchPages<T extends MessagePage>(data: InfiniteData<T, MessageCursor> | undefined, messageId: string, update: Update) {
  if (!data) return data
  let changed = false
  const pages = data.pages.map((page) => {
    const items = patchList(page.items, messageId, update)
    if (items === page.items) return page
    changed = true
    return { ...page, items }
  })
  return changed ? { ...data, pages } : data
}

/**
 * Puts a message into a paged list: over the row with the same id, else over the optimistic row with the same nonce,
 * else at the end, and only when the last page is the live tail (`after === null`). Unconfirmed rows have ids that
 * sort last, so they stay under confirmed messages.
 */
function upsertPages<T extends MessagePage>(data: InfiniteData<T, MessageCursor> | undefined, message: Message) {
  if (!data || data.pages.length === 0) return data
  const matches = (item: Message) => item.id === message.id || (message.nonce !== null && item.nonce === message.nonce)
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
export function upsertMessage(queryClient: QueryClient, workspaceId: string, message: Message) {
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

function upsertById<T>(items: T[] | undefined, item: T, idOf: (item: T) => string): T[] | undefined {
  if (!items) return items
  return items.some((current) => idOf(current) === idOf(item))
    ? items.map((current) => (idOf(current) === idOf(item) ? item : current))
    : [...items, item]
}

function messageCreated(queryClient: QueryClient, workspaceId: string, message: Message) {
  const { conversationId, threadRootId } = message
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
          lastReply: { authorId: message.authorId, body: message.body, createdAt: message.createdAt },
        },
  )
  queryClient.setQueryData<FollowedThread[]>(chatKeys.followedThreads(workspaceId), (threads) =>
    threads?.some((thread) => thread.root.id === threadRootId)
      ? sortFollowedThreads(threads.map((thread) => (thread.root.id === threadRootId ? { ...thread, lastReply: message } : thread)))
      : threads,
  )
  void queryClient.invalidateQueries({ queryKey: chatKeys.threads(workspaceId, conversationId) })
}

function messageUpdated(queryClient: QueryClient, workspaceId: string, message: Message) {
  const { conversationId, threadRootId } = message
  patchMessage(queryClient, workspaceId, { conversationId, messageId: message.id, threadRootId }, (current) =>
    // An unconfirmed row keeps its send state; everything else comes from the event.
    current.sendState ? { ...message, sendState: current.sendState } : message,
  )
  queryClient.setQueryData<Message[]>(chatKeys.pins(workspaceId, conversationId), (pins) => {
    if (!pins) return pins
    const others = pins.filter((pin) => pin.id !== message.id)
    return message.pinned ? [...others, message].sort((a, b) => byId(b, a)) : others
  })
}

/**
 * Writes one client event into the React Query cache. Typing, presence and connection events are not cached: the
 * provider sends those to the live stores.
 */
export function applyChatEvent(queryClient: QueryClient, workspaceId: string, event: ChatEvent) {
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
    case 'resync':
      void queryClient.invalidateQueries({ queryKey: chatKeys.all(workspaceId) })
      break
    case 'typing':
    case 'presence':
    case 'connection':
      break
  }
}
