// Port of the chat reference ThreadPanel: resizable side pane (468px default, 320-720) with an
// inline-renamable title, the root message, a separator, grouped replies, and a composer.
// With `fullScreen` it fills the chat area instead (route /chat/:channelId/thread/:rootId).
import { useEffect, useMemo, useRef, useState } from 'react'
import { Edit, Xmark } from 'reicon-react'
import { ExpandIcon } from '../../../components/ui/icons/ExpandIcon'
import { FollowIcon } from '../../../components/ui/icons/FollowIcon'
import { useNavigate, useSearchParams } from 'react-router'
import { followThread, renameThread } from '../../../mock/actions'
import type { AppState, Channel, ChatMessage } from '../../../mock/types'
import { buildMentionTokens, jumpToMessage, messageMentionsCurrentUser, threadTitleOf } from '../chatLib'
import { MessageInput } from './MessageInput'
import { MessageItem } from './MessageItem'

const DEFAULT_WIDTH = 468
const GROUP_WINDOW_MS = 300_000

export function ThreadPanel({
  state,
  channel,
  root,
  onClose,
  isMobile,
  fullScreen = false,
}: {
  state: AppState
  channel: Channel
  root: ChatMessage
  onClose: () => void
  isMobile?: boolean
  fullScreen?: boolean
}) {
  const navigate = useNavigate()
  const [width, setWidth] = useState(DEFAULT_WIDTH)
  const [editingTitle, setEditingTitle] = useState(false)
  const [titleDraft, setTitleDraft] = useState('')
  const bottomRef = useRef<HTMLDivElement>(null)
  const me = state.users.find((u) => u.id === state.currentUserId)
  const mentionTokens = useMemo(() => buildMentionTokens(state.users), [state.users])
  const replies = useMemo(
    () =>
      state.chatMessages
        .filter((m) => m.threadRootId === root.id)
        .sort((a, b) => a.createdAt.localeCompare(b.createdAt)),
    [state.chatMessages, root.id],
  )
  const threadTitle = threadTitleOf(root)
  // the chat reference: a dedicated thread-starter root is not re-rendered inside the panel
  const shouldRenderRoot = !root.startsThread || Boolean(root.threadRootId)

  const [searchParams] = useSearchParams()
  const jumpTargetId = searchParams.get('message_id')

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'auto', block: 'end' })
    if (jumpTargetId) requestAnimationFrame(() => jumpToMessage(jumpTargetId))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [root.id])

  useEffect(() => {
    const last = replies[replies.length - 1]
    if (last && last.authorId === state.currentUserId && Date.now() - new Date(last.createdAt).getTime() < 2000) {
      bottomRef.current?.scrollIntoView({ behavior: 'smooth', block: 'end' })
    }
  }, [replies, state.currentUserId])

  function handleResizeStart(event: React.PointerEvent<HTMLDivElement>) {
    event.preventDefault()
    function onMove(moveEvent: PointerEvent) {
      setWidth(Math.min(720, Math.max(320, window.innerWidth - moveEvent.clientX)))
    }
    function onUp() {
      window.removeEventListener('pointermove', onMove)
      window.removeEventListener('pointerup', onUp)
      document.body.style.cursor = ''
      document.body.style.userSelect = ''
    }
    document.body.style.cursor = 'col-resize'
    document.body.style.userSelect = 'none'
    window.addEventListener('pointermove', onMove)
    window.addEventListener('pointerup', onUp)
  }

  function startEditing() {
    setTitleDraft(threadTitle)
    setEditingTitle(true)
  }

  function saveTitle() {
    const next = titleDraft.trim()
    if (next && next !== threadTitle) renameThread(root.id, next)
    setEditingTitle(false)
  }

  return (
    <div className="fc-thread-panel" data-fullscreen={fullScreen ? 'true' : undefined} style={isMobile || fullScreen ? undefined : { width }}>
      {!isMobile && !fullScreen ? <div className="fc-thread-resize" onPointerDown={handleResizeStart} title="Resize thread panel" /> : null}
      <div className="fc-thread-header">
        <div className="fc-thread-header-title">
          {editingTitle ? (
            <input
              autoFocus
              className="fc-thread-title-input"
              value={titleDraft}
              aria-label="Thread name"
              onChange={(e) => setTitleDraft(e.target.value)}
              onBlur={saveTitle}
              onKeyDown={(e) => {
                if (e.key === 'Escape') {
                  setTitleDraft(threadTitle)
                  setEditingTitle(false)
                } else if (e.key === 'Enter') {
                  e.preventDefault()
                  saveTitle()
                }
              }}
            />
          ) : (
            <h2 className="truncate">{threadTitle}</h2>
          )}
        </div>
        <div className="fc-thread-header-actions">
          <button
            type="button"
            className="fc-thread-icon-button"
            data-active={root.threadFollowed ? 'true' : undefined}
            title={root.threadFollowed ? 'Unfollow thread' : 'Follow thread'}
            aria-label={root.threadFollowed ? 'Unfollow thread' : 'Follow thread'}
            onClick={() => followThread(root.id, !root.threadFollowed)}
          >
            <FollowIcon size={16} />
          </button>
          {!fullScreen ? (
            <button
              type="button"
              className="fc-thread-icon-button"
              title="Open full screen"
              aria-label="Open full screen"
              onClick={() => navigate(`/chat/${channel.id}/thread/${root.id}`)}
            >
              <ExpandIcon size={16} />
            </button>
          ) : null}
          <button type="button" className="fc-thread-icon-button" title="Edit thread name" aria-label="Edit thread name" onClick={startEditing}>
            <Edit size={16} />
          </button>
          <button type="button" className="fc-thread-icon-button" title="Close thread" aria-label="Close thread" onClick={onClose}>
            <Xmark size={16} />
          </button>
        </div>
      </div>

      <div className="fc-thread-body">
        {shouldRenderRoot ? (
          <MessageItem
            state={state}
            message={root}
            compact={false}
            mentionedCurrentUser={messageMentionsCurrentUser(root, me)}
            mentionTokens={mentionTokens}
            onReply={() => {}}
            hideThreadPreview
          />
        ) : null}
        {shouldRenderRoot && replies.length > 0 ? <div className="fc-thread-separator" /> : null}
        {replies.map((reply, index) => {
          const previous = index > 0 ? replies[index - 1] : null
          const compact =
            !!previous &&
            previous.authorId === reply.authorId &&
            previous.authorType === reply.authorType &&
            new Date(reply.createdAt).getTime() - new Date(previous.createdAt).getTime() < GROUP_WINDOW_MS
          return (
            <MessageItem
              key={reply.id}
              state={state}
              message={reply}
              compact={compact}
              mentionedCurrentUser={messageMentionsCurrentUser(reply, me)}
              mentionTokens={mentionTokens}
              onReply={() => {}}
              hideThreadPreview
            />
          )
        })}
        <div ref={bottomRef} />
      </div>

      <MessageInput state={state} channel={channel} threadRootId={root.id} placeholder="Send a message" showThreadAction={false} autoFocus />
    </div>
  )
}
