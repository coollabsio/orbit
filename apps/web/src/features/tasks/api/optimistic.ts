import type { QueryClient } from '@tanstack/react-query'
import type { PageTaskRecord, TaskRecord } from '@/api/generated/types.gen'
import { queryKeys } from '@/api/queryKeys'

export interface WorkspaceTaskSnapshot {
  entries: Array<[readonly unknown[], unknown]>
}

function mapTaskData(data: unknown, taskId: string, replace: (task: TaskRecord) => TaskRecord): unknown {
  if (!data || typeof data !== 'object') return data
  if ('id' in data && data.id === taskId && 'workspace_id' in data) return replace(data as TaskRecord)
  if ('items' in data && Array.isArray(data.items)) {
    return { ...data, items: data.items.map((item) => mapTaskData(item, taskId, replace)) }
  }
  if ('pages' in data && Array.isArray(data.pages)) {
    return { ...data, pages: data.pages.map((page) => mapTaskData(page, taskId, replace)) }
  }
  return data
}

export function patchWorkspaceTask(
  queryClient: QueryClient,
  workspaceId: string,
  taskId: string,
  patch: Partial<TaskRecord>,
): WorkspaceTaskSnapshot {
  const entries = queryClient.getQueriesData({ queryKey: queryKeys.tasks.all(workspaceId) })
  for (const [key, data] of entries) {
    queryClient.setQueryData(key, mapTaskData(data, taskId, (task) => ({ ...task, ...patch })))
  }
  return { entries }
}

export function restoreWorkspaceTasks(queryClient: QueryClient, snapshot: WorkspaceTaskSnapshot): void {
  for (const [key, data] of snapshot.entries) queryClient.setQueryData(key, data)
}

/**
 * Replaces the cached copies of a task with a server record. Create/update/bulk responses carry no `ancestors`
 * (GET /tasks/{id} only), so a cached breadcrumb is kept until the refetch that follows every write replaces it.
 */
export function reconcileWorkspaceTask(
  queryClient: QueryClient,
  workspaceId: string,
  record: TaskRecord,
): void {
  queryClient.setQueriesData(
    { queryKey: queryKeys.tasks.all(workspaceId) },
    (data) => mapTaskData(data, record.id, (cached) => withAncestors(record, cached)),
  )
}

type AncestorWire = { parent_task_id?: string | null; parent?: { id: string } | null; ancestors?: Array<{ id: string }> | null }

/** Read structurally: only GET /tasks/{id} sends `ancestors`. */
function withAncestors(record: TaskRecord, cached: TaskRecord): TaskRecord {
  const next = record as TaskRecord & AncestorWire
  const kept = (cached as TaskRecord & AncestorWire).ancestors
  if (next.ancestors != null || kept == null) return record
  // same parent: the chain above it is unchanged; re-parented: the new direct parent until the refetch brings the chain
  const sameParent = (kept.at(-1)?.id ?? null) === (next.parent_task_id ?? null)
  return { ...record, ancestors: sameParent ? kept : next.parent ? [next.parent] : [] } as TaskRecord
}

function findTaskData(data: unknown, taskId: string): TaskRecord | undefined {
  if (!data || typeof data !== 'object') return undefined
  if ('id' in data && data.id === taskId && 'workspace_id' in data) return data as TaskRecord
  const children = 'items' in data && Array.isArray(data.items) ? data.items : 'pages' in data && Array.isArray(data.pages) ? data.pages : []
  for (const child of children) {
    const found = findTaskData(child, taskId)
    if (found) return found
  }
  return undefined
}

/** The first cached record of a task in any task query of the workspace (list, board, detail, sub-issues). */
export function findCachedTask(queryClient: QueryClient, workspaceId: string, taskId: string): TaskRecord | undefined {
  for (const [, data] of queryClient.getQueriesData({ queryKey: queryKeys.tasks.all(workspaceId) })) {
    const found = findTaskData(data, taskId)
    if (found) return found
  }
  return undefined
}

function collectTaskData(data: unknown, into: TaskRecord[]): void {
  if (!data || typeof data !== 'object') return
  if ('id' in data && 'workspace_id' in data) {
    into.push(data as TaskRecord)
    return
  }
  const children = 'items' in data && Array.isArray(data.items) ? data.items : 'pages' in data && Array.isArray(data.pages) ? data.pages : []
  for (const child of children) collectTaskData(child, into)
}

/**
 * Every descendant of a task that any cached query knows about (lists, boards, the detail's sub-issue levels): the
 * "Set parent" picker must not offer them. Deeper, unloaded ones are refused by the server (`parent_cycle`).
 */
export function cachedDescendantIds(queryClient: QueryClient, workspaceId: string, taskId: string): string[] {
  const records: TaskRecord[] = []
  for (const [, data] of queryClient.getQueriesData({ queryKey: queryKeys.tasks.all(workspaceId) })) collectTaskData(data, records)
  const children = new Map<string, Set<string>>()
  for (const record of records) {
    const parentId = (record as TaskRecord & { parent_task_id?: string | null }).parent_task_id
    if (!parentId) continue
    if (!children.has(parentId)) children.set(parentId, new Set())
    children.get(parentId)!.add(record.id)
  }
  const found = new Set<string>()
  const queue = [taskId]
  while (queue.length > 0) {
    for (const child of children.get(queue.shift()!) ?? []) {
      if (child === taskId || found.has(child)) continue
      found.add(child)
      queue.push(child)
    }
  }
  return [...found]
}

export type { PageTaskRecord }
