import { useEffect, useRef } from 'react'
import { useMutation, useQuery, useQueryClient, type QueryClient } from '@tanstack/react-query'
import { apiClient, type createApiClient } from '@/api/client'
import { cancelAsanaImport, createAsanaImport, getAsanaImport, listAsanaImports, startAsanaImport } from '@orbit/api/generated/sdk.gen'
import type { AsanaImport, AsanaImportStatus, AsanaImportTree } from '@orbit/api/generated/types.gen'
import { ApiProblem } from '@orbit/api/problem'
import { queryKeys } from '@/api/queryKeys'

type ApiClient = ReturnType<typeof createApiClient>

/** How often an import that is still working is polled. */
export const ASANA_IMPORT_POLL_MS = 2000

/** A job is working on it: poll. */
export function isActiveAsanaImport(status: AsanaImportStatus | undefined): boolean {
  return status === 'scanning' || status === 'queued' || status === 'importing'
}

/** Nothing will change any more. */
export function isFinalAsanaImport(status: AsanaImportStatus | undefined): boolean {
  return status === 'completed' || status === 'failed' || status === 'cancelled' || status === 'expired'
}

export const ASANA_IMPORT_STATUS_LABELS: Record<AsanaImportStatus, string> = {
  scanning: 'Scanning',
  ready: 'Ready to import',
  queued: 'Queued',
  importing: 'Importing',
  completed: 'Completed',
  failed: 'Failed',
  cancelled: 'Cancelled',
  expired: 'Expired',
}

/** A user-facing sentence for a failed Asana import request. */
export function asanaImportErrorMessage(error: unknown): string {
  if (error instanceof ApiProblem) {
    switch (error.code) {
      case 'asana_token_invalid':
        return 'Asana rejected this token. Check that you copied the whole token and that it was not revoked.'
      case 'app_key_missing':
        return 'Orbit is not ready to import from Asana yet. Ask the Orbit administrator to set the app key.'
      case 'import_in_progress':
        return 'You already have an import running. Wait for it to finish or cancel it.'
      case 'asana_unavailable':
        return 'Asana could not be reached. Try again in a moment.'
      case 'asana_import_state_conflict':
        return 'This import changed in the meantime (it may have expired or been cancelled).'
      case 'asana_import_not_found':
        return 'This import no longer exists.'
    }
    if (error.status === 422) return 'The request was not valid. Check your input and try again.'
  }
  return 'Something went wrong. Try again.'
}

/**
 * Sends the token once. Deliberately not a `useMutation`: mutation state keeps its variables in the query client,
 * and the token must not outlive the request on the client.
 */
export async function createAsanaImportRequest(client: ApiClient, workspaceId: string, token: string): Promise<AsanaImport> {
  const { data } = await createAsanaImport({ client, path: { workspace_id: workspaceId }, body: { token }, throwOnError: true })
  if (!data) throw new Error('Create import response was empty.')
  return data
}

/** Stores a fresh import in the detail cache and refreshes the list. */
export function reconcileAsanaImport(queryClient: QueryClient, workspaceId: string, item: AsanaImport): void {
  queryClient.setQueryData(queryKeys.asanaImports.detail(workspaceId, item.id), item)
  void queryClient.invalidateQueries({ queryKey: queryKeys.asanaImports.list(workspaceId) })
}

export function isAsanaImportInProgress(error: unknown): boolean {
  return error instanceof ApiProblem && error.code === 'import_in_progress'
}

/** The member's 20 most recent Asana imports in this workspace, polled slowly while one runs. */
export function useAsanaImports(workspaceId: string) {
  return useQuery({
    queryKey: queryKeys.asanaImports.list(workspaceId),
    queryFn: async () => {
      const { data } = await listAsanaImports({ client: apiClient, path: { workspace_id: workspaceId }, throwOnError: true })
      if (!data) throw new Error('Imports response was empty.')
      return data.items
    },
    refetchInterval: (query) => (query.state.data?.some((item) => isActiveAsanaImport(item.status)) ? ASANA_IMPORT_POLL_MS * 2 : false),
  })
}

/**
 * One import without its scan tree (status, progress, report), polled only while a job works on it. When it reaches
 * a final state the projects and tasks are refreshed, so the imported projects appear in the sidebar.
 */
export function useAsanaImport(workspaceId: string, importId: string | undefined, pollMs = ASANA_IMPORT_POLL_MS) {
  const queryClient = useQueryClient()
  const query = useQuery({
    queryKey: queryKeys.asanaImports.detail(workspaceId, importId ?? ''),
    enabled: Boolean(importId),
    queryFn: async () => {
      const { data } = await getAsanaImport({
        client: apiClient,
        path: { workspace_id: workspaceId, import_id: importId! },
        throwOnError: true,
      })
      if (!data) throw new Error('Import response was empty.')
      return data
    },
    refetchInterval: (current) => (isActiveAsanaImport(current.state.data?.status) ? pollMs : false),
  })
  const status = query.data?.status
  const previous = useRef(status)
  useEffect(() => {
    const before = previous.current
    previous.current = status
    if (before !== undefined && before !== status && isFinalAsanaImport(status)) {
      void queryClient.invalidateQueries({ queryKey: queryKeys.projects(workspaceId) })
      void queryClient.invalidateQueries({ queryKey: queryKeys.tasks.all(workspaceId) })
      void queryClient.invalidateQueries({ queryKey: queryKeys.asanaImports.list(workspaceId) })
    }
  }, [queryClient, status, workspaceId])
  return query
}

/** The scan tree of a `ready` import, fetched once with `?include=tree` under its own key and never polled. */
export function useAsanaImportTree(workspaceId: string, importId: string, enabled = true) {
  return useQuery<AsanaImportTree | null>({
    queryKey: queryKeys.asanaImports.tree(workspaceId, importId),
    enabled,
    staleTime: Infinity,
    refetchOnWindowFocus: false,
    queryFn: async () => {
      const { data } = await getAsanaImport({
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

export function useStartAsanaImport(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation<AsanaImport, Error, { importId: string; projectGids: string[] }>({
    mutationFn: async ({ importId, projectGids }) => {
      const { data } = await startAsanaImport({
        client: apiClient,
        path: { workspace_id: workspaceId, import_id: importId },
        body: { project_gids: projectGids },
        throwOnError: true,
      })
      if (!data) throw new Error('Start import response was empty.')
      return data
    },
    onSuccess: (item) => reconcileAsanaImport(queryClient, workspaceId, item),
    onError: (error, { importId }) => {
      // Expired or cancelled elsewhere: show the server's state.
      if (error instanceof ApiProblem && error.status === 409) {
        void queryClient.invalidateQueries({ queryKey: queryKeys.asanaImports.detail(workspaceId, importId) })
      }
    },
  })
}

export function useCancelAsanaImport(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation<AsanaImport, Error, { importId: string }>({
    mutationFn: async ({ importId }) => {
      const { data } = await cancelAsanaImport({
        client: apiClient,
        path: { workspace_id: workspaceId, import_id: importId },
        throwOnError: true,
      })
      if (!data) throw new Error('Cancel import response was empty.')
      return data
    },
    onSuccess: (item) => {
      reconcileAsanaImport(queryClient, workspaceId, item)
      // What was imported before the cancel stays: show it.
      void queryClient.invalidateQueries({ queryKey: queryKeys.projects(workspaceId) })
    },
    onError: (_error, { importId }) => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.asanaImports.detail(workspaceId, importId) })
    },
  })
}
