import { type QueryClient, useQueryClient } from '@tanstack/react-query'
import { queryKeys } from '@/api/queryKeys'
import { type ReactNode, useEffect, useState } from 'react'
import { useCurrentUser } from '@/features/auth/api'
import { useAwayMinutes, watchAway, watchIdle } from '@/features/realtime/idle'
import { handleNotice } from '@/features/realtime/notify'
import { replacePresence, resetPresence, setPresence } from '@/features/realtime/presence'
import { useCustomEmojiByName } from '@/features/workspaces/customEmoji'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { ChatContext } from './chatContext'
import type { ChatClient } from './client'
import { applyChatEvent } from './events'
import { createHttpChatClient } from './httpClient'
import { reportConnection } from '@/lib/connection'
import { CustomEmojiContext, NO_CUSTOM_EMOJI } from '@/lib/customEmojiContext'
import { claimLocalCache, trimMedia, withinTime } from '@/lib/localCache'
import { forgetConversation, persistChatCache, restoreChatCache } from './persist'
import { clearTyping, noteTyping, resetLiveStore, setConnectionStatus } from './liveStore'

/** One client for each workspace and user, kept for the page lifetime so chat state survives navigation. */
const clients = new Map<string, ChatClient>()

/** The only place that picks the implementation. */
function clientFor(workspaceId: string, currentUserId: string, queryClient: QueryClient): ChatClient {
  const key = `${workspaceId}:${currentUserId}`
  let client = clients.get(key)
  if (!client) {
    client = createHttpChatClient({
      workspaceId,
      currentUserId,
      // Chat writes are not in the workspace's change feed, so the inbox learns of a chat mention here.
      onInboxChanged: () => void queryClient.invalidateQueries({ queryKey: [...queryKeys.workspace(workspaceId), 'notifications'] }),
      onNotify: (notice) => void handleNotice(notice).catch(() => {}),
    })
    clients.set(key, client)
  }
  return client
}

/**
 * Provides the chat client and keeps the query cache and the live stores current from its events. Mount once.
 * With `enabled` false there is no client: chat queries stay idle and the badge counts are zero. It also supplies the
 * workspace's custom emoji to everything below it (`CustomEmojiContext`).
 */
export function ChatProvider({ enabled = true, children }: { enabled?: boolean; children: ReactNode }) {
  const queryClient = useQueryClient()
  const workspaceId = useWorkspace().workspace.id
  const currentUserId = useCurrentUser().data?.id ?? null
  const customEmoji = useCustomEmojiByName(workspaceId, enabled).data ?? NO_CUSTOM_EMOJI
  // The copy of chat on this device goes into the query cache before the first query reads the server.
  const copy = enabled && currentUserId ? `${workspaceId}:${currentUserId}` : null
  const [restored, setRestored] = useState<string | null>(null)
  useEffect(() => {
    if (!copy || !currentUserId) return
    let active = true
    void withinTime(claimLocalCache(currentUserId).then(() => restoreChatCache(queryClient, workspaceId))).then(() => active && setRestored(copy))
    void trimMedia().catch(() => {})
    const stop = persistChatCache(queryClient, workspaceId)
    return () => {
      active = false
      stop()
    }
  }, [copy, currentUserId, queryClient, workspaceId])
  const client = enabled && currentUserId && restored === copy ? clientFor(workspaceId, currentUserId, queryClient) : null

  useEffect(() => {
    if (!client) return
    let active = true
    resetLiveStore()
    resetPresence()
    void client
      .getPresence()
      .then((presence) => active && replacePresence(presence))
      .catch(() => {})
    const unsubscribe = client.subscribe((event) => {
      switch (event.type) {
        case 'typing':
          if (event.userId !== currentUserId) noteTyping(event.conversationId, event.threadRootId, event.userId)
          break
        case 'presence':
          setPresence(event.userId, event.presence)
          break
        case 'self.changed':
          void queryClient.invalidateQueries({ queryKey: queryKeys.currentUser })
          break
        case 'connection':
          setConnectionStatus(event.status)
          break
        case 'conversation.removed':
          forgetConversation(workspaceId, event.conversationId)
          applyChatEvent(queryClient, workspaceId, event)
          break
        case 'message.created':
          clearTyping(event.message.conversationId, event.message.threadRootId, event.message.authorId)
          applyChatEvent(queryClient, workspaceId, event)
          break
        default:
          applyChatEvent(queryClient, workspaceId, event)
      }
    })
    return () => {
      active = false
      unsubscribe()
      // Logout or a workspace switch: nothing reports this socket as back, so the banner must not wait for it.
      reportConnection('chat', false)
    }
  }, [client, currentUserId, queryClient, workspaceId])

  // The others see the user as idle after ten minutes without input in this tab.
  useEffect(() => (client ? watchIdle((idle) => client.setIdle(idle)) : undefined), [client])
  // A window out of focus for the time the user chose: their other devices get the notifications too.
  const [awayMinutes] = useAwayMinutes()
  useEffect(() => (client ? watchAway((away) => client.setAway(away), awayMinutes * 60_000) : undefined), [client, awayMinutes])

  return (
    <CustomEmojiContext value={customEmoji}>
      <ChatContext.Provider value={{ client, workspaceId, currentUserId }}>{children}</ChatContext.Provider>
    </CustomEmojiContext>
  )
}
