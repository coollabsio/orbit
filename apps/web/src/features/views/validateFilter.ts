import { apiClient } from '@/api/client'
import { queryTasks } from '@/api/generated/sdk.gen'
import type { TaskQueryBody } from '@/api/generated/types.gen'
import { ApiProblem } from '@/api/problem'
import type { FilterIssue } from './filterTree'
import type { FilterGroup } from './viewState'

/** The `path` extension member of a 422 problem (Consolidated contract note 2). */
export function problemPath(error: unknown): string | null {
  if (!(error instanceof ApiProblem) || error.status !== 422) return null
  const path = (error.problem as ApiProblem['problem'] & { path?: unknown }).path
  return typeof path === 'string' ? path : null
}

/** Dry-runs the user's tree (no preset/project scope, so paths map 1:1 to the editor) with limit 1. */
export async function validateFilterOnServer(workspaceId: string, filter: FilterGroup): Promise<FilterIssue | null> {
  try {
    await queryTasks({
      client: apiClient,
      path: { workspace_id: workspaceId },
      body: { filter, order_by: 'manual', order_direction: 'asc', show_completed: 'all', limit: 1 } as unknown as TaskQueryBody,
      throwOnError: true,
    })
    return null
  } catch (error) {
    const path = problemPath(error)
    if (path === null) throw error
    return { path, message: (error as ApiProblem).detail }
  }
}
