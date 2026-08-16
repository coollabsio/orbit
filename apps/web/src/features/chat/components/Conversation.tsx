import { Fragment, useEffect, useMemo, useRef, useState } from 'react'
import { ArrowLeft, Hashtag } from 'reicon-react'
import { useNavigate } from 'react-router'
import { dayLabel } from '../../../lib/format'
import type { AppState, Channel, ChatMessage } from '../../../mock/types'
import { Composer } from './Composer'
import { MessageItem } from './MessageItem'

const GROUP_WINDOW_MS = 5 * 60_000

function authorKey(message: ChatMessage): string {
  return message.authorType === 'user'
    ? `user:${message.authorId}`
    : `${message.authorType}:${message.externalAuthor?.name ?? message.authorId}`
}

interface ConversationProps {
  state: AppState
  channel: Channel
}

export function Conversation({ state, channel }: ConversationProps) {
  const navigate = useNavigate()
  const scrollRef = useRef<HTMLDivElement>(null)
  const [replyTo, setReplyTo] = useState<ChatMessage | null>(null)

  const messages = useMemo(
    () =>
      state.chatMessages
        .filter((m) => m.channelId === channel.id)
        .sort((a, b) => a.createdAt.localeCompare(b.createdAt)),
    [state.chatMessages, channel.id],
  )

  useEffect(() => {
    const el = scrollRef.current
    if (el) el.scrollTop = el.scrollHeight
  }, [channel.id, messages.length])

  return (
    <div className="pane chat-conversation">
      <div className="pane-header">
        <button
          type="button"
          className="icon-button chat-back"
          aria-label="Back to channels"
          onClick={() => navigate('/chat')}
        >
          <ArrowLeft size={16} />
        </button>
        <Hashtag size={16} className="text-faint" />
        <span className="pane-title">{channel.name}</span>
        <span className="chat-header-description truncate">{channel.description}</span>
        <span className="spacer" />
      </div>
      <div className="chat-messages" ref={scrollRef}>
        {messages.map((message, index) => {
          const prev = index > 0 ? messages[index - 1] : null
          const newDay = !prev || dayLabel(prev.createdAt) !== dayLabel(message.createdAt)
          const groupStart =
            newDay ||
            !prev ||
            authorKey(prev) !== authorKey(message) ||
            message.replyToId !== null ||
            new Date(message.createdAt).getTime() - new Date(prev.createdAt).getTime() >
              GROUP_WINDOW_MS
          return (
            <Fragment key={message.id}>
              {newDay ? (
                <div className="chat-day-separator">{dayLabel(message.createdAt)}</div>
              ) : null}
              <MessageItem
                state={state}
                message={message}
                groupStart={groupStart}
                onReply={setReplyTo}
              />
            </Fragment>
          )
        })}
      </div>
      <Composer
        state={state}
        channel={channel}
        replyTo={replyTo}
        onClearReply={() => setReplyTo(null)}
      />
    </div>
  )
}
