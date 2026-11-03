import { type QueryClient } from '@tanstack/react-query'

import { type MessagePages, removeMessage, type ThreadPages, upsertMessage, upsertState, upsertThreadMessage } from './data/chat'
import type * as chat from './data/chat-api'
import { keys } from './data/keys'
import { type ChatAction, chatActions } from './data/live'
import type { ConversationStateRecord } from './data/orbit'

/**
 * Keys of the chat queries outside `data/keys.ts`. Threads sit under the `messages` prefix so a chat resync (`chatKeys`) refetches
 * them with the conversations.
 */
export const chatQueryKeys = {
  followedThreads: (workspaceId: string) => ['messages', workspaceId, 'followed-threads'] as const,
  thread: (workspaceId: string, rootId: string) => ['messages', workspaceId, 'thread', rootId] as const,
  around: (workspaceId: string, conversationId: string, messageId: string) => ['messages', workspaceId, 'around', conversationId, messageId] as const,
  conversationList: (workspaceId: string, conversationId: string, kind: ConversationList) => ['messages', workspaceId, kind, conversationId] as const,
  search: (workspaceId: string, query: Omit<chat.SearchQuery, 'cursor'>) => ['chat-search', workspaceId, query] as const,
  customEmoji: (workspaceId: string) => ['chat-emoji', workspaceId] as const,
  emojibase: (server: string) => ['emojibase', server] as const,
}

export type ConversationList = 'pins' | 'files' | 'threads'

export function applyChatActions(queryClient: QueryClient, workspaceId: string, actions: readonly ChatAction[]) {
  for (const action of actions) {
    switch (action.kind) {
      case 'upsert-message':
        queryClient.setQueryData<MessagePages>(keys.messages(workspaceId, action.conversationId), (data) => data && upsertMessage(data, action.message))
        break
      case 'remove-message':
        queryClient.setQueryData<MessagePages>(keys.messages(workspaceId, action.conversationId), (data) => data && removeMessage(data, action.messageId))
        break
      case 'state':
        queryClient.setQueryData<ConversationStateRecord[]>(keys.chatStates(workspaceId), (states) => states && upsertState(states, action.state))
        break
      case 'invalidate':
        for (const queryKey of action.keys) void queryClient.invalidateQueries({ queryKey })
        break
    }
  }
}

/**
 * Applies the events of a write the way the live socket applies them, and also to the open thread and the followed
 * threads, which the socket hook does not patch.
 */
export function applyChatEvents(queryClient: QueryClient, workspaceId: string, events: readonly chat.ChatEvent[]) {
  applyChatActions(
    queryClient,
    workspaceId,
    events.flatMap((event) => chatActions(event, workspaceId)),
  )
  for (const event of events) {
    if (event.type === 'message.created' || event.type === 'message.updated') {
      const rootId = event.message.thread_root_id ?? event.message.id
      queryClient.setQueryData<ThreadPages>(chatQueryKeys.thread(workspaceId, rootId), (data) => data && upsertThreadMessage(data, event.message))
    }
    if (event.type === 'message.deleted' && event.thread_root_id) void queryClient.invalidateQueries({ queryKey: chatQueryKeys.thread(workspaceId, event.thread_root_id) })
    if (event.type === 'thread.changed' || (event.type === 'message.created' && event.message.thread_root_id)) {
      void queryClient.invalidateQueries({ queryKey: chatQueryKeys.followedThreads(workspaceId) })
    }
  }
}
