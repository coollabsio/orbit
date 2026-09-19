// Port of the chat reference MessageItem (the chat reference frontend/src/components/chat/MessageItem.tsx)
import { useCallback, useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { Copy, MoreHorizontal, Pencil, Reply, SmilePlus, Trash2 } from 'lucide-react'
import { EmojiPicker } from '../../../components/ui/EmojiPicker'
import { PinIcon } from '../../../components/ui/icons/PinIcon'
import { ThreadIcon } from '../../../components/ui/icons/ThreadIcon'
import {
  deleteChatMessage,
  editChatMessage,
  togglePinMessage,
  toggleReaction,
} from '../../../mock/actions'
import type { AppState, ChatMessage } from '../../../mock/types'
import {
  type MentionToken,
  authorUser,
  authorColor,
  displayName,
  extractPreview,
  formatShortTime,
  jumpToMessage,
  threadTitleOf,
} from '../chatLib'
import { MessageContent } from './MessageContent'
import { EmbedCards } from './Embeds'
import { Attachments } from './Attachments'
import { WebhookIcon } from '../../../components/ui/WebhookIcon'
import { relativeTime } from '../../../lib/format'
import { Emoji } from '../../../components/ui/Emoji'
import { ConfirmDeleteModal } from './ChannelModals'

const TOOLBAR_EMOJIS = ['👍', '👀', '😂']

const authorNameClass = 'text-sm font-semibold text-foreground max-[899px]:text-xs'
const timeClass = 'text-[11px] font-semibold text-muted-foreground max-[899px]:text-[10px]'
const sourceBadgeClass =
  'inline-flex h-4 items-center rounded-[3px] px-1 text-[10px] leading-4 font-bold text-white data-[source=discord]:bg-[#5865f2] data-[source=github]:bg-[#3f3f46] data-[source=webhook]:bg-[#047857] data-[source=webhook]:normal-case max-[899px]:data-[source=webhook]:h-3.5 max-[899px]:data-[source=webhook]:px-[3px] max-[899px]:data-[source=webhook]:text-[9px] max-[899px]:data-[source=webhook]:leading-[14px]'
const ctxItemClass =
  'group/item flex w-full items-center gap-2.5 rounded-lg px-2.5 py-1.5 text-sm font-medium text-foreground transition-colors hover:bg-muted data-[danger=true]:text-destructive data-[danger=true]:hover:bg-destructive/10'
const ctxIconClass = 'inline-flex size-4 shrink-0 items-center justify-center text-muted-foreground group-data-[danger=true]/item:text-destructive'

export interface MessageItemProps {
  state: AppState
  message: ChatMessage
  compact: boolean
  mentionedCurrentUser: boolean
  mentionTokens: MentionToken[]
  onReply: (message: ChatMessage) => void
  /** Open (or create) the thread rooted at this message. Absent inside the thread panel. */
  onOpenThread?: (message: ChatMessage) => void
  /** the chat reference hideThreadPreview: the thread panel renders the root without its preview card. */
  hideThreadPreview?: boolean
}

export function MessageItem({
  state,
  message,
  compact,
  mentionedCurrentUser,
  mentionTokens,
  onReply,
  onOpenThread,
  hideThreadPreview,
}: MessageItemProps) {
  const [hovered, setHovered] = useState(false)
  const [editing, setEditing] = useState(false)
  const [editText, setEditText] = useState('')
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number } | null>(null)
  const [deleteConfirmOpen, setDeleteConfirmOpen] = useState(false)

  const author = authorUser(state, message)
  const nameColor = authorColor(state, message)
  const isExternal = message.authorType === 'discord' || message.authorType === 'github'
  const isWebhook = message.authorType === 'webhook'
  const isAuthor = message.authorType === 'user' && message.authorId === state.currentUserId
  // the chat reference: canDelete = isAuthor || Boolean(message.webhook_name)
  const canDelete = isAuthor || isWebhook
  const name = displayName(state, message)
  const timeStr = formatShortTime(message.createdAt)

  const startEditing = useCallback(() => {
    setEditText(message.content)
    setEditing(true)
    setContextMenu(null)
  }, [message.content])

  const handleCopyText = useCallback(() => {
    void navigator.clipboard.writeText(message.content)
    setContextMenu(null)
  }, [message.content])

  const handleReply = useCallback(() => {
    onReply(message)
    setContextMenu(null)
  }, [message, onReply])

  const requestDelete = useCallback(() => {
    setContextMenu(null)
    setDeleteConfirmOpen(true)
  }, [])

  const commitEdit = useCallback(() => {
    const trimmed = editText.trim()
    if (trimmed && trimmed !== message.content) editChatMessage(message.id, trimmed)
    setEditing(false)
  }, [editText, message.content, message.id])

  const replyTarget = message.replyToId
    ? state.chatMessages.find((m) => m.id === message.replyToId)
    : null
  const threadReplies = state.chatMessages.filter((m) => m.threadRootId === message.id)
  const lastReply = threadReplies.length > 0 ? threadReplies[threadReplies.length - 1] : null
  const hasThread = threadReplies.length > 0 || message.startsThread
  const isThreadStarter = message.startsThread && !message.threadRootId
  const showThreadPreview = !hideThreadPreview && !!onOpenThread && threadReplies.length > 0
  const openThread = () => {
    onOpenThread?.(message)
    setContextMenu(null)
  }

  const body = (
    <div className="min-w-0 flex-1">
      {!compact ? (
        <div className="flex items-baseline gap-2 max-[899px]:gap-1.5">
          <span className={authorNameClass} style={nameColor ? { color: nameColor } : undefined}>
            {name}
          </span>
          {isExternal ? (
            <span className={sourceBadgeClass} data-source={message.authorType}>
              {message.authorType === 'discord' ? 'DISCORD' : 'GITHUB'}
            </span>
          ) : null}
          {isWebhook ? (
            <span className={sourceBadgeClass} data-source="webhook">
              Webhook
            </span>
          ) : null}
          <span className={timeClass}>
            {timeStr}
            {message.editedAt ? ' (edited)' : ''}
          </span>
          {message.pinned ? (
            <span className="flex items-center gap-1 text-xs text-amber-500" title="Pinned message">
              <PinIcon size={12} />
              <span>Pinned</span>
            </span>
          ) : null}
        </div>
      ) : null}
      {editing ? (
        <EditingTextarea
          value={editText}
          onChange={setEditText}
          onCommit={commitEdit}
          onCancel={() => setEditing(false)}
        />
      ) : message.authorType === 'github' ? (
        <div className="mt-1 w-fit max-w-full rounded-md border border-border bg-muted px-3 py-2 font-mono text-[13px] whitespace-pre-wrap text-foreground/90 [overflow-wrap:anywhere]">
          {message.content}
        </div>
      ) : (
        <>
          <MessageContent content={message.content} mentionTokens={mentionTokens} />
          {compact && message.editedAt ? <span className="ml-1 text-xs text-muted-foreground">(edited)</span> : null}
        </>
      )}
      {message.attachments && message.attachments.length > 0 ? (
        <Attachments attachments={message.attachments} hasTextContent={!!message.content.trim()} />
      ) : null}
      {message.embeds && message.embeds.length > 0 ? (
        <EmbedCards embeds={message.embeds} hasTextContent={!!message.content.trim()} mentionTokens={mentionTokens} />
      ) : null}
      <Reactions message={message} currentUserId={state.currentUserId} />
      {showThreadPreview ? (
        <ThreadPreview
          state={state}
          message={message}
          lastReply={lastReply}
          compact={compact}
          replyCount={threadReplies.length}
          onOpen={openThread}
        />
      ) : null}
    </div>
  )

  if (isThreadStarter && onOpenThread) {
    return (
      <div id={`message-${message.id}`} className="group relative mt-2 flex flex-col rounded-lg px-0.5 py-1 max-[899px]:mt-[5px] max-[899px]:py-0.5">
        <div className="relative flex gap-4 max-[899px]:gap-2">
          <div className="relative flex w-10 shrink-0 justify-center self-stretch max-[899px]:w-[30px]">
            <span className="relative z-10 flex size-10 items-center justify-center text-muted-foreground">
              <ThreadIcon size={20} />
            </span>
            <span className="pointer-events-none absolute top-7 left-1/2 h-9 w-7 -translate-x-1 rounded-bl-lg border-b-2 border-l-2 border-muted" />
          </div>
          <div className="min-w-0 flex-1">
            <div className="text-sm leading-5 text-muted-foreground [&>strong]:text-foreground">
              <span className={authorNameClass} style={nameColor ? { color: nameColor } : undefined}>
                {name}
              </span>{' '}
              started a thread: <strong>{threadTitleOf(message)}</strong>.{' '}
              <span className={timeClass}>{timeStr}</span>
            </div>
            <ThreadPreview
              state={state}
              message={message}
              lastReply={lastReply}
              compact
              starterCard
              replyCount={threadReplies.length}
              onOpen={openThread}
            />
          </div>
        </div>
      </div>
    )
  }

  return (
    <>
      <div
        id={`message-${message.id}`}
        className={`group relative flex flex-col rounded-lg px-0.5 py-1 max-[899px]:py-0.5 ${
          mentionedCurrentUser ? 'rounded-l-none hover:bg-transparent' : 'hover:bg-foreground/[0.02]'
        } ${!compact ? 'mt-2 max-[899px]:mt-[5px]' : ''}`}
        data-full={!compact ? 'true' : undefined}
        data-mention={mentionedCurrentUser ? 'true' : undefined}
        onMouseEnter={() => setHovered(true)}
        onMouseLeave={() => setHovered(false)}
        onContextMenu={(e) => {
          e.preventDefault()
          setContextMenu({ x: e.clientX, y: e.clientY })
        }}
      >
        {mentionedCurrentUser ? (
          <>
            <span className="pointer-events-none absolute inset-0 rounded-lg bg-primary/10" />
            <span className="pointer-events-none absolute top-1 bottom-1 left-0 z-0 w-0.5 rounded-r-full bg-primary" />
          </>
        ) : null}
        {replyTarget ? <ReplyReference state={state} reply={replyTarget} /> : null}
        <div className="relative flex gap-4 max-[899px]:gap-2">
          {compact ? (
            <div className="flex w-10 items-start justify-start pt-0.5 text-[10px] leading-none font-bold whitespace-nowrap text-muted-foreground opacity-0 group-hover:opacity-100 max-[899px]:w-[30px] max-[899px]:group-hover:opacity-0">{timeStr}</div>
          ) : (
            <div className="relative flex w-10 shrink-0 justify-center self-stretch max-[899px]:w-[30px]">
              {isWebhook ? (
                <div className="relative z-10 flex size-10 shrink-0 items-center justify-center overflow-hidden rounded-full bg-amber-500/15 text-sm font-semibold text-amber-400 max-[899px]:size-[30px] max-[899px]:text-[11px]">
                  {message.webhookIconUrl ? <img src={message.webhookIconUrl} alt="" className="size-full rounded-full object-cover" /> : <WebhookIcon size={20} />}
                </div>
              ) : (
                <div
                  className="relative z-10 flex size-10 shrink-0 items-center justify-center overflow-hidden rounded-full bg-muted text-sm font-semibold text-muted-foreground max-[899px]:size-[30px] max-[899px]:text-[11px]"
                  style={author ? { background: `color-mix(in srgb, ${author.color} 22%, transparent)`, color: author.color } : undefined}
                >
                  {name.charAt(0).toUpperCase()}
                </div>
              )}
            </div>
          )}
          {body}
        </div>

        {hovered && !editing ? (
          <div className="absolute -top-3 right-2 z-10 flex items-center rounded-lg border border-border bg-background shadow-sm max-[899px]:hidden">
            {TOOLBAR_EMOJIS.map((emoji) => (
              <button
                key={emoji}
                className="inline-flex items-center justify-center rounded-md p-1.5 text-lg leading-5 text-foreground transition-colors hover:bg-muted"
                title={`React with ${emoji}`}
                onClick={() => toggleReaction(message.id, emoji)}
              >
                {emoji}
              </button>
            ))}
            <button
              type="button"
              aria-label="More message actions"
              className="inline-flex items-center justify-center rounded-md p-1.5 text-foreground transition-colors hover:bg-muted"
              onClick={(event) => {
                const rect = event.currentTarget.getBoundingClientRect()
                setContextMenu({ x: rect.right, y: rect.bottom + 4 })
              }}
            >
              <MoreHorizontal className="size-4" />
            </button>
          </div>
        ) : null}
      </div>

      {contextMenu ? (
        <MessageContextMenu
          position={contextMenu}
          message={message}
          isAuthor={isAuthor}
          canDelete={canDelete}
          onClose={() => setContextMenu(null)}
          onReaction={(emoji) => {
            toggleReaction(message.id, emoji)
            setContextMenu(null)
          }}
          onCopyText={handleCopyText}
          onEdit={startEditing}
          onReply={handleReply}
          onThread={onOpenThread ? openThread : undefined}
          threadLabel={hasThread ? 'Open Thread' : 'Create Thread'}
          onPin={() => {
            togglePinMessage(message.id)
            setContextMenu(null)
          }}
          onDelete={requestDelete}
        />
      ) : null}

      {deleteConfirmOpen ? (
        <ConfirmDeleteModal
          title="Delete message?"
          description="This will remove the message from the conversation."
          onClose={() => setDeleteConfirmOpen(false)}
          onConfirm={() => {
            deleteChatMessage(message.id)
            setDeleteConfirmOpen(false)
          }}
        />
      ) : null}
    </>
  )
}

/** the chat reference ThreadPreview: reply-count card under the parent message. */
function ThreadPreview({
  state,
  message,
  lastReply,
  compact,
  starterCard,
  replyCount,
  onOpen,
}: {
  state: AppState
  message: ChatMessage
  lastReply: ChatMessage | null
  compact: boolean
  starterCard?: boolean
  replyCount: number
  onOpen: () => void
}) {
  const title = threadTitleOf(message)
  const previewMessage = lastReply ?? message
  const previewAuthorColor = authorColor(state, previewMessage)
  const previewText = extractPreview(previewMessage.content)
  const totalMessages = replyCount + 1
  return (
    <div className={`relative ${starterCard ? 'mt-1.5' : 'mt-2'}`} data-starter={starterCard || undefined}>
      {!compact && !starterCard ? <span className="pointer-events-none absolute -left-9 top-1 h-4 w-9 rounded-bl-lg border-b-2 border-l-2 border-muted" /> : null}
      <button type="button" className="flex w-fit max-w-[min(448px,100%)] items-start gap-2 rounded-md border border-border bg-muted/20 px-2.5 py-2 text-left transition-colors hover:bg-muted/45" onClick={onOpen}>
        {!starterCard ? (
          <span className="mt-0.5 flex size-5 shrink-0 items-center justify-center rounded-full bg-muted text-muted-foreground">
            <ThreadIcon size={14} />
          </span>
        ) : null}
        <span className="min-w-0 flex-1">
          <span className="flex min-w-0 items-center gap-2">
            <span className="truncate text-xs font-bold text-foreground">{title}</span>
            <span className="shrink-0 text-xs font-bold text-primary">
              {starterCard ? 'See Thread ›' : `${totalMessages} ${totalMessages === 1 ? 'Message' : 'Messages'} ›`}
            </span>
          </span>
          {starterCard && !lastReply ? (
            <span className="mt-1 flex min-w-0 items-center gap-1.5 text-xs text-muted-foreground">There are no recent messages in this thread.</span>
          ) : (
            <span className="mt-1 flex min-w-0 items-center gap-1.5 text-xs text-muted-foreground">
              <span className="shrink-0 font-semibold whitespace-nowrap text-foreground" style={previewAuthorColor ? { color: previewAuthorColor } : undefined}>
                {displayName(state, previewMessage)}
              </span>
              {previewText ? <span className="min-w-0 truncate font-medium text-foreground">{previewText}</span> : null}
              <span className="shrink-0">{relativeTime(previewMessage.createdAt)}</span>
            </span>
          )}
        </span>
      </button>
    </div>
  )
}

function ReplyReference({ state, reply }: { state: AppState; reply: ChatMessage }) {
  const name = displayName(state, reply)
  const preview = reply.content.trim() || 'No message content'
  return (
    <button type="button" className="relative mb-0.5 ml-5 flex h-6 min-w-0 max-w-[min(720px,calc(100%-20px))] cursor-pointer items-center gap-1.5 pl-9 text-left text-xs leading-5 text-muted-foreground transition-colors hover:text-foreground" onClick={() => jumpToMessage(reply.id)}>
      <span className="pointer-events-none absolute top-3 left-0 h-6 w-8 rounded-tl-md border-t-2 border-l-2 border-muted-foreground/40" />
      <span className="grid size-4 shrink-0 place-items-center rounded-full bg-muted text-[9px] font-bold text-muted-foreground">{name.charAt(0).toUpperCase()}</span>
      <span className="shrink-0 font-bold text-muted-foreground">@{name}</span>
      <span className="min-w-0 truncate font-semibold">{preview}</span>
    </button>
  )
}

function EditingTextarea({
  value,
  onChange,
  onCommit,
  onCancel,
}: {
  value: string
  onChange: (value: string) => void
  onCommit: () => void
  onCancel: () => void
}) {
  const ref = useRef<HTMLTextAreaElement>(null)

  useEffect(() => {
    const el = ref.current
    if (!el) return
    el.focus()
    el.setSelectionRange(el.value.length, el.value.length)
    el.style.height = 'auto'
    el.style.height = `${el.scrollHeight}px`
  }, [])

  return (
    <div>
      <textarea
        ref={ref}
        className="w-full resize-none rounded-lg border border-border bg-muted px-3 py-2 text-sm leading-5 text-foreground focus:border-primary focus:outline-none"
        value={value}
        rows={1}
        onChange={(e) => {
          onChange(e.target.value)
          e.target.style.height = 'auto'
          e.target.style.height = `${e.target.scrollHeight}px`
        }}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && !e.shiftKey) {
            e.preventDefault()
            onCommit()
          }
          if (e.key === 'Escape') onCancel()
        }}
      />
      <div className="mt-1 text-[11px] text-muted-foreground [&_b]:font-semibold [&_b]:text-foreground">
        escape to <b>cancel</b> · enter to <b>save</b>
      </div>
    </div>
  )
}

function Reactions({ message, currentUserId }: { message: ChatMessage; currentUserId: string }) {
  if (message.reactions.length === 0) return null
  return (
    <div className="mt-2 flex flex-wrap gap-1">
      {message.reactions.map((r) => (
        <button
          key={r.emoji}
          className="inline-flex w-max cursor-pointer items-center gap-1 rounded-full px-2.5 py-0.5 text-sm font-medium whitespace-nowrap ring-1 ring-inset ring-border transition-colors bg-muted text-foreground hover:bg-muted/80 data-[mine=true]:bg-primary/10 data-[mine=true]:ring-primary/40 data-[mine=true]:hover:bg-primary/20"
          data-mine={r.userIds.includes(currentUserId) ? 'true' : undefined}
          onClick={() => toggleReaction(message.id, r.emoji)}
        >
          <Emoji value={r.emoji} size={15} /> {r.userIds.length}
        </button>
      ))}
    </div>
  )
}

function MessageContextMenu({
  position,
  message,
  isAuthor,
  canDelete,
  onClose,
  onReaction,
  onCopyText,
  onEdit,
  onReply,
  onThread,
  threadLabel,
  onPin,
  onDelete,
}: {
  position: { x: number; y: number }
  message: ChatMessage
  isAuthor: boolean
  canDelete: boolean
  onClose: () => void
  onReaction: (emoji: string) => void
  onCopyText: () => void
  onEdit: () => void
  onReply: () => void
  onThread?: () => void
  threadLabel: string
  onPin: () => void
  onDelete: () => void
}) {
  const menuRef = useRef<HTMLDivElement>(null)
  const [adjusted, setAdjusted] = useState(position)
  const [showEmojis, setShowEmojis] = useState(false)

  useEffect(() => {
    const menu = menuRef.current
    if (!menu) return
    const rect = menu.getBoundingClientRect()
    setAdjusted({
      x: Math.min(Math.max(position.x, 8), window.innerWidth - rect.width - 8),
      y: Math.min(Math.max(position.y, 8), window.innerHeight - rect.height - 8),
    })
  }, [position, showEmojis])

  useEffect(() => {
    function onPointerDown(event: MouseEvent) {
      if (!menuRef.current?.contains(event.target as Node)) onClose()
    }
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === 'Escape') onClose()
    }
    document.addEventListener('mousedown', onPointerDown)
    document.addEventListener('keydown', onKeyDown)
    return () => {
      document.removeEventListener('mousedown', onPointerDown)
      document.removeEventListener('keydown', onKeyDown)
    }
  }, [onClose])

  // portal: message rows keep a transform from their enter animation, which would make
  // position:fixed resolve against the row instead of the viewport
  return createPortal(
    <div
      ref={menuRef}
      className="fixed z-[100] min-w-48 rounded-xl border border-border bg-popover text-foreground shadow-xl duration-100 animate-in fade-in zoom-in-95"
      data-picker={showEmojis || undefined}
      style={{ left: adjusted.x, top: adjusted.y }}
    >
      {showEmojis ? (
        <EmojiPicker onPick={onReaction} />
      ) : (
        <div className="px-1 py-1.5">
          <button type="button" className={ctxItemClass} onClick={() => setShowEmojis(true)}>
            <span className={ctxIconClass}>
              <SmilePlus size={16} />
            </span>
            <span>Add Reaction</span>
          </button>
          <button type="button" className={ctxItemClass} onClick={onReply}>
            <span className={ctxIconClass}>
              <Reply size={16} />
            </span>
            <span>Reply</span>
          </button>
          {onThread ? (
            <button type="button" className={ctxItemClass} onClick={onThread}>
              <span className={ctxIconClass}>
                <ThreadIcon size={16} />
              </span>
              <span>{threadLabel}</span>
            </button>
          ) : null}
          <div className="mx-2 my-1.5 h-px bg-border" />
          {isAuthor ? (
            <button type="button" className={ctxItemClass} onClick={onEdit}>
              <span className={ctxIconClass}>
                <Pencil size={16} />
              </span>
              <span>Edit Message</span>
            </button>
          ) : null}
          <button type="button" className={ctxItemClass} onClick={onPin}>
            <span className={ctxIconClass}>
              <PinIcon size={16} />
            </span>
            <span>{message.pinned ? 'Unpin Message' : 'Pin Message'}</span>
          </button>
          <button type="button" className={ctxItemClass} onClick={onCopyText}>
            <span className={ctxIconClass}>
              <Copy size={16} />
            </span>
            <span>Copy Text</span>
          </button>
          {canDelete ? (
            <>
              <div className="mx-2 my-1.5 h-px bg-border" />
              <button type="button" className={ctxItemClass} data-danger="true" onClick={onDelete}>
                <span className={ctxIconClass}>
                  <Trash2 size={16} />
                </span>
                <span>Delete Message</span>
              </button>
            </>
          ) : null}
        </div>
      )}
    </div>,
    document.body,
  )
}
