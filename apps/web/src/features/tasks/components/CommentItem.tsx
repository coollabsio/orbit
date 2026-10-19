import { toast } from 'sonner'
import { Suspense, useMemo, useRef, useState } from 'react'
import { Copy, Edit as Pencil, Trash as Trash2 } from 'reicon-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { Tip } from '@/components/common/Tip'
import { Kbd } from '@/components/ui/kbd'
import { UserAvatar } from '@/components/common/UserAvatar'
import { ConfirmDeleteModal } from '@/components/common/ConfirmDeleteModal'
import { Attachments } from '@/components/common/Attachments'
import { contentEquals } from '@/features/docs/editor/content'
import { uploadInlineFile } from '@/features/tasks/api/inlineFiles'
import { isEmptyRichText } from '@/features/tasks/lib/richText'
// Type-only import: erased at build time, so the BlockNote chunk stays lazy.
import type { PageEditorHandle } from '@/features/docs/editor/PageEditor'
import { EditorFallback, PageEditor } from './lazyEditor'
import type { TaskComment, TaskViewState } from '@/features/tasks/api/models'
import { useDeleteTaskComment, useUpdateTaskComment } from '@/features/tasks/api/tasks'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { agoLabel } from '@/features/tasks/tasksLib'
import { ProfileTrigger } from '@/components/common/ProfileTrigger'

interface CommentItemProps {
  state: TaskViewState
  taskId: string
  comment: TaskComment
  reply?: boolean
}

/** A task comment with author, timestamp, body, and a compact hover action row. */
export function CommentItem({ state, taskId, comment, reply }: CommentItemProps) {
  const { workspace } = useWorkspace()
  const updateComment = useUpdateTaskComment(workspace.id, taskId)
  const deleteComment = useDeleteTaskComment(workspace.id, taskId)
  const [editing, setEditing] = useState(false)
  const [confirmDelete, setConfirmDelete] = useState(false)
  const editorRef = useRef<PageEditorHandle>(null)
  const author = state.users.find((u) => u.id === comment.authorId)
  const name = author?.name ?? 'Someone'
  const mentions = useMemo(() => ({ members: state.users, candidates: state.users.filter((user) => !user.suspended), privatePage: false }), [state.users])
  const fileScope = useMemo(() => ({ workspaceId: workspace.id, taskId }), [workspace.id, taskId])
  const hasBody = comment.bodyJson ? !isEmptyRichText(comment.bodyJson) : comment.body.trim() !== ''

  const commitEdit = () => {
    const content = editorRef.current?.getContent() ?? []
    // an emptied comment is deleted, not saved
    if (!isEmptyRichText(content) && !contentEquals(content, comment.bodyJson)) {
      updateComment.mutate({ commentId: comment.id, bodyJson: content, version: comment.version })
    }
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
              <div className="ml-auto flex shrink-0 items-center gap-px opacity-0 transition-opacity group-hover/comment:opacity-100 group-focus-within/comment:opacity-100 max-[899px]:opacity-100">
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
                      onClick={() => setEditing(true)}
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
          {editing || hasBody ? (
            <div data-realtime-safe={editing ? '' : undefined} className={cn('text-[13px] leading-[19px] text-foreground [overflow-wrap:anywhere]', editing ? 'mt-1.5 rounded-md border px-2 py-1' : 'mt-0.5')}>
              <Suspense fallback={<EditorFallback />}>
                <PageEditor
                  // a saved edit brings a new version: start from the stored content again
                  key={`${comment.id}:${comment.version}:${editing}`}
                  ref={editorRef}
                  pageId={`comment:${comment.id}`}
                  profile="task"
                  fileScope={fileScope}
                  initialContent={comment.bodyJson ?? undefined}
                  initialMarkdown={comment.body}
                  editable={editing}
                  submitShortcut={editing ? commitEdit : undefined}
                  onEscape={editing ? () => setEditing(false) : undefined}
                  uploadFile={editing ? (file) => uploadInlineFile(workspace.id, taskId, file) : undefined}
                  mentions={mentions}
                  className="-mx-[54px] max-[899px]:mx-0"
                />
              </Suspense>
            </div>
          ) : null}
          {editing ? (
            <div className="mt-1 flex items-center gap-2 text-[11px] text-muted-foreground">
              <span><Kbd className="h-4 text-[11px]">escape</Kbd> to <b className="text-foreground">cancel</b> · <Kbd className="h-4 text-[11px]">⌘ enter</Kbd> to <b className="text-foreground">save</b></span>
              <Button variant="ghost" size="xs" className="ml-auto" onClick={() => setEditing(false)}>Cancel</Button>
              <Button size="xs" onClick={commitEdit}>Save</Button>
            </div>
          ) : null}
          {comment.attachments && comment.attachments.length > 0 ? (
            <Attachments attachments={comment.attachments} hasTextContent={hasBody} />
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
