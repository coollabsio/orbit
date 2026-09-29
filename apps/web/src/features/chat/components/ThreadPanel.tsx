// Port of the chat reference ThreadPanel: resizable side pane (468px default, 320-720) with an
// inline-renamable title, the root message, a separator, grouped replies, and a composer.
// With `fullScreen` it fills the chat area instead (route /chat/:channelId/thread/:rootId).
import { useEffect, useMemo, useRef, useState } from 'react'
import { Edit as Pencil, Xmark as X } from 'reicon-react'
import { Input } from '@/components/ui/input'
import { Separator } from '@/components/ui/separator'
import { ExpandIcon } from '@/components/common/icons/ExpandIcon'
import { FollowIcon } from '@/components/common/icons/FollowIcon'
import { useNavigate, useSearchParams } from 'react-router'
import { followThread, renameThread } from '@/mock/actions'
import type { AppState, Channel, ChatMessage } from '@/mock/types'
import { jumpToMessage, messageMentionsCurrentUser } from '@/features/chat/chatLib'
import { buildMentionTokens } from '@/lib/mentions'
import { threadTitleOf } from '@/lib/messagePreview'
import { ChatIconButton } from './ChatIconButton'
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
  const mentionTokens = useMemo(() => buildMentionTokens(state.users, state.channels), [state.users, state.channels])
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
    <div
      className="relative flex h-full shrink-0 flex-col border-l border-border bg-background data-[fullscreen=true]:min-w-0 data-[fullscreen=true]:flex-1 data-[fullscreen=true]:border-l-0"
      data-fullscreen={fullScreen ? 'true' : undefined}
      style={isMobile || fullScreen ? undefined : { width }}
    >
      {!isMobile && !fullScreen ? <div className="absolute inset-y-0 left-0 z-10 w-1 cursor-col-resize transition-colors hover:bg-primary/40" onPointerDown={handleResizeStart} title="Resize thread panel" /> : null}
      <div className="flex h-[47px] shrink-0 items-center justify-between border-b border-border px-4">
        <div className="flex min-w-0 flex-1 items-center gap-2 text-muted-foreground">
          {editingTitle ? (
            <Input
              autoFocus
              className="w-auto min-w-0 flex-1 font-semibold"
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
            <h2 className="min-w-0 truncate text-[15px] font-semibold text-foreground">{threadTitle}</h2>
          )}
        </div>
        <div className="ml-2 flex shrink-0 items-center gap-1">
          <ChatIconButton
            size="sm"
            active={root.threadFollowed}
            className="data-[active=true]:bg-primary/10"
            title={root.threadFollowed ? 'Unfollow thread' : 'Follow thread'}
            aria-label={root.threadFollowed ? 'Unfollow thread' : 'Follow thread'}
            onClick={() => followThread(root.id, !root.threadFollowed)}
          >
            <FollowIcon size={16} />
          </ChatIconButton>
          {!fullScreen ? (
            <ChatIconButton
              size="sm"
              title="Open full screen"
              aria-label="Open full screen"
              onClick={() => navigate(`/chat/${channel.id}/thread/${root.id}`)}
            >
              <ExpandIcon size={16} />
            </ChatIconButton>
          ) : null}
          <ChatIconButton size="sm" title="Edit thread name" aria-label="Edit thread name" onClick={startEditing}>
            <Pencil size={16} />
          </ChatIconButton>
          <ChatIconButton size="sm" title="Close thread" aria-label="Close thread" onClick={onClose}>
            <X size={16} />
          </ChatIconButton>
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-auto overscroll-contain px-4 py-2">
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
        {shouldRenderRoot && replies.length > 0 ? <Separator className="my-2" /> : null}
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
