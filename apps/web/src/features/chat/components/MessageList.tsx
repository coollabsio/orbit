// Port of the chat reference MessageList (grouping, date separators, jump-to-present, enter animation)
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useSearchParams } from 'react-router'
import { Message as MessageSquare, Trash as Trash2 } from 'reicon-react'
import { PinIcon } from '@/components/common/icons/PinIcon'
import { Button } from '@/components/ui/button'
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuTrigger } from '@/components/ui/context-menu'
import { hidePinNotice } from '@/mock/actions'
import { ConfirmDeleteModal } from '@/components/common/ConfirmDeleteModal'
import type { AppState, Channel, ChatMessage } from '@/mock/types'
import {
  formatMessageDate,
  isSameDay,
  jumpToMessage,
  messageMentionsCurrentUser,
} from '@/features/chat/chatLib'
import { buildMentionTokens } from '@/lib/mentions'
import { MessageItem } from './MessageItem'

const GROUP_WINDOW_MS = 300_000 // the chat reference: 300 seconds

const menuItemClass =
  'gap-2 rounded-lg px-2.5 py-1.5 font-medium text-destructive hover:bg-destructive/10 focus:text-destructive data-highlighted:text-destructive [&>svg]:text-destructive focus:*:[svg]:text-destructive'

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
  const mentionTokens = useMemo(() => buildMentionTokens(state.users, state.channels), [state.users, state.channels])
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
    <div className="relative min-h-0 flex-1">
      <div ref={containerRef} className="h-full overflow-y-auto px-4 pt-2 pb-5 max-[899px]:px-2.5 max-[899px]:pt-1.5 max-[899px]:pb-3.5" onScroll={updateJumpToPresent}>
        {messages.length === 0 ? (
          <div className="flex min-h-full items-center justify-center px-4 py-16">
            <div className="flex max-w-sm flex-col items-center text-center">
              <div className="mb-4 flex size-14 items-center justify-center rounded-2xl bg-muted text-muted-foreground">
                <MessageSquare className="size-7" />
              </div>
              <h2 className="text-lg font-semibold text-foreground">No messages yet</h2>
              <p className="mt-1 text-sm leading-6 text-muted-foreground">Start the conversation in #{channel.name}.</p>
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
                className="duration-[120ms] animate-in fade-in slide-in-from-bottom-0.5 fill-mode-both motion-reduce:animate-none"
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
        <div className="pointer-events-none absolute inset-x-0 bottom-3 flex justify-center">
          <div className="pointer-events-auto flex items-center gap-3 rounded-xl border border-border bg-popover/95 px-3 py-2 shadow-xl backdrop-blur-sm">
            <span className="text-xs font-semibold text-foreground">You're Viewing Older Messages</span>
            <Button type="button" className="h-auto rounded-lg px-3 py-1.5 text-xs font-bold transition hover:bg-primary hover:brightness-110" onClick={jumpToPresent}>
              Jump To Present
            </Button>
          </div>
        </div>
      ) : null}
    </div>
  )
}

function DateSeparator({ iso }: { iso: string }) {
  return (
    <div className="relative my-4 flex items-center justify-center before:absolute before:inset-x-0 before:top-1/2 before:h-px before:-translate-y-1/2 before:bg-border before:content-[''] max-[899px]:my-3">
      <span className="relative rounded-full bg-background px-3 text-xs font-semibold text-muted-foreground max-[899px]:text-[10px]">{formatMessageDate(iso)}</span>
    </div>
  )
}

/** the chat reference PinnedNotice: "X pinned a message to this channel." with a right-click "Delete pin notice". */
function PinnedNotice({ state, message }: { state: AppState; message: ChatMessage }) {
  const [confirmOpen, setConfirmOpen] = useState(false)
  const actor = state.users.find((u) => u.id === message.pinnedBy)
  const actorName = actor?.name ?? 'Someone'

  return (
    <>
      <ContextMenu>
        <ContextMenuTrigger className="my-1 flex items-center gap-4 px-0.5 py-2 text-sm leading-5 text-muted-foreground select-auto max-[899px]:my-0.5 max-[899px]:gap-2 max-[899px]:py-1 max-[899px]:text-[11px] max-[899px]:leading-[15px]">
          <div className="flex w-10 justify-center [&>svg]:-rotate-[35deg] max-[899px]:w-[30px] max-[899px]:[&>svg]:size-3.5">
            <PinIcon size={20} />
          </div>
          <div className="min-w-0 flex-1 truncate [&>strong]:text-foreground">
            <strong style={actor ? { color: actor.color } : undefined}>{actorName}</strong> pinned <strong>a message</strong> to this channel.
          </div>
        </ContextMenuTrigger>
        <ContextMenuContent className="min-w-44 p-1.5">
          <ContextMenuItem className={menuItemClass} onClick={() => setConfirmOpen(true)}>
            <Trash2 size={16} />
            Delete pin notice
          </ContextMenuItem>
        </ContextMenuContent>
      </ContextMenu>
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
