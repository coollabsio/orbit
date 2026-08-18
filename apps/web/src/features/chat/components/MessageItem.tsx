// Port of the chat reference MessageItem (the chat reference frontend/src/components/chat/MessageItem.tsx)
import { useCallback, useEffect, useRef, useState } from 'react'
import { Copy, Edit, Pin, Reply, Trash } from 'reicon-react'
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
  displayName,
  formatShortTime,
  jumpToMessage,
} from '../chatLib'
import { MessageContent } from './MessageContent'
import { ConfirmDeleteModal } from './ChannelModals'

const TOOLBAR_EMOJIS = ['👍', '👀', '😂']

interface MessageItemProps {
  state: AppState
  message: ChatMessage
  compact: boolean
  mentionedCurrentUser: boolean
  mentionTokens: MentionToken[]
  onReply: (message: ChatMessage) => void
}

export function MessageItem({ state, message, compact, mentionedCurrentUser, mentionTokens, onReply }: MessageItemProps) {
  const [hovered, setHovered] = useState(false)
  const [editing, setEditing] = useState(false)
  const [editText, setEditText] = useState('')
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number } | null>(null)
  const [deleteConfirmOpen, setDeleteConfirmOpen] = useState(false)

  const author = authorUser(state, message)
  const isExternal = message.authorType === 'discord' || message.authorType === 'github'
  const isAuthor = message.authorType === 'user' && message.authorId === state.currentUserId
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

  const body = (
    <div className="fc-msg-body">
      {!compact ? (
        <div className="fc-msg-header">
          <span className="fc-msg-author" style={author ? { color: author.color } : undefined}>
            {name}
          </span>
          {isExternal ? (
            <span className="fc-source-badge" data-source={message.authorType}>
              {message.authorType === 'discord' ? 'DISCORD' : 'GITHUB'}
            </span>
          ) : null}
          <span className="fc-msg-time">
            {timeStr}
            {message.editedAt ? ' (edited)' : ''}
          </span>
          {message.pinned ? (
            <span className="fc-pinned-tag" title="Pinned message">
              <Pin size={12} />
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
      <Reactions message={message} currentUserId={state.currentUserId} />
    </div>
  )

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
              <div className="fc-avatar" style={author ? { background: `color-mix(in srgb, ${author.color} 22%, transparent)`, color: author.color } : undefined}>
                {name.charAt(0).toUpperCase()}
              </div>
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
            <div className="fc-toolbar-sep" />
            <button title={message.pinned ? 'Unpin message' : 'Pin message'} onClick={() => togglePinMessage(message.id)}>
              <Pin />
            </button>
            {isAuthor ? (
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
          onClose={() => setContextMenu(null)}
          onReaction={(emoji) => {
            toggleReaction(message.id, emoji)
            setContextMenu(null)
          }}
          onCopyText={handleCopyText}
          onEdit={startEditing}
          onReply={handleReply}
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
          {r.emoji} {r.userIds.length}
        </button>
      ))}
    </div>
  )
}

function MessageContextMenu({
  position,
  message,
  isAuthor,
  onClose,
  onReaction,
  onCopyText,
  onEdit,
  onReply,
  onPin,
  onDelete,
}: {
  position: { x: number; y: number }
  message: ChatMessage
  isAuthor: boolean
  onClose: () => void
  onReaction: (emoji: string) => void
  onCopyText: () => void
  onEdit: () => void
  onReply: () => void
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

  return (
    <div ref={menuRef} className="fc-menu fc-context-menu" style={{ left: adjusted.x, top: adjusted.y }}>
      <div style={{ display: 'flex', gap: 2, padding: '2px 4px 6px' }}>
        {TOOLBAR_EMOJIS.map((emoji) => (
          <button
            key={emoji}
            style={{ borderRadius: 6, padding: 6, fontSize: 18, lineHeight: '20px' }}
            onClick={() => onReaction(emoji)}
          >
            {emoji}
          </button>
        ))}
      </div>
      <div className="fc-menu-separator" />
      <button className="fc-menu-item" onClick={onReply}>
        <Reply size={16} />
        Reply
      </button>
      <button className="fc-menu-item" onClick={onCopyText}>
        <Copy size={16} />
        Copy Text
      </button>
      {isAuthor ? (
        <button className="fc-menu-item" onClick={onEdit}>
          <Edit size={16} />
          Edit Message
        </button>
      ) : null}
      <button className="fc-menu-item" onClick={onPin}>
        <Pin size={16} />
        {message.pinned ? 'Unpin Message' : 'Pin Message'}
      </button>
      {isAuthor ? (
        <>
          <div className="fc-menu-separator" />
          <button className="fc-menu-item" data-danger="true" onClick={onDelete}>
            <Trash size={16} />
            Delete Message
          </button>
        </>
      ) : null}
    </div>
  )
}
