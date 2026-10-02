import { createContext, useContext } from 'react'
import type { Message } from '../../api/types'
import type { ChatPeople } from './people'
import type { MenuAnchor } from './useMessageActions'

/** What every row of one message list shares; `MessageList` provides it. */
export interface MessageListContextValue {
  currentUserId: string | null
  /** The list shows a thread: no "Reply in thread" and no reply summary rows. */
  inThread: boolean
  phone: boolean
  people: ChatPeople
  /** The three reactions of the hover toolbar. */
  quickEmojis: string[]
  react: (message: Message, emoji: string) => void
  openThread: (message: Message) => void
  openMenu: (message: Message, anchor: MenuAnchor, align: 'start' | 'end') => void
  /** The full emoji picker, to add a reaction. */
  openPicker: (message: Message, anchor: MenuAnchor, align: 'start' | 'end') => void
  /** The mobile action sheet (long press). */
  openSheet: (message: Message) => void
  saveEdit: (message: Message, text: string) => void
  endEdit: () => void
  retry: (message: Message) => void
  discard: (message: Message) => void
}

export const MessageListContext = createContext<MessageListContextValue | null>(null)

export function useMessageList(): MessageListContextValue {
  const value = useContext(MessageListContext)
  if (!value) throw new Error('A message row must be inside MessageList')
  return value
}
