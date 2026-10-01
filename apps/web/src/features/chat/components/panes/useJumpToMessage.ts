import type { Message } from '@/features/chat/api/types'
import { useChatNavigation } from '@/features/chat/useChatNavigation'

/** Goes to a message where it lives: a reply opens its thread in the pane, any other message its conversation. */
export function useJumpToMessage() {
  const { openConversation, openThreadPane } = useChatNavigation()
  return (message: Pick<Message, 'id' | 'conversationId' | 'threadRootId'>) => {
    if (message.threadRootId) openThreadPane(message.conversationId, message.threadRootId, message.id)
    else openConversation(message.conversationId, message.id)
  }
}
