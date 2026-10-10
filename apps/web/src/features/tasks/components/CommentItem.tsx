import { toast } from 'sonner'
import { lazy, Suspense, useEffect, useRef, useState } from 'react'
import { Copy, EmojiHappy, Edit as Pencil, Trash as Trash2 } from 'reicon-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { Tip } from '@/components/common/Tip'
import { Kbd } from '@/components/ui/kbd'
import { Textarea } from '@/components/ui/textarea'
import { UserAvatar } from '@/components/common/UserAvatar'
import { ConfirmDeleteModal } from '@/components/common/ConfirmDeleteModal'
import { Attachments } from '@/components/common/Attachments'
import { EmojiPicker } from '@/components/common/EmojiPicker'
import { ReactionChips } from '@/components/common/ReactionChips'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import type { MentionToken } from '@/lib/mentions'
import { renderMarkdownBlocks } from '@/lib/markdown'
import type { TaskComment, TaskViewState } from '@/features/tasks/api/models'
import { useDeleteTaskComment, useToggleCommentReaction, useUpdateTaskComment, useUploadTaskImage } from '@/features/tasks/api/tasks'
import { normalizeMarkdown } from '@/components/common/markdownEditor/convert'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { agoLabel } from '@/features/tasks/tasksLib'
import { ProfileTrigger } from '@/components/common/ProfileTrigger'

const MarkdownEditor = lazy(() => import('@/components/common/markdownEditor/MarkdownEditor'))

interface CommentItemProps {
  state: TaskViewState
  taskId: string
  comment: TaskComment
  mentionTokens: MentionToken[]
  reply?: boolean
}

/** A task comment with author, timestamp, body, and a compact hover action row. */
export function CommentItem({ state, taskId, comment, mentionTokens, reply }: CommentItemProps) {
  const { workspace } = useWorkspace()
  const updateComment = useUpdateTaskComment(workspace.id, taskId)
  const deleteComment = useDeleteTaskComment(workspace.id, taskId)
  const [editing, setEditing] = useState(false)
  const [editText, setEditText] = useState('')
  const [confirmDelete, setConfirmDelete] = useState(false)
  const toggleReaction = useToggleCommentReaction(workspace.id, taskId)
  const [pickerOpen, setPickerOpen] = useState(false)
  /** Adds the user's reaction, or removes it when they have it. */
  const react = (emoji: string) => {
    const on = !comment.reactions.some((reaction) => reaction.emoji === emoji && reaction.userIds.includes(state.currentUserId))
    toggleReaction.mutate({ commentId: comment.id, emoji, on, userId: state.currentUserId }, { onError: () => toast.error('Could not save the reaction. Try again.') })
  }
  const author = state.users.find((u) => u.id === comment.authorId)
  const name = author?.name ?? 'Someone'

  // The markdown of the rich editor after the person changed the document; `null` until then: a comment that was
  // opened and closed with no edit is not saved.
  const richDraft = useRef<string | null>(null)
  const closeRichEdit = (save: boolean) => {
    const draft = richDraft.current?.trim() ?? null
    richDraft.current = null
    if (save && draft && normalizeMarkdown(draft) !== normalizeMarkdown(comment.body)) updateComment.mutate({ commentId: comment.id, body: draft, version: comment.version })
    setEditing(false)
  }
  const uploadImage = useUploadTaskImage(workspace.id, taskId)
  const commitEdit = () => {
    const trimmed = editText.trim()
    if (trimmed && trimmed !== comment.body) updateComment.mutate({ commentId: comment.id, body: trimmed, version: comment.version })
    setEditing(false)
  }

  return (
    <>
      <article
        className={cn(
          'group/comment flex gap-2 px-3.5 py-2.5 transition-colors hover:bg-muted/40 focus-within:bg-muted/40',
          reply && 'border-t',
        )}
        data-reply={reply || undefined}
      >
        <ProfileTrigger userId={author?.id} name={name} kind="avatar" tabIndex={-1} className="flex shrink-0 self-start">
          <UserAvatar user={author} size={20} name={name} />
        </ProfileTrigger>
        <div className="min-w-0 flex-1">
          <div className="flex min-h-5 items-center gap-1.5">
            <ProfileTrigger userId={author?.id} name={name} className="min-w-0 truncate text-[13px] font-semibold text-foreground">
              {name}
            </ProfileTrigger>
            <span className="shrink-0 text-[11px] leading-none text-muted-foreground/70" aria-hidden="true">·</span>
            <time className="shrink-0 text-[11px] font-medium text-muted-foreground" dateTime={comment.createdAt}>
              {agoLabel(comment.createdAt)}
              {comment.editedAt ? ' (edited)' : ''}
            </time>
            {!editing ? (
              <div className="ml-auto flex shrink-0 items-center gap-px opacity-0 transition-opacity group-hover/comment:opacity-100 group-focus-within/comment:opacity-100 data-open:opacity-100 max-[899px]:opacity-100" data-open={pickerOpen || undefined}>
                <Popover open={pickerOpen} onOpenChange={setPickerOpen} modal={false}>
                  <Tip label="Add reaction">
                    <PopoverTrigger render={<Button variant="ghost" size="icon-sm" className="size-6 text-muted-foreground/70" aria-label="Add reaction"><EmojiHappy className="size-3.5" /></Button>} />
                  </Tip>
                  {pickerOpen ? (
                    <PopoverContent side="bottom" align="end" className="w-auto gap-0 p-0">
                      <EmojiPicker
                        custom
                        onPick={(emoji) => {
                          react(emoji)
                          setPickerOpen(false)
                        }}
                      />
                    </PopoverContent>
                  ) : null}
                </Popover>
                <Tip label="Copy text">
                  <Button variant="ghost" size="icon-sm" className="size-6 text-muted-foreground/70" aria-label="Copy text" onClick={() => void navigator.clipboard.writeText(comment.body).then(() => toast('Text copied'), () => toast.error('Could not copy to the clipboard.'))}>
                    <Copy className="size-3.5" />
                  </Button>
                </Tip>
                {comment.canEdit ? (
                  <Tip label="Edit comment">
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      className="size-6 text-muted-foreground/70"
                      aria-label="Edit comment"
                      onClick={() => {
                        setEditText(comment.body)
                        setEditing(true)
                      }}
                    >
                      <Pencil className="size-3.5" />
                    </Button>
                  </Tip>
                ) : null}
                {comment.canDelete ? (
                  <Tip label="Delete comment">
                    <Button variant="ghost" size="icon-sm" className="size-6 text-muted-foreground/70 hover:bg-destructive/10 hover:text-destructive" aria-label="Delete comment" onClick={() => setConfirmDelete(true)}>
                      <Trash2 className="size-3.5" />
                    </Button>
                  </Tip>
                ) : null}
              </div>
            ) : null}
          </div>
          {editing ? (
            // the read view stays until the editor's chunk is here; text that the rich editor would change opens in the plain field
            <Suspense fallback={<div className="mt-0.5 text-[13px] leading-[19px] [overflow-wrap:anywhere]">{renderMarkdownBlocks(comment.body, comment.id, mentionTokens)}</div>}>
              <MarkdownEditor
                value={comment.body}
                members={state.users}
                ariaLabel="Edit comment"
                className="mt-1.5 rounded-md border px-2 py-1 focus-within:border-ring"
                compact
                autoFocus
                fallback={<EditingTextarea value={editText} onChange={setEditText} onCommit={commitEdit} onCancel={() => setEditing(false)} />}
                onChange={(markdown) => { richDraft.current = markdown }}
                onSubmit={() => closeRichEdit(true)}
                onBlur={() => closeRichEdit(true)}
                onCancel={() => closeRichEdit(false)}
                uploadImage={uploadImage}
              />
            </Suspense>
          ) : comment.body.trim() ? (
            <div className="mt-0.5 text-[13px] leading-[19px] font-normal text-foreground [overflow-wrap:anywhere]">{renderMarkdownBlocks(comment.body, comment.id, mentionTokens)}</div>
          ) : null}
          {comment.attachments && comment.attachments.length > 0 ? (
            <Attachments attachments={comment.attachments} hasTextContent={!!comment.body.trim()} />
          ) : null}
          <ReactionChips
            slot="comment-reactions"
            reactions={comment.reactions}
            currentUserId={state.currentUserId}
            nameOf={(id) => state.users.find((user) => user.id === id)?.name ?? 'Unknown'}
            onToggle={react}
          />
        </div>
      </article>

      {confirmDelete ? (
        <ConfirmDeleteModal
          title="Delete comment?"
          description={reply ? 'This will remove the reply.' : 'This will remove the comment and its replies.'}
          onClose={() => setConfirmDelete(false)}
          onConfirm={() => {
            deleteComment.mutate({ commentId: comment.id, version: comment.version })
            setConfirmDelete(false)
          }}
        />
      ) : null}
      {updateComment.isError ? <p role="alert" className="text-xs text-destructive">Comment update failed. <Button variant="ghost" onClick={() => updateComment.variables && updateComment.mutate(updateComment.variables)}>Retry</Button></p> : null}
      {deleteComment.isError ? <p role="alert" className="text-xs text-destructive">Comment deletion failed. <Button variant="ghost" onClick={() => deleteComment.variables && deleteComment.mutate(deleteComment.variables)}>Retry</Button></p> : null}
    </>
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
    <div className="mt-1.5">
      <Textarea
        ref={ref}
        className="field-sizing-fixed min-h-[52px] resize-y text-[13px] leading-5 md:text-[13px]"
        value={value}
        rows={2}
        aria-label="Edit comment"
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
      <div className="mt-1 text-[11px] text-muted-foreground">
        <Kbd className="h-4 text-[11px]">escape</Kbd> to <b className="text-foreground">cancel</b> · <Kbd className="h-4 text-[11px]">enter</Kbd> to <b className="text-foreground">save</b>
      </div>
    </div>
  )
}
