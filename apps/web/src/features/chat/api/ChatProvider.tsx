import { type QueryClient, useQueryClient } from '@tanstack/react-query'
import { queryKeys } from '@/api/queryKeys'
import { type ReactNode, useEffect } from 'react'
import { useCurrentUser } from '@/features/auth/api'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { ChatContext } from './chatContext'
import type { ChatClient } from './client'
import { applyChatEvent } from './events'
import { createHttpChatClient } from './httpClient'
import { clearTyping, noteTyping, resetLiveStore, setConnectionStatus, setOnlineUsers, setPresence } from './liveStore'

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
    })
    clients.set(key, client)
  }
  return client
}

/**
 * Provides the chat client and keeps the query cache and the live stores current from its events. Mount once.
 * With `enabled` false there is no client: chat queries stay idle and the badge counts are zero.
 */
export function ChatProvider({ enabled = true, children }: { enabled?: boolean; children: ReactNode }) {
  const queryClient = useQueryClient()
  const workspaceId = useWorkspace().workspace.id
  const currentUserId = useCurrentUser().data?.id ?? null
  const client = enabled && currentUserId ? clientFor(workspaceId, currentUserId, queryClient) : null

  useEffect(() => {
    if (!client) return
    let active = true
    resetLiveStore()
    void client
      .getPresence()
      .then((userIds) => active && setOnlineUsers(userIds))
      .catch(() => {})
    const unsubscribe = client.subscribe((event) => {
      switch (event.type) {
        case 'typing':
          if (event.userId !== currentUserId) noteTyping(event.conversationId, event.threadRootId, event.userId)
          break
        case 'presence':
          setPresence(event.userId, event.online)
          break
        case 'connection':
          setConnectionStatus(event.status)
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
    }
  }, [client, currentUserId, queryClient, workspaceId])

  return <ChatContext.Provider value={{ client, workspaceId, currentUserId }}>{children}</ChatContext.Provider>
}
