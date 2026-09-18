import { useMemo, useState } from 'react'
import { ChevronDown } from 'reicon-react'
import { Avatar } from '../../../components/ui/Avatar'
import { TaskStatusIcon } from '../../../components/workspace/TaskStatusIcon'
import type { Task, TaskActivity, TaskViewState } from '../api/models'
import { useCreateTaskComment } from '../api/tasks'
import { useWorkspace } from '../../workspaces/workspaceContext'
import { buildMentionTokens } from '../../chat/chatLib'
import { agoLabel, buildFeed, type CommentThread } from '../tasksLib'
import { CommentItem } from './CommentItem'
import { TaskCommentComposer } from './TaskCommentComposer'
import { documentFromText } from '../api/richText'
import { asDocument } from '../../../components/editor/document'

interface ActivityFeedProps {
  task: Task
  state: TaskViewState
}

/**
 * Chronological feed: activity events (timeline rows) and comment threads (cards) interleaved by time,
 * so a change made after a comment shows below that comment.
 */
export function ActivityFeed({ task, state }: ActivityFeedProps) {
  const [expanded, setExpanded] = useState(false)
  const { workspace } = useWorkspace()
  const createComment = useCreateTaskComment(workspace.id, task.id)
  const mentionTokens = useMemo(() => buildMentionTokens(state.users, []), [state.users])
  const userById = (id: string) => state.users.find((u) => u.id === id)
  const hasMoreActivity = task.activity.length > 3
  const visibleActivity = expanded || !hasMoreActivity
    ? task.activity
    : [...task.activity]
        .sort((a, b) => b.createdAt.localeCompare(a.createdAt))
        .slice(0, 3)
  const feed = buildFeed({ ...task, activity: visibleActivity })

  const renderTimeline = (items: TaskActivity[], key: string) => (
    <ol key={key} className="tasks-timeline">
      {items.map((item) => {
        const actor = userById(item.actorId)
        const status = item.statusId ? state.statuses.find((s) => s.id === item.statusId) : undefined
        return (
          <li key={item.id} className="tasks-timeline-item">
            <span className="tasks-timeline-icon">
              {status ? <TaskStatusIcon status={status} size={14} /> : <Avatar user={actor} size={14} />}
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
        <TaskCommentComposer compact placeholder="Leave a reply…" pending={createComment.isPending} progress={createComment.progress} error={createComment.isError ? `${createComment.remainingCount || 'Reply'} upload failed.` : undefined} onSend={(body, files) => createComment.mutateAsync({ bodyJson: asDocument(documentFromText(body)), files, parentId: thread.root.id })} />
      </div>
    </div>
  )

  return (
    <div className="tasks-activity">
      <div className="tasks-activity-heading">
        <h3>Activity</h3>
        {hasMoreActivity ? (
          <button
            type="button"
            className="icon-button tasks-activity-toggle"
            aria-label={expanded ? 'Show fewer activities' : 'Show all activities'}
            aria-expanded={expanded}
            onClick={() => setExpanded((current) => !current)}
          >
            <ChevronDown size={14} />
          </button>
        ) : null}
      </div>
      {feed.map((entry, index) =>
        entry.kind === 'activity' ? renderTimeline(entry.items, `timeline-${index}`) : renderThread(entry.thread),
      )}
    </div>
  )
}
