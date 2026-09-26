import { useEffect } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { apiClient, type createApiClient } from '@/api/client'
import { listRecentPages, recordPageVisit, setPageLock } from '@/api/generated/sdk.gen'
import type { Page, RecentPage } from '@/api/generated/types.gen'
import { ApiProblem } from '@/api/problem'
import { queryKeys } from '@/api/queryKeys'
import { reconcilePage } from './pages'

type ApiClient = ReturnType<typeof createApiClient>

/** A page stays open this long before it counts as visited (skims through the tree do not). */
export const VISIT_AFTER_MS = 1500
export const RECENT_PALETTE_LIMIT = 6

export function isPageLocked(error: unknown): boolean {
  return error instanceof ApiProblem && error.status === 423
}

export async function lockPage(client: ApiClient, workspaceId: string, pageId: string, locked: boolean): Promise<Page> {
  const { data } = await setPageLock({ client, path: { workspace_id: workspaceId, page_id: pageId }, body: { locked }, throwOnError: true })
  if (!data) throw new Error('Lock response was empty.')
  return data
}

/**
 * Locks or unlocks a page (any member who can see it). The server then closes every co-editing socket of the page
 * with 4423, so open editors (this one too) reload the page and reconnect read-only or editable.
 */
export function useSetPageLock(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation<Page, Error, { pageId: string; locked: boolean }>({
    mutationFn: ({ pageId, locked }) => lockPage(apiClient, workspaceId, pageId, locked),
    onSuccess: (page) => reconcilePage(queryClient, workspaceId, page),
  })
}

export function recentPagesQueryOptions(workspaceId: string, limit = RECENT_PALETTE_LIMIT, client: ApiClient = apiClient) {
  return {
    queryKey: [...queryKeys.pages.recent(workspaceId), limit] as const,
    queryFn: async (): Promise<RecentPage[]> => {
      const { data } = await listRecentPages({ client, path: { workspace_id: workspaceId }, query: { limit }, throwOnError: true })
      if (!data) throw new Error('Recent pages response was empty.')
      return data.items
    },
  }
}

/** The caller's recently opened pages (live, visible ones only), newest first. */
export function useRecentPages(workspaceId: string, enabled = true, limit = RECENT_PALETTE_LIMIT) {
  return useQuery({ ...recentPagesQueryOptions(workspaceId, limit), enabled })
}

/**
 * Records a visit once the page has stayed open for {@link VISIT_AFTER_MS} (one per mount: background refetches and
 * realtime refreshes never count). Failures are ignored; the recent list is a convenience.
 */
export function usePageVisit(workspaceId: string, pageId: string, enabled = true) {
  const queryClient = useQueryClient()
  useEffect(() => {
    if (!enabled) return
    const timer = setTimeout(() => {
      recordPageVisit({ client: apiClient, path: { workspace_id: workspaceId, page_id: pageId }, throwOnError: true })
        .then(() => queryClient.invalidateQueries({ queryKey: queryKeys.pages.recent(workspaceId) }))
        .catch(() => {})
    }, VISIT_AFTER_MS)
    return () => clearTimeout(timer)
  }, [workspaceId, pageId, enabled, queryClient])
}
