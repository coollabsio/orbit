import { keepPreviousData, useMutation, useQuery, useQueryClient, type QueryClient } from '@tanstack/react-query'
import { apiClient, type createApiClient } from '@/api/client'
import {
  createPage,
  deletePage,
  getPage,
  listPages,
  listPageTrash,
  movePage,
  restorePage,
  searchPages,
  updatePage,
  uploadPageFile,
} from '@/api/generated/sdk.gen'
import type { CreatePageBody, Page, PageFile, PageSummary, PageUpdateBody } from '@/api/generated/types.gen'
import { ApiProblem } from '@/api/problem'
import { queryKeys } from '@/api/queryKeys'
import { confirmAction } from '@/components/common/confirmAction'
import { isTaskVersionConflict } from '@/features/tasks/api/conflicts'
import { applyMove, descendantsOf, removeSubtree, type MoveTarget } from '../pageTree'

type ApiClient = ReturnType<typeof createApiClient>

function required<T>(data: T | undefined, message: string): T {
  if (data === undefined) throw new Error(message)
  return data
}

/** Same 409 shape as tasks (`conflict` with `current_version`, `current`, `refresh`). */
export function isPageVersionConflict(error: unknown): error is ApiProblem {
  return isTaskVersionConflict(error)
}

export function isPageNotFound(error: unknown): boolean {
  return error instanceof ApiProblem && error.status === 404
}

/** The server's current page from a 409 body (`conflict.current`), when it carries one. */
export function conflictCurrentPage(error: unknown): Page | null {
  if (!(error instanceof ApiProblem)) return null
  const current = (error.problem.conflict as { current?: unknown } | null | undefined)?.current
  if (!current || typeof current !== 'object' || !('id' in current) || !('content' in current)) return null
  return current as Page
}

export function conflictCurrentVersion(error: unknown): number | null {
  if (!(error instanceof ApiProblem)) return null
  const version = error.problem.conflict?.current_version
  return typeof version === 'number' ? version : null
}

export function toSummary(page: Page | PageSummary): PageSummary {
  const { id, parent_id, teamspace_id, private: isPrivate, title, icon, position, version, updated_at } = page
  return { id, parent_id, teamspace_id, private: isPrivate, title, icon, position, version, updated_at }
}

/** Writes a server page into the detail cache and its summary into the tree cache (no refetch). */
export function reconcilePage(queryClient: QueryClient, workspaceId: string, page: Page): void {
  queryClient.setQueryData(queryKeys.pages.detail(workspaceId, page.id), page)
  queryClient.setQueryData(queryKeys.pages.tree(workspaceId), (tree: PageSummary[] | undefined) => {
    if (!tree) return tree
    const summary = toSummary(page)
    return tree.some((item) => item.id === page.id)
      ? tree.map((item) => (item.id === page.id ? summary : item))
      : [...tree, summary]
  })
}

/** Patches tree summaries locally (e.g. a title while it is being typed). */
export function patchTreePage(queryClient: QueryClient, workspaceId: string, pageId: string, patch: Partial<PageSummary>): void {
  queryClient.setQueryData(queryKeys.pages.tree(workspaceId), (tree: PageSummary[] | undefined) =>
    tree?.map((item) => (item.id === pageId ? { ...item, ...patch } : item)),
  )
}

export async function fetchPageTree(client: ApiClient, workspaceId: string): Promise<PageSummary[]> {
  const { data } = await listPages({ client, path: { workspace_id: workspaceId }, throwOnError: true })
  return required(data, 'Pages response was empty.').items
}

export async function fetchPage(client: ApiClient, workspaceId: string, pageId: string): Promise<Page> {
  const { data } = await getPage({ client, path: { workspace_id: workspaceId, page_id: pageId }, throwOnError: true })
  return required(data, 'Page response was empty.')
}

export async function savePage(client: ApiClient, workspaceId: string, pageId: string, body: PageUpdateBody): Promise<Page> {
  const { data } = await updatePage({ client, path: { workspace_id: workspaceId, page_id: pageId }, body, throwOnError: true })
  return required(data, 'Update page response was empty.')
}

/**
 * Uploads one file to a page (editor image/file blocks, uploaded covers). The returned `url` is a same-origin
 * download path that the page's readers can load, so it can go straight into a block or `cover_url`.
 */
export async function uploadPageFileRequest(client: ApiClient, workspaceId: string, pageId: string, file: File): Promise<PageFile> {
  const { data } = await uploadPageFile({ client, path: { workspace_id: workspaceId, page_id: pageId }, body: { file }, throwOnError: true })
  return required(data, 'Upload response was empty.')
}

/** A short, user-facing reason for a failed page upload. */
export function pageUploadErrorMessage(error: unknown): string {
  if (error instanceof ApiProblem) {
    if (error.status === 413) return 'The file is too large to upload.'
    if (error.status === 404) return 'This page is no longer available.'
    if (error.status === 422) return 'The file name is not valid.'
  }
  return 'Could not upload the file. Try again.'
}

export function pageTreeQueryOptions(workspaceId: string, client: ApiClient = apiClient) {
  return {
    queryKey: queryKeys.pages.tree(workspaceId),
    queryFn: () => fetchPageTree(client, workspaceId),
  }
}

export function usePageTree(workspaceId: string, enabled = true) {
  return useQuery({ ...pageTreeQueryOptions(workspaceId), enabled })
}

export function usePage(workspaceId: string, pageId: string | undefined) {
  return useQuery({
    queryKey: queryKeys.pages.detail(workspaceId, pageId ?? ''),
    enabled: Boolean(pageId),
    queryFn: async () => {
      const { data } = await getPage({ client: apiClient, path: { workspace_id: workspaceId, page_id: pageId! }, throwOnError: true })
      return required(data, 'Page response was empty.')
    },
  })
}

export function usePageTrash(workspaceId: string) {
  return useQuery({
    queryKey: queryKeys.pages.trash(workspaceId),
    queryFn: async () => {
      const { data } = await listPageTrash({ client: apiClient, path: { workspace_id: workspaceId }, throwOnError: true })
      return required(data, 'Page trash response was empty.').items
    },
  })
}

/** Server-side title + body search. Idle (no request) while the query is blank; debounce on the caller side. */
export function usePageSearch(workspaceId: string, query: string) {
  const q = query.trim().slice(0, 200)
  return useQuery({
    queryKey: queryKeys.pages.search(workspaceId, q),
    enabled: q.length > 0,
    placeholderData: keepPreviousData,
    queryFn: async () => {
      const { data } = await searchPages({ client: apiClient, path: { workspace_id: workspaceId }, query: { q }, throwOnError: true })
      return required(data, 'Page search response was empty.').items
    },
  })
}

async function promptForConflict(error: Error, refresh: () => void) {
  if (
    isPageVersionConflict(error) &&
    (await confirmAction({ title: 'Refresh pages?', description: 'This page changed on the server. Refresh it now?', confirmLabel: 'Refresh' }))
  ) {
    refresh()
  }
}

export function useCreatePage(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation<Page, Error, CreatePageBody>({
    mutationFn: async (body) => {
      const { data } = await createPage({ client: apiClient, path: { workspace_id: workspaceId }, body, throwOnError: true })
      return required(data, 'Create page response was empty.')
    },
    onSuccess: (page) => {
      reconcilePage(queryClient, workspaceId, page)
      // Appending renumbers nothing, but an explicit position shifts later siblings.
      void queryClient.invalidateQueries({ queryKey: queryKeys.pages.tree(workspaceId) })
    },
  })
}

export function useUpdatePage(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation<Page, Error, { pageId: string; body: PageUpdateBody }>({
    mutationFn: ({ pageId, body }) => savePage(apiClient, workspaceId, pageId, body),
    onSuccess: (page) => reconcilePage(queryClient, workspaceId, page),
  })
}

type TreeSnapshot = { tree: PageSummary[] | undefined }

export interface MovePageInput extends MoveTarget {
  pageId: string
  version: number
}

export function useMovePage(workspaceId: string) {
  const queryClient = useQueryClient()
  const treeKey = queryKeys.pages.tree(workspaceId)
  return useMutation<Page, Error, MovePageInput, TreeSnapshot>({
    mutationFn: async ({ pageId, version, parent_id, position, teamspace_id, private: isPrivate }) => {
      const { data } = await movePage({
        client: apiClient,
        path: { workspace_id: workspaceId, page_id: pageId },
        body: {
          expected_version: version,
          parent_id,
          position,
          ...(teamspace_id ? { teamspace_id } : {}),
          ...(isPrivate ? { private: true } : {}),
        },
        throwOnError: true,
      })
      return required(data, 'Move page response was empty.')
    },
    onMutate: async ({ pageId, version: _version, ...move }) => {
      await queryClient.cancelQueries({ queryKey: treeKey })
      const tree = queryClient.getQueryData<PageSummary[]>(treeKey)
      if (tree) queryClient.setQueryData(treeKey, applyMove(tree, pageId, move))
      return { tree }
    },
    onError: (error, _input, snapshot) => {
      if (snapshot?.tree) queryClient.setQueryData(treeKey, snapshot.tree)
      return promptForConflict(error, () => void queryClient.invalidateQueries({ queryKey: treeKey }))
    },
    // Only the moved page's version bumps; the open editor picks the new version up from the detail cache.
    onSuccess: (page) => reconcilePage(queryClient, workspaceId, page),
    // A space change also moves the subtree, so cached sub-page details, search hits and trash rows go stale.
    onSettled: (_page, _error, input) =>
      void queryClient.invalidateQueries({ queryKey: input.teamspace_id || input.private ? queryKeys.pages.all(workspaceId) : treeKey }),
  })
}

export function useTrashPage(workspaceId: string) {
  const queryClient = useQueryClient()
  const treeKey = queryKeys.pages.tree(workspaceId)
  return useMutation<void, Error, { pageId: string; version: number }, TreeSnapshot & { subtree: string[] }>({
    mutationFn: async ({ pageId, version }) => {
      await deletePage({
        client: apiClient,
        path: { workspace_id: workspaceId, page_id: pageId },
        query: { expected_version: version },
        throwOnError: true,
      })
    },
    onMutate: async ({ pageId }) => {
      await queryClient.cancelQueries({ queryKey: treeKey })
      const tree = queryClient.getQueryData<PageSummary[]>(treeKey)
      const subtree = [pageId, ...descendantsOf(tree ?? [], pageId).map((page) => page.id)]
      if (tree) queryClient.setQueryData(treeKey, removeSubtree(tree, pageId))
      return { tree, subtree }
    },
    onError: (error, _input, snapshot) => {
      if (snapshot?.tree) queryClient.setQueryData(treeKey, snapshot.tree)
      return promptForConflict(error, () => void queryClient.invalidateQueries({ queryKey: queryKeys.pages.all(workspaceId) }))
    },
    onSuccess: (_result, _input, snapshot) => {
      for (const id of snapshot?.subtree ?? []) queryClient.removeQueries({ queryKey: queryKeys.pages.detail(workspaceId, id), exact: true })
      void queryClient.invalidateQueries({ queryKey: treeKey })
      void queryClient.invalidateQueries({ queryKey: queryKeys.pages.trash(workspaceId) })
      void queryClient.invalidateQueries({ queryKey: queryKeys.pages.favorites(workspaceId) })
    },
  })
}

export function useRestorePage(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation<Page, Error, { pageId: string; version: number }>({
    mutationFn: async ({ pageId, version }) => {
      const { data } = await restorePage({
        client: apiClient,
        path: { workspace_id: workspaceId, page_id: pageId },
        body: { expected_version: version },
        throwOnError: true,
      })
      return required(data, 'Restore page response was empty.')
    },
    onError: (error) => promptForConflict(error, () => void queryClient.invalidateQueries({ queryKey: queryKeys.pages.trash(workspaceId) })),
    onSuccess: (page) => {
      queryClient.setQueryData(queryKeys.pages.detail(workspaceId, page.id), page)
      // The restored subtree comes back too, so refetch rather than patch.
      void queryClient.invalidateQueries({ queryKey: queryKeys.pages.tree(workspaceId) })
      void queryClient.invalidateQueries({ queryKey: queryKeys.pages.trash(workspaceId) })
      // Favorites of the restored subtree come back.
      void queryClient.invalidateQueries({ queryKey: queryKeys.pages.favorites(workspaceId) })
    },
  })
}
