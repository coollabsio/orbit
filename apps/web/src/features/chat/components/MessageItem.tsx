import { EmojiHappy, Reply } from 'reicon-react'
import { Avatar } from '../../../components/ui/Avatar'
import { Dropdown } from '../../../components/ui/Dropdown'
import { timeOfDay } from '../../../lib/format'
import { toggleReaction } from '../../../mock/actions'
import type { AppState, ChatMessage } from '../../../mock/types'
import { ReactionBar } from './ReactionBar'

const EMOJI_SET = ['👍', '🎉', '❤️', '🚀', '👀', '😄']

interface MessageItemProps {
  state: AppState
  message: ChatMessage
  groupStart: boolean
  onReply: (message: ChatMessage) => void
}

export function MessageItem({ state, message, groupStart, onReply }: MessageItemProps) {
  const author = state.users.find((u) => u.id === message.authorId)
  const isExternal = message.authorType === 'discord' || message.authorType === 'github'
  const displayName = isExternal
    ? (message.externalAuthor?.name ?? message.authorType)
    : (author?.name ?? 'Unknown')

  const replyTarget = message.replyToId
    ? state.chatMessages.find((m) => m.id === message.replyToId)
    : null
  const replyAuthorName = replyTarget
    ? replyTarget.authorType === 'user'
      ? (state.users.find((u) => u.id === replyTarget.authorId)?.name ?? 'Unknown')
      : (replyTarget.externalAuthor?.name ?? replyTarget.authorType)
    : null

  return (
    <div className="chat-msg" data-group-start={groupStart ? 'true' : undefined}>
      <div className="chat-msg-gutter">
        {groupStart ? (
          isExternal ? (
            <Avatar user={null} name={displayName} size={32} />
          ) : (
            <Avatar user={author} size={32} />
          )
        ) : (
          <span className="chat-msg-gutter-time">{timeOfDay(message.createdAt)}</span>
        )}
      </div>
      <div className="chat-msg-main">
        {groupStart ? (
          <div className="chat-msg-meta">
            <span className="chat-msg-author truncate">{displayName}</span>
            {isExternal ? (
              <span
                className="badge"
                data-tone={message.authorType === 'discord' ? 'info' : undefined}
              >
                {message.authorType === 'discord' ? 'Discord' : 'GitHub'}
                {message.externalAuthor?.source ? ` · ${message.externalAuthor.source}` : ''}
              </span>
            ) : null}
            <span className="chat-msg-time">{timeOfDay(message.createdAt)}</span>
          </div>
        ) : null}
        {replyTarget && replyAuthorName ? (
          <div className="chat-msg-reply-quote">
            <span className="truncate">
              ↩ {replyAuthorName}: {replyTarget.content.slice(0, 60)}
            </span>
          </div>
        ) : null}
        <div
          className="chat-msg-content"
          data-recessed={message.authorType === 'github' ? 'true' : undefined}
        >
          {message.content}
        </div>
        <ReactionBar message={message} currentUserId={state.currentUserId} />
      </div>
      <div className="chat-msg-actions">
        <Dropdown
          align="right"
          className="chat-emoji-popover"
          trigger={(open) => (
            <button
              type="button"
              className="icon-button"
              aria-label="Add reaction"
              aria-expanded={open}
            >
              <EmojiHappy size={16} />
            </button>
          )}
        >
          {(close) =>
            EMOJI_SET.map((emoji) => (
              <button
                key={emoji}
                type="button"
                className="popover-option"
                onClick={() => {
                  toggleReaction(message.id, emoji)
                  close()
                }}
              >
                {emoji}
              </button>
            ))
          }
        </Dropdown>
        <button
          type="button"
          className="icon-button"
          aria-label="Reply"
          onClick={() => onReply(message)}
        >
          <Reply size={16} />
        </button>
      </div>
    </div>
  )
}
