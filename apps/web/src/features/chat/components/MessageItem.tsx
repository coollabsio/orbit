// Port of the chat reference MessageItem (the chat reference frontend/src/components/chat/MessageItem.tsx)
import { useCallback, useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { Copy, Edit, EmojiHappy, Reply, Trash } from 'reicon-react'
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

interface MessageItemProps {
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
    <div className="fc-msg-body">
      {!compact ? (
        <div className="fc-msg-header">
          <span className="fc-msg-author" style={nameColor ? { color: nameColor } : undefined}>
            {name}
          </span>
          {isExternal ? (
            <span className="fc-source-badge" data-source={message.authorType}>
              {message.authorType === 'discord' ? 'DISCORD' : 'GITHUB'}
            </span>
          ) : null}
          {isWebhook ? (
            <span className="fc-source-badge" data-source="webhook">
              Webhook
            </span>
          ) : null}
          <span className="fc-msg-time">
            {timeStr}
            {message.editedAt ? ' (edited)' : ''}
          </span>
          {message.pinned ? (
            <span className="fc-pinned-tag" title="Pinned message">
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
        <div className="fc-msg-text" data-recessed="true">
          {message.content}
        </div>
      ) : (
        <>
          <MessageContent content={message.content} mentionTokens={mentionTokens} />
          {compact && message.editedAt ? <span className="fc-msg-edited">(edited)</span> : null}
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
      <div id={`message-${message.id}`} className="fc-msg fc-thread-starter" data-full="true">
        <div className="fc-msg-row">
          <div className="fc-msg-gutter">
            <span className="fc-thread-starter-icon">
              <ThreadIcon size={20} />
            </span>
            <span className="fc-thread-starter-elbow" />
          </div>
          <div className="fc-msg-body">
            <div className="fc-thread-starter-text">
              <span className="fc-msg-author" style={nameColor ? { color: nameColor } : undefined}>
                {name}
              </span>{' '}
              started a thread: <strong>{threadTitleOf(message)}</strong>.{' '}
              <span className="fc-msg-time">{timeStr}</span>
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
        className="fc-msg"
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
            <span className="fc-highlight-bg" />
            <span className="fc-highlight-bar" />
          </>
        ) : null}
        {replyTarget ? <ReplyReference state={state} reply={replyTarget} /> : null}
        <div className="fc-msg-row">
          {compact ? (
            <div className="fc-msg-hover-time">{timeStr}</div>
          ) : (
            <div className="fc-msg-gutter">
              {isWebhook ? (
                <div className="fc-avatar" data-webhook="true">
                  {message.webhookIconUrl ? <img src={message.webhookIconUrl} alt="" /> : <WebhookIcon size={20} />}
                </div>
              ) : (
                <div className="fc-avatar" style={author ? { background: `color-mix(in srgb, ${author.color} 22%, transparent)`, color: author.color } : undefined}>
                  {name.charAt(0).toUpperCase()}
                </div>
              )}
            </div>
          )}
          {body}
        </div>

        {hovered && !editing ? (
          <div className="fc-toolbar">
            {TOOLBAR_EMOJIS.map((emoji) => (
              <button
                key={emoji}
                className="fc-toolbar-emoji"
                title={`React with ${emoji}`}
                onClick={() => toggleReaction(message.id, emoji)}
              >
                {emoji}
              </button>
            ))}
            <div className="fc-toolbar-sep" />
            <button title="Copy message" onClick={handleCopyText}>
              <Copy />
            </button>
            {isAuthor ? (
              <button title="Edit message" onClick={startEditing}>
                <Edit />
              </button>
            ) : null}
            <button title="Reply" onClick={handleReply}>
              <Reply />
            </button>
            {onOpenThread ? (
              <button title={hasThread ? 'Open Thread' : 'Create Thread'} onClick={openThread}>
                <ThreadIcon size={16} />
              </button>
            ) : null}
            <div className="fc-toolbar-sep" />
            <button title={message.pinned ? 'Unpin message' : 'Pin message'} onClick={() => togglePinMessage(message.id)}>
              <PinIcon />
            </button>
            {canDelete ? (
              <>
                <div className="fc-toolbar-sep" />
                <button data-danger="true" title="Delete message" onClick={requestDelete}>
                  <Trash />
                </button>
              </>
            ) : null}
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
    <div className="fc-thread-preview-wrap" data-starter={starterCard || undefined}>
      {!compact && !starterCard ? <span className="fc-thread-preview-elbow" /> : null}
      <button type="button" className="fc-thread-preview" onClick={onOpen}>
        {!starterCard ? (
          <span className="fc-thread-preview-icon">
            <ThreadIcon size={14} />
          </span>
        ) : null}
        <span className="fc-thread-preview-body">
          <span className="fc-thread-preview-head">
            <span className="fc-thread-preview-title">{title}</span>
            <span className="fc-thread-preview-count">
              {starterCard ? 'See Thread ›' : `${totalMessages} ${totalMessages === 1 ? 'Message' : 'Messages'} ›`}
            </span>
          </span>
          {starterCard && !lastReply ? (
            <span className="fc-thread-preview-line">There are no recent messages in this thread.</span>
          ) : (
            <span className="fc-thread-preview-line">
              <span className="fc-thread-preview-author" style={previewAuthorColor ? { color: previewAuthorColor } : undefined}>
                {displayName(state, previewMessage)}
              </span>
              {previewText ? <span className="fc-thread-preview-text">{previewText}</span> : null}
              <span className="fc-thread-preview-time">{relativeTime(previewMessage.createdAt)}</span>
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
    <button type="button" className="fc-reply-ref" onClick={() => jumpToMessage(reply.id)}>
      <span className="fc-reply-elbow" />
      <span className="fc-reply-avatar">{name.charAt(0).toUpperCase()}</span>
      <span className="fc-reply-name">@{name}</span>
      <span className="fc-reply-preview">{preview}</span>
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
        className="fc-edit-area"
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
      <div className="fc-edit-hint">
        escape to <b>cancel</b> · enter to <b>save</b>
      </div>
    </div>
  )
}

function Reactions({ message, currentUserId }: { message: ChatMessage; currentUserId: string }) {
  if (message.reactions.length === 0) return null
  return (
    <div className="fc-reactions">
      {message.reactions.map((r) => (
        <button
          key={r.emoji}
          className="fc-reaction"
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

  useEffect(() => {
    const menu = menuRef.current
    if (!menu) return
    const rect = menu.getBoundingClientRect()
    setAdjusted({
      x: Math.min(Math.max(position.x, 8), window.innerWidth - rect.width - 8),
      y: Math.min(Math.max(position.y, 8), window.innerHeight - rect.height - 8),
    })
  }, [position])

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

  const [showEmojis, setShowEmojis] = useState(false)

  // portal: message rows keep a transform from their enter animation, which would make
  // position:fixed resolve against the row instead of the viewport
  return createPortal(
    <div ref={menuRef} className="fc-ctx" style={{ left: adjusted.x, top: adjusted.y }}>
      {showEmojis ? (
        <div className="fc-ctx-emojis">
          {TOOLBAR_EMOJIS.map((emoji) => (
            <button key={emoji} type="button" onClick={() => onReaction(emoji)}>
              {emoji}
            </button>
          ))}
        </div>
      ) : null}
      <div className="fc-ctx-items">
        <button type="button" className="fc-ctx-item" onClick={() => setShowEmojis((prev) => !prev)}>
          <span className="fc-ctx-icon">
            <EmojiHappy size={16} />
          </span>
          <span>Add Reaction</span>
        </button>
        <button type="button" className="fc-ctx-item" onClick={onReply}>
          <span className="fc-ctx-icon">
            <Reply size={16} />
          </span>
          <span>Reply</span>
        </button>
        {onThread ? (
          <button type="button" className="fc-ctx-item" onClick={onThread}>
            <span className="fc-ctx-icon">
              <ThreadIcon size={16} />
            </span>
            <span>{threadLabel}</span>
          </button>
        ) : null}
        <div className="fc-ctx-separator" />
        {isAuthor ? (
          <button type="button" className="fc-ctx-item" onClick={onEdit}>
            <span className="fc-ctx-icon">
              <Edit size={16} />
            </span>
            <span>Edit Message</span>
          </button>
        ) : null}
        <button type="button" className="fc-ctx-item" onClick={onPin}>
          <span className="fc-ctx-icon">
            <PinIcon size={16} />
          </span>
          <span>{message.pinned ? 'Unpin Message' : 'Pin Message'}</span>
        </button>
        <button type="button" className="fc-ctx-item" onClick={onCopyText}>
          <span className="fc-ctx-icon">
            <Copy size={16} />
          </span>
          <span>Copy Text</span>
        </button>
        {canDelete ? (
          <>
            <div className="fc-ctx-separator" />
            <button type="button" className="fc-ctx-item" data-danger="true" onClick={onDelete}>
              <span className="fc-ctx-icon">
                <Trash size={16} />
              </span>
              <span>Delete Message</span>
            </button>
          </>
        ) : null}
      </div>
    </div>,
    document.body,
  )
}
