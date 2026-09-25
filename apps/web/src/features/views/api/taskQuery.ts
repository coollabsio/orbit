import { useCallback } from 'react'
import { keepPreviousData, useQuery } from '@tanstack/react-query'
import { apiClient, type createApiClient } from '@/api/client'
import { queryTasks } from '@/api/generated/sdk.gen'
import type { PageTaskRecord, TaskQueryBody, TaskRecord } from '@/api/generated/types.gen'
import { fetchAllPages } from '@/api/pagination'
import { queryKeys } from '@/api/queryKeys'
import type { DisplayOptions, FilterGroup } from '@/features/views/viewState'

type ApiClient = ReturnType<typeof createApiClient>

/** The server clamps page sizes to 100. */
export const TASK_QUERY_PAGE_SIZE = 100
const NO_TASKS: TaskRecord[] = []

/** POST /tasks/query body: the effective filter plus the display options the server applies. */
export function taskQueryBody(filter: FilterGroup, display: DisplayOptions): TaskQueryBody {
  return {
    // The hand-written FilterGroup is the source of truth; the generated schema type describes the same JSON.
    filter: filter as unknown as TaskQueryBody['filter'],
    order_by: display.order_by,
    order_direction: display.order_direction,
    show_completed: display.show_completed,
  }
}

/** Every page of one query, in server order (grouping happens on the client). */
export async function fetchAllTaskQueryPages(client: ApiClient, workspaceId: string, body: TaskQueryBody): Promise<PageTaskRecord> {
  const page = await fetchAllPages(async (cursor) => {
    const { data } = await queryTasks({
      client,
      path: { workspace_id: workspaceId },
      body: { ...body, cursor, limit: TASK_QUERY_PAGE_SIZE },
      throwOnError: true,
    })
    if (!data) throw new Error('Task query response was empty.')
    return data
  })
  return { items: page.items, next_cursor: null }
}

export function taskQueryOptions(workspaceId: string, body: TaskQueryBody, client: ApiClient = apiClient) {
  return {
    queryKey: queryKeys.taskQuery(workspaceId, body),
    queryFn: () => fetchAllTaskQueryPages(client, workspaceId, body),
  }
}

/** Exhaustive task list for a view. `enabled: false` while the view state is still loading. */
export function useTaskQuery(
  workspaceId: string,
  filter: FilterGroup,
  display: DisplayOptions,
  enabled = true,
): { tasks: TaskRecord[]; isLoading: boolean; error: unknown; retry: () => void } {
  const query = useQuery({
    ...taskQueryOptions(workspaceId, taskQueryBody(filter, display)),
    enabled,
    placeholderData: keepPreviousData,
  })
  const { refetch } = query
  const retry = useCallback(() => { void refetch() }, [refetch])
  return { tasks: query.data?.items ?? NO_TASKS, isLoading: query.isPending, error: query.error, retry }
}
