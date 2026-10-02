import { type InfiniteData, useInfiniteQuery, useQuery } from '@tanstack/react-query'
import { type ChatBadges, chatBadges, type UnreadConversation, unreadConversations } from '../lib/sidebar'
import { useChatContext } from './chatContext'
import type { ChatClient, MessageCursor, SearchInput } from './client'
import { chatKeys } from './keys'
import { ChatError, type Conversation, type ConversationState, type FollowedThread, type Message, type MessagePage } from './types'

/** Chat queries never go stale on their own: `applyChatEvent` keeps them current. `resync` invalidates them. */
const LIVE = { staleTime: Infinity } as const

/** Queries are disabled until the client exists; this guards the direct calls. */
export function requireClient(client: ChatClient | null): ChatClient {
  if (!client) throw new ChatError('offline', 'Chat is still loading.')
  return client
}

function conversationsOptions(client: ChatClient | null, workspaceId: string) {
  return {
    ...LIVE,
    queryKey: chatKeys.conversations(workspaceId),
    queryFn: () => requireClient(client).listConversations(),
    enabled: client !== null,
  }
}

function statesOptions(client: ChatClient | null, workspaceId: string) {
  return {
    ...LIVE,
    queryKey: chatKeys.states(workspaceId),
    queryFn: () => requireClient(client).listStates(),
    enabled: client !== null,
  }
}

function followedThreadsOptions(client: ChatClient | null, workspaceId: string) {
  return {
    ...LIVE,
    queryKey: chatKeys.followedThreads(workspaceId),
    queryFn: () => requireClient(client).listFollowedThreads(),
    enabled: client !== null,
  }
}

/** Every conversation the user can see: joined ones and all public channels (filter by `isMember` for the sidebar). */
export function useConversations() {
  const { client, workspaceId } = useChatContext()
  return useQuery(conversationsOptions(client, workspaceId))
}

/** One conversation from the list; `data` is `undefined` when it is unknown or not visible to the user. */
export function useConversation(conversationId: string | null | undefined) {
  const { client, workspaceId } = useChatContext()
  return useQuery({
    ...conversationsOptions(client, workspaceId),
    select: (conversations: Conversation[]) => conversations.find((conversation) => conversation.id === conversationId),
  })
}

export function useCategories() {
  const { client, workspaceId } = useChatContext()
  return useQuery({
    ...LIVE,
    queryKey: chatKeys.categories(workspaceId),
    queryFn: () => requireClient(client).listCategories(),
    enabled: client !== null,
  })
}

/** The user's read cursor, counts, notification setting and favorite flag for each joined conversation. */
export function useConversationStates() {
  const { client, workspaceId } = useChatContext()
  return useQuery(statesOptions(client, workspaceId))
}

export function useConversationState(conversationId: string | null | undefined) {
  const { client, workspaceId } = useChatContext()
  return useQuery({
    ...statesOptions(client, workspaceId),
    select: (states: ConversationState[]) => states.find((state) => state.conversationId === conversationId),
  })
}

/**
 * The main list of a conversation, paged in both directions. Without `around` it starts at the newest page; with it,
 * at a window around that message (jump to message, first unread). `fetchPreviousPage` loads older messages,
 * `fetchNextPage` newer ones. New messages are appended by events only when the newest page is loaded.
 *
 * The list holds `MAX_MESSAGE_PAGES` pages: one more page at one end drops the page at the other end, so a long
 * scroll through the history does not keep every message in memory and on the page.
 */
export function useMessages(conversationId: string | null | undefined, options: { around?: string | null } = {}) {
  const { client, workspaceId } = useChatContext()
  const around = options.around ?? null
  return useInfiniteQuery({
    ...LIVE,
    queryKey: chatKeys.messages(workspaceId, conversationId ?? '', around),
    enabled: client !== null && Boolean(conversationId),
    initialPageParam: (around ? { around } : {}) as MessageCursor,
    maxPages: MAX_MESSAGE_PAGES,
    queryFn: ({ pageParam }) => requireClient(client).listMessages(conversationId ?? '', pageParam),
    getPreviousPageParam: (first): MessageCursor | undefined => (first.before ? { before: first.before } : undefined),
    getNextPageParam: (last): MessageCursor | undefined => (last.after ? { after: last.after } : undefined),
  })
}

/**
 * A thread: every page carries the root and the user's thread state; `items` are the replies. Starts at the newest,
 * and holds a limited number of pages like `useMessages`.
 */
export function useThread(rootId: string | null | undefined) {
  const { client, workspaceId } = useChatContext()
  return useInfiniteQuery({
    ...LIVE,
    queryKey: chatKeys.thread(workspaceId, rootId ?? ''),
    enabled: client !== null && Boolean(rootId),
    initialPageParam: {} as MessageCursor,
    maxPages: MAX_MESSAGE_PAGES,
    queryFn: ({ pageParam }) => requireClient(client).getThread(rootId ?? '', pageParam),
    getPreviousPageParam: (first): MessageCursor | undefined => (first.before ? { before: first.before } : undefined),
    getNextPageParam: (last): MessageCursor | undefined => (last.after ? { after: last.after } : undefined),
  })
}

/** All pages of `useMessages` or `useThread` as one ascending list. */
export function flattenMessages(data: InfiniteData<MessagePage, unknown> | undefined): Message[] {
  return data ? data.pages.flatMap((page) => page.items) : []
}

/** 8 pages of 50 messages. */
const MAX_MESSAGE_PAGES = 8

const NONE: Message[] = []

/**
 * The user's messages that the server has not confirmed (sending or failed) for one list: a conversation's main list,
 * or a thread with `rootId`. They go under the confirmed messages.
 */
export function useUnconfirmedMessages(conversationId: string, rootId: string | null = null): Message[] {
  const { workspaceId } = useChatContext()
  const { data } = useQuery({
    queryKey: chatKeys.outbox(workspaceId),
    // Only `updateOutbox` writes it; with data from the start there is no fetch.
    queryFn: () => NONE,
    initialData: NONE,
    staleTime: Infinity,
    gcTime: Infinity,
    select: (waiting: Message[]) =>
      waiting.filter((message) =>
        rootId === null
          ? message.conversationId === conversationId && (message.threadRootId === null || message.alsoInChannel)
          : message.threadRootId === rootId,
      ),
  })
  return data.length > 0 ? data : NONE
}

/** Roots that have replies in one conversation, newest reply first (the Threads pane). */
export function useConversationThreads(conversationId: string | null | undefined) {
  const { client, workspaceId } = useChatContext()
  return useQuery({
    ...LIVE,
    queryKey: chatKeys.threads(workspaceId, conversationId ?? ''),
    queryFn: () => requireClient(client).listThreads(conversationId ?? ''),
    enabled: client !== null && Boolean(conversationId),
  })
}

/** The Threads view: followed threads, unread first, then newest reply first. */
export function useFollowedThreads() {
  const { client, workspaceId } = useChatContext()
  return useQuery(followedThreadsOptions(client, workspaceId))
}

/** The user's state in one followed thread (for the reply summary row); `undefined` when the thread is not followed. */
export function useThreadState(rootId: string | null | undefined) {
  const { client, workspaceId } = useChatContext()
  return useQuery({
    ...followedThreadsOptions(client, workspaceId),
    select: (threads: FollowedThread[]) => threads.find((thread) => thread.root.id === rootId)?.state,
  })
}

export function usePins(conversationId: string | null | undefined) {
  const { client, workspaceId } = useChatContext()
  return useQuery({
    ...LIVE,
    queryKey: chatKeys.pins(workspaceId, conversationId ?? ''),
    queryFn: () => requireClient(client).listPins(conversationId ?? ''),
    enabled: client !== null && Boolean(conversationId),
  })
}

/** Messages with attachments, newest first. */
export function useFiles(conversationId: string | null | undefined) {
  const { client, workspaceId } = useChatContext()
  return useQuery({
    ...LIVE,
    queryKey: chatKeys.files(workspaceId, conversationId ?? ''),
    queryFn: () => requireClient(client).listFiles(conversationId ?? ''),
    enabled: client !== null && Boolean(conversationId),
  })
}

/** Search, paged by cursor (`fetchNextPage`). Runs once there is a query or a filter. Results are not live. */
export function useSearchMessages(input: Omit<SearchInput, 'cursor'>) {
  const { client, workspaceId } = useChatContext()
  const filters = {
    query: input.query.trim(),
    conversationId: input.conversationId,
    authorId: input.authorId,
    hasFile: input.hasFile,
  }
  return useInfiniteQuery({
    queryKey: chatKeys.search(workspaceId, filters),
    enabled: client !== null && (filters.query !== '' || Boolean(filters.authorId) || Boolean(filters.hasFile)),
    initialPageParam: undefined as string | undefined,
    queryFn: ({ pageParam }) => requireClient(client).searchMessages({ ...filters, cursor: pageParam }),
    getNextPageParam: (last) => last.cursor ?? undefined,
  })
}

/** Counts for the Unreads row (`unreads`), the Threads row (`threads`) and the Chat nav item (`total`). */
export function useChatBadges(): ChatBadges {
  const conversations = useConversations().data
  const states = useConversationStates().data
  const threads = useFollowedThreads().data
  return chatBadges(conversations ?? [], states ?? [], threads ?? [])
}

/** The Chat item in the app sidebar and the mobile dock: all count badges plus unread followed threads. */
export function useChatBadgeCount(): number {
  return useChatBadges().total
}

/** The groups of the Unreads view: unread, not muted, newest activity first. */
export function useUnreadConversations(): UnreadConversation[] {
  const conversations = useConversations().data
  const states = useConversationStates().data
  return unreadConversations(conversations ?? [], states ?? [])
}
