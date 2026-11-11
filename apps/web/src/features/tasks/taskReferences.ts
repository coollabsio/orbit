// Finding a task to point at: the "@" menus of chat and the editors, and the command palette. One server search (title,
// description, identifier `ENG-12` and bare number), with each hit's project and status for the row.
import { useContext, useMemo } from 'react'
import { keepPreviousData, QueryClientContext, useQuery, useQueryClient, type QueryClient } from '@tanstack/react-query'
import { apiClient } from '@/api/client'
import { queryKeys } from '@/api/queryKeys'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import { getTask } from '@orbit/api/generated/sdk.gen'
import type { TaskRecord } from '@orbit/api/generated/types.gen'
import { parseTaskIdentifier, taskPath } from '@/lib/taskLinks'
import { taskIdentifier, type TaskStatusDef } from './api/models'
import { projectsQuery, statusesQuery } from './api/projects'
import { taskListPage } from './api/tasks'

/** A task as a reference row shows it. */
export interface TaskReference {
  id: string
  identifier: string
  title: string
  projectName: string
  /** Undefined while the project's statuses are unknown. */
  status: TaskStatusDef | undefined
  /** In-app path (`/tasks/ENG-12`). */
  path: string
}

/** Hits for one search: enough to pick from, few enough to scan. */
export const TASK_REFERENCE_LIMIT = 8
/** Projects and statuses change rarely: a search reuses what the app already loaded. */
const REUSE_MS = 60_000

/** `ENG-12`, `eng-`, `#12` or `12`: the user is typing an identifier, so tasks lead the results. */
export function looksLikeTaskId(query: string): boolean {
  const text = query.trim()
  return /^[A-Za-z0-9_]{1,20}-\d*$/.test(text) && /[A-Za-z_]/.test(text) || /^#?\d+$/.test(text)
}

/**
 * Best hit first: the exact identifier, then an identifier that starts with the query (`ENG-1` → `ENG-12`), then titles
 * that start with it, then titles that contain it; the server's order otherwise (description hits last).
 */
export function rankTaskReferences<T extends Pick<TaskReference, 'identifier' | 'title'>>(references: readonly T[], query: string): T[] {
  const needle = query.trim().toLowerCase().replace(/^#/, '')
  if (!needle) return [...references]
  const rank = (reference: T) => {
    const id = reference.identifier.toLowerCase()
    const title = reference.title.toLowerCase()
    if (id === needle || /^\d+$/.test(needle) && id.endsWith(`-${needle}`)) return 0
    if (looksLikeTaskId(needle) && id.startsWith(needle)) return 1
    if (title.startsWith(needle)) return 2
    if (title.includes(needle)) return 3
    return 4
  }
  return references
    .map((reference, index) => ({ reference, index, rank: rank(reference) }))
    .sort((a, b) => a.rank - b.rank || a.index - b.index)
    .map(({ reference }) => reference)
}

function searchKey(workspaceId: string, search: string) {
  return queryKeys.tasks.list(workspaceId, { search, limit: TASK_REFERENCE_LIMIT, reference: true })
}

/** The tasks that match `query`, ranked; the recently updated ones for an empty query. Usable outside React (editor menus). */
export async function searchTaskReferences(queryClient: QueryClient, workspaceId: string, query: string): Promise<TaskReference[]> {
  const search = query.trim()
  return queryClient.fetchQuery({ queryKey: searchKey(workspaceId, search), queryFn: () => loadReferences(queryClient, workspaceId, search), staleTime: 15_000 })
}

async function loadReferences(queryClient: QueryClient, workspaceId: string, search: string): Promise<TaskReference[]> {
  const [page, projects, exact] = await Promise.all([
    taskListPage(apiClient, workspaceId, { search: search || undefined, limit: TASK_REFERENCE_LIMIT, sort: 'updated_at', order: 'desc' }),
    queryClient.fetchQuery({ ...projectsQuery(workspaceId), staleTime: REUSE_MS }),
    exactTask(workspaceId, search),
  ])
  // The list holds the latest hits only, which may leave out the task the identifier names: that one always comes.
  const items = exact && !page.items.some((task) => task.id === exact.id) ? [exact, ...page.items.slice(0, TASK_REFERENCE_LIMIT - 1)] : page.items
  const projectIds = [...new Set(items.map((task) => task.project_id))]
  // A failed status load only costs the icon.
  const statusLists = await Promise.all(projectIds.map((id) => queryClient.fetchQuery({ ...statusesQuery(workspaceId, id), staleTime: REUSE_MS }).catch(() => [])))
  const statuses = statusLists.flat()
  const references = items.map((task) => {
    const project = projects.find((candidate) => candidate.id === task.project_id)
    return {
      id: task.id,
      identifier: taskIdentifier(task.id, project, task.number),
      title: task.title,
      projectName: project?.name ?? '',
      status: statuses.find((status) => status.id === task.status_id),
      path: taskPath({ id: task.id, number: task.number, projectKey: project?.key }),
    }
  })
  return rankTaskReferences(references, search)
}

/** The task a full identifier (`ENG-12`) names, or null (not an identifier, no such task, or not visible). */
async function exactTask(workspaceId: string, search: string): Promise<TaskRecord | null> {
  const parsed = parseTaskIdentifier(search)
  if (!parsed) return null
  const { data } = await getTask({ client: apiClient, path: { workspace_id: workspaceId, task_id: `${parsed.key}-${parsed.number}` } }).catch(() => ({ data: undefined }))
  return data && !data.deleted_at ? data : null
}

/**
 * `searchTaskReferences` as a hook (an empty query lists recently updated tasks): keeps the last hits while the next
 * search loads, so the list does not flicker.
 */
export function useTaskReferences(workspaceId: string, query: string, enabled = true) {
  const queryClient = useQueryClient()
  const search = query.trim()
  return useQuery({
    queryKey: searchKey(workspaceId, search),
    queryFn: () => loadReferences(queryClient, workspaceId, search),
    enabled,
    staleTime: 15_000,
    placeholderData: keepPreviousData,
  })
}

/** Searches tasks from code outside React (editor menus); null outside a workspace or a query client (tests, embeds). */
export type TaskReferenceSearch = (query: string) => Promise<TaskReference[]>

export function useTaskReferenceSearch(): TaskReferenceSearch | null {
  const queryClient = useContext(QueryClientContext)
  const workspaceId = useContext(WorkspaceContext)?.workspace.id
  return useMemo(() => (queryClient && workspaceId ? (query: string) => searchTaskReferences(queryClient, workspaceId, query) : null), [queryClient, workspaceId])
}
