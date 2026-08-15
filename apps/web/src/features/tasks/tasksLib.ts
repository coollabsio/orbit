import type { Task, TaskStatus } from '../../mock/types'
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
    if (f.tab === 'my' && t.assigneeId !== f.currentUserId) return false
    if (f.projectId && t.projectId !== f.projectId) return false
    if (f.status && t.status !== f.status) return false
    if (f.assigneeId && t.assigneeId !== f.assigneeId) return false
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

export interface FeedItem {
  kind: 'activity' | 'comment'
  id: string
  authorId: string
  text: string
  createdAt: string
}

export function buildFeed(task: Task): FeedItem[] {
  const items: FeedItem[] = [
    ...task.activity.map((a) => ({
      kind: 'activity' as const,
      id: a.id,
      authorId: a.actorId,
      text: a.text,
      createdAt: a.createdAt,
    })),
    ...task.comments.map((c) => ({
      kind: 'comment' as const,
      id: c.id,
      authorId: c.authorId,
      text: c.body,
      createdAt: c.createdAt,
    })),
  ]
  return items.sort((a, b) => a.createdAt.localeCompare(b.createdAt))
}
