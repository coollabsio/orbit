import { useEffect, useRef, useState } from 'react'
import { Copy, Edit, Trash } from 'reicon-react'
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

/** A task comment rendered like a chat message: markdown, attachments, hover toolbar with edit/delete. */
export function CommentItem({ state, taskId, comment, mentionTokens, reply }: CommentItemProps) {
  const { workspace } = useWorkspace()
  const updateComment = useUpdateTaskComment(workspace.id, taskId)
  const deleteComment = useDeleteTaskComment(workspace.id, taskId)
  const [hovered, setHovered] = useState(false)
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
      <div
        className="fc-msg tasks-comment"
        data-full="true"
        data-reply={reply || undefined}
        onMouseEnter={() => setHovered(true)}
        onMouseLeave={() => setHovered(false)}
      >
        <div className="fc-msg-row">
          <div className="fc-msg-gutter">
            <div
              className="fc-avatar"
              style={author ? { background: `color-mix(in srgb, ${author.color} 22%, transparent)`, color: author.color } : undefined}
            >
              {name.charAt(0).toUpperCase()}
            </div>
          </div>
          <div className="fc-msg-body">
            <div className="fc-msg-header">
              <span className="fc-msg-author">{name}</span>
              <span className="fc-msg-time">
                {agoLabel(comment.createdAt)}
                {comment.editedAt ? ' (edited)' : ''}
              </span>
            </div>
            {editing ? (
              <EditingTextarea value={editText} onChange={setEditText} onCommit={commitEdit} onCancel={() => setEditing(false)} />
            ) : comment.body.trim() ? (
              <div className="fc-msg-text">{renderMarkdownBlocks(comment.body, comment.id, mentionTokens)}</div>
            ) : null}
            {comment.attachments && comment.attachments.length > 0 ? (
              <Attachments attachments={comment.attachments} hasTextContent={!!comment.body.trim()} />
            ) : null}
          </div>
        </div>

        {hovered && !editing ? (
          <div className="fc-toolbar">
            <button title="Copy text" onClick={() => void navigator.clipboard.writeText(comment.body)}>
              <Copy />
            </button>
            {isAuthor ? (
              <>
                <button
                  title="Edit comment"
                  onClick={() => {
                    setEditText(comment.body)
                    setEditing(true)
                  }}
                >
                  <Edit />
                </button>
                <div className="fc-toolbar-sep" />
                <button data-danger="true" title="Delete comment" onClick={() => setConfirmDelete(true)}>
                  <Trash />
                </button>
              </>
            ) : null}
          </div>
        ) : null}
      </div>

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
