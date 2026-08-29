import type { Task, TaskActivity, TaskComment, TaskStatus } from '../../mock/types'
import { relativeTime } from '../../lib/format'
import { STATUS_ORDER } from '../../components/workspace/taskMeta'

export interface TaskFilterState {
  tab: 'my' | 'all'
  currentUserId: string
  projectId: string | null
  status: TaskStatus | null
  assigneeId: string | null
}

export function filterTasks(tasks: Task[], f: TaskFilterState): Task[] {
  return tasks.filter((t) => {
    if (f.tab === 'my' && !t.assigneeIds.includes(f.currentUserId)) return false
    if (f.projectId && t.projectId !== f.projectId) return false
    if (f.status && t.status !== f.status) return false
    if (f.assigneeId && !t.assigneeIds.includes(f.assigneeId)) return false
    return true
  })
}

export interface StatusGroup {
  status: TaskStatus
  tasks: Task[]
}

export function groupTasksByStatus(tasks: Task[]): StatusGroup[] {
  return STATUS_ORDER.map((status) => ({
    status,
    tasks: tasks
      .filter((t) => t.status === status)
      .sort((a, b) => b.updatedAt.localeCompare(a.updatedAt)),
  })).filter((group) => group.tasks.length > 0)
}

export interface CommentThread {
  root: TaskComment
  replies: TaskComment[]
}

/** Top-level comments (oldest first) with their replies. */
export function commentThreads(task: Task): CommentThread[] {
  const byTime = (a: TaskComment, b: TaskComment) => a.createdAt.localeCompare(b.createdAt)
  return task.comments
    .filter((c) => !c.parentId)
    .sort(byTime)
    .map((root) => ({ root, replies: task.comments.filter((c) => c.parentId === root.id).sort(byTime) }))
}

/** "2d ago" style label; falls back to a short date for older items. */
export function agoLabel(iso: string): string {
  const label = relativeTime(iso)
  if (label === 'now') return 'just now'
  return /^\d+[mhd]$/.test(label) ? `${label} ago` : label
}

export type FeedEntry = { kind: 'activity'; at: string; items: TaskActivity[] } | { kind: 'thread'; at: string; thread: CommentThread }

/** Activity events and comment threads in one chronological list; consecutive events share a timeline block. */
export function buildFeed(task: Task): FeedEntry[] {
  const events = [...task.activity].sort((a, b) => a.createdAt.localeCompare(b.createdAt))
  const threads = commentThreads(task)
  const mixed: Array<{ at: string; event?: TaskActivity; thread?: CommentThread }> = [
    ...events.map((event) => ({ at: event.createdAt, event })),
    ...threads.map((thread) => ({ at: thread.root.createdAt, thread })),
  ].sort((a, b) => a.at.localeCompare(b.at))

  const feed: FeedEntry[] = []
  for (const item of mixed) {
    if (item.thread) {
      feed.push({ kind: 'thread', at: item.at, thread: item.thread })
    } else if (item.event) {
      const last = feed[feed.length - 1]
      if (last && last.kind === 'activity') last.items.push(item.event)
      else feed.push({ kind: 'activity', at: item.at, items: [item.event] })
    }
  }
  return feed
}
