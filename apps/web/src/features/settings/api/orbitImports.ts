import { useEffect, useRef } from 'react'
import { useMutation, useQuery, useQueryClient, type QueryClient } from '@tanstack/react-query'
import { cancelOrbitImport, createOrbitImport, getOrbitImport, listOrbitImports, startOrbitImport } from '@orbit/api/generated/sdk.gen'
import type { OrbitImport, OrbitImportSelection, OrbitImportTree } from '@orbit/api/generated/types.gen'
import { orbitImportProblemMessage } from '@orbit/api/orbitImports'
import { ApiProblem } from '@orbit/api/problem'
import { apiClient } from '@/api/client'
import { queryKeys } from '@/api/queryKeys'
import { chatKeys } from '@/features/chat/api/keys'
import { UploadUnreachable, uploadImportArchive } from '@/features/docs/api/obsidianImports'
import { IMPORT_POLL_MS, isActiveImport, isFinalImport } from '@/features/tasks/api/importStatus'

/** A user-facing sentence for a failed Orbit import request. */
export function orbitImportErrorMessage(error: unknown): string {
  if (error instanceof ApiProblem) {
    const message = orbitImportProblemMessage(error.code)
    if (message) return message
    if (error.status === 404) return 'This import no longer exists.'
    if (error.status === 413) return 'The file is larger than the server’s upload limit.'
    if (error.status === 422) return 'The request was not valid. Check your input and try again.'
  }
  if (error instanceof UploadUnreachable) return 'The server could not be reached. Check your connection and try again.'
  return 'Something went wrong. Try again.'
}

export const isOrbitImportInProgress = (error: unknown) => error instanceof ApiProblem && error.code === 'import_in_progress'

/** Sends the token once. Not a `useMutation`: its state would keep the token in the query client. */
export async function createOrbitImportRequest(workspaceId: string, url: string, token: string): Promise<OrbitImport> {
  const { data } = await createOrbitImport({ client: apiClient, path: { workspace_id: workspaceId }, body: { url, token }, throwOnError: true })
  if (!data) throw new Error('Create import response was empty.')
  return data
}

export function uploadOrbitExport(workspaceId: string, file: File, options: { onProgress?: (fraction: number) => void; signal?: AbortSignal }) {
  return uploadImportArchive<OrbitImport>(`/api/v1/workspaces/${workspaceId}/imports/orbit/upload`, file, options)
}

/** Stores a fresh import in the detail cache and refreshes the list. */
export function reconcileOrbitImport(queryClient: QueryClient, workspaceId: string, item: OrbitImport): void {
  queryClient.setQueryData(queryKeys.orbitImports.detail(workspaceId, item.id), item)
  void queryClient.invalidateQueries({ queryKey: queryKeys.orbitImports.list(workspaceId) })
}

/** An import fills projects, tasks, pages and channels. */
function refreshImported(queryClient: QueryClient, workspaceId: string) {
  void queryClient.invalidateQueries({ queryKey: queryKeys.projects(workspaceId) })
  void queryClient.invalidateQueries({ queryKey: queryKeys.tasks.all(workspaceId) })
  void queryClient.invalidateQueries({ queryKey: queryKeys.pages.all(workspaceId) })
  void queryClient.invalidateQueries({ queryKey: chatKeys.conversations(workspaceId) })
}

/** The member's 20 most recent Orbit imports in this workspace, polled slowly while one runs. */
export function useOrbitImports(workspaceId: string) {
  return useQuery({
    queryKey: queryKeys.orbitImports.list(workspaceId),
    queryFn: async () => {
      const { data } = await listOrbitImports({ client: apiClient, path: { workspace_id: workspaceId }, throwOnError: true })
      if (!data) throw new Error('Imports response was empty.')
      return data.imports
    },
    refetchInterval: (query) => (query.state.data?.some((item) => isActiveImport(item.status)) ? IMPORT_POLL_MS * 2 : false),
  })
}

/** One import without its tree, polled while a job works on it. A final state refreshes what it imported. */
export function useOrbitImport(workspaceId: string, importId: string | undefined) {
  const queryClient = useQueryClient()
  const query = useQuery({
    queryKey: queryKeys.orbitImports.detail(workspaceId, importId ?? ''),
    enabled: Boolean(importId),
    queryFn: async () => {
      const { data } = await getOrbitImport({ client: apiClient, path: { workspace_id: workspaceId, import_id: importId! }, throwOnError: true })
      if (!data) throw new Error('Import response was empty.')
      return data
    },
    refetchInterval: (current) => (isActiveImport(current.state.data?.status) ? IMPORT_POLL_MS : false),
  })
  const status = query.data?.status
  const previous = useRef(status)
  useEffect(() => {
    const before = previous.current
    previous.current = status
    if (before !== undefined && before !== status && isFinalImport(status)) {
      refreshImported(queryClient, workspaceId)
      void queryClient.invalidateQueries({ queryKey: queryKeys.orbitImports.list(workspaceId) })
    }
  }, [queryClient, status, workspaceId])
  return query
}

/** The scan tree of a `ready` import, fetched once with `?include=tree` and never polled. */
export function useOrbitImportTree(workspaceId: string, importId: string) {
  return useQuery<OrbitImportTree | null>({
    queryKey: queryKeys.orbitImports.tree(workspaceId, importId),
    staleTime: Infinity,
    refetchOnWindowFocus: false,
    queryFn: async () => {
      const { data } = await getOrbitImport({
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

export function useStartOrbitImport(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation<OrbitImport, Error, { importId: string; selection: OrbitImportSelection }>({
    mutationFn: async ({ importId, selection }) => {
      const { data } = await startOrbitImport({
        client: apiClient,
        path: { workspace_id: workspaceId, import_id: importId },
        body: { selection },
        throwOnError: true,
      })
      if (!data) throw new Error('Start import response was empty.')
      return data
    },
    onSuccess: (item) => reconcileOrbitImport(queryClient, workspaceId, item),
    onError: (error, { importId }) => {
      // Expired or cancelled elsewhere: show the server's state.
      if (error instanceof ApiProblem && error.status === 409) {
        void queryClient.invalidateQueries({ queryKey: queryKeys.orbitImports.detail(workspaceId, importId) })
      }
    },
  })
}

export function useCancelOrbitImport(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation<OrbitImport, Error, { importId: string }>({
    mutationFn: async ({ importId }) => {
      const { data } = await cancelOrbitImport({ client: apiClient, path: { workspace_id: workspaceId, import_id: importId }, throwOnError: true })
      if (!data) throw new Error('Cancel import response was empty.')
      return data
    },
    onSuccess: (item) => {
      reconcileOrbitImport(queryClient, workspaceId, item)
      refreshImported(queryClient, workspaceId)
    },
    onError: (_error, { importId }) => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.orbitImports.detail(workspaceId, importId) })
    },
  })
}
