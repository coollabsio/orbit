/**
 * Time tracking on tasks: the labels, `/spend`, and the time entry calls. The web's `features/tasks/timeLib.ts` has
 * the same functions with the same rules; durations come from `@orbit/api/duration`.
 */
import { formatDuration, parseDuration } from '@orbit/api/duration'

import type { OrbitClient } from './orbit'

export interface TimeEntry {
  id: string
  workspace_id: string
  task_id: string
  project_id: string
  user_id: string
  minutes: number
  /** The local day the work was done: `YYYY-MM-DD`. */
  worked_on: string
  note: string | null
  billable: boolean
  created_by: string
  /** The comment it was logged with (`/spend`); null otherwise or once that comment is deleted. */
  comment_id: string | null
  version: number
  created_at: string
  updated_at: string
  can_edit: boolean
}

export interface TaskTime {
  /** Oldest first (by `worked_on`, then `created_at`). */
  items: TimeEntry[]
  total_minutes: number
  sub_issues_minutes: number
}

/** What the form sends. `user_id` is only sent by a member who may log time for others. */
export interface TimeEntryDraft {
  minutes: number
  worked_on: string
  note: string
  billable: boolean
  user_id?: string
}

const pad2 = (value: number) => String(value).padStart(2, '0')

/** The local day of `date` as `YYYY-MM-DD`. */
export const dayValue = (date: Date) => `${date.getFullYear()}-${pad2(date.getMonth() + 1)}-${pad2(date.getDate())}`

/** Today in the device's time zone, as `YYYY-MM-DD`. */
export const todayLocal = (now: Date = new Date()) => dayValue(now)

/** A `YYYY-MM-DD` day as a local date (`new Date('2026-11-10')` would be UTC midnight, the day before in America). */
export function dayDate(day: string): Date {
  const [year, month, date] = day.split('-').map(Number)
  return new Date(year!, month! - 1, date!)
}

/** `Nov 10`, with the year when it is not this year. */
export function workedOnLabel(day: string, now: Date = new Date()): string {
  const date = dayDate(day)
  return date.toLocaleDateString(undefined, { month: 'short', day: 'numeric', ...(date.getFullYear() === now.getFullYear() ? {} : { year: 'numeric' }) })
}

/** The Time property: `1h 30m`, and `+2h sub-issues` when the sub-issues logged time. */
export function timeTotalLabel(time: Pick<TaskTime, 'total_minutes' | 'sub_issues_minutes'>): { own: string; subIssues: string | null } {
  return {
    own: formatDuration(time.total_minutes),
    subIssues: time.sub_issues_minutes > 0 ? `+${formatDuration(time.sub_issues_minutes)} sub-issues` : null,
  }
}

// ---- calls (not in the generated SDK yet)

const JSON_HEADERS = { 'Content-Type': 'application/json' }

export async function taskTime(client: OrbitClient, workspaceId: string, taskId: string): Promise<TaskTime> {
  return (
    await client.get<{ 200: TaskTime }, unknown, true>({
      url: '/api/v1/workspaces/{workspace_id}/tasks/{task_id}/time-entries',
      path: { workspace_id: workspaceId, task_id: taskId },
      throwOnError: true,
    })
  ).data
}

export async function createTimeEntry(client: OrbitClient, workspaceId: string, taskId: string, draft: TimeEntryDraft): Promise<TimeEntry> {
  const note = draft.note.trim()
  return (
    await client.post<{ 201: TimeEntry }, unknown, true>({
      url: '/api/v1/workspaces/{workspace_id}/tasks/{task_id}/time-entries',
      path: { workspace_id: workspaceId, task_id: taskId },
      body: { minutes: draft.minutes, worked_on: draft.worked_on, billable: draft.billable, ...(note ? { note } : {}), ...(draft.user_id ? { user_id: draft.user_id } : {}) },
      headers: JSON_HEADERS,
      throwOnError: true,
    })
  ).data
}

export async function updateTimeEntry(client: OrbitClient, workspaceId: string, entry: Pick<TimeEntry, 'id' | 'version'>, draft: TimeEntryDraft): Promise<TimeEntry> {
  return (
    await client.patch<{ 200: TimeEntry }, unknown, true>({
      url: '/api/v1/workspaces/{workspace_id}/time-entries/{entry_id}',
      path: { workspace_id: workspaceId, entry_id: entry.id },
      body: {
        minutes: draft.minutes,
        worked_on: draft.worked_on,
        note: draft.note.trim(),
        billable: draft.billable,
        ...(draft.user_id ? { user_id: draft.user_id } : {}),
        expected_version: entry.version,
      },
      headers: JSON_HEADERS,
      throwOnError: true,
    })
  ).data
}

export async function deleteTimeEntry(client: OrbitClient, workspaceId: string, entryId: string): Promise<void> {
  await client.delete({ url: '/api/v1/workspaces/{workspace_id}/time-entries/{entry_id}', path: { workspace_id: workspaceId, entry_id: entryId }, throwOnError: true })
}

/** A `/spend 1h 30m` line of a comment, with an optional work day: `/spend 2h 2026-11-10`. */
const SPEND_LINE = /^\/spend\s+(.+?)(?:\s+(\d{4}-\d{2}-\d{2}))?\s*$/i

export interface Spend {
  minutes: number
  /** `YYYY-MM-DD`; absent for today. */
  workedOn?: string
}

/**
 * Takes the first `/spend …` line out of a Markdown comment (GitLab's quick action), as the web does with its blocks.
 * `spend` is null without such a line; a `/spend` line whose duration cannot be read throws.
 */
export function takeSpend(body: string): { body: string; spend: Spend | null } {
  const lines = body.split('\n')
  let fenced = false
  for (const [index, line] of lines.entries()) {
    // a line in a fenced code block only shows the syntax, as a web code block does
    if (/^\s*(```|~~~)/.test(line)) fenced = !fenced
    const match = fenced ? null : SPEND_LINE.exec(line.trim())
    if (!match) continue
    const minutes = parseDuration(match[1]!)
    if (minutes === null) throw new Error(`Could not read “${match[1]}” in /spend. Use 1h 30m, 90m or 1.5h, up to 24h.`)
    const rest = lines.filter((_, other) => other !== index).join('\n').trim()
    return { body: rest, spend: { minutes, ...(match[2] ? { workedOn: match[2] } : {}) } }
  }
  return { body, spend: null }
}
