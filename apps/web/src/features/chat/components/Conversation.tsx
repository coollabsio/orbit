import { Fragment, useEffect, useMemo, useRef, useState } from 'react'
import { ArrowLeft, Hashtag, Messages, People } from 'reicon-react'
import { useNavigate } from 'react-router'
import { dayLabel } from '../../../lib/format'
import type { AppState, Channel, ChatMessage } from '../../../mock/types'
import { authorKey } from '../chatLib'
import { Composer } from './Composer'
import { MessageItem } from './MessageItem'

const GROUP_WINDOW_MS = 5 * 60_000

interface ConversationProps {
  state: AppState
  channel: Channel
  membersOpen: boolean
  onToggleMembers: () => void
}

export function Conversation({ state, channel, membersOpen, onToggleMembers }: ConversationProps) {
  const navigate = useNavigate()
  const scrollRef = useRef<HTMLDivElement>(null)
  const [replyTo, setReplyTo] = useState<ChatMessage | null>(null)
  const [awayFromBottom, setAwayFromBottom] = useState(false)

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

  const scrollToBottom = () => {
    const el = scrollRef.current
    if (el) el.scrollTo({ top: el.scrollHeight, behavior: 'smooth' })
  }

  const onScroll = () => {
    const el = scrollRef.current
    if (!el) return
    setAwayFromBottom(el.scrollHeight - el.scrollTop - el.clientHeight > 400)
  }

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
        <Hashtag size={18} className="text-faint" />
        <span className="pane-title">{channel.name}</span>
        {channel.description ? (
          <span className="chat-header-topic">{channel.description}</span>
        ) : null}
        <span className="spacer" />
        <button
          type="button"
          className="icon-button chat-members-toggle"
          aria-label="Toggle member list"
          data-active={membersOpen ? 'true' : undefined}
          onClick={onToggleMembers}
        >
          <People size={18} />
        </button>
      </div>
      <div className="chat-scroll-region">
        <div className="chat-messages" ref={scrollRef} onScroll={onScroll}>
          {messages.length === 0 ? (
            <div className="empty-state">
              <span className="chat-empty-icon-tile">
                <Messages size={28} />
              </span>
              <h2>No messages yet</h2>
              <p>Start the conversation in #{channel.name}.</p>
            </div>
          ) : (
            messages.map((message, index) => {
              const prev = index > 0 ? messages[index - 1] : null
              const newDay = !prev || dayLabel(prev.createdAt) !== dayLabel(message.createdAt)
              const groupStart =
                newDay ||
                !prev ||
                authorKey(prev) !== authorKey(message) ||
                message.replyToId !== null ||
                prev.replyToId !== null ||
                new Date(message.createdAt).getTime() - new Date(prev.createdAt).getTime() >
                  GROUP_WINDOW_MS
              return (
                <Fragment key={message.id}>
                  {newDay ? (
                    <div className="chat-day-separator">
                      <span>{dayLabel(message.createdAt)}</span>
                    </div>
                  ) : null}
                  <MessageItem
                    state={state}
                    message={message}
                    groupStart={groupStart}
                    onReply={setReplyTo}
                  />
                </Fragment>
              )
            })
          )}
        </div>
        {awayFromBottom ? (
          <div className="chat-jump-bar">
            <div className="chat-jump-bar-inner">
              You're viewing older messages
              <button type="button" className="button button-primary" onClick={scrollToBottom}>
                Jump to present
              </button>
            </div>
          </div>
        ) : null}
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
