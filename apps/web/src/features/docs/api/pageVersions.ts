import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { apiClient, type createApiClient } from '@/api/client'
import { getPageVersion, listPageVersions, restorePageVersion } from '@/api/generated/sdk.gen'
import type { Page, PageVersion, PageVersionList } from '@/api/generated/types.gen'
import { queryKeys } from '@/api/queryKeys'
import { reconcilePage } from './pages'

type ApiClient = ReturnType<typeof createApiClient>

/** Versions per request; older ones load on demand. */
export const VERSION_PAGE_SIZE = 50

function required<T>(data: T | undefined, message: string): T {
  if (data === undefined) throw new Error(message)
  return data
}

export async function fetchPageVersions(
  client: ApiClient,
  workspaceId: string,
  pageId: string,
  cursor?: string,
): Promise<PageVersionList> {
  const { data } = await listPageVersions({
    client,
    path: { workspace_id: workspaceId, page_id: pageId },
    query: { limit: VERSION_PAGE_SIZE, ...(cursor ? { cursor } : {}) },
    throwOnError: true,
  })
  return required(data, 'Page versions response was empty.')
}

export async function fetchPageVersion(client: ApiClient, workspaceId: string, pageId: string, versionId: string): Promise<PageVersion> {
  const { data } = await getPageVersion({
    client,
    path: { workspace_id: workspaceId, page_id: pageId, version_id: versionId },
    throwOnError: true,
  })
  return required(data, 'Page version response was empty.')
}

/** Puts a version back on the page; resolves to the page as saved (409 when `expectedVersion` is stale). */
export async function restorePageVersionRequest(
  client: ApiClient,
  workspaceId: string,
  pageId: string,
  versionId: string,
  expectedVersion: number,
): Promise<Page> {
  const { data } = await restorePageVersion({
    client,
    path: { workspace_id: workspaceId, page_id: pageId, version_id: versionId },
    body: { expected_version: expectedVersion },
    throwOnError: true,
  })
  return required(data, 'Restore version response was empty.')
}

/** The page's history, newest first, in cursor pages (`fetchNextPage` loads older versions). */
export function usePageVersions(workspaceId: string, pageId: string, enabled = true) {
  return useInfiniteQuery({
    queryKey: queryKeys.pages.versions(workspaceId, pageId),
    enabled,
    initialPageParam: undefined as string | undefined,
    queryFn: ({ pageParam }) => fetchPageVersions(apiClient, workspaceId, pageId, pageParam),
    getNextPageParam: (last) => last.next_cursor ?? undefined,
    select: (data) => data.pages.flatMap((page) => page.items),
  })
}

/** One version with its content. Versions never change, so it is fetched once. */
export function usePageVersion(workspaceId: string, pageId: string, versionId: string | null) {
  return useQuery({
    queryKey: queryKeys.pages.version(workspaceId, pageId, versionId ?? ''),
    enabled: Boolean(versionId),
    staleTime: Infinity,
    queryFn: () => fetchPageVersion(apiClient, workspaceId, pageId, versionId!),
  })
}

/**
 * Restores a version onto the page. On success the page caches hold the restored page and the history is refetched
 * (the restore stored the previous state). The open editor applies the returned page itself.
 */
export function useRestorePageVersion(workspaceId: string, pageId: string) {
  const queryClient = useQueryClient()
  return useMutation<Page, Error, { versionId: string; expectedVersion: number }>({
    mutationFn: ({ versionId, expectedVersion }) =>
      restorePageVersionRequest(apiClient, workspaceId, pageId, versionId, expectedVersion),
    onSuccess: (page) => {
      reconcilePage(queryClient, workspaceId, page)
      void queryClient.invalidateQueries({ queryKey: queryKeys.pages.versions(workspaceId, pageId), exact: true })
    },
  })
}
