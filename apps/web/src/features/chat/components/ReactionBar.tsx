import { toggleReaction } from '../../../mock/actions'
import type { ChatMessage } from '../../../mock/types'

export function ReactionBar({ message, currentUserId }: { message: ChatMessage; currentUserId: string }) {
  if (message.reactions.length === 0) return null
  return (
    <div className="chat-reactions">
      {message.reactions.map((reaction) => (
        <button
          key={reaction.emoji}
          type="button"
          className="chat-reaction-pill"
          data-mine={reaction.userIds.includes(currentUserId) ? 'true' : undefined}
          onClick={() => toggleReaction(message.id, reaction.emoji)}
        >
          {reaction.emoji} {reaction.userIds.length}
        </button>
      ))}
    </div>
  )
}
