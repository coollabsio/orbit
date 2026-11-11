import { useEffect, useRef } from 'react'
import { useMutation, useQuery, useQueryClient, type QueryClient } from '@tanstack/react-query'
import { apiClient, CONTRACT_ID } from '@/api/client'
import { cancelObsidianImport, getObsidianImport, listObsidianImports, startObsidianImport } from '@orbit/api/generated/sdk.gen'
import type {
  ObsidianImport,
  ObsidianImportDestinationBody,
  ObsidianImportSelectionBody,
  ObsidianImportTree,
  ObsidianSkippedFiles,
  Problem,
} from '@orbit/api/generated/types.gen'
import { ApiProblem } from '@orbit/api/problem'
import { queryKeys } from '@/api/queryKeys'
import { isActiveImport, isFinalImport } from './notionImports'

/** How often an import that is still working is polled. */
export const OBSIDIAN_IMPORT_POLL_MS = 2000

const SKIPPED_LABELS: Record<keyof ObsidianSkippedFiles, [one: string, many: string]> = {
  system: ['settings or system file (.obsidian/, .trash/)', 'settings and system files (.obsidian/, .trash/)'],
  canvas: ['canvas', 'canvases'],
  csv: ['CSV file', 'CSV files'],
  unreferenced: ['file no note links to', 'files no note links to'],
  too_large: ['file over the upload limit', 'files over the upload limit'],
  duplicates: ['file whose name differs only in letter case', 'files whose names differ only in letter case'],
}

/** The skipped files as "2 canvases" parts (zero counts left out) and their total. */
export function skippedSummary(skipped: ObsidianSkippedFiles): { total: number; parts: string[] } {
  const entries = (Object.keys(SKIPPED_LABELS) as (keyof ObsidianSkippedFiles)[]).filter((kind) => skipped[kind] > 0)
  return {
    total: entries.reduce((sum, kind) => sum + skipped[kind], 0),
    parts: entries.map((kind) => `${skipped[kind]} ${SKIPPED_LABELS[kind][skipped[kind] === 1 ? 0 : 1]}`),
  }
}

/** A user-facing sentence for a failed Obsidian import request (upload, start, cancel, load). */
export function obsidianImportErrorMessage(error: unknown): string {
  if (error instanceof ApiProblem) {
    switch (error.code) {
      case 'import_in_progress':
        return 'You already have an import running. Wait for it to finish or cancel it.'
      case 'obsidian_archive_too_large':
        return 'The file is larger than the server’s upload limit. Remove large attachments from the vault and zip it again.'
      case 'obsidian_archive_invalid':
        return 'Upload the vault as one .zip file.'
      case 'obsidian_import_too_large':
        return 'An import can contain at most 5,000 pages. Select fewer notes.'
      case 'obsidian_import_state_conflict':
        return 'This import changed in the meantime (it may have expired or been cancelled).'
      case 'obsidian_import_not_found':
        return 'This import no longer exists.'
      case 'page_not_found':
        return 'The destination page no longer exists. Choose another one.'
      case 'teamspace_not_found':
        return 'The destination teamspace no longer exists. Choose another one.'
    }
    if (error.status === 413) return 'The file is larger than the server’s upload limit.'
    if (error.status === 422) return 'The request was not valid. Check your input and try again.'
  }
  if (error instanceof UploadUnreachable) return 'The server could not be reached. Check your connection and try again.'
  return 'Something went wrong. Try again.'
}


/** The upload never got a response (offline, connection dropped). */
export class UploadUnreachable extends Error {}

function uploadProblem(request: XMLHttpRequest): ApiProblem {
  const body = request.response as Partial<Problem> | null
  if (body && typeof body.code === 'string' && typeof body.status === 'number') return new ApiProblem(body as Problem)
  // The body limit in front of the route answers 413 without a problem document.
  return new ApiProblem({
    type: 'about:blank',
    title: 'Request failed',
    status: request.status,
    code: 'http_error',
    detail: 'The server could not complete the request.',
    instance: new URL(request.responseURL || 'http://localhost/').pathname,
    request_id: request.getResponseHeader('x-request-id') ?? 'unknown',
  })
}

type UploadOptions = { onProgress?: (fraction: number) => void; signal?: AbortSignal }

/** Uploads the vault zip as the `file` field and starts the scan. */
export function uploadObsidianArchive(workspaceId: string, file: File, options: UploadOptions = {}): Promise<ObsidianImport> {
  return uploadImportArchive<ObsidianImport>(`/api/v1/workspaces/${workspaceId}/imports/obsidian`, file, options)
}

/**
 * Posts a zip as the `file` field of `url`, reporting progress from 0 to 1. One `XMLHttpRequest` because `fetch`
 * cannot report upload progress (like chat attachments). Resolves with the 201 body; rejects with an `ApiProblem`,
 * an `UploadUnreachable`, or an `AbortError` when `signal` aborts.
 */
export function uploadImportArchive<T>(url: string, file: File, { onProgress, signal }: UploadOptions = {}): Promise<T> {
  return new Promise((resolve, reject) => {
    if (signal?.aborted) {
      reject(new DOMException('The upload was cancelled.', 'AbortError'))
      return
    }
    const request = new XMLHttpRequest()
    request.open('POST', url)
    request.setRequestHeader('X-Orbit-Contract', CONTRACT_ID)
    request.withCredentials = true
    request.responseType = 'json'
    request.upload.onprogress = (event) => {
      if (event.lengthComputable) onProgress?.(event.loaded / event.total)
    }
    request.onload = () => {
      if (request.status === 201) resolve(request.response as T)
      else reject(uploadProblem(request))
    }
    request.onerror = () => reject(new UploadUnreachable('The server could not be reached.'))
    request.onabort = () => reject(new DOMException('The upload was cancelled.', 'AbortError'))
    signal?.addEventListener('abort', () => request.abort(), { once: true })
    const body = new FormData()
    body.append('file', file)
    request.send(body)
  })
}

/** Stores a fresh import in the detail cache and refreshes the list. */
export function reconcileObsidianImport(queryClient: QueryClient, workspaceId: string, item: ObsidianImport): void {
  queryClient.setQueryData(queryKeys.obsidianImports.detail(workspaceId, item.id), item)
  void queryClient.invalidateQueries({ queryKey: queryKeys.obsidianImports.list(workspaceId) })
}

export function useObsidianImports(workspaceId: string) {
  return useQuery({
    queryKey: queryKeys.obsidianImports.list(workspaceId),
    queryFn: async () => {
      const { data } = await listObsidianImports({ client: apiClient, path: { workspace_id: workspaceId }, throwOnError: true })
      if (!data) throw new Error('Imports response was empty.')
      return data.items
    },
    refetchInterval: (query) => (query.state.data?.some((item) => isActiveImport(item.status)) ? OBSIDIAN_IMPORT_POLL_MS * 2 : false),
  })
}

/**
 * One import without its scan tree, polled only while a job works on it (scanning, queued, importing). When it
 * reaches a final state the page queries and the imports list are refreshed. The tree comes from
 * `useObsidianImportTree`.
 */
export function useObsidianImport(workspaceId: string, importId: string | undefined, pollMs = OBSIDIAN_IMPORT_POLL_MS) {
  const queryClient = useQueryClient()
  const query = useQuery({
    queryKey: queryKeys.obsidianImports.detail(workspaceId, importId ?? ''),
    enabled: Boolean(importId),
    queryFn: async () => {
      const { data } = await getObsidianImport({
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
      void queryClient.invalidateQueries({ queryKey: queryKeys.obsidianImports.list(workspaceId) })
    }
  }, [queryClient, status, workspaceId])
  return query
}

/** The scan tree of a `ready` import, fetched once with `?include=tree` under its own key and never polled. */
export function useObsidianImportTree(workspaceId: string, importId: string) {
  return useQuery<ObsidianImportTree | null>({
    queryKey: queryKeys.obsidianImports.tree(workspaceId, importId),
    staleTime: Infinity,
    refetchOnWindowFocus: false,
    queryFn: async () => {
      const { data } = await getObsidianImport({
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

export function useStartObsidianImport(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation<
    ObsidianImport,
    Error,
    { importId: string; selection: ObsidianImportSelectionBody; destination: ObsidianImportDestinationBody }
  >({
    mutationFn: async ({ importId, selection, destination }) => {
      const { data } = await startObsidianImport({
        client: apiClient,
        path: { workspace_id: workspaceId, import_id: importId },
        body: { selection, destination },
        throwOnError: true,
      })
      if (!data) throw new Error('Start import response was empty.')
      return data
    },
    onSuccess: (item) => reconcileObsidianImport(queryClient, workspaceId, item),
    onError: (error, { importId }) => {
      // Expired or cancelled elsewhere: show the server's state.
      if (error instanceof ApiProblem && error.status === 409) {
        void queryClient.invalidateQueries({ queryKey: queryKeys.obsidianImports.detail(workspaceId, importId) })
      }
    },
  })
}

export function useCancelObsidianImport(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation<ObsidianImport, Error, { importId: string }>({
    mutationFn: async ({ importId }) => {
      const { data } = await cancelObsidianImport({
        client: apiClient,
        path: { workspace_id: workspaceId, import_id: importId },
        throwOnError: true,
      })
      if (!data) throw new Error('Cancel import response was empty.')
      return data
    },
    onSuccess: (item) => {
      reconcileObsidianImport(queryClient, workspaceId, item)
      // Cancelling moves the empty pages it created to the trash.
      void queryClient.invalidateQueries({ queryKey: queryKeys.pages.all(workspaceId) })
    },
    onError: (_error, { importId }) => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.obsidianImports.detail(workspaceId, importId) })
    },
  })
}
