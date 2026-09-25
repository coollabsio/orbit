import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { apiClient, type createApiClient } from '@/api/client'
import { addPageFavorite, listPageFavorites, movePageFavorite, removePageFavorite } from '@/api/generated/sdk.gen'
import { queryKeys } from '@/api/queryKeys'

type ApiClient = ReturnType<typeof createApiClient>

/**
 * The caller's favorite page ids in order. Personal (other members never see them); the server lists
 * only live pages the caller can see, so trashed favorites drop out and come back on restore.
 */
export function favoritesQueryOptions(workspaceId: string, client: ApiClient = apiClient) {
  return {
    queryKey: queryKeys.pages.favorites(workspaceId),
    queryFn: async (): Promise<string[]> => {
      const { data } = await listPageFavorites({ client, path: { workspace_id: workspaceId }, throwOnError: true })
      if (!data) throw new Error('Favorites response was empty.')
      return [...data.items].sort((a, b) => a.position - b.position).map((item) => item.page_id)
    },
  }
}

export function usePageFavorites(workspaceId: string, enabled = true) {
  return useQuery({ ...favoritesQueryOptions(workspaceId), enabled })
}

/** `ids` with `pageId` moved to index `position` (clamped); unchanged when `pageId` is not in it. */
export function moveFavoriteId(ids: readonly string[], pageId: string, position: number): string[] {
  if (!ids.includes(pageId)) return [...ids]
  const rest = ids.filter((id) => id !== pageId)
  const index = Math.max(0, Math.min(position, rest.length))
  return [...rest.slice(0, index), pageId, ...rest.slice(index)]
}

/**
 * A drop of favorite `dragId` before/after favorite `targetId` → its new index among the favorites,
 * or null when the drop changes nothing (or either row is not a favorite).
 */
export function favoriteDropIndex(ids: readonly string[], dragId: string, targetId: string, zone: 'before' | 'after'): number | null {
  const from = ids.indexOf(dragId)
  const rest = ids.filter((id) => id !== dragId)
  const target = rest.indexOf(targetId)
  if (from < 0 || target < 0) return null
  const index = zone === 'before' ? target : target + 1
  return index === from ? null : index
}

type FavoritesSnapshot = { ids: string[] | undefined }

export interface ToggleFavoriteInput {
  pageId: string
  favorite: boolean
}

/** Adds (`favorite: true`, appended last) or removes a favorite; optimistic with rollback. */
export function useToggleFavorite(workspaceId: string) {
  const queryClient = useQueryClient()
  const key = queryKeys.pages.favorites(workspaceId)
  return useMutation<void, Error, ToggleFavoriteInput, FavoritesSnapshot>({
    mutationFn: async ({ pageId, favorite }) => {
      const path = { workspace_id: workspaceId, page_id: pageId }
      if (favorite) await addPageFavorite({ client: apiClient, path, throwOnError: true })
      else await removePageFavorite({ client: apiClient, path, throwOnError: true })
    },
    onMutate: async ({ pageId, favorite }) => {
      await queryClient.cancelQueries({ queryKey: key })
      const ids = queryClient.getQueryData<string[]>(key)
      const without = (ids ?? []).filter((id) => id !== pageId)
      queryClient.setQueryData<string[]>(key, favorite ? (ids?.includes(pageId) ? ids : [...without, pageId]) : without)
      return { ids }
    },
    onError: (_error, _input, snapshot) => {
      queryClient.setQueryData(key, snapshot?.ids)
    },
    onSettled: () => void queryClient.invalidateQueries({ queryKey: key }),
  })
}

/** Moves a favorite to `position` among the favorites; optimistic, adopts the server order, rolls back on error. */
export function useMoveFavorite(workspaceId: string) {
  const queryClient = useQueryClient()
  const key = queryKeys.pages.favorites(workspaceId)
  return useMutation<string[], Error, { pageId: string; position: number }, FavoritesSnapshot>({
    mutationFn: async ({ pageId, position }) => {
      const { data } = await movePageFavorite({
        client: apiClient,
        path: { workspace_id: workspaceId, page_id: pageId },
        body: { position },
        throwOnError: true,
      })
      if (!data) throw new Error('Move favorite response was empty.')
      return data.items.map((item) => item.page_id)
    },
    onMutate: async ({ pageId, position }) => {
      await queryClient.cancelQueries({ queryKey: key })
      const ids = queryClient.getQueryData<string[]>(key)
      if (ids) queryClient.setQueryData(key, moveFavoriteId(ids, pageId, position))
      return { ids }
    },
    onError: (_error, _input, snapshot) => {
      if (snapshot?.ids) queryClient.setQueryData(key, snapshot.ids)
      void queryClient.invalidateQueries({ queryKey: key })
    },
    onSuccess: (ids) => queryClient.setQueryData(key, ids),
  })
}
