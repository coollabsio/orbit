// Port of the chat reference MessageItem (the chat reference frontend/src/components/chat/MessageItem.tsx)
import { useCallback, useEffect, useRef, useState, type ComponentProps, type ReactNode } from 'react'
import { cva } from 'class-variance-authority'
import { Copy, MoreH as MoreHorizontal, Edit as Pencil, Reply, SmileCircle as SmilePlus, Trash as Trash2 } from 'reicon-react'
import { EmojiPicker } from '@/components/common/EmojiPicker'
import { PinIcon } from '@/components/common/icons/PinIcon'
import { ThreadIcon } from '@/components/common/icons/ThreadIcon'
import {
  deleteChatMessage,
  editChatMessage,
  togglePinMessage,
  toggleReaction,
} from '@/mock/actions'
import type { AppState, ChatMessage } from '@/mock/types'
import {
  authorUser,
  authorColor,
  displayName,
  formatShortTime,
  jumpToMessage,
} from '@/features/chat/chatLib'
import type { MentionToken } from '@/lib/mentions'
import { extractPreview, threadTitleOf } from '@/lib/messagePreview'
import { MessageContent } from './MessageContent'
import { EmbedCards } from './Embeds'
import { Attachments } from '@/components/common/Attachments'
import { WebhookIcon } from '@/components/common/icons/WebhookIcon'
import { relativeTime } from '@/lib/format'
import { Emoji } from '@/components/common/Emoji'
import { ConfirmDeleteModal } from '@/components/common/ConfirmDeleteModal'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuSeparator } from '@/components/ui/dropdown-menu'
import { Popover, PopoverContent } from '@/components/ui/popover'
import { Textarea } from '@/components/ui/textarea'
import { cn } from 'cn'
import { ChatDropdownMenuItem } from './ChatMenuItem'
import { InitialAvatar } from './InitialAvatar'

const TOOLBAR_EMOJIS = ['👍', '👀', '😂']

type MenuAnchor = NonNullable<ComponentProps<typeof DropdownMenuContent>['anchor']>

/** Zero-size virtual element at the pointer, so the menu opens with its corner at the click point. */
function pointAnchor(x: number, y: number): MenuAnchor {
  return { getBoundingClientRect: () => DOMRect.fromRect({ x, y, width: 0, height: 0 }) }
}

function AuthorName({ color, children }: { color?: string; children: ReactNode }) {
  return (
    <span data-slot="message-author" className="text-sm font-semibold text-foreground max-[899px]:text-xs" style={color ? { color } : undefined}>
      {children}
    </span>
  )
}

function MessageTime({ children }: { children: ReactNode }) {
  return (
    <span data-slot="message-time" className="text-[11px] font-semibold text-muted-foreground max-[899px]:text-[10px]">
      {children}
    </span>
  )
}

const sourceBadgeVariants = cva('inline-flex h-4 items-center rounded-[3px] px-1 text-[10px] leading-4 font-bold text-white', {
  variants: {
    source: {
      discord: 'bg-[#5865f2]',
      github: 'bg-[#3f3f46]',
      webhook: 'bg-[#047857] max-[899px]:h-3.5 max-[899px]:px-[3px] max-[899px]:text-[9px] max-[899px]:leading-[14px]',
    },
  },
})

/** Brand label beside the author name for messages from Discord, GitHub or a webhook. */
function SourceBadge({ source }: { source: 'discord' | 'github' | 'webhook' }) {
  return (
    <span data-slot="source-badge" data-source={source} className={sourceBadgeVariants({ source })}>
      {source === 'discord' ? 'DISCORD' : source === 'github' ? 'GITHUB' : 'Webhook'}
    </span>
  )
}

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
  // the actions menu and the reaction picker share one anchor: the pointer (right-click) or the toolbar More button
  const [menuAnchor, setMenuAnchor] = useState<MenuAnchor | null>(null)
  const [menuOpen, setMenuOpen] = useState(false)
  const [pickerOpen, setPickerOpen] = useState(false)
  const moreButtonRef = useRef<HTMLButtonElement>(null)
  const anchoredToMore = menuAnchor === moreButtonRef
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
    setMenuOpen(false)
  }, [message.content])

  const handleCopyText = useCallback(() => {
    void navigator.clipboard.writeText(message.content)
    setMenuOpen(false)
  }, [message.content])

  const handleReply = useCallback(() => {
    onReply(message)
    setMenuOpen(false)
  }, [message, onReply])

  const requestDelete = useCallback(() => {
    setMenuOpen(false)
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
    setMenuOpen(false)
  }

  const body = (
    <div className="min-w-0 flex-1">
      {!compact ? (
        <div className="flex items-baseline gap-2 max-[899px]:gap-1.5">
          <AuthorName color={nameColor}>{name}</AuthorName>
          {isExternal || isWebhook ? <SourceBadge source={message.authorType as 'discord' | 'github' | 'webhook'} /> : null}
          <MessageTime>
            {timeStr}
            {message.editedAt ? ' (edited)' : ''}
          </MessageTime>
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
              <AuthorName color={nameColor}>{name}</AuthorName> started a thread: <strong>{threadTitleOf(message)}</strong>.{' '}
              <MessageTime>{timeStr}</MessageTime>
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
        className={cn(
          'group relative flex flex-col rounded-lg px-0.5 py-1 max-[899px]:py-0.5',
          mentionedCurrentUser ? 'rounded-l-none' : 'hover:bg-foreground/[0.02]',
          !compact && 'mt-2 max-[899px]:mt-[5px]',
        )}
        data-full={!compact ? 'true' : undefined}
        data-mention={mentionedCurrentUser ? 'true' : undefined}
        onMouseEnter={() => setHovered(true)}
        onMouseLeave={() => setHovered(false)}
        onContextMenu={(e) => {
          e.preventDefault()
          setMenuAnchor(pointAnchor(e.clientX, e.clientY))
          setPickerOpen(false)
          setMenuOpen(true)
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
                <InitialAvatar name={name} color={author?.color} className="relative z-10 size-10 text-sm max-[899px]:size-[30px] max-[899px]:text-[11px]" />
              )}
            </div>
          )}
          {body}
        </div>

        {/* stays mounted while its menu/picker is open: the More button is their anchor */}
        {(hovered || (anchoredToMore && (menuOpen || pickerOpen))) && !editing ? (
          <div className="absolute -top-3 right-2 z-10 flex items-center rounded-lg border border-border bg-background shadow-sm max-[899px]:hidden">
            {TOOLBAR_EMOJIS.map((emoji) => (
              <Button
                key={emoji}
                type="button"
                variant="ghost"
                size="icon-sm"
                className="text-lg"
                title={`React with ${emoji}`}
                onClick={() => toggleReaction(message.id, emoji)}
              >
                {emoji}
              </Button>
            ))}
            <Button
              ref={moreButtonRef}
              type="button"
              variant="ghost"
              size="icon-sm"
              aria-label="More message actions"
              onClick={() => {
                setMenuAnchor(moreButtonRef)
                setPickerOpen(false)
                setMenuOpen(true)
              }}
            >
              <MoreHorizontal className="size-4" />
            </Button>
          </div>
        ) : null}
      </div>

      {menuAnchor ? (
        <MessageContextMenu
          open={menuOpen}
          onOpenChange={setMenuOpen}
          anchor={menuAnchor}
          anchoredToMore={anchoredToMore}
          message={message}
          isAuthor={isAuthor}
          canDelete={canDelete}
          onAddReaction={() => {
            // open after the menu's closing click settles, so the picker does not treat it as an outside press
            requestAnimationFrame(() => setPickerOpen(true))
          }}
          onCopyText={handleCopyText}
          onEdit={startEditing}
          onReply={handleReply}
          onThread={onOpenThread ? openThread : undefined}
          threadLabel={hasThread ? 'Open Thread' : 'Create Thread'}
          onPin={() => {
            togglePinMessage(message.id)
            setMenuOpen(false)
          }}
          onDelete={requestDelete}
        />
      ) : null}

      {menuAnchor ? (
        <Popover open={pickerOpen} onOpenChange={setPickerOpen}>
          <PopoverContent
            anchor={menuAnchor}
            side="bottom"
            align={anchoredToMore ? 'end' : 'start'}
            sideOffset={anchoredToMore ? 4 : 0}
            className="w-auto gap-0 p-0"
          >
            <EmojiPicker
              onPick={(emoji) => {
                toggleReaction(message.id, emoji)
                setPickerOpen(false)
              }}
            />
          </PopoverContent>
        </Popover>
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
    <div className={cn('relative', starterCard ? 'mt-1.5' : 'mt-2')} data-starter={starterCard || undefined}>
      {!compact && !starterCard ? <span className="pointer-events-none absolute -left-9 top-1 h-4 w-9 rounded-bl-lg border-b-2 border-l-2 border-muted" /> : null}
      <Button type="button" variant="ghost" className="flex h-auto w-fit max-w-[min(448px,100%)] items-start justify-start gap-2 rounded-md border-border bg-muted/20 px-2.5 py-2 text-left font-normal whitespace-normal hover:bg-muted/45 dark:hover:bg-muted/45" onClick={onOpen}>
        {!starterCard ? (
          <span className="mt-0.5 flex size-5 shrink-0 items-center justify-center rounded-full bg-muted text-muted-foreground">
            <ThreadIcon size={14} className="size-3.5" />
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
      </Button>
    </div>
  )
}

function ReplyReference({ state, reply }: { state: AppState; reply: ChatMessage }) {
  const name = displayName(state, reply)
  const preview = reply.content.trim() || 'No message content'
  return (
    <Button type="button" variant="ghost" className="relative mb-0.5 ml-5 h-6 min-w-0 max-w-[min(720px,calc(100%-20px))] cursor-pointer justify-start gap-1.5 rounded-none pr-0 pl-9 text-left text-xs leading-5 font-normal text-muted-foreground hover:bg-transparent hover:text-foreground dark:hover:bg-transparent" onClick={() => jumpToMessage(reply.id)}>
      <span className="pointer-events-none absolute top-3 left-0 h-6 w-8 rounded-tl-md border-t-2 border-l-2 border-muted-foreground/40" />
      <span className="grid size-4 shrink-0 place-items-center rounded-full bg-muted text-[9px] font-bold text-muted-foreground">{name.charAt(0).toUpperCase()}</span>
      <span className="shrink-0 font-bold text-muted-foreground">@{name}</span>
      <span className="min-w-0 truncate font-semibold">{preview}</span>
    </Button>
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
      <Textarea
        ref={ref}
        className="min-h-0 resize-none field-sizing-fixed"
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
        <Button
          key={r.emoji}
          type="button"
          variant="ghost"
          className="h-auto w-max cursor-pointer gap-1 rounded-full bg-muted px-2.5 py-0.5 tabular-nums ring-1 ring-border ring-inset hover:bg-muted/80 dark:hover:bg-muted/80 data-[mine=true]:bg-primary/10 data-[mine=true]:ring-primary/40 data-[mine=true]:hover:bg-primary/20 dark:data-[mine=true]:hover:bg-primary/20"
          data-mine={r.userIds.includes(currentUserId) ? 'true' : undefined}
          onClick={() => toggleReaction(message.id, r.emoji)}
        >
          <Emoji value={r.emoji} size={15} /> {r.userIds.length}
        </Button>
      ))}
    </div>
  )
}

function MessageContextMenu({
  open,
  onOpenChange,
  anchor,
  anchoredToMore,
  message,
  isAuthor,
  canDelete,
  onAddReaction,
  onCopyText,
  onEdit,
  onReply,
  onThread,
  threadLabel,
  onPin,
  onDelete,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  anchor: MenuAnchor
  anchoredToMore: boolean
  message: ChatMessage
  isAuthor: boolean
  canDelete: boolean
  onAddReaction: () => void
  onCopyText: () => void
  onEdit: () => void
  onReply: () => void
  onThread?: () => void
  threadLabel: string
  onPin: () => void
  onDelete: () => void
}) {
  return (
    <DropdownMenu open={open} onOpenChange={onOpenChange}>
      <DropdownMenuContent
        anchor={anchor}
        side="bottom"
        align={anchoredToMore ? 'end' : 'start'}
        sideOffset={anchoredToMore ? 4 : 0}
        // no focus return: "Add Reaction" hands focus to the picker's search input
        finalFocus={false}
        className="w-auto min-w-48"
      >
        <ChatDropdownMenuItem onClick={onAddReaction}>
          <SmilePlus />
          Add Reaction
        </ChatDropdownMenuItem>
        <ChatDropdownMenuItem onClick={onReply}>
          <Reply />
          Reply
        </ChatDropdownMenuItem>
        {onThread ? (
          <ChatDropdownMenuItem onClick={onThread}>
            <ThreadIcon size={16} />
            {threadLabel}
          </ChatDropdownMenuItem>
        ) : null}
        <DropdownMenuSeparator />
        {isAuthor ? (
          <ChatDropdownMenuItem onClick={onEdit}>
            <Pencil />
            Edit Message
          </ChatDropdownMenuItem>
        ) : null}
        <ChatDropdownMenuItem onClick={onPin}>
          <PinIcon size={16} />
          {message.pinned ? 'Unpin Message' : 'Pin Message'}
        </ChatDropdownMenuItem>
        <ChatDropdownMenuItem onClick={onCopyText}>
          <Copy />
          Copy Text
        </ChatDropdownMenuItem>
        {canDelete ? (
          <>
            <DropdownMenuSeparator />
            <ChatDropdownMenuItem danger onClick={onDelete}>
              <Trash2 />
              Delete Message
            </ChatDropdownMenuItem>
          </>
        ) : null}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
