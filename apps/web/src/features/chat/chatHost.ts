import { createContext, useContext, type ReactNode } from 'react'
import type { Message } from './api/types'

/**
 * What chat components need from other features. A feature component may not import another feature, so `ChatPage`
 * (a route component) supplies these.
 */
export interface ChatHost {
  /** A card for an Orbit task, page or view URL in a message; `null` for any other URL. */
  renderLinkCard: (url: string) => ReactNode
  /**
   * A bare Orbit URL in the text of a message: a chip that names the task, page or view, or nothing when `hidden` (the card
   * stands in for the URL). `plain` is the default link, for every other URL and for what the reader cannot see. A typed
   * task identifier comes here as the URL of its task, with the typed text as `plain`.
   */
  renderLink: (url: string, plain: ReactNode, hidden: boolean) => ReactNode
  /** The keys of the workspace's projects: `ENG-12` in a message is a task only with one of these keys. */
  taskKeys: readonly string[]
  /** Opens the new task dialog, filled in from this message. */
  createTask: (message: Message) => void
}

export const ChatHostContext = createContext<ChatHost>({ renderLinkCard: () => null, renderLink: (_url, plain) => plain, taskKeys: [], createTask: () => {} })

export function useChatHost(): ChatHost {
  return useContext(ChatHostContext)
}
