import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { apiClient, type createApiClient } from '@/api/client'
import { addPageFavorite, listPageFavorites, movePageFavorite, removePageFavorite } from '@orbit/api/generated/sdk.gen'
import type { PageFavorite } from '@orbit/api/generated/types.gen'
import { queryKeys } from '@/api/queryKeys'

type ApiClient = ReturnType<typeof createApiClient>

/**
 * The caller's favorites in order, with what the list shows of each page: database rows (and their sub-pages) are
 * not in the tree. Personal (other members never see them); the server lists only live pages the caller can see, so
 * trashed favorites drop out and come back on restore.
 */
export function favoritesQueryOptions(workspaceId: string, client: ApiClient = apiClient) {
  return {
    queryKey: queryKeys.pages.favorites(workspaceId),
    queryFn: async (): Promise<PageFavorite[]> => {
      const { data } = await listPageFavorites({ client, path: { workspace_id: workspaceId }, throwOnError: true })
      if (!data) throw new Error('Favorites response was empty.')
      return [...data.items].sort((a, b) => a.position - b.position)
    },
  }
}

export function usePageFavorites(workspaceId: string, enabled = true) {
  return useQuery({ ...favoritesQueryOptions(workspaceId), enabled })
}

/** True when `pageId` is one of `favorites`. */
export function isFavorite(favorites: readonly PageFavorite[] | undefined, pageId: string): boolean {
  return favorites?.some((favorite) => favorite.page_id === pageId) ?? false
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

const renumbered = (favorites: readonly PageFavorite[]) => favorites.map((favorite, position) => ({ ...favorite, position }))

/** `favorites` in the order of `ids`, renumbered. */
function inOrder(favorites: readonly PageFavorite[], ids: readonly string[]): PageFavorite[] {
  const byId = new Map(favorites.map((favorite) => [favorite.page_id, favorite]))
  return renumbered(ids.flatMap((id) => byId.get(id) ?? []))
}

type FavoritesSnapshot = { favorites: PageFavorite[] | undefined }

/** What the list shows of a page added to the favorites until the server's entry replaces it. */
export type FavoritePage = Pick<PageFavorite, 'title' | 'icon' | 'kind'> & { database_id?: string | null }

export interface ToggleFavoriteInput {
  pageId: string
  favorite: boolean
  /** Shown at once when adding. */
  page?: FavoritePage
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
    onMutate: async ({ pageId, favorite, page }) => {
      await queryClient.cancelQueries({ queryKey: key })
      const favorites = queryClient.getQueryData<PageFavorite[]>(key)
      const without = (favorites ?? []).filter((item) => item.page_id !== pageId)
      const added: PageFavorite = {
        page_id: pageId,
        position: without.length,
        title: page?.title ?? '',
        icon: page?.icon ?? null,
        kind: page?.kind ?? 'page',
        database_id: page?.database_id ?? null,
        database_title: null,
      }
      const next = favorite ? (isFavorite(favorites, pageId) ? favorites : [...without, added]) : renumbered(without)
      queryClient.setQueryData<PageFavorite[]>(key, next)
      return { favorites }
    },
    onError: (_error, _input, snapshot) => {
      queryClient.setQueryData(key, snapshot?.favorites)
    },
    onSettled: () => void queryClient.invalidateQueries({ queryKey: key }),
  })
}

/** Moves a favorite to `position` among the favorites; optimistic, adopts the server order, rolls back on error. */
export function useMoveFavorite(workspaceId: string) {
  const queryClient = useQueryClient()
  const key = queryKeys.pages.favorites(workspaceId)
  return useMutation<PageFavorite[], Error, { pageId: string; position: number }, FavoritesSnapshot>({
    mutationFn: async ({ pageId, position }) => {
      const { data } = await movePageFavorite({
        client: apiClient,
        path: { workspace_id: workspaceId, page_id: pageId },
        body: { position },
        throwOnError: true,
      })
      if (!data) throw new Error('Move favorite response was empty.')
      return data.items
    },
    onMutate: async ({ pageId, position }) => {
      await queryClient.cancelQueries({ queryKey: key })
      const favorites = queryClient.getQueryData<PageFavorite[]>(key)
      if (favorites) {
        const ids = favorites.map((favorite) => favorite.page_id)
        queryClient.setQueryData(key, inOrder(favorites, moveFavoriteId(ids, pageId, position)))
      }
      return { favorites }
    },
    onError: (_error, _input, snapshot) => {
      if (snapshot?.favorites) queryClient.setQueryData(key, snapshot.favorites)
      void queryClient.invalidateQueries({ queryKey: key })
    },
    onSuccess: (favorites) => queryClient.setQueryData(key, favorites),
  })
}
