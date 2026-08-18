import { Copy, Reply } from 'reicon-react'
import { Avatar } from '../../../components/ui/Avatar'
import { toggleReaction } from '../../../mock/actions'
import type { AppState, ChatMessage } from '../../../mock/types'
import { chatTime, displayName, jumpToMessage, mentionsUser, splitMentions } from '../chatLib'
import { ReactionBar } from './ReactionBar'

const QUICK_EMOJIS = ['👍', '👀', '😂']

interface MessageItemProps {
  state: AppState
  message: ChatMessage
  groupStart: boolean
  onReply: (message: ChatMessage) => void
}

export function MessageItem({ state, message, groupStart, onReply }: MessageItemProps) {
  const author = state.users.find((u) => u.id === message.authorId)
  const me = state.users.find((u) => u.id === state.currentUserId)
  const isExternal = message.authorType === 'discord' || message.authorType === 'github'
  const name = displayName(state, message)
  const mentioned = mentionsUser(message, me)

  const replyTarget = message.replyToId
    ? state.chatMessages.find((m) => m.id === message.replyToId)
    : null

  return (
    <div
      className="chat-msg"
      id={`message-${message.id}`}
      data-group-start={groupStart ? 'true' : undefined}
      data-mention={mentioned ? 'true' : undefined}
    >
      <div className="chat-msg-gutter">
        {groupStart ? (
          <Avatar user={isExternal ? null : author} name={isExternal ? name : undefined} size={40} />
        ) : (
          <span className="chat-msg-gutter-time">{chatTime(message.createdAt)}</span>
        )}
      </div>
      <div className="chat-msg-main">
        {replyTarget ? (
          <button
            type="button"
            className="chat-reply-ref"
            onClick={() => jumpToMessage(replyTarget.id)}
          >
            <span className="chat-reply-ref-name">@{displayName(state, replyTarget)}</span>
            <span className="chat-reply-ref-preview">{replyTarget.content}</span>
          </button>
        ) : null}
        {groupStart ? (
          <div className="chat-msg-meta">
            <span className="chat-msg-author truncate" style={author ? { color: author.color } : undefined}>
              {name}
            </span>
            {isExternal ? (
              <span className="chat-msg-source-badge" data-source={message.authorType}>
                {message.authorType === 'discord' ? 'DISCORD' : 'GITHUB'}
                {message.externalAuthor?.source ? ` · ${message.externalAuthor.source}` : ''}
              </span>
            ) : null}
            <span className="chat-msg-time">{chatTime(message.createdAt)}</span>
          </div>
        ) : null}
        <div
          className="chat-msg-content"
          data-recessed={message.authorType === 'github' ? 'true' : undefined}
        >
          {splitMentions(message.content, state.users).map((part, index) =>
            part.mention ? (
              <span key={index} className="chat-mention-token">
                {part.text}
              </span>
            ) : (
              <span key={index}>{part.text}</span>
            ),
          )}
        </div>
        <ReactionBar message={message} currentUserId={state.currentUserId} />
      </div>

      <div className="chat-hover-toolbar">
        {QUICK_EMOJIS.map((emoji) => (
          <button
            key={emoji}
            type="button"
            aria-label={`React ${emoji}`}
            onClick={() => toggleReaction(message.id, emoji)}
          >
            {emoji}
          </button>
        ))}
        <span className="chat-hover-toolbar-sep" />
        <button
          type="button"
          aria-label="Reply"
          onClick={() => onReply(message)}
        >
          <Reply size={16} />
        </button>
        <button
          type="button"
          aria-label="Copy text"
          onClick={() => navigator.clipboard.writeText(message.content)}
        >
          <Copy size={16} />
        </button>
      </div>
    </div>
  )
}
