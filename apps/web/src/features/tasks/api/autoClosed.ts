import type { QueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'
import type { ProjectRecord, TaskRecord } from '@/api/generated/types.gen'
import { queryKeys } from '@/api/queryKeys'
import { refIdentifier, taskIdentifier } from './models'
import { findCachedTask } from './optimistic'
import { keepIdentifiersTogether } from '@/lib/toast'

/** A task the server closed by automation in the same request (`auto_closed` on PATCH and bulk responses). */
export type AutoClosedTask = { id: string; status_id: string }

type ParentWire = { parent_task_id?: string | null; parent?: { id: string; project_key: string; number?: number } | null; ancestors?: Array<{ id: string }> | null }

const isAutoClosed = (item: unknown): item is AutoClosedTask =>
  typeof item === 'object' && item !== null
  && typeof (item as { id?: unknown }).id === 'string' && typeof (item as { status_id?: unknown }).status_id === 'string'

/** Read structurally, so any response shape works: no `auto_closed` array means nothing was closed. */
export function autoClosedOf(response: unknown): AutoClosedTask[] {
  if (!response || typeof response !== 'object') return []
  const items = (response as { auto_closed?: unknown }).auto_closed
  return Array.isArray(items) ? items.filter(isAutoClosed) : []
}

/** "Closed parent ORB-12", "Closed 3 sub-issues", "Closed 2 sub-issues and parent ORB-12", "Closed 2 parent tasks". */
export function autoClosedMessage({ items, parentIds, identifierOf }: {
  items: AutoClosedTask[]; parentIds: ReadonlySet<string>; identifierOf: (taskId: string) => string
}): string | null {
  if (items.length === 0) return null
  const parents = items.filter((item) => parentIds.has(item.id))
  const subs = items.length - parents.length
  const subText = `${subs} sub-issue${subs === 1 ? '' : 's'}`
  const parentText = parents.length === 1 ? `parent ${identifierOf(parents[0]!.id)}` : `${parents.length} parent tasks`
  if (parents.length === 0) return `Closed ${subText}`
  if (subs === 0) return `Closed ${parentText}`
  return `Closed ${subText} and ${parentText}`
}

/**
 * Toast for tasks the server closed by automation in the same request (spec §7.8). A closed task above one of
 * `changed` (its parent chain, from the records and the cache) is a parent; anything else is a sub-issue.
 */
export function announceAutoClosed(queryClient: QueryClient, workspaceId: string, changed: TaskRecord[], response: unknown): void {
  const items = autoClosedOf(response)
  if (items.length === 0) return
  const projects = queryClient.getQueryData<ProjectRecord[]>(queryKeys.projects(workspaceId)) ?? []
  const parentIds = new Set<string>()
  const embedded = new Map<string, string>()
  for (const record of changed as Array<TaskRecord & ParentWire>) {
    if (record.parent) embedded.set(record.parent.id, refIdentifier({ id: record.parent.id, projectKey: record.parent.project_key, number: record.parent.number }))
    for (const ancestor of record.ancestors ?? []) parentIds.add(ancestor.id)
    let id = record.parent_task_id ?? null
    while (id && !parentIds.has(id)) {
      parentIds.add(id)
      id = (findCachedTask(queryClient, workspaceId, id) as (TaskRecord & ParentWire) | undefined)?.parent_task_id ?? null
    }
  }
  const identifierOf = (taskId: string) => {
    const cached = findCachedTask(queryClient, workspaceId, taskId)
    return embedded.get(taskId) ?? taskIdentifier(taskId, projects.find((project) => project.id === cached?.project_id), cached?.number)
  }
  const message = autoClosedMessage({ items, parentIds, identifierOf })
  if (message) toast.success(keepIdentifiersTogether(message))
}
