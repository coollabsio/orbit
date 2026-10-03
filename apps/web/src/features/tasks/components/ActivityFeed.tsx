import { Fragment, useMemo, useState, type ReactNode } from 'react'
import { ChevronDown } from 'reicon-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { UserAvatar } from '@/components/common/UserAvatar'
import { TaskStatusIcon } from './TaskStatusIcon'
import type { Task, TaskActivity, TaskViewState } from '@/features/tasks/api/models'
import { useCreateTaskComment } from '@/features/tasks/api/tasks'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { buildMentionTokens } from '@/lib/mentions'
import { activityChangeText, agoLabel, buildFeed, type CommentThread } from '@/features/tasks/tasksLib'
import { CommentItem } from './CommentItem'
import { TaskCommentComposer } from './TaskCommentComposer'
import { ProfileTrigger } from '@/components/common/ProfileTrigger'

interface ActivityFeedProps {
  task: Task
  state: TaskViewState
  /** Opens a task named in an activity entry (relation events). */
  onOpenTask?: (taskId: string) => void
}

/**
 * Chronological feed: activity events (timeline rows) and comment threads (cards) interleaved by time,
 * so a change made after a comment shows below that comment.
 */
/** Activity sentence; the related task's identifier becomes a link when the feed can open tasks. */
function ActivityText({ item, state, onOpenTask }: { item: TaskActivity; state: TaskViewState; onOpenTask?: (taskId: string) => void }) {
  if (item.change) return <>{activityChangeText(item.change, state)}</>
  const related = item.related
  const at = related ? item.text.indexOf(related.identifier) : -1
  if (!related || at === -1 || !onOpenTask) return <>{item.text}</>
  return (
    <>
      {item.text.slice(0, at)}
      <Button type="button" variant="link" className="h-auto p-0 text-xs text-foreground" onClick={() => onOpenTask(related.taskId)}>
        {related.identifier}
      </Button>
      {item.text.slice(at + related.identifier.length)}
    </>
  )
}

/** A timeline row: the connector line runs above and below its glyph; the first and last rows drop their outer half. */
function TimelineRow({ children }: { children: ReactNode }) {
  return (
    <li className="relative flex min-w-0 items-center gap-2.5 py-[7px] pl-1.5 text-xs text-muted-foreground before:absolute before:top-0 before:left-[12.5px] before:h-[calc(50%-8px)] before:w-px before:bg-border after:absolute after:top-[calc(50%+8px)] after:bottom-0 after:left-[12.5px] after:w-px after:bg-border first:before:hidden last:after:hidden">
      {children}
    </li>
  )
}

/** Rows a folded run of activity keeps: the first one and the latest ones. */
const VISIBLE_ACTIVITY = 5

export function ActivityFeed({ task, state, onOpenTask }: ActivityFeedProps) {
  // the unfolded runs, by the id of their first row
  const [unfolded, setUnfolded] = useState<string[]>([])
  const { workspace } = useWorkspace()
  const createComment = useCreateTaskComment(workspace.id, task.id)
  const mentionTokens = useMemo(() => buildMentionTokens(state.users, []), [state.users])
  const userById = (id: string) => state.users.find((u) => u.id === id)
  const feed = buildFeed(task)

  // A run of activity rows between two comments folds on its own: its first and latest rows stay, and
  // "Show N more" takes the place of the rows between them. Comments always show.
  const renderTimeline = (run: TaskActivity[]) => {
    const runId = run[0]!.id
    const expanded = unfolded.includes(runId)
    const hiddenCount = run.length > VISIBLE_ACTIVITY + 1 ? run.length - VISIBLE_ACTIVITY : 0
    const items = expanded || hiddenCount === 0 ? run : [run[0]!, ...run.slice(hiddenCount + 1)]
    return (
    <ol key={runId} className="m-0 flex list-none flex-col p-0">
      {items.map((item) => {
        const actor = userById(item.actorId)
        const status = item.statusId ? state.statuses.find((s) => s.id === item.statusId) : undefined
        return (
          <Fragment key={item.id}>
            <TimelineRow>
              <span className="inline-flex size-3.5 shrink-0 items-center justify-center">
                {status ? (
                  <TaskStatusIcon status={status} size={14} />
                ) : (
                  <ProfileTrigger userId={actor?.id} name={actor?.name ?? 'Someone'} kind="avatar" tabIndex={-1} className="relative z-1 flex">
                    <UserAvatar user={actor} size={14} />
                  </ProfileTrigger>
                )}
              </span>
              <span className="truncate">
                <ProfileTrigger userId={actor?.id} name={actor?.name ?? item.actorName ?? 'Someone'} className="font-medium text-foreground">
                  {item.actorName ?? actor?.name ?? 'Someone'}
                </ProfileTrigger>{' '}
                <ActivityText item={item} state={state} onOpenTask={onOpenTask} /> · {agoLabel(item.createdAt)}
              </span>
            </TimelineRow>
            {hiddenCount > 0 && item.id === runId ? (
              <TimelineRow>
                <Button variant="ghost" size="xs" className="-ml-1 gap-2 px-1 text-muted-foreground" aria-expanded={expanded} onClick={() => setUnfolded((current) => expanded ? current.filter((id) => id !== runId) : [...current, runId])}>
                  <ChevronDown className={cn('size-3.5 transition-transform', expanded && 'rotate-180')} />
                  {expanded ? 'Show less' : `Show ${hiddenCount} more`}
                </Button>
              </TimelineRow>
            ) : null}
          </Fragment>
        )
      })}
    </ol>
    )
  }

  const renderThread = (thread: CommentThread) => (
    <div key={thread.root.id} className="my-3 overflow-hidden rounded-[10px] border bg-card">
      <CommentItem state={state} taskId={task.id} comment={thread.root} mentionTokens={mentionTokens} />
      {thread.replies.map((reply) => (
        <CommentItem key={reply.id} state={state} taskId={task.id} comment={reply} mentionTokens={mentionTokens} reply />
      ))}
      <div className="border-t py-1.5 pr-2 pl-3.5">
        <TaskCommentComposer compact placeholder="Leave a reply…" pending={createComment.isPending} progress={createComment.progress} error={createComment.isError ? `${createComment.remainingCount || 'Reply'} upload failed.` : undefined} onSend={(body, files) => createComment.mutateAsync({ body, files, parentId: thread.root.id })} />
      </div>
    </div>
  )

  return (
    <div>
      <div className="mt-5 mb-2 flex items-center gap-1 border-t pt-3.5 max-[899px]:mt-0 max-[899px]:border-t-0 max-[899px]:pt-0">
        <h3 className="m-0 text-sm font-semibold text-foreground">Activity</h3>
      </div>
      {feed.map((entry) =>
        entry.kind === 'activity' ? renderTimeline(entry.items) : renderThread(entry.thread),
      )}
    </div>
  )
}
