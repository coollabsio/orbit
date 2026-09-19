import { useMemo, useState } from 'react'
import { ChevronDown } from 'lucide-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { UserAvatar } from '../../../components/ui/UserAvatar'
import { TaskStatusIcon } from '../../../components/workspace/TaskStatusIcon'
import type { Task, TaskActivity, TaskViewState } from '../api/models'
import { useCreateTaskComment } from '../api/tasks'
import { useWorkspace } from '../../workspaces/workspaceContext'
import { buildMentionTokens } from '../../chat/chatLib'
import { agoLabel, buildFeed, type CommentThread } from '../tasksLib'
import { CommentItem } from './CommentItem'
import { TaskCommentComposer } from './TaskCommentComposer'

interface ActivityFeedProps {
  task: Task
  state: TaskViewState
}

const TIMELINE_ITEM =
  "relative flex min-w-0 items-center gap-2.5 py-[7px] pl-1.5 text-xs text-muted-foreground before:absolute before:top-0 before:left-[12.5px] before:h-[calc(50%-8px)] before:w-px before:bg-border before:content-[''] after:absolute after:bottom-0 after:left-[12.5px] after:top-[calc(50%+8px)] after:w-px after:bg-border after:content-[''] first:before:hidden last:after:hidden"

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
    <ol key={key} className="m-0 flex list-none flex-col p-0">
      {items.map((item) => {
        const actor = userById(item.actorId)
        const status = item.statusId ? state.statuses.find((s) => s.id === item.statusId) : undefined
        return (
          <li key={item.id} className={TIMELINE_ITEM}>
            <span className="inline-flex size-3.5 shrink-0 items-center justify-center">
              {status ? <TaskStatusIcon status={status} size={14} /> : <UserAvatar user={actor} size={14} />}
            </span>
            <span className="truncate">
              <span className="font-medium text-foreground">{item.actorName ?? actor?.name ?? 'Someone'}</span> {item.text} · {agoLabel(item.createdAt)}
            </span>
          </li>
        )
      })}
    </ol>
  )

  const renderThread = (thread: CommentThread) => (
    <div key={thread.root.id} className="mt-3 overflow-hidden rounded-[10px] border border-border bg-card">
      <CommentItem state={state} taskId={task.id} comment={thread.root} mentionTokens={mentionTokens} />
      {thread.replies.map((reply) => (
        <CommentItem key={reply.id} state={state} taskId={task.id} comment={reply} mentionTokens={mentionTokens} reply />
      ))}
      <div className="border-t border-border px-3 pt-2.5 pb-3">
        <TaskCommentComposer compact placeholder="Leave a reply…" pending={createComment.isPending} progress={createComment.progress} error={createComment.isError ? `${createComment.remainingCount || 'Reply'} upload failed.` : undefined} onSend={(body, files) => createComment.mutateAsync({ body, files, parentId: thread.root.id })} />
      </div>
    </div>
  )

  return (
    <div>
      <div className="mt-5 mb-2 flex items-center gap-1 border-t border-border pt-3.5 max-[899px]:mt-0 max-[899px]:border-t-0 max-[899px]:pt-0">
        <h3 className="m-0 text-sm font-semibold text-foreground">Activity</h3>
        {hasMoreActivity ? (
          <Button
            variant="ghost"
            size="icon-sm"
            className="size-6 text-muted-foreground"
            aria-label={expanded ? 'Show fewer activities' : 'Show all activities'}
            aria-expanded={expanded}
            onClick={() => setExpanded((current) => !current)}
          >
            <ChevronDown className={cn('size-3.5 transition-transform', expanded && 'rotate-180')} />
          </Button>
        ) : null}
      </div>
      {feed.map((entry, index) =>
        entry.kind === 'activity' ? renderTimeline(entry.items, `timeline-${index}`) : renderThread(entry.thread),
      )}
    </div>
  )
}
