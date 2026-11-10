import { useEffect, useRef } from 'react'
import { useMutation, useQuery, useQueryClient, type QueryClient } from '@tanstack/react-query'
import { apiClient, type createApiClient } from '@/api/client'
import { cancelLinearImport, createLinearImport, getLinearImport, listLinearImports, startLinearImport } from '@orbit/api/generated/sdk.gen'
import type { LinearImport, LinearImportTree } from '@orbit/api/generated/types.gen'
import { ApiProblem } from '@orbit/api/problem'
import { queryKeys } from '@/api/queryKeys'
import { IMPORT_POLL_MS, isActiveImport, isFinalImport } from '@/features/tasks/api/importStatus'

type ApiClient = ReturnType<typeof createApiClient>

/** A user-facing sentence for a failed Linear import request. */
export function linearImportErrorMessage(error: unknown): string {
  if (error instanceof ApiProblem) {
    switch (error.code) {
      case 'linear_token_invalid':
        return 'Linear rejected this API key. Check that you copied the whole key and that it was not revoked.'
      case 'app_key_missing':
        return 'Orbit is not ready to import from Linear yet. Ask the Orbit administrator to set the app key.'
      case 'import_in_progress':
        return 'You already have an import running. Wait for it to finish or cancel it.'
      case 'linear_unavailable':
        return 'Linear could not be reached. Try again in a moment.'
      case 'linear_import_state_conflict':
        return 'This import changed in the meantime (it may have expired or been cancelled).'
      case 'linear_import_not_found':
        return 'This import no longer exists.'
    }
    if (error.status === 422) return 'The request was not valid. Check your input and try again.'
  }
  return 'Something went wrong. Try again.'
}

/**
 * Sends the API key once. Deliberately not a `useMutation`: mutation state keeps its variables in the query client,
 * and the key must not outlive the request on the client.
 */
export async function createLinearImportRequest(client: ApiClient, workspaceId: string, key: string): Promise<LinearImport> {
  const { data } = await createLinearImport({ client, path: { workspace_id: workspaceId }, body: { token: key }, throwOnError: true })
  if (!data) throw new Error('Create import response was empty.')
  return data
}

/** Stores a fresh import in the detail cache and refreshes the list. */
export function reconcileLinearImport(queryClient: QueryClient, workspaceId: string, item: LinearImport): void {
  queryClient.setQueryData(queryKeys.linearImports.detail(workspaceId, item.id), item)
  void queryClient.invalidateQueries({ queryKey: queryKeys.linearImports.list(workspaceId) })
}

export function isLinearImportInProgress(error: unknown): boolean {
  return error instanceof ApiProblem && error.code === 'import_in_progress'
}

/** The member's 20 most recent Linear imports in this workspace, polled slowly while one runs. */
export function useLinearImports(workspaceId: string) {
  return useQuery({
    queryKey: queryKeys.linearImports.list(workspaceId),
    queryFn: async () => {
      const { data } = await listLinearImports({ client: apiClient, path: { workspace_id: workspaceId }, throwOnError: true })
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
export function useLinearImport(workspaceId: string, importId: string | undefined, pollMs = IMPORT_POLL_MS) {
  const queryClient = useQueryClient()
  const query = useQuery({
    queryKey: queryKeys.linearImports.detail(workspaceId, importId ?? ''),
    enabled: Boolean(importId),
    queryFn: async () => {
      const { data } = await getLinearImport({
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
      void queryClient.invalidateQueries({ queryKey: queryKeys.linearImports.list(workspaceId) })
    }
  }, [queryClient, status, workspaceId])
  return query
}

/** The scan tree of a `ready` import, fetched once with `?include=tree` under its own key and never polled. */
export function useLinearImportTree(workspaceId: string, importId: string, enabled = true) {
  return useQuery<LinearImportTree | null>({
    queryKey: queryKeys.linearImports.tree(workspaceId, importId),
    enabled,
    staleTime: Infinity,
    refetchOnWindowFocus: false,
    queryFn: async () => {
      const { data } = await getLinearImport({
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

export function useStartLinearImport(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation<LinearImport, Error, { importId: string; teamIds: string[] }>({
    mutationFn: async ({ importId, teamIds }) => {
      const { data } = await startLinearImport({
        client: apiClient,
        path: { workspace_id: workspaceId, import_id: importId },
        body: { team_ids: teamIds },
        throwOnError: true,
      })
      if (!data) throw new Error('Start import response was empty.')
      return data
    },
    onSuccess: (item) => reconcileLinearImport(queryClient, workspaceId, item),
    onError: (error, { importId }) => {
      // Expired or cancelled elsewhere: show the server's state.
      if (error instanceof ApiProblem && error.status === 409) {
        void queryClient.invalidateQueries({ queryKey: queryKeys.linearImports.detail(workspaceId, importId) })
      }
    },
  })
}

export function useCancelLinearImport(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation<LinearImport, Error, { importId: string }>({
    mutationFn: async ({ importId }) => {
      const { data } = await cancelLinearImport({
        client: apiClient,
        path: { workspace_id: workspaceId, import_id: importId },
        throwOnError: true,
      })
      if (!data) throw new Error('Cancel import response was empty.')
      return data
    },
    onSuccess: (item) => {
      reconcileLinearImport(queryClient, workspaceId, item)
      // What was imported before the cancel stays: show it.
      void queryClient.invalidateQueries({ queryKey: queryKeys.projects(workspaceId) })
    },
    onError: (_error, { importId }) => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.linearImports.detail(workspaceId, importId) })
    },
  })
}

export type LinearImportProgressView = {
  label: string
  /** 1–4 while a stage runs, else null. */
  step: number | null
  done: number
  total: number
  detail: string
}

const count = (value: number) => value.toLocaleString('en-US')

/** What the progress page of a queued or importing Linear import shows. */
export function linearImportProgress(item: LinearImport): LinearImportProgressView {
  const { done, total, failed, expected, read, created, stage } = item.progress
  const imported = `${count(done)} of ${count(total)} issues imported`
  if (item.status === 'queued') return { label: 'Waiting for the import to start…', step: null, done: 0, total: 0, detail: '' }
  switch (stage) {
    case 'reading':
      return { label: 'Reading issues from Linear…', step: 1, done: read, total: Math.max(expected, read), detail: `${count(read)} of about ${count(expected)} issues read${done > 0 ? `, ${count(done)} imported` : ''}` }
    case 'creating':
      return { label: 'Creating tasks…', step: 2, done: created, total, detail: `${count(created)} of ${count(total)} tasks created` }
    case 'details':
      return { label: 'Adding files and comments…', step: 3, done: done + failed, total, detail: imported }
    case 'linking':
      return { label: 'Linking related issues…', step: 4, done: done + failed, total, detail: imported }
    default:
      return { label: 'Importing issues…', step: null, done: done + failed, total, detail: imported }
  }
}

/** The rate-limit notice while Linear makes the import wait, else null. */
export function linearRateLimitNotice(waitingUntil: string | null, now: number): string | null {
  const until = waitingUntil ? Date.parse(waitingUntil) : NaN
  if (!(until > now)) return null
  const minutes = Math.max(1, Math.ceil((until - now) / 60_000))
  const time = new Date(until).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
  return `Linear's rate limit is reached. The import goes on at ${time} (in about ${minutes} min).`
}
