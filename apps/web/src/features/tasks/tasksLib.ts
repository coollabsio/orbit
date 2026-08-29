import type { Task, TaskComment, TaskStatus } from '../../mock/types'
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
