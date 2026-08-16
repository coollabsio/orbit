import { toggleReaction } from '../../../mock/actions'
import type { ChatMessage } from '../../../mock/types'

interface ReactionBarProps {
  message: ChatMessage
  currentUserId: string
}

export function ReactionBar({ message, currentUserId }: ReactionBarProps) {
  if (message.reactions.length === 0) return null
  return (
    <div className="chat-reactions">
      {message.reactions.map((reaction) => (
        <button
          key={reaction.emoji}
          type="button"
          className="chat-reaction"
          data-mine={reaction.userIds.includes(currentUserId) ? 'true' : undefined}
          onClick={() => toggleReaction(message.id, reaction.emoji)}
        >
          <span>{reaction.emoji}</span>
          <span>{reaction.userIds.length}</span>
        </button>
      ))}
    </div>
  )
}
