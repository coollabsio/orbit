/** The chat queries and writes beyond the shared lists in `src/queries.ts`. */
import { useInfiniteQuery, useQuery, useQueryClient } from '@tanstack/react-query'
import { useEffect, useMemo } from 'react'
import { Alert, Platform } from 'react-native'

import { BUILT_IN_EMOJI, type EmojiGroup, emojiGroups, isChatAdmin } from '../data/chat'
import * as chat from '../data/chat-api'
import { keys } from '../data/keys'
import * as api from '../data/orbit'
import type { MessageRecord, OrbitClient } from '../data/orbit'
import { applyChatEvents, chatQueryKeys, type ConversationList } from '../chat-cache'
import { useOrbit } from '../orbit-context'
import { useMembers } from '../queries'
import { errorMessage } from '../session-context'



/** A browser has no native alert with buttons: the web preview confirms with `window.confirm`. */
export function confirmAction(title: string, message: string, confirmLabel: string): Promise<boolean> {
  if (Platform.OS === 'web') return Promise.resolve(window.confirm(`${title}\n\n${message}`))
  return new Promise((resolve) =>
    Alert.alert(title, message, [
      { text: 'Cancel', style: 'cancel', onPress: () => resolve(false) },
      { text: confirmLabel, style: 'destructive', onPress: () => resolve(true) },
    ]),
  )
}

export function showError(title: string, error: unknown) {
  if (Platform.OS === 'web') window.alert(`${title}\n\n${errorMessage(error)}`)
  else Alert.alert(title, errorMessage(error))
}

/**
 * Runs a chat write: its events go into the cache, and a failure shows `failure` with the server's reason, unless
 * `handled` takes the error (a form shows "name taken" under its field). Resolves to the result, or `null` after a failure.
 */
export function useChatWrite() {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  return async <T>(
    failure: string,
    write: (client: OrbitClient, workspaceId: string) => Promise<chat.ChatWrite<T>>,
    handled?: (error: unknown) => boolean,
  ): Promise<T | null> => {
    try {
      const { result, events } = await write(client, workspaceId)
      applyChatEvents(queryClient, workspaceId, events)
      return result
    } catch (error) {
      if (!handled?.(error)) showError(failure, error)
      return null
    }
  }
}

/** Your workspace role makes you a chat admin (owner or admin): you manage every channel and the categories. */
export function useIsChatAdmin(): boolean {
  const { user } = useOrbit()
  const members = useMembers()
  return isChatAdmin(members.data?.find((member) => member.user_id === user.id)?.role)
}

export function useFollowedThreads() {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: chatQueryKeys.followedThreads(workspaceId), queryFn: () => chat.followedThreads(client, workspaceId) })
}

export function useThread(rootId: string) {
  const { client, workspaceId } = useOrbit()
  return useInfiniteQuery({
    queryKey: chatQueryKeys.thread(workspaceId, rootId),
    initialPageParam: undefined as string | undefined,
    queryFn: ({ pageParam }) => chat.thread(client, workspaceId, rootId, pageParam),
    getPreviousPageParam: (first) => first.before ?? undefined,
    getNextPageParam: () => undefined,
  })
}

export function useConversationList(conversationId: string, kind: ConversationList) {
  const { client, workspaceId } = useOrbit()
  const load = { pins: chat.pins, files: chat.fileMessages, threads: chat.conversationThreads }[kind]
  return useQuery({ queryKey: chatQueryKeys.conversationList(workspaceId, conversationId, kind), queryFn: () => load(client, workspaceId, conversationId) })
}

/** Search waits for a query or a filter, like the web. */
export function useChatSearch(query: Omit<chat.SearchQuery, 'cursor'>) {
  const { client, workspaceId } = useOrbit()
  return useInfiniteQuery({
    queryKey: chatQueryKeys.search(workspaceId, query),
    enabled: query.query.trim() !== '' || Boolean(query.authorId) || Boolean(query.hasFile),
    initialPageParam: undefined as string | undefined,
    queryFn: ({ pageParam }) => chat.search(client, workspaceId, { ...query, cursor: pageParam }),
    getNextPageParam: (last) => last.cursor ?? undefined,
  })
}

/**
 * The messages of a conversation. Without `around`: from the newest page back, the cache the live socket patches. With
 * it (a search result): the page around that message, loading older and newer pages as the list reaches them.
 */
export function useMessages(conversationId: string, around: string | undefined) {
  const { client, workspaceId } = useOrbit()
  return useInfiniteQuery({
    queryKey: around ? chatQueryKeys.around(workspaceId, conversationId, around) : keys.messages(workspaceId, conversationId),
    initialPageParam: (around ? { around } : undefined) as chat.MessageCursor | undefined,
    queryFn: ({ pageParam }) => (pageParam ? chat.messagePage(client, workspaceId, conversationId, pageParam) : api.messages(client, workspaceId, conversationId)),
    getPreviousPageParam: (first) => (first.before ? { before: first.before } : undefined),
    getNextPageParam: (last) => (last.after ? { after: last.after } : undefined),
  })
}

/** The workspace's custom emoji by name (`party_parrot` → its image path). */
export function useCustomEmoji(): ReadonlyMap<string, string> {
  const { client, workspaceId } = useOrbit()
  const query = useQuery({ queryKey: chatQueryKeys.customEmoji(workspaceId), queryFn: () => chat.customEmoji(client, workspaceId), staleTime: 5 * 60_000 })
  return useMemo(() => new Map((query.data ?? []).map((emoji) => [emoji.name, emoji.url])), [query.data])
}

/** The picker's groups from the server's emojibase copy; the built-in set while it loads or when it cannot. */
export function useEmojiGroups(): EmojiGroup[] {
  const { session } = useOrbit()
  const query = useQuery({ queryKey: chatQueryKeys.emojibase(session.server), queryFn: () => chat.emojibase(session.server), staleTime: Infinity, gcTime: Infinity, retry: 1 })
  return useMemo(() => (query.data ? emojiGroups(query.data) : BUILT_IN_EMOJI), [query.data])
}

/** An image source for a server path: the session's bearer token goes with it. */
export function useServerImage() {
  const { session } = useOrbit()
  return (path: string) => ({ uri: chat.serverUrl(session.server, path), headers: { Authorization: `Bearer ${session.token}` } })
}

/**
 * The live socket patches conversations, not threads. A new reply changes its root in the conversation's cache
 * (`reply_count`, `last_reply_at`): when that copy moves past the thread's own, the thread refetches.
 */
export function useThreadFollowsRoot(root: MessageRecord | undefined) {
  const { workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  useEffect(() => {
    if (!root) return
    const key = keys.messages(workspaceId, root.conversation_id)
    return queryClient.getQueryCache().subscribe((event) => {
      if (event.type !== 'updated' || JSON.stringify(event.query.queryKey) !== JSON.stringify(key)) return
      const data = event.query.state.data as { pages: { items: MessageRecord[] }[] } | undefined
      const copy = data?.pages.flatMap((page) => page.items).find((item) => item.id === root.id)
      if (copy && (copy.reply_count !== root.reply_count || copy.last_reply_at !== root.last_reply_at || copy.edited_at !== root.edited_at)) {
        void queryClient.invalidateQueries({ queryKey: chatQueryKeys.thread(workspaceId, root.id) })
      }
    })
  }, [queryClient, root, workspaceId])
}
