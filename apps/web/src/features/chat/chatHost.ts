import { createContext, useContext, type ReactNode } from 'react'
import type { Message } from './api/types'

/**
 * What chat components need from other features. A feature component may not import another feature, so `ChatPage`
 * (a route component) supplies these.
 */
export interface ChatHost {
  /** A card for an Orbit task or page URL in a message; `null` for any other URL. */
  renderLinkCard: (url: string) => ReactNode
  /** Opens the new task dialog, filled in from this message. */
  createTask: (message: Message) => void
}

export const ChatHostContext = createContext<ChatHost>({ renderLinkCard: () => null, createTask: () => {} })

export function useChatHost(): ChatHost {
  return useContext(ChatHostContext)
}
