// Port of the chat reference MessageList (grouping, date separators, jump-to-present, enter animation)
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { useSearchParams } from 'react-router'
import { Messages, Pin, Trash } from 'reicon-react'
import { hidePinNotice } from '../../../mock/actions'
import { ConfirmDeleteModal } from './ChannelModals'
import type { AppState, Channel, ChatMessage } from '../../../mock/types'
import {
  buildMentionTokens,
  formatMessageDate,
  isSameDay,
  jumpToMessage,
  messageMentionsCurrentUser,
} from '../chatLib'
import { MessageItem } from './MessageItem'

const GROUP_WINDOW_MS = 300_000 // the chat reference: 300 seconds

export function MessageList({
  state,
  channel,
  onReply,
  onOpenThread,
}: {
  state: AppState
  channel: Channel
  onReply: (message: ChatMessage) => void
  onOpenThread: (message: ChatMessage) => void
}) {
  const [searchParams] = useSearchParams()
  const jumpTargetId = searchParams.get('message_id')
  const containerRef = useRef<HTMLDivElement>(null)
  const bottomRef = useRef<HTMLDivElement>(null)
  const isAtBottomRef = useRef(true)
  const [showJumpToPresent, setShowJumpToPresent] = useState(false)

  const me = state.users.find((u) => u.id === state.currentUserId)
  const mentionTokens = useMemo(() => buildMentionTokens(state.users), [state.users])
  const messages = useMemo(
    () =>
      state.chatMessages
        .filter((m) => m.channelId === channel.id && !m.threadRootId)
        .sort((a, b) => a.createdAt.localeCompare(b.createdAt)),
    [state.chatMessages, channel.id],
  )
  // the chat reference buildTimeline: a "pinned a message" notice follows the pinned message in time order
  const notices = useMemo(
    () => messages.filter((m) => m.pinned && m.pinnedAt && !m.pinNoticeHidden).map((m) => ({ message: m, at: m.pinnedAt! })),
    [messages],
  )
  const noticesBefore = (index: number) => {
    const current = messages[index]
    const next = messages[index + 1]
    return notices.filter((n) => n.at > current.createdAt && (!next || n.at <= next.createdAt))
  }

  // ChatArea remounts this component per channel (key={channel.id}), so this
  // only needs to pin the initial scroll to the bottom.
  useEffect(() => {
    const el = containerRef.current
    if (el) el.scrollTop = el.scrollHeight
    // the chat reference: ?message_id= (search result / pin jump) scrolls to and flashes the message
    if (jumpTargetId) requestAnimationFrame(() => jumpToMessage(jumpTargetId))
    // eslint-disable-next-line react-hooks/exhaustive-deps
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
              !msg.replyToId &&
              !msg.startsThread &&
              !prevMsg.startsThread

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
                  onOpenThread={onOpenThread}
                />
                {noticesBefore(i).map((notice) => (
                  <PinnedNotice key={`${notice.message.id}-pin`} state={state} message={notice.message} />
                ))}
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

/** the chat reference PinnedNotice: "X pinned a message to this channel." with a right-click "Delete pin notice". */
function PinnedNotice({ state, message }: { state: AppState; message: ChatMessage }) {
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null)
  const [confirmOpen, setConfirmOpen] = useState(false)
  const actor = state.users.find((u) => u.id === message.pinnedBy)
  const actorName = actor?.name ?? 'Someone'

  useEffect(() => {
    if (!menu) return
    const close = () => setMenu(null)
    document.addEventListener('mousedown', close)
    document.addEventListener('keydown', close)
    return () => {
      document.removeEventListener('mousedown', close)
      document.removeEventListener('keydown', close)
    }
  }, [menu])

  return (
    <>
      <div
        className="fc-pin-notice"
        onContextMenu={(e) => {
          e.preventDefault()
          setMenu({ x: e.clientX, y: e.clientY })
        }}
      >
        <div className="fc-pin-notice-icon">
          <Pin size={20} />
        </div>
        <div className="fc-pin-notice-text">
          <strong style={actor ? { color: actor.color } : undefined}>{actorName}</strong> pinned <strong>a message</strong> to this channel.
        </div>
      </div>
      {menu
        ? createPortal(
            <div className="fc-menu fc-context-menu" style={{ left: menu.x, top: menu.y }} onMouseDown={(e) => e.stopPropagation()}>
              <button
                className="fc-menu-item"
                data-danger="true"
                onClick={() => {
                  setMenu(null)
                  setConfirmOpen(true)
                }}
              >
                <Trash size={16} />
                Delete pin notice
              </button>
            </div>,
            document.body,
          )
        : null}
      {confirmOpen ? (
        <ConfirmDeleteModal
          title="Delete pin notice?"
          description="This removes the notice from the channel. The message stays pinned."
          onClose={() => setConfirmOpen(false)}
          onConfirm={() => {
            hidePinNotice(message.id)
            setConfirmOpen(false)
          }}
        />
      ) : null}
    </>
  )
}
