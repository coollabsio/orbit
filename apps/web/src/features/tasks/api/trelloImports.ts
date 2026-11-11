import { useEffect, useRef } from 'react'
import { useMutation, useQuery, useQueryClient, type QueryClient } from '@tanstack/react-query'
import { apiClient, type createApiClient } from '@/api/client'
import { cancelTrelloImport, createTrelloImport, getTrelloImport, listTrelloImports, startTrelloImport } from '@orbit/api/generated/sdk.gen'
import type { CreateTrelloImportBody, TrelloImport, TrelloImportTree } from '@orbit/api/generated/types.gen'
import { ApiProblem } from '@orbit/api/problem'
import { queryKeys } from '@/api/queryKeys'
import { IMPORT_POLL_MS, isActiveImport, isFinalImport } from '@/features/tasks/api/importStatus'

type ApiClient = ReturnType<typeof createApiClient>

/** The connect field a rejected credential belongs to, so its error shows under that field. */
export type TrelloCredentialField = keyof CreateTrelloImportBody

export function trelloCredentialField(error: unknown): TrelloCredentialField | null {
  if (!(error instanceof ApiProblem)) return null
  if (error.code === 'trello_key_invalid') return 'key'
  if (error.code === 'trello_token_invalid') return 'token'
  return null
}

/** A user-facing sentence for a failed Trello import request. */
export function trelloImportErrorMessage(error: unknown): string {
  if (error instanceof ApiProblem) {
    switch (error.code) {
      // The detail carries Trello's own reason (invalid key, expired token…).
      case 'trello_key_invalid':
        return error.detail || 'Trello rejected this API key. Copy the key again from the Trello Auth tab of your Power-Up.'
      case 'trello_token_invalid':
        return error.detail || 'Trello rejected this token. Generate a new token for this API key and paste it.'
      case 'app_key_missing':
        return 'Orbit is not ready to import from Trello yet. Ask the Orbit administrator to set the app key.'
      case 'import_in_progress':
        return 'You already have an import running. Wait for it to finish or cancel it.'
      case 'trello_unavailable':
        return 'Trello could not be reached. Try again in a moment.'
      case 'trello_import_state_conflict':
        return 'This import changed in the meantime (it may have expired or been cancelled).'
      case 'trello_import_not_found':
        return 'This import no longer exists.'
    }
    if (error.status === 422) return 'The request was not valid. Check your input and try again.'
  }
  return 'Something went wrong. Try again.'
}

/**
 * Sends the key and token once. Deliberately not a `useMutation`: mutation state keeps its variables in the query
 * client, and the credentials must not outlive the request on the client.
 */
export async function createTrelloImportRequest(client: ApiClient, workspaceId: string, credentials: CreateTrelloImportBody): Promise<TrelloImport> {
  const { data } = await createTrelloImport({ client, path: { workspace_id: workspaceId }, body: credentials, throwOnError: true })
  if (!data) throw new Error('Create import response was empty.')
  return data
}

/** Stores a fresh import in the detail cache and refreshes the list. */
export function reconcileTrelloImport(queryClient: QueryClient, workspaceId: string, item: TrelloImport): void {
  queryClient.setQueryData(queryKeys.trelloImports.detail(workspaceId, item.id), item)
  void queryClient.invalidateQueries({ queryKey: queryKeys.trelloImports.list(workspaceId) })
}

export function isTrelloImportInProgress(error: unknown): boolean {
  return error instanceof ApiProblem && error.code === 'import_in_progress'
}

/** The member's 20 most recent Trello imports in this workspace, polled slowly while one runs. */
export function useTrelloImports(workspaceId: string) {
  return useQuery({
    queryKey: queryKeys.trelloImports.list(workspaceId),
    queryFn: async () => {
      const { data } = await listTrelloImports({ client: apiClient, path: { workspace_id: workspaceId }, throwOnError: true })
      if (!data) throw new Error('Imports response was empty.')
      return data.items
    },
    refetchInterval: (query) => (query.state.data?.some((item) => isActiveImport(item.status)) ? IMPORT_POLL_MS * 2 : false),
  })
}

/**
 * One import without its scan tree (status, progress, report), polled only while a job works on it. When it reaches
 * a final state the projects and tasks are refreshed, so the imported projects appear in the sidebar.
 */
export function useTrelloImport(workspaceId: string, importId: string | undefined, pollMs = IMPORT_POLL_MS) {
  const queryClient = useQueryClient()
  const query = useQuery({
    queryKey: queryKeys.trelloImports.detail(workspaceId, importId ?? ''),
    enabled: Boolean(importId),
    queryFn: async () => {
      const { data } = await getTrelloImport({
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
      void queryClient.invalidateQueries({ queryKey: queryKeys.projects(workspaceId) })
      void queryClient.invalidateQueries({ queryKey: queryKeys.tasks.all(workspaceId) })
      void queryClient.invalidateQueries({ queryKey: queryKeys.trelloImports.list(workspaceId) })
    }
  }, [queryClient, status, workspaceId])
  return query
}

/** The scan tree of a `ready` import, fetched once with `?include=tree` under its own key and never polled. */
export function useTrelloImportTree(workspaceId: string, importId: string, enabled = true) {
  return useQuery<TrelloImportTree | null>({
    queryKey: queryKeys.trelloImports.tree(workspaceId, importId),
    enabled,
    staleTime: Infinity,
    refetchOnWindowFocus: false,
    queryFn: async () => {
      const { data } = await getTrelloImport({
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

export function useStartTrelloImport(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation<TrelloImport, Error, { importId: string; boardIds: string[] }>({
    mutationFn: async ({ importId, boardIds }) => {
      const { data } = await startTrelloImport({
        client: apiClient,
        path: { workspace_id: workspaceId, import_id: importId },
        body: { board_ids: boardIds },
        throwOnError: true,
      })
      if (!data) throw new Error('Start import response was empty.')
      return data
    },
    onSuccess: (item) => reconcileTrelloImport(queryClient, workspaceId, item),
    onError: (error, { importId }) => {
      // Expired or cancelled elsewhere: show the server's state.
      if (error instanceof ApiProblem && error.status === 409) {
        void queryClient.invalidateQueries({ queryKey: queryKeys.trelloImports.detail(workspaceId, importId) })
      }
    },
  })
}

export function useCancelTrelloImport(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation<TrelloImport, Error, { importId: string }>({
    mutationFn: async ({ importId }) => {
      const { data } = await cancelTrelloImport({
        client: apiClient,
        path: { workspace_id: workspaceId, import_id: importId },
        throwOnError: true,
      })
      if (!data) throw new Error('Cancel import response was empty.')
      return data
    },
    onSuccess: (item) => {
      reconcileTrelloImport(queryClient, workspaceId, item)
      // What was imported before the cancel stays: show it.
      void queryClient.invalidateQueries({ queryKey: queryKeys.projects(workspaceId) })
    },
    onError: (_error, { importId }) => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.trelloImports.detail(workspaceId, importId) })
    },
  })
}
