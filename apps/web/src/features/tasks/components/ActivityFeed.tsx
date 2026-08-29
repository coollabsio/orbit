import { useMemo } from 'react'
import { Avatar } from '../../../components/ui/Avatar'
import { TaskStatusIcon } from '../../../components/workspace/TaskStatusIcon'
import { addTaskComment } from '../../../mock/actions'
import type { AppState, Task, TaskActivity } from '../../../mock/types'
import { buildMentionTokens } from '../../chat/chatLib'
import { MessageInput } from '../../chat/components/MessageInput'
import { agoLabel, buildFeed, type CommentThread } from '../tasksLib'
import { CommentItem } from './CommentItem'

interface ActivityFeedProps {
  task: Task
  state: AppState
}

/**
 * Chronological feed: activity events (timeline rows) and comment threads (cards) interleaved by time,
 * so a change made after a comment shows below that comment.
 */
export function ActivityFeed({ task, state }: ActivityFeedProps) {
  const mentionTokens = useMemo(() => buildMentionTokens(state.users), [state.users])
  const userById = (id: string) => state.users.find((u) => u.id === id)
  const feed = buildFeed(task)

  const renderTimeline = (items: TaskActivity[], key: string) => (
    <ol key={key} className="tasks-timeline">
      {items.map((item) => {
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
  )

  const renderThread = (thread: CommentThread) => (
    <div key={thread.root.id} className="tasks-thread">
      <CommentItem state={state} taskId={task.id} comment={thread.root} mentionTokens={mentionTokens} />
      {thread.replies.map((reply) => (
        <CommentItem key={reply.id} state={state} taskId={task.id} comment={reply} mentionTokens={mentionTokens} reply />
      ))}
      <div className="tasks-thread-composer">
        <MessageInput
          state={state}
          placeholder="Leave a reply…"
          showThreadAction={false}
          onSend={(content, attachments) => {
            addTaskComment(task.id, content, thread.root.id, attachments)
          }}
        />
      </div>
    </div>
  )

  return (
    <div className="tasks-activity">
      <h3 className="tasks-activity-heading">Activity</h3>
      {feed.map((entry, index) =>
        entry.kind === 'activity' ? renderTimeline(entry.items, `timeline-${index}`) : renderThread(entry.thread),
      )}
    </div>
  )
}
