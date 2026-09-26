// Orbit's comment thread card (Comments panel and the floating thread) and the new-comment card. They replace
// BlockNote's stock comment UI but keep its data flow: threads come from the comments extension's thread store,
// bodies are comment-editor BlockNote documents (mentions, formatting), saves go through `CommentEditorSubmitExtension`.
import { memo, use, useRef, useState, type KeyboardEvent, type MouseEvent } from 'react'
import { CommentEditorSubmitExtension, CommentsExtension, type CommentData, type ThreadData } from '@blocknote/core/comments'
import { useBlockNoteEditor, useCreateBlockNote, useExtension, type FloatingComposer, type ThreadProps } from '@blocknote/react'
import type { ComponentProps } from 'react'
import { CheckCircle, Edit, MoreH, RotateLeft, Trash } from 'reicon-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { UserAvatar } from '@/components/common/UserAvatar'
import { fullDate, relativeTime } from '@/lib/format'
import type { User } from '@/mock/types'
import { CommentComposer, type AnyCommentEditor } from './CommentComposer'
import { CommentEditor } from './CommentEditor'
import { COMMENT_PLACEHOLDERS, commentEditorDictionary } from './dictionary'
import { CommentMembersContext } from './mentionContext'
import { commentEditorSchema } from './mentions'
import type { OrbitThreadMetadata } from './threadStore'

export const FORMER_MEMBER = 'Former member'

/** Popups opened from a card (menus) carry this, so the floating thread does not treat clicks there as "outside". */
export const COMMENT_POPUP_ATTR = 'data-comment-popup'

/** "just now", "5m ago", "3d ago", then a short date. */
function commentTime(date: Date): string {
  const label = relativeTime(date.toISOString())
  if (label === 'now') return 'just now'
  return /^\d+[mhd]$/.test(label) ? `${label} ago` : label
}

function useMember(userId: string | undefined) {
  const members = use(CommentMembersContext)
  const member = userId ? members.get(userId) : undefined
  return { name: member?.name ?? FORMER_MEMBER, member }
}

function MemberAvatar({ userId, size }: { userId: string; size: number }) {
  const { name, member } = useMember(userId)
  const user = member?.color ? ({ id: member.id, name: member.name, color: member.color, online: false } as User) : undefined
  return <UserAvatar user={user} name={name} size={size} className="shrink-0" />
}

/** Catches store failures (the store already reported them through `onError`). */
function quietly(action: () => Promise<unknown>) {
  return () => {
    void action().catch(() => {})
  }
}

export interface ThreadCardProps {
  thread: ThreadData
  /** The thread is selected in the editor (its anchor is highlighted). */
  selected?: boolean
  /** The anchored text is gone from the document. */
  orphaned?: boolean
  /** `floating`: the popover next to the text; `panel`: an entry of the Comments panel. */
  variant: 'panel' | 'floating'
  /** The reply editor (the floating controller owns one to guard unsaved text); the card creates its own otherwise. */
  replyEditor?: AnyCommentEditor
  /** Panel cards: select the thread (scrolls to and highlights its anchor). */
  onSelect?: (threadId: string) => void
}

export const ThreadCard = memo(function ThreadCard({ thread, selected = false, orphaned = false, variant, replyEditor, onSelect }: ThreadCardProps) {
  const comments = useExtension(CommentsExtension)
  const store = comments.threadStore
  const quote = (thread.metadata as OrbitThreadMetadata | undefined)?.quote?.trim() ?? ''
  const { name: resolvedByName } = useMember(thread.resolvedBy)
  const canToggle = thread.resolved ? store.auth.canUnresolveThread(thread) : store.auth.canResolveThread(thread)
  const toggleResolved = quietly(() =>
    thread.resolved ? store.unresolveThread({ threadId: thread.id }) : store.resolveThread({ threadId: thread.id }),
  )

  const select = (event: MouseEvent<HTMLElement>) => {
    if (!onSelect) return
    // Buttons and menus inside the card act on their own.
    if (event.target instanceof Element && event.target.closest('button, [role="menuitem"]')) return
    onSelect(thread.id)
  }
  const onKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    if (!onSelect || event.target !== event.currentTarget || (event.key !== 'Enter' && event.key !== ' ')) return
    event.preventDefault()
    onSelect(thread.id)
  }

  return (
    <article
      className={cn(
        'flex flex-col gap-3 rounded-lg border border-border bg-card p-3 text-card-foreground outline-none transition-colors',
        variant === 'floating' ? 'w-[360px] max-w-[calc(100vw-24px)] shadow-lg' : 'focus-visible:ring-3 focus-visible:ring-ring/50',
        variant === 'panel' && onSelect && 'cursor-pointer hover:border-foreground/20',
        variant === 'panel' && selected && 'border-primary/50 ring-1 ring-primary/20 hover:border-primary/50',
      )}
      aria-label={quote ? `Comment thread on “${quote}”` : 'Comment thread'}
      tabIndex={variant === 'panel' ? 0 : undefined}
      data-thread-card=""
      data-thread-id={thread.id}
      data-selected={selected || undefined}
      data-resolved={thread.resolved || undefined}
      data-orphaned={orphaned || undefined}
      onClick={select}
      onKeyDown={onKeyDown}
    >
      <header className="flex items-start gap-2">
        <div className={cn('min-w-0 flex-1', thread.resolved && 'opacity-70')}>
          {orphaned ? (
            <span className="mb-1 inline-flex h-[18px] items-center rounded bg-muted px-1.5 text-[11px] font-medium text-muted-foreground" data-thread-orphaned="">
              Text removed
            </span>
          ) : null}
          {quote ? (
            <p
              className={cn(
                'line-clamp-2 border-l-2 pl-2 text-xs leading-[18px] text-muted-foreground [overflow-wrap:anywhere]',
                orphaned ? 'border-border line-through decoration-muted-foreground/50' : 'border-amber-400 dark:border-amber-400/70',
              )}
              title={quote}
              data-thread-quote=""
            >
              {quote}
            </p>
          ) : null}
        </div>
        {canToggle ? (
          <Tooltip>
            <TooltipTrigger
              render={
                <Button
                  type="button"
                  variant="ghost"
                  size="icon-xs"
                  className={cn('-my-0.5 size-7 text-muted-foreground', thread.resolved ? 'hover:text-foreground' : 'hover:text-emerald-600 dark:hover:text-emerald-400')}
                  aria-label={thread.resolved ? 'Reopen' : 'Resolve'}
                  onClick={toggleResolved}
                />
              }
            >
              {thread.resolved ? <RotateLeft className="size-4" /> : <CheckCircle className="size-4" />}
            </TooltipTrigger>
            <TooltipContent>{thread.resolved ? 'Reopen' : 'Resolve'}</TooltipContent>
          </Tooltip>
        ) : null}
      </header>
      <div className={cn('flex flex-col gap-3', thread.resolved && 'opacity-70')}>
        {thread.comments.map((comment) =>
          comment.deletedAt ? (
            <p key={comment.id} className="pl-8 text-xs text-muted-foreground italic">
              Comment deleted
            </p>
          ) : (
            // Remount on remote edits: the row's editor reads the body once.
            <CommentRow key={comment.id + JSON.stringify(comment.body ?? null)} thread={thread} comment={comment} />
          ),
        )}
      </div>
      {thread.resolved && thread.resolvedUpdatedAt ? (
        <p className="flex items-center gap-1.5 text-xs text-muted-foreground" data-thread-resolved="">
          <CheckCircle className="size-3.5 shrink-0 text-emerald-600 dark:text-emerald-400" aria-hidden="true" />
          <span className="min-w-0 truncate">
            Resolved by <span className="font-medium text-foreground">{resolvedByName}</span>
          </span>
          <span aria-hidden="true">·</span>
          <time className="shrink-0" dateTime={thread.resolvedUpdatedAt.toISOString()} title={fullDate(thread.resolvedUpdatedAt.toISOString())}>
            {commentTime(thread.resolvedUpdatedAt)}
          </time>
        </p>
      ) : null}
      {!thread.resolved && store.auth.canAddComment(thread) ? <ReplyArea threadId={thread.id} editor={replyEditor} /> : null}
    </article>
  )
})

function CommentRow({ thread, comment }: { thread: ThreadData; comment: CommentData }) {
  const comments = useExtension(CommentsExtension)
  const store = comments.threadStore
  const { name } = useMember(comment.userId)
  const [editing, setEditing] = useState(false)
  const [menuOpen, setMenuOpen] = useState(false)
  const editor = useCreateBlockNote({
    initialContent: comment.body,
    trailingBlock: false,
    dictionary: commentEditorDictionary(COMMENT_PLACEHOLDERS.edit_comment),
    schema: comments.commentEditorSchema ?? commentEditorSchema,
    extensions: [
      CommentEditorSubmitExtension({
        submitOnEnter: comments.submitOnEnter,
        onSubmit: async (edited) => {
          await store.updateComment({ threadId: thread.id, commentId: comment.id, comment: { body: edited.document } })
          setEditing(false)
        },
      }),
    ],
  })
  const canEdit = store.auth.canUpdateComment(comment)
  const canDelete = store.auth.canDeleteComment(comment)
  const edited = comment.updatedAt.getTime() !== comment.createdAt.getTime()
  const cancelEdit = () => {
    editor.replaceBlocks(editor.document, comment.body)
    setEditing(false)
  }

  return (
    <div className="group/comment flex gap-2" data-comment-id={comment.id}>
      <MemberAvatar userId={comment.userId} size={24} />
      <div className="min-w-0 flex-1">
        <div className="flex h-6 items-center gap-1.5">
          <span className="min-w-0 truncate text-[13px] font-medium text-foreground" data-comment-author="">
            {name}
          </span>
          <time className="shrink-0 text-xs text-muted-foreground" dateTime={comment.createdAt.toISOString()} title={fullDate(comment.createdAt.toISOString())}>
            {commentTime(comment.createdAt)}
            {edited ? ' · edited' : ''}
          </time>
          <span className="flex-1" />
          {(canEdit || canDelete) && !editing ? (
            <DropdownMenu modal={false} open={menuOpen} onOpenChange={setMenuOpen}>
              <DropdownMenuTrigger
                render={
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon-xs"
                    className={cn(
                      'text-muted-foreground opacity-0 group-hover/comment:opacity-100 group-focus-within/comment:opacity-100 focus-visible:opacity-100 max-[899px]:opacity-100',
                      menuOpen && 'opacity-100',
                    )}
                    aria-label="Comment actions"
                  />
                }
              >
                <MoreH className="size-4" />
              </DropdownMenuTrigger>
              {menuOpen ? (
                <DropdownMenuContent align="end" className="w-36" {...{ [COMMENT_POPUP_ATTR]: '' }}>
                  {canEdit ? (
                    <DropdownMenuItem onClick={() => setEditing(true)}>
                      <Edit /> Edit
                    </DropdownMenuItem>
                  ) : null}
                  {canDelete ? (
                    <DropdownMenuItem variant="destructive" onClick={quietly(() => store.deleteComment({ threadId: thread.id, commentId: comment.id }))}>
                      <Trash /> Delete
                    </DropdownMenuItem>
                  ) : null}
                </DropdownMenuContent>
              ) : null}
            </DropdownMenu>
          ) : null}
        </div>
        {editing ? (
          <CommentComposer editor={editor} kind="edit" submitLabel="Save" label="Edit comment" onCancel={cancelEdit} autoFocus className="mt-1" />
        ) : (
          <CommentEditor editor={editor} editable={false} className="orbit-comment-body" />
        )}
      </div>
    </div>
  )
}

/** "Reply…" as a one-line field; the real composer mounts on focus and folds back when left empty. */
function ReplyArea({ threadId, editor }: { threadId: string; editor?: AnyCommentEditor }) {
  const [open, setOpen] = useState(false)
  if (!open) {
    return (
      <button
        type="button"
        className="flex h-8 w-full items-center rounded-lg border border-input bg-transparent px-2.5 text-left text-[13px] text-muted-foreground transition-colors hover:border-ring/60 focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 focus-visible:outline-none dark:bg-input/30"
        data-reply-trigger=""
        onClick={() => setOpen(true)}
        onFocus={() => setOpen(true)}
      >
        {COMMENT_PLACEHOLDERS.comment_reply}
      </button>
    )
  }
  return editor ? (
    <ReplyComposer editor={editor} onClose={() => setOpen(false)} />
  ) : (
    <OwnReplyComposer threadId={threadId} onClose={() => setOpen(false)} />
  )
}

function OwnReplyComposer({ threadId, onClose }: { threadId: string; onClose: () => void }) {
  const comments = useExtension(CommentsExtension)
  const editor = useCreateBlockNote({
    trailingBlock: false,
    dictionary: commentEditorDictionary(COMMENT_PLACEHOLDERS.comment_reply),
    schema: comments.commentEditorSchema ?? commentEditorSchema,
    extensions: [
      CommentEditorSubmitExtension({
        submitOnEnter: comments.submitOnEnter,
        onSubmit: async (reply) => {
          await comments.threadStore.addComment({ threadId, comment: { body: reply.document } })
          reply.removeBlocks(reply.document)
        },
      }),
    ],
  })
  return <ReplyComposer editor={editor} onClose={onClose} />
}

function ReplyComposer({ editor, onClose }: { editor: AnyCommentEditor; onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null)
  const cancel = () => {
    editor.removeBlocks(editor.document)
    onClose()
  }
  return (
    <div
      ref={ref}
      onBlur={() => {
        // Fold back once focus has left an empty reply (also right after sending one). Checked a moment later: the
        // editor view can be recreated while mounting, which blurs the old one before the new one takes focus.
        setTimeout(() => {
          const wrapper = ref.current
          if (!wrapper || wrapper.contains(document.activeElement)) return
          if (editor.isEmpty) onClose()
        }, 0)
      }}
    >
      <CommentComposer editor={editor} kind="reply" submitLabel="Reply" label="Reply" onCancel={cancel} autoFocus />
    </div>
  )
}

/** The floating thread (panel closed): the same card next to the highlighted text. */
export function FloatingThreadCard({ thread, newCommentEditor }: ThreadProps) {
  return <ThreadCard thread={thread} selected variant="floating" replyEditor={newCommentEditor} />
}

/** The floating card for a new thread on the selected text. */
export function NewCommentCard({ newCommentEditor }: ComponentProps<typeof FloatingComposer>) {
  const editor = useBlockNoteEditor()
  const comments = useExtension(CommentsExtension)
  const cancel = () => {
    comments.stopPendingComment()
    editor.focus()
  }
  return (
    <div className="w-[360px] max-w-[calc(100vw-24px)] rounded-lg border border-border bg-popover p-3 text-popover-foreground shadow-lg" data-new-comment="">
      <CommentComposer editor={newCommentEditor} kind="new" submitLabel="Comment" label="New comment" onCancel={cancel} autoFocus />
    </div>
  )
}
