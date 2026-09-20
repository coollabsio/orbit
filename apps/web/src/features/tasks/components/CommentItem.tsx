import { useEffect, useRef, useState } from 'react'
import { Copy, Pencil, Trash2 } from 'lucide-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { Kbd } from '@/components/ui/kbd'
import { Textarea } from '@/components/ui/textarea'
import { UserAvatar } from '@/components/common/UserAvatar'
import { ConfirmDeleteModal } from '@/components/common/ConfirmDeleteModal'
import { Attachments } from '@/components/common/Attachments'
import type { MentionToken } from '@/lib/mentions'
import { renderMarkdownBlocks } from '@/lib/markdown'
import type { TaskComment, TaskViewState } from '@/features/tasks/api/models'
import { useDeleteTaskComment, useUpdateTaskComment } from '@/features/tasks/api/tasks'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { agoLabel } from '@/features/tasks/tasksLib'

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
  const author = state.users.find((u) => u.id === comment.authorId)
  const isAuthor = comment.authorId === state.currentUserId
  const name = author?.name ?? 'Someone'

  const commitEdit = () => {
    const trimmed = editText.trim()
    if (trimmed && trimmed !== comment.body) updateComment.mutate({ commentId: comment.id, body: trimmed, version: comment.version })
    setEditing(false)
  }

  return (
    <>
      <article
        className={cn(
          'group/comment flex gap-2.5 px-3.5 py-3 transition-colors hover:bg-muted/40 focus-within:bg-muted/40',
          reply && 'border-t border-border pl-[52px] max-[899px]:pl-10',
        )}
        data-reply={reply || undefined}
      >
        <UserAvatar user={author} size={reply ? 22 : 28} name={name} />
        <div className="min-w-0 flex-1">
          <div className="flex min-h-[22px] items-center gap-1.5">
            <span className="min-w-0 truncate text-[13px] font-semibold text-foreground">{name}</span>
            <span className="shrink-0 text-[11px] leading-none text-muted-foreground/70" aria-hidden="true">·</span>
            <time className="shrink-0 text-[11px] font-medium text-muted-foreground" dateTime={comment.createdAt}>
              {agoLabel(comment.createdAt)}
              {comment.editedAt ? ' (edited)' : ''}
            </time>
            {!editing ? (
              <div className="ml-auto flex shrink-0 items-center gap-px opacity-0 transition-opacity group-hover/comment:opacity-100 group-focus-within/comment:opacity-100 max-[899px]:opacity-100">
                <Button variant="ghost" size="icon-sm" className="size-6 text-muted-foreground/70" aria-label="Copy text" title="Copy text" onClick={() => void navigator.clipboard.writeText(comment.body)}>
                  <Copy className="size-3.5" />
                </Button>
                {isAuthor ? (
                  <>
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      className="size-6 text-muted-foreground/70"
                      aria-label="Edit comment"
                      title="Edit comment"
                      onClick={() => {
                        setEditText(comment.body)
                        setEditing(true)
                      }}
                    >
                      <Pencil className="size-3.5" />
                    </Button>
                    <Button variant="ghost" size="icon-sm" className="size-6 text-muted-foreground/70 hover:bg-destructive/10 hover:text-destructive" aria-label="Delete comment" title="Delete comment" onClick={() => setConfirmDelete(true)}>
                      <Trash2 className="size-3.5" />
                    </Button>
                  </>
                ) : null}
              </div>
            ) : null}
          </div>
          {editing ? (
            <EditingTextarea value={editText} onChange={setEditText} onCommit={commitEdit} onCancel={() => setEditing(false)} />
          ) : comment.body.trim() ? (
            <div className="mt-0.5 text-[13px] leading-[19px] font-normal text-foreground [overflow-wrap:anywhere]">{renderMarkdownBlocks(comment.body, comment.id, mentionTokens)}</div>
          ) : null}
          {comment.attachments && comment.attachments.length > 0 ? (
            <Attachments attachments={comment.attachments} hasTextContent={!!comment.body.trim()} />
          ) : null}
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
        className="inline-block field-sizing-fixed min-h-[52px] w-full resize-y rounded-lg border border-input bg-transparent px-3 py-2 text-[13px] leading-5 md:text-[13px] text-foreground outline-none transition-colors placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 dark:bg-input/30"
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
