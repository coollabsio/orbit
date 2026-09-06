import { useEffect, useRef, useState } from 'react'
import { Copy, Edit, Trash } from 'reicon-react'
import { Avatar } from '../../../components/ui/Avatar'
import { ConfirmDeleteModal } from '../../chat/components/ChannelModals'
import { Attachments } from '../../chat/components/Attachments'
import type { MentionToken } from '../../chat/chatLib'
import { renderMarkdownBlocks } from '../../chat/markdown'
import type { TaskComment, TaskViewState } from '../api/models'
import { useDeleteTaskComment, useUpdateTaskComment } from '../api/tasks'
import { useWorkspace } from '../../workspaces/workspaceContext'
import { agoLabel } from '../tasksLib'

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
      <article className="tasks-comment" data-reply={reply || undefined}>
        <Avatar user={author} size={reply ? 22 : 28} name={name} />
        <div className="tasks-comment-body">
          <div className="tasks-comment-header">
            <span className="tasks-comment-author">{name}</span>
            <span className="tasks-comment-dot" aria-hidden="true">·</span>
            <time className="tasks-comment-time" dateTime={comment.createdAt}>
              {agoLabel(comment.createdAt)}
              {comment.editedAt ? ' (edited)' : ''}
            </time>
            {!editing ? (
              <div className="tasks-comment-actions">
                <button type="button" className="icon-button" aria-label="Copy text" title="Copy text" onClick={() => void navigator.clipboard.writeText(comment.body)}>
                  <Copy size={14} />
                </button>
                {isAuthor ? (
                  <>
                    <button
                      type="button"
                      className="icon-button"
                      aria-label="Edit comment"
                      title="Edit comment"
                      onClick={() => {
                        setEditText(comment.body)
                        setEditing(true)
                      }}
                    >
                      <Edit size={14} />
                    </button>
                    <button type="button" className="icon-button" data-danger="true" aria-label="Delete comment" title="Delete comment" onClick={() => setConfirmDelete(true)}>
                      <Trash size={14} />
                    </button>
                  </>
                ) : null}
              </div>
            ) : null}
          </div>
          {editing ? (
            <EditingTextarea value={editText} onChange={setEditText} onCommit={commitEdit} onCancel={() => setEditing(false)} />
          ) : comment.body.trim() ? (
            <div className="tasks-comment-text">{renderMarkdownBlocks(comment.body, comment.id, mentionTokens)}</div>
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
      {updateComment.isError ? <p role="alert" className="text-danger text-xs">Comment update failed. <button className="button button-ghost" onClick={() => updateComment.variables && updateComment.mutate(updateComment.variables)}>Retry</button></p> : null}
      {deleteComment.isError ? <p role="alert" className="text-danger text-xs">Comment deletion failed. <button className="button button-ghost" onClick={() => deleteComment.variables && deleteComment.mutate(deleteComment.variables)}>Retry</button></p> : null}
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
    <div className="tasks-comment-edit">
      <textarea
        ref={ref}
        className="input"
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
      <div className="tasks-comment-edit-hint">
        escape to <b>cancel</b> · enter to <b>save</b>
      </div>
    </div>
  )
}
