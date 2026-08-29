import { useState } from 'react'
import { ArrowUp, Paperclip2 } from 'reicon-react'
import { Avatar } from '../../../components/ui/Avatar'
import { TaskStatusIcon } from '../../../components/workspace/TaskStatusIcon'
import { addTaskComment } from '../../../mock/actions'
import type { Task, TaskComment, User } from '../../../mock/types'
import { agoLabel, commentThreads } from '../tasksLib'

interface ActivityFeedProps {
  task: Task
  users: User[]
  currentUserId: string
}

/** Activity timeline (icon + connector per event) followed by comment threads with a reply box each. */
export function ActivityFeed({ task, users, currentUserId }: ActivityFeedProps) {
  const [drafts, setDrafts] = useState<Record<string, string>>({})
  const userById = (id: string) => users.find((u) => u.id === id)
  const me = userById(currentUserId)
  const activity = [...task.activity].sort((a, b) => a.createdAt.localeCompare(b.createdAt))
  const threads = commentThreads(task)

  const sendReply = (rootId: string) => {
    const body = (drafts[rootId] ?? '').trim()
    if (!body) return
    addTaskComment(task.id, body, rootId)
    setDrafts((prev) => ({ ...prev, [rootId]: '' }))
  }

  const renderComment = (comment: TaskComment, reply: boolean) => {
    const author = userById(comment.authorId)
    return (
      <div key={comment.id} className="tasks-thread-comment" data-reply={reply || undefined}>
        <div className="tasks-comment-head">
          <Avatar user={author} size={20} />
          <span className="tasks-comment-author">{author?.name ?? 'Someone'}</span>
          <span className="text-faint text-xs">{agoLabel(comment.createdAt)}</span>
        </div>
        <div className="tasks-comment-text">{comment.body}</div>
      </div>
    )
  }

  return (
    <div className="tasks-activity">
      <h3 className="tasks-activity-heading">Activity</h3>
      <ol className="tasks-timeline">
        {activity.map((item) => {
          const actor = userById(item.actorId)
          return (
            <li key={item.id} className="tasks-timeline-item">
              <span className="tasks-timeline-icon">
                {item.status ? <TaskStatusIcon status={item.status} size={14} /> : <Avatar user={actor} size={14} />}
              </span>
              <span className="tasks-timeline-text truncate">
                <span className="tasks-timeline-actor">{actor?.name ?? 'Someone'}</span> {item.text} · {agoLabel(item.createdAt)}
              </span>
            </li>
          )
        })}
      </ol>

      {threads.map((thread) => (
        <div key={thread.root.id} className="tasks-thread">
          {renderComment(thread.root, false)}
          {thread.replies.map((reply) => renderComment(reply, true))}
          <form
            className="tasks-reply"
            onSubmit={(e) => {
              e.preventDefault()
              sendReply(thread.root.id)
            }}
          >
            <Avatar user={me} size={18} />
            <input
              className="tasks-reply-input"
              placeholder="Leave a reply…"
              aria-label="Reply"
              value={drafts[thread.root.id] ?? ''}
              onChange={(e) => setDrafts((prev) => ({ ...prev, [thread.root.id]: e.target.value }))}
            />
            <button type="button" className="icon-button" aria-label="Attach file" title="Attach file">
              <Paperclip2 size={14} />
            </button>
            <button type="submit" className="tasks-send" aria-label="Send reply" disabled={!(drafts[thread.root.id] ?? '').trim()}>
              <ArrowUp size={14} />
            </button>
          </form>
        </div>
      ))}
    </div>
  )
}
