import type { QueryClient } from '@tanstack/react-query'
import type { PageTaskRecord, TaskRecord } from '@/api/generated/types.gen'
import { queryKeys } from '@/api/queryKeys'

export interface WorkspaceTaskSnapshot {
  entries: Array<[readonly unknown[], unknown]>
}

/** A row of a cached relations list (`TaskRelationRecord`): the other task as a short `RelatedTask`. */
function isRelationOf(item: unknown, taskId: string): item is { task: Record<string, unknown> } {
  if (!item || typeof item !== 'object' || !('task' in item)) return false
  const related = item.task
  return related !== null && typeof related === 'object' && 'id' in related && related.id === taskId && 'status_id' in related
}

function mapTaskData(data: unknown, taskId: string, replace: (task: TaskRecord) => TaskRecord): unknown {
  if (!data || typeof data !== 'object') return data
  // a relations list: the related task keeps its own fields (status, title, version…) and takes their new values
  if (Array.isArray(data)) {
    return data.map((item) => {
      if (!isRelationOf(item, taskId)) return item
      const next = replace(item.task as TaskRecord) as Record<string, unknown>
      return { ...item, task: Object.fromEntries(Object.keys(item.task).map((key) => [key, next[key]])) }
    })
  }
  if ('id' in data && data.id === taskId && 'workspace_id' in data) return replace(data as TaskRecord)
  if ('items' in data && Array.isArray(data.items)) {
    return { ...data, items: data.items.map((item) => mapTaskData(item, taskId, replace)) }
  }
  if ('pages' in data && Array.isArray(data.pages)) {
    return { ...data, pages: data.pages.map((page) => mapTaskData(page, taskId, replace)) }
  }
  return data
}

/** Patches every cached copy of a task: detail, lists, sub-issues, and the task inside other tasks' relations lists. */
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
 * Replaces the cached copies of a task (relation rows included) with a server record. Create/update/bulk responses
 * carry no `ancestors` (GET /tasks/{id} only), so a cached breadcrumb is kept until the refetch that follows every
 * write replaces it.
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

/** Every task record in a cached value: a record, a page (`items`) or an infinite query (`pages`), as `mapTaskData`. */
function* taskRecordsIn(data: unknown): Generator<TaskRecord> {
  if (!data || typeof data !== 'object') return
  if ('id' in data && 'workspace_id' in data) {
    yield data as TaskRecord
    return
  }
  const children = 'items' in data && Array.isArray(data.items) ? data.items : 'pages' in data && Array.isArray(data.pages) ? data.pages : []
  for (const child of children) yield* taskRecordsIn(child)
}

function findTaskData(data: unknown, taskId: string): TaskRecord | undefined {
  for (const record of taskRecordsIn(data)) if (record.id === taskId) return record
  return undefined
}

/**
 * The records of `taskIds` as they were before an optimistic patch (one per task, from the snapshot `onMutate`
 * took): a write that clears `parent_task_id` still knows the old parent, so an auto-closed one reads as a parent.
 */
export function snapshotTasks(snapshot: WorkspaceTaskSnapshot | undefined, taskIds: readonly string[]): TaskRecord[] {
  const found: TaskRecord[] = []
  for (const taskId of taskIds) {
    for (const [, data] of snapshot?.entries ?? []) {
      const record = findTaskData(data, taskId)
      if (record) {
        found.push(record)
        break
      }
    }
  }
  return found
}

/** The first cached record of a task in any task query of the workspace (list, board, detail, sub-issues). */
export function findCachedTask(queryClient: QueryClient, workspaceId: string, taskId: string): TaskRecord | undefined {
  for (const [, data] of queryClient.getQueriesData({ queryKey: queryKeys.tasks.all(workspaceId) })) {
    const found = findTaskData(data, taskId)
    if (found) return found
  }
  return undefined
}

/**
 * Every descendant of a task that any cached query knows about (lists, boards, the detail's sub-issue levels): the
 * "Set parent" picker must not offer them. Deeper, unloaded ones are refused by the server (`parent_cycle`).
 */
export function cachedDescendantIds(queryClient: QueryClient, workspaceId: string, taskId: string): string[] {
  const records = queryClient.getQueriesData({ queryKey: queryKeys.tasks.all(workspaceId) }).flatMap(([, data]) => [...taskRecordsIn(data)])
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
