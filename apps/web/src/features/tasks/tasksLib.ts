import type { StatusCategory, Task, TaskActivity, TaskChange, TaskComment, TaskPriority, TaskStatusDef, TaskViewState } from '@/features/tasks/api/models'
import { relativeTime, shortDate } from '@/lib/format'
import { defaultStatusOf, PRIORITY_LABEL, sortStatuses, statusKeyOf } from './taskMeta'

/** Local quick search (never saved): title, description or identifier contains the text. */
export function quickSearchTasks(tasks: Task[], search: string): Task[] {
  const needle = search.trim().toLowerCase()
  if (!needle) return tasks
  return tasks.filter((task) => [task.title, task.description, task.identifier].some((value) => value.toLowerCase().includes(needle)))
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

/** Activity sentence for one changed field, e.g. "Changed status from Backlog to Done". Deleted statuses, members and labels get a neutral word. */
export function activityChangeText(change: TaskChange, state: Pick<TaskViewState, 'users' | 'statuses' | 'labels'>): string {
  const names = (ids: string[], all: Array<{ id: string; name: string }>, unknown: string) =>
    ids.map((id) => all.find((item) => item.id === id)?.name ?? unknown).join(', ')
  // "Assigned Ada and unassigned Bo"
  const both = (added: string, removed: string) => [added, removed && (added ? removed.replace(/^./, (letter) => letter.toLowerCase()) : removed)].filter(Boolean).join(' and ')
  switch (change.field) {
    case 'status': {
      const from = state.statuses.find((status) => status.id === change.from)?.name
      const to = state.statuses.find((status) => status.id === change.to)?.name ?? 'another status'
      return from ? `Changed status from ${from} to ${to}` : `Changed status to ${to}`
    }
    case 'priority':
      return change.to === 'none' ? 'Removed priority' : `Set priority to ${PRIORITY_LABEL[change.to as TaskPriority] ?? change.to}`
    case 'assignees':
      return both(
        change.added.length > 0 ? `Assigned ${names(change.added, state.users, 'someone')}` : '',
        change.removed.length > 0 ? `Unassigned ${names(change.removed, state.users, 'someone')}` : '',
      )
    case 'labels':
      return both(
        change.added.length > 0 ? `Added label${change.added.length > 1 ? 's' : ''} ${names(change.added, state.labels, 'a deleted label')}` : '',
        change.removed.length > 0 ? `Removed label${change.removed.length > 1 ? 's' : ''} ${names(change.removed, state.labels, 'a deleted label')}` : '',
      )
    case 'due':
      if (!change.end) return 'Removed due date'
      return `Set due date to ${change.start ? `${shortDate(change.start)} – ` : ''}${shortDate(change.end)}`
    case 'title':
      return `Changed title from "${change.from}" to "${change.to}"`
    case 'project':
      return `Moved from ${change.from} to ${change.to}`
    case 'description':
      return 'Updated the description'
    case 'source_url':
      return 'Updated the source link'
  }
}
