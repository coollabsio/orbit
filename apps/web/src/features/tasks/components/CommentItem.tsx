import { useMemo, useState } from 'react'
import { Copy, Edit, Trash } from 'reicon-react'
import { RichTextEditor } from '../../../components/editor/RichTextEditor'
import { RichTextView } from '../../../components/editor/RichTextView'
import { isEmptyDocument, sameDocument, taskIdentifiersInDocument, type RichTextDocument } from '../../../components/editor/document'
import { useTaskChips } from '../../../components/editor/useTaskChips'
import { Avatar } from '../../../components/ui/Avatar'
import { ConfirmDeleteModal } from '../../chat/components/ChannelModals'
import { Attachments } from '../../chat/components/Attachments'
import type { TaskComment, TaskStatusDef, TaskViewState } from '../api/models'
import { useDeleteTaskComment, useUpdateTaskComment } from '../api/tasks'
import { agoLabel } from '../tasksLib'

interface CommentItemProps {
  state: TaskViewState
  taskId: string
  workspaceId: string
  statuses: TaskStatusDef[]
  comment: TaskComment
  reply?: boolean
}

/** A task comment with author, timestamp, rich text body, and a compact hover action row. */
export function CommentItem({ state, taskId, workspaceId, statuses, comment, reply }: CommentItemProps) {
  const updateComment = useUpdateTaskComment(workspaceId, taskId)
  const deleteComment = useDeleteTaskComment(workspaceId, taskId)
  const [editing, setEditing] = useState(false)
  const [confirmDelete, setConfirmDelete] = useState(false)
  const identifiers = useMemo(() => taskIdentifiersInDocument(comment.bodyJson), [comment.bodyJson])
  const chips = useTaskChips(workspaceId, identifiers, statuses)
  const author = state.users.find((u) => u.id === comment.authorId)
  const isAuthor = comment.authorId === state.currentUserId
  const name = author?.name ?? 'Someone'
  const hasText = !isEmptyDocument(comment.bodyJson)

  const commitEdit = (document: RichTextDocument) => {
    // Clearing the text is not a delete; that has its own confirmed action.
    if (!isEmptyDocument(document) && !sameDocument(document, comment.bodyJson)) {
      updateComment.mutate({ commentId: comment.id, bodyJson: document, version: comment.version })
    }
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
                <button type="button" className="icon-button" aria-label="Copy text" title="Copy text" onClick={() => void navigator.clipboard.writeText(comment.bodyText)}>
                  <Copy size={14} />
                </button>
                {isAuthor ? (
                  <>
                    <button type="button" className="icon-button" aria-label="Edit comment" title="Edit comment" onClick={() => setEditing(true)}>
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
            <div className="tasks-comment-edit">
              <RichTextEditor
                value={comment.bodyJson}
                placeholder="Edit comment…"
                ariaLabel="Edit comment"
                compact
                autofocus
                workspaceId={workspaceId}
                members={state.users}
                statuses={statuses}
                onSubmit={commitEdit}
                onCancel={() => setEditing(false)}
              />
              <div className="tasks-comment-edit-hint">
                escape to <b>cancel</b> · ⌘/Ctrl + enter to <b>save</b>
              </div>
            </div>
          ) : hasText ? (
            <div className="tasks-comment-text">
              <RichTextView document={comment.bodyJson} chips={chips} />
            </div>
          ) : null}
          {comment.attachments && comment.attachments.length > 0 ? (
            <Attachments attachments={comment.attachments} hasTextContent={hasText} />
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
