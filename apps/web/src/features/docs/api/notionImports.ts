import { useEffect, useRef } from 'react'
import { useMutation, useQuery, useQueryClient, type QueryClient } from '@tanstack/react-query'
import { apiClient, type createApiClient } from '@/api/client'
import { cancelNotionImport, createNotionImport, getNotionImport, listNotionImports, startNotionImport } from '@/api/generated/sdk.gen'
import type {
  NotionImport,
  NotionImportDestinationBody,
  NotionImportSelectionBody,
  NotionImportStatus,
  NotionImportTree,
} from '@/api/generated/types.gen'
import { ApiProblem } from '@/api/problem'
import { queryKeys } from '@/api/queryKeys'

type ApiClient = ReturnType<typeof createApiClient>

/** How often an import that is still working is polled. */
export const NOTION_IMPORT_POLL_MS = 2000

/** A job is working on it: poll. */
export function isActiveImport(status: NotionImportStatus | undefined): boolean {
  return status === 'scanning' || status === 'queued' || status === 'importing'
}

/** Nothing will change any more. */
export function isFinalImport(status: NotionImportStatus | undefined): boolean {
  return status === 'completed' || status === 'failed' || status === 'cancelled' || status === 'expired'
}

export const NOTION_IMPORT_STATUS_LABELS: Record<NotionImportStatus, string> = {
  scanning: 'Scanning',
  ready: 'Ready to import',
  queued: 'Queued',
  importing: 'Importing',
  completed: 'Completed',
  failed: 'Failed',
  cancelled: 'Cancelled',
  expired: 'Expired',
}

/** A user-facing sentence for a failed Notion import request. */
export function notionImportErrorMessage(error: unknown): string {
  if (error instanceof ApiProblem) {
    switch (error.code) {
      case 'notion_token_invalid':
        return 'Notion rejected this token. Check that you copied the whole token and that it was not revoked.'
      case 'app_key_missing':
        return 'The server administrator must set an app key before anyone can import from Notion.'
      case 'import_in_progress':
        return 'You already have an import running. Wait for it to finish or cancel it.'
      case 'notion_unavailable':
        return 'Notion could not be reached. Try again in a moment.'
      case 'notion_import_too_large':
        return 'An import can contain at most 5,000 pages. Select fewer pages.'
      case 'notion_import_state_conflict':
        return 'This import changed in the meantime (it may have expired or been cancelled).'
      case 'notion_import_not_found':
        return 'This import no longer exists.'
      case 'page_not_found':
        return 'The destination page no longer exists. Choose another one.'
      case 'teamspace_not_found':
        return 'The destination teamspace no longer exists. Choose another one.'
    }
    if (error.status === 422) return 'The request was not valid. Check your input and try again.'
  }
  return 'Something went wrong. Try again.'
}

export function isImportInProgress(error: unknown): boolean {
  return error instanceof ApiProblem && error.code === 'import_in_progress'
}

/**
 * Sends the token once. Deliberately not a `useMutation`: mutation state keeps its variables in the query client,
 * and the token must not outlive the request on the client.
 */
export async function createNotionImportRequest(client: ApiClient, workspaceId: string, token: string): Promise<NotionImport> {
  const { data } = await createNotionImport({ client, path: { workspace_id: workspaceId }, body: { token }, throwOnError: true })
  if (!data) throw new Error('Create import response was empty.')
  return data
}

/** Stores a fresh import in the detail cache and refreshes the list. */
export function reconcileNotionImport(queryClient: QueryClient, workspaceId: string, item: NotionImport): void {
  queryClient.setQueryData(queryKeys.notionImports.detail(workspaceId, item.id), item)
  void queryClient.invalidateQueries({ queryKey: queryKeys.notionImports.list(workspaceId) })
}

export function useNotionImports(workspaceId: string) {
  return useQuery({
    queryKey: queryKeys.notionImports.list(workspaceId),
    queryFn: async () => {
      const { data } = await listNotionImports({ client: apiClient, path: { workspace_id: workspaceId }, throwOnError: true })
      if (!data) throw new Error('Imports response was empty.')
      return data.items
    },
    refetchInterval: (query) => (query.state.data?.some((item) => isActiveImport(item.status)) ? NOTION_IMPORT_POLL_MS * 2 : false),
  })
}

/**
 * One import without its scan tree (status, progress, report), polled every 2 s only while a job works on it
 * (scanning, queued, importing). When it reaches a final state the page queries are refreshed (imported pages
 * appear, trashed empty pages disappear) along with the imports list. The tree comes from `useNotionImportTree`.
 */
export function useNotionImport(workspaceId: string, importId: string | undefined, pollMs = NOTION_IMPORT_POLL_MS) {
  const queryClient = useQueryClient()
  const query = useQuery({
    queryKey: queryKeys.notionImports.detail(workspaceId, importId ?? ''),
    enabled: Boolean(importId),
    queryFn: async () => {
      const { data } = await getNotionImport({
        client: apiClient,
        path: { workspace_id: workspaceId, import_id: importId! },
        throwOnError: true,
      })
      if (!data) throw new Error('Import response was empty.')
      return data
    },
    refetchInterval: (current) => (isActiveImport(current.state.data?.status) ? pollMs : false),
  })
  const status = query.data?.status
  const previous = useRef(status)
  useEffect(() => {
    const before = previous.current
    previous.current = status
    if (before !== undefined && before !== status && isFinalImport(status)) {
      void queryClient.invalidateQueries({ queryKey: queryKeys.pages.all(workspaceId) })
      void queryClient.invalidateQueries({ queryKey: queryKeys.notionImports.list(workspaceId) })
    }
  }, [queryClient, status, workspaceId])
  return query
}

/**
 * The scan tree of a `ready` import (up to 5,000 nodes), fetched once with `?include=tree` under its own key and
 * never polled: a finished scan does not change.
 */
export function useNotionImportTree(workspaceId: string, importId: string, enabled = true) {
  return useQuery<NotionImportTree | null>({
    queryKey: queryKeys.notionImports.tree(workspaceId, importId),
    enabled,
    staleTime: Infinity,
    refetchOnWindowFocus: false,
    queryFn: async () => {
      const { data } = await getNotionImport({
        client: apiClient,
        path: { workspace_id: workspaceId, import_id: importId },
        query: { include: 'tree' },
        throwOnError: true,
      })
      if (!data) throw new Error('Import response was empty.')
      return data.tree
    },
  })
}

export function useStartNotionImport(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation<
    NotionImport,
    Error,
    { importId: string; selection: NotionImportSelectionBody; destination: NotionImportDestinationBody }
  >({
    mutationFn: async ({ importId, selection, destination }) => {
      const { data } = await startNotionImport({
        client: apiClient,
        path: { workspace_id: workspaceId, import_id: importId },
        body: { selection, destination },
        throwOnError: true,
      })
      if (!data) throw new Error('Start import response was empty.')
      return data
    },
    onSuccess: (item) => reconcileNotionImport(queryClient, workspaceId, item),
    onError: (error, { importId }) => {
      // Expired or cancelled elsewhere: show the server's state.
      if (error instanceof ApiProblem && error.status === 409) {
        void queryClient.invalidateQueries({ queryKey: queryKeys.notionImports.detail(workspaceId, importId) })
      }
    },
  })
}

export function useCancelNotionImport(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation<NotionImport, Error, { importId: string }>({
    mutationFn: async ({ importId }) => {
      const { data } = await cancelNotionImport({
        client: apiClient,
        path: { workspace_id: workspaceId, import_id: importId },
        throwOnError: true,
      })
      if (!data) throw new Error('Cancel import response was empty.')
      return data
    },
    onSuccess: (item) => {
      reconcileNotionImport(queryClient, workspaceId, item)
      // Cancelling moves the empty pages it created to the trash.
      void queryClient.invalidateQueries({ queryKey: queryKeys.pages.all(workspaceId) })
    },
    onError: (_error, { importId }) => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.notionImports.detail(workspaceId, importId) })
    },
  })
}
