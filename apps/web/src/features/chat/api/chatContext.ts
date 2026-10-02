import { createContext, useContext } from 'react'
import type { ChatClient } from './client'

export interface ChatContextValue {
  /** `null` until the current user has loaded, and while chat is turned off. */
  client: ChatClient | null
  workspaceId: string
  currentUserId: string | null
}

export const ChatContext = createContext<ChatContextValue | null>(null)

export function useChatContext(): ChatContextValue {
  const value = useContext(ChatContext)
  if (!value) throw new Error('useChatContext must be used within ChatProvider')
  return value
}

/** The chat backend of the current workspace; `null` while it loads. */
export function useChatClient(): ChatClient | null {
  return useChatContext().client
}
