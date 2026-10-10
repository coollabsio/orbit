import { ReactionChips } from '@/components/common/ReactionChips'
import type { Message } from '../../api/types'
import { useMessageList } from './messageListContext'

/** Reaction chips under a message. */
export function MessageReactions({ message }: { message: Message }) {
  const { currentUserId, people, react } = useMessageList()
  if (message.deleted) return null
  return (
    <ReactionChips
      slot="message-reactions"
      reactions={message.reactions}
      currentUserId={currentUserId}
      nameOf={(id) => people.byId.get(id)?.name ?? 'Unknown'}
      onToggle={(emoji) => react(message, emoji)}
    />
  )
}
