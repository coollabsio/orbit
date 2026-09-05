import type { StatusCategory, Task, TaskActivity, TaskComment, TaskStatusDef } from './api/models'
import { relativeTime } from '../../lib/format'
import { PRIORITY_ORDER, defaultStatusOf, sortStatuses, statusKeyOf } from '../../components/workspace/taskMeta'

export interface TaskFilterState {
  currentUserId: string
  projectId: string | null
  /** Status group key (see `statusKeyOf`), so "Todo" matches across projects. */
  statusKey: string | null
  assigneeId: string | null
  statuses: TaskStatusDef[]
}

export function filterTasks(tasks: Task[], f: TaskFilterState): Task[] {
  const keyById = new Map(f.statuses.map((s) => [s.id, statusKeyOf(s)]))
  return tasks.filter((t) => {
    if (f.projectId && t.projectId !== f.projectId) return false
    if (f.statusKey && keyById.get(t.statusId) !== f.statusKey) return false
    if (f.assigneeId && !t.assigneeIds.includes(f.assigneeId)) return false
    return true
  })
}

/** A status column/group: one status per project merged by key (same category + name). */
export interface StatusGroup {
  key: string
  name: string
  category: StatusCategory
  /** Representative definition (icon color). */
  status: TaskStatusDef
  statusIds: string[]
}

/** Groups for the given statuses (all projects or one), in workflow order, including empty ones. */
export function statusGroups(statuses: TaskStatusDef[], projectId: string | null): StatusGroup[] {
  const groups: StatusGroup[] = []
  for (const status of sortStatuses(projectId ? statuses.filter((s) => s.projectId === projectId) : statuses)) {
    const key = statusKeyOf(status)
    const existing = groups.find((g) => g.key === key)
    if (existing) existing.statusIds.push(status.id)
    else groups.push({ key, name: status.name, category: status.category, status, statusIds: [status.id] })
  }
  return groups
}

export interface TaskGroup extends StatusGroup {
  tasks: Task[]
}

/** Tasks bucketed into status groups (empty groups dropped), ordered by `sort` inside a group. */
export function groupTasksByStatus(tasks: Task[], groups: StatusGroup[], sort: SortKey = 'manual'): TaskGroup[] {
  return groups
    .map((group) => ({ ...group, tasks: sortTasks(tasks.filter((t) => group.statusIds.includes(t.statusId)), sort) }))
    .filter((group) => group.tasks.length > 0)
}

/** The status of `projectId` that belongs to a group key; falls back to the project's default status. */
export function resolveStatusId(statuses: TaskStatusDef[], projectId: string, key: string | null): string | undefined {
  const match = key ? statuses.find((s) => s.projectId === projectId && statusKeyOf(s) === key) : undefined
  return (match ?? defaultStatusOf(statuses, projectId))?.id
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

export type SortKey = 'manual' | 'priority' | 'created' | 'updated' | 'title'

export const SORT_OPTIONS: { key: SortKey; label: string }[] = [
  { key: 'manual', label: 'Manual' },
  { key: 'priority', label: 'Priority' },
  { key: 'created', label: 'Created' },
  { key: 'updated', label: 'Last updated' },
  { key: 'title', label: 'Title' },
]

/** Order inside a group/column. Manual = the position set by drag and drop. */
export function sortTasks(tasks: Task[], sort: SortKey): Task[] {
  const list = [...tasks]
  switch (sort) {
    case 'priority':
      return list.sort((a, b) => PRIORITY_ORDER.indexOf(a.priority) - PRIORITY_ORDER.indexOf(b.priority) || a.position - b.position)
    case 'created':
      return list.sort((a, b) => b.createdAt.localeCompare(a.createdAt))
    case 'updated':
      return list.sort((a, b) => b.updatedAt.localeCompare(a.updatedAt))
    case 'title':
      return list.sort((a, b) => (a.title || 'Untitled').localeCompare(b.title || 'Untitled'))
    default:
      return list.sort((a, b) => a.position - b.position)
  }
}
