import { useQueryClient } from '@tanstack/react-query'
import { type ReactNode, useEffect } from 'react'
import { useCurrentUser } from '@/features/auth/api'
import { useMembers } from '@/features/workspaces/api'
import type { User } from '@/features/workspaces/models'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { ChatContext } from './chatContext'
import type { ChatClient } from './client'
import { applyChatEvent } from './events'
import { clearTyping, noteTyping, resetLiveStore, setConnectionStatus, setOnlineUsers, setPresence } from './liveStore'
import { createMockChatClient } from './mockClient'

/** One client for each workspace and user, kept for the page lifetime so chat state survives navigation. */
const clients = new Map<string, ChatClient>()

/** The only place that picks the implementation: the HTTP client replaces this one call. */
function clientFor(workspaceId: string, currentUserId: string, members: User[]): ChatClient {
  const key = `${workspaceId}:${currentUserId}`
  let client = clients.get(key)
  if (!client) {
    client = createMockChatClient({ workspaceId, currentUserId, members })
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
  const members = useMembers(workspaceId).data
  const currentUserId = useCurrentUser().data?.id ?? null
  const client = enabled && members && currentUserId ? clientFor(workspaceId, currentUserId, members) : null

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
