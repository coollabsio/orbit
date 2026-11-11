/**
 * A task's Activity feed, as the web builds it (`api/models.ts` and `tasksLib.ts`): audit events become sentences,
 * one per changed field, and interleave with comment threads by time. No React.
 */
import type { TaskRecurrence } from '@orbit/api/generated/types.gen'
import { recurrenceChangeText } from '@orbit/api/recurrence'

import { shortDate } from './format'
import type { CommentRecord, TaskActivityEvent } from './orbit'
import { estimateLabel, PRIORITY_LABEL, type Priority } from './tasks'

interface Named {
  id: string
  name: string
}

export interface ActivityContext {
  statuses: readonly Named[]
  people: readonly Named[]
  labels: readonly Named[]
  projects: readonly Named[]
  /** The task's project's milestones, and its cycles named by `cycleLabel`. */
  milestones: readonly Named[]
  cycles: readonly Named[]
  /** `GEN-12` for a task the app knows, else a neutral word. */
  identifierOf: (taskId: string) => string
  locale?: string
}

export interface ActivityRow {
  id: string
  actorId: string
  /** A service account's name; people are named from `actorId`. */
  actorName?: string
  text: string
  /** A status change or an automatic close: the row shows this status's glyph instead of the actor's avatar. */
  statusId?: string
  createdAt: string
}

export interface CommentThread {
  root: CommentRecord
  replies: CommentRecord[]
}

export type FeedEntry = { kind: 'activity'; items: ActivityRow[] } | { kind: 'thread'; thread: CommentThread }

const RELATION_FALLBACK: Record<string, string> = {
  'task.marked_duplicate': 'marked as duplicate',
  'task.unmarked_duplicate': 'unmarked as duplicate',
  'task.relation_added': 'added relation',
  'task.relation_removed': 'removed relation',
}

const string = (value: unknown) => (typeof value === 'string' ? value : undefined)
const ids = (value: unknown) => (Array.isArray(value) ? value.filter((id): id is string => typeof id === 'string') : [])

function relationText(action: string, metadata: Record<string, unknown>, context: ActivityContext): string | null {
  const fallback = RELATION_FALLBACK[action]
  if (!fallback) return null
  const taskId = string(metadata.related_task_id)
  if (!taskId) return fallback
  const identifier = context.identifierOf(taskId)
  const title = string(metadata.related_task_title) ?? ''
  const incoming = metadata.direction === 'incoming'
  const blocks = metadata.type === 'blocks'
  switch (action) {
    case 'task.marked_duplicate':
      return incoming ? `marked ${identifier} as duplicate` : `marked as duplicate of ${identifier}${title ? ` · ${title}` : ''}`
    case 'task.unmarked_duplicate':
      return incoming ? `unmarked ${identifier} as duplicate` : fallback
    case 'task.relation_added':
      return blocks ? (incoming ? `added blocker ${identifier}` : `blocks ${identifier}`) : `added related ${identifier}`
    default:
      return blocks ? (incoming ? `removed blocker ${identifier}` : `no longer blocks ${identifier}`) : `removed related ${identifier}`
  }
}

function subIssueText(action: string, metadata: Record<string, unknown>, context: ActivityContext): { text: string; statusId?: string } | null {
  const task = (key: string) => {
    const taskId = string(metadata[key])
    return taskId ? context.identifierOf(taskId) : undefined
  }
  if (action === 'task.parent_changed') {
    const to = task('to')
    if (to) return { text: `set parent to ${to}` }
    const from = task('from')
    return { text: from ? `removed parent ${from}` : 'changed parent' }
  }
  if (action === 'task.auto_closed') {
    const statusId = string(metadata.to_status_id)
    const source = task('source_task_id')
    if (metadata.reason === 'parent_closed' && source) return { text: `closed automatically because ${source} was closed`, statusId }
    return { text: 'closed automatically because all sub-issues were done', statusId }
  }
  return null
}

/** One sentence per field a `task.updated` event changed; `null` for an event without the `changes` metadata. */
function changeRows(metadata: Record<string, unknown>, context: ActivityContext): { text: string; statusId?: string; field: string }[] | null {
  const changes = metadata.changes
  if (typeof changes !== 'object' || changes === null) return null
  const names = (list: string[], all: readonly Named[], unknown: string) => list.map((id) => all.find((item) => item.id === id)?.name ?? unknown).join(', ')
  const both = (added: string, removed: string) => [added, removed].filter(Boolean).join(' and ')
  return Object.entries(changes as Record<string, unknown>).flatMap(([field, value]) => {
    if (field === 'description') return [{ field, text: 'updated the description' }]
    if (field === 'source_url') return [{ field, text: 'updated the source link' }]
    if (field === 'recurrence') return [{ field, text: recurrenceChangeText(value as TaskRecurrence | null) }]
    if (typeof value !== 'object' || value === null) return []
    const change = value as Record<string, unknown>
    const from = string(change.from) ?? ''
    const to = string(change.to) ?? ''
    switch (field) {
      case 'status': {
        const fromName = context.statuses.find((status) => status.id === from)?.name
        const toName = context.statuses.find((status) => status.id === to)?.name ?? 'another status'
        return [{ field, statusId: to, text: fromName ? `changed status from ${fromName} to ${toName}` : `changed status to ${toName}` }]
      }
      case 'priority':
        return [{ field, text: to === 'none' ? 'removed priority' : `set priority to ${PRIORITY_LABEL[to as Priority] ?? to}` }]
      case 'title':
        return [{ field, text: `changed title from "${from}" to "${to}"` }]
      case 'project': {
        const project = (id: string) => context.projects.find((item) => item.id === id)?.name ?? 'another project'
        return [{ field, text: `moved from ${project(from)} to ${project(to)}` }]
      }
      case 'assignees': {
        const added = ids(change.added)
        const removed = ids(change.removed)
        return [{ field, text: both(added.length > 0 ? `assigned ${names(added, context.people, 'someone')}` : '', removed.length > 0 ? `unassigned ${names(removed, context.people, 'someone')}` : '') }]
      }
      case 'labels': {
        const added = ids(change.added)
        const removed = ids(change.removed)
        const label = (list: string[], verb: string) => (list.length > 0 ? `${verb} label${list.length > 1 ? 's' : ''} ${names(list, context.labels, 'a deleted label')}` : '')
        return [{ field, text: both(label(added, 'added'), label(removed, 'removed')) }]
      }
      case 'due': {
        const start = string(change.start)
        const end = string(change.end)
        if (!end) return [{ field, text: 'removed due date' }]
        return [{ field, text: `set due date to ${start ? `${shortDate(start, context.locale)} – ` : ''}${shortDate(end, context.locale)}` }]
      }
      case 'milestone': {
        if (!to) return [{ field, text: 'removed the milestone' }]
        return [{ field, text: `set milestone to ${context.milestones.find((item) => item.id === to)?.name ?? 'another milestone'}` }]
      }
      case 'cycle': {
        if (!to) return [{ field, text: 'removed the cycle' }]
        return [{ field, text: `moved to ${context.cycles.find((item) => item.id === to)?.name ?? 'another cycle'}` }]
      }
      case 'estimate':
        return [{ field, text: typeof change.to === 'number' ? `set estimate to ${estimateLabel(change.to)}` : 'removed the estimate' }]
      default:
        return []
    }
  })
}

const ACTIVITY_TEXT: Record<string, string> = {
  'backup.item_restored': 'restored from a backup',
}

/** Audit events to feed rows: a `task.updated` event gives one row per field, the rest one row each. */
export function activityRows(events: readonly TaskActivityEvent[], context: ActivityContext): ActivityRow[] {
  return events.flatMap((event) => {
    const metadata = (event.metadata ?? {}) as Record<string, unknown>
    const base = { id: event.id, actorId: event.actor_id ?? '', actorName: string(metadata.actor_service_account_name), createdAt: event.occurred_at }
    const changes = event.action === 'task.updated' ? changeRows(metadata, context) : null
    if (changes) return changes.map((change) => ({ ...base, id: `${event.id}:${change.field}`, text: change.text, statusId: change.statusId }))
    const related = relationText(event.action, metadata, context)
    const subIssue = related ? null : subIssueText(event.action, metadata, context)
    return [{ ...base, text: related ?? subIssue?.text ?? ACTIVITY_TEXT[event.action] ?? event.action.split('.').reverse().join(' '), statusId: subIssue?.statusId }]
  })
}

/** Rows and comment threads in time order; consecutive rows share one timeline block, as on the web. */
export function buildFeed(rows: readonly ActivityRow[], comments: readonly CommentRecord[]): FeedEntry[] {
  const byTime = (a: CommentRecord, b: CommentRecord) => a.created_at.localeCompare(b.created_at)
  const threads = comments
    .filter((comment) => !comment.parent_id)
    .sort(byTime)
    .map((root) => ({ root, replies: comments.filter((comment) => comment.parent_id === root.id).sort(byTime) }))
  const mixed = [
    ...rows.map((row) => ({ at: row.createdAt, row })),
    ...threads.map((thread) => ({ at: thread.root.created_at, thread })),
  ].sort((a, b) => a.at.localeCompare(b.at))
  const feed: FeedEntry[] = []
  for (const item of mixed) {
    if ('thread' in item) {
      feed.push({ kind: 'thread', thread: item.thread })
      continue
    }
    const last = feed.at(-1)
    if (last?.kind === 'activity') last.items.push(item.row)
    else feed.push({ kind: 'activity', items: [item.row] })
  }
  return feed
}

/** Rows a folded run keeps: the first one and the latest ones. */
const VISIBLE_ACTIVITY = 5

/** A long run folds to its first and latest rows; `hidden` is how many "Show N more" stands for. */
export function foldRun(run: readonly ActivityRow[]): { items: ActivityRow[]; hidden: number } {
  const hidden = run.length > VISIBLE_ACTIVITY + 1 ? run.length - VISIBLE_ACTIVITY : 0
  return { items: hidden === 0 ? [...run] : [run[0]!, ...run.slice(hidden + 1)], hidden }
}
