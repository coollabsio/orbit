// Port of the chat reference MessageList (grouping, date separators, jump-to-present, enter animation)
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Messages } from 'reicon-react'
import type { AppState, Channel, ChatMessage } from '../../../mock/types'
import {
  buildMentionTokens,
  formatMessageDate,
  isSameDay,
  messageMentionsCurrentUser,
} from '../chatLib'
import { MessageItem } from './MessageItem'

const GROUP_WINDOW_MS = 300_000 // the chat reference: 300 seconds

export function MessageList({
  state,
  channel,
  onReply,
}: {
  state: AppState
  channel: Channel
  onReply: (message: ChatMessage) => void
}) {
  const containerRef = useRef<HTMLDivElement>(null)
  const bottomRef = useRef<HTMLDivElement>(null)
  const isAtBottomRef = useRef(true)
  const [showJumpToPresent, setShowJumpToPresent] = useState(false)

  const me = state.users.find((u) => u.id === state.currentUserId)
  const mentionTokens = useMemo(() => buildMentionTokens(state.users), [state.users])
  const messages = useMemo(
    () =>
      state.chatMessages
        .filter((m) => m.channelId === channel.id)
        .sort((a, b) => a.createdAt.localeCompare(b.createdAt)),
    [state.chatMessages, channel.id],
  )

  // ChatArea remounts this component per channel (key={channel.id}), so this
  // only needs to pin the initial scroll to the bottom.
  useEffect(() => {
    const el = containerRef.current
    if (el) el.scrollTop = el.scrollHeight
  }, [])

  useEffect(() => {
    if (isAtBottomRef.current) bottomRef.current?.scrollIntoView({ behavior: 'auto' })
  }, [messages.length])

  const updateJumpToPresent = useCallback(() => {
    const el = containerRef.current
    if (!el) return
    const isAtBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 50
    isAtBottomRef.current = isAtBottom
    if (isAtBottom) {
      setShowJumpToPresent(false)
      return
    }
    const containerTop = el.getBoundingClientRect().top
    const visibleMessage = Array.from(el.querySelectorAll<HTMLElement>('[data-message-created-at]')).find(
      (node) => node.getBoundingClientRect().bottom > containerTop + 24,
    )
    const createdAt = Number(visibleMessage?.dataset.messageCreatedAt)
    const messageIndex = Number(visibleMessage?.dataset.messageIndex)
    const olderThanTenHours = Number.isFinite(createdAt) && Date.now() - createdAt > 36_000_000
    const farBehind = Number.isFinite(messageIndex) && messages.length - 1 - messageIndex > 50
    setShowJumpToPresent(olderThanTenHours || farBehind)
  }, [messages.length])

  function jumpToPresent() {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth', block: 'end' })
    isAtBottomRef.current = true
    setShowJumpToPresent(false)
  }

  return (
    <div className="fc-list-region">
      <div ref={containerRef} className="fc-list-scroll" onScroll={updateJumpToPresent}>
        {messages.length === 0 ? (
          <div className="fc-empty">
            <div className="fc-empty-inner">
              <div className="fc-empty-icon">
                <Messages size={28} />
              </div>
              <h2>No messages yet</h2>
              <p>Start the conversation in #{channel.name}.</p>
            </div>
          </div>
        ) : (
          messages.map((msg, i) => {
            const prevMsg = i > 0 ? messages[i - 1] : null
            const startsNewDay = !prevMsg || !isSameDay(prevMsg.createdAt, msg.createdAt)
            const compact =
              !!prevMsg &&
              !startsNewDay &&
              prevMsg.authorType === msg.authorType &&
              prevMsg.authorId === msg.authorId &&
              (prevMsg.externalAuthor?.name ?? '') === (msg.externalAuthor?.name ?? '') &&
              new Date(msg.createdAt).getTime() - new Date(prevMsg.createdAt).getTime() < GROUP_WINDOW_MS &&
              !msg.replyToId

            return (
              <div
                key={msg.id}
                data-message-created-at={new Date(msg.createdAt).getTime()}
                data-message-index={i}
                className="animate-message-row-enter"
                style={{ animationDelay: `${Math.min(i, 8) * 8}ms` }}
              >
                {startsNewDay ? <DateSeparator iso={msg.createdAt} /> : null}
                <MessageItem
                  state={state}
                  message={msg}
                  compact={compact}
                  mentionedCurrentUser={messageMentionsCurrentUser(msg, me)}
                  mentionTokens={mentionTokens}
                  onReply={onReply}
                />
              </div>
            )
          })
        )}
        <div ref={bottomRef} />
      </div>
      {showJumpToPresent ? (
        <div className="fc-jump">
          <div className="fc-jump-inner">
            <span>You're Viewing Older Messages</span>
            <button type="button" className="fc-jump-button" onClick={jumpToPresent}>
              Jump To Present
            </button>
          </div>
        </div>
      ) : null}
    </div>
  )
}

function DateSeparator({ iso }: { iso: string }) {
  return (
    <div className="fc-date-separator">
      <span>{formatMessageDate(iso)}</span>
    </div>
  )
}
