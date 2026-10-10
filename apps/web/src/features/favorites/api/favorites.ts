import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { apiClient } from '@/api/client'
import { addFavorite, listFavorites, removeFavorite, reorderFavorites } from '@/api/generated/sdk.gen'
import type { FavoriteRecord, SavedViewRecord } from '@/api/generated/types.gen'
import { queryKeys } from '@/api/queryKeys'

export type Favorite = FavoriteRecord
export type FavoriteKind = 'task' | 'view' | 'project' | 'milestone'
export interface FavoriteKey {
  kind: FavoriteKind
  targetId: string
}

const sameItem = (item: Favorite, key: FavoriteKey) => item.kind === key.kind && item.target_id === key.targetId

export function favoritesQueryOptions(workspaceId: string) {
  return {
    queryKey: queryKeys.favorites(workspaceId),
    queryFn: async (): Promise<Favorite[]> => {
      const { data } = await listFavorites({ client: apiClient, path: { workspace_id: workspaceId }, throwOnError: true })
      if (!data?.items) throw new Error('Favorites response was empty.')
      return data.items
    },
  }
}

/** The caller's favorite tasks, views, projects and milestones of the workspace, in sidebar order. */
export function useFavorites(workspaceId: string, enabled = true) {
  return useQuery({ ...favoritesQueryOptions(workspaceId), enabled })
}

type ViewFavoriteFields = Pick<SavedViewRecord, 'id' | 'is_favorite' | 'favorite_position'>
interface SetSnapshot {
  favorites: Favorite[] | undefined
  views: ViewFavoriteFields[] | undefined
  view: ViewFavoriteFields | undefined
}

/** Adds or removes a favorite. The star and the sidebar change at once; a failure puts them back. */
export function useSetFavorite(workspaceId: string) {
  const queryClient = useQueryClient()
  const favoritesKey = queryKeys.favorites(workspaceId)
  const viewsKey = queryKeys.views(workspaceId)
  return useMutation<void, Error, FavoriteKey & { favorite: boolean }, SetSnapshot>({
    mutationFn: async ({ kind, targetId, favorite }) => {
      const path = { workspace_id: workspaceId, kind, target_id: targetId }
      if (favorite) await addFavorite({ client: apiClient, path, throwOnError: true })
      else await removeFavorite({ client: apiClient, path, throwOnError: true })
    },
    onMutate: async ({ kind, targetId, favorite }) => {
      const viewKey = queryKeys.view(workspaceId, targetId)
      await queryClient.cancelQueries({ queryKey: viewsKey })
      const favorites = queryClient.getQueryData<Favorite[]>(favoritesKey)
      const views = queryClient.getQueryData<ViewFavoriteFields[]>(viewsKey)
      const view = kind === 'view' ? queryClient.getQueryData<ViewFavoriteFields>(viewKey) : undefined
      // a removed item goes at once; an added one shows when the list (with its title) comes back
      if (!favorite) queryClient.setQueryData<Favorite[]>(favoritesKey, (items) => items?.filter((item) => !sameItem(item, { kind, targetId })))
      if (kind === 'view') {
        // like the server, a new favorite goes last (max + 1), so it does not jump when the lists refetch
        const positions = [
          ...(favorites ?? []).filter((item) => !sameItem(item, { kind, targetId })).map((item) => item.position),
          ...(views ?? []).filter((item) => item.is_favorite && item.id !== targetId).map((item) => item.favorite_position ?? 0),
        ]
        const patch = { is_favorite: favorite, favorite_position: favorite ? (positions.length > 0 ? Math.max(...positions) + 1 : 0) : null }
        queryClient.setQueryData<ViewFavoriteFields[]>(viewsKey, (items) => items?.map((item) => item.id === targetId ? { ...item, ...patch } : item))
        // the open view's header star reads the detail query
        queryClient.setQueryData<ViewFavoriteFields>(viewKey, (item) => item && { ...item, ...patch })
      }
      return { favorites, views, view }
    },
    onError: (_error, { kind, targetId }, snapshot) => {
      if (snapshot?.favorites) queryClient.setQueryData(favoritesKey, snapshot.favorites)
      if (snapshot?.views) queryClient.setQueryData(viewsKey, snapshot.views)
      if (kind === 'view' && snapshot?.view) queryClient.setQueryData(queryKeys.view(workspaceId, targetId), snapshot.view)
    },
    // the views list is the prefix of the favorites and of each open view
    onSettled: () => void queryClient.invalidateQueries({ queryKey: viewsKey }),
  })
}

/** Saves a new order of every favorite. */
export function useReorderFavorites(workspaceId: string) {
  const queryClient = useQueryClient()
  const favoritesKey = queryKeys.favorites(workspaceId)
  const viewsKey = queryKeys.views(workspaceId)
  return useMutation<void, Error, FavoriteKey[], Pick<SetSnapshot, 'favorites' | 'views'>>({
    mutationFn: async (keys) => {
      await reorderFavorites({
        client: apiClient,
        path: { workspace_id: workspaceId },
        body: { items: keys.map((key) => ({ kind: key.kind, target_id: key.targetId })) },
        throwOnError: true,
      })
    },
    onMutate: async (keys) => {
      await queryClient.cancelQueries({ queryKey: viewsKey })
      const favorites = queryClient.getQueryData<Favorite[]>(favoritesKey)
      const views = queryClient.getQueryData<ViewFavoriteFields[]>(viewsKey)
      queryClient.setQueryData<Favorite[]>(favoritesKey, (items) => items && keys.flatMap((key, position) =>
        items.filter((item) => sameItem(item, key)).map((item) => ({ ...item, position }))))
      queryClient.setQueryData<ViewFavoriteFields[]>(viewsKey, (items) => items?.map((item) => {
        const position = keys.findIndex((key) => key.kind === 'view' && key.targetId === item.id)
        return position === -1 ? item : { ...item, favorite_position: position }
      }))
      return { favorites, views }
    },
    onError: (_error, _keys, snapshot) => {
      if (snapshot?.favorites) queryClient.setQueryData(favoritesKey, snapshot.favorites)
      if (snapshot?.views) queryClient.setQueryData(viewsKey, snapshot.views)
    },
    onSettled: () => void queryClient.invalidateQueries({ queryKey: viewsKey }),
  })
}

/** The order of every favorite after the views among them take the order `viewIds`; the other kinds keep their slots. */
export function withViewOrder(favorites: Favorite[], viewIds: string[]): FavoriteKey[] {
  const next = [...viewIds]
  const keys = favorites.map((item): FavoriteKey => item.kind === 'view'
    ? { kind: 'view', targetId: next.shift() ?? item.target_id }
    : { kind: item.kind as FavoriteKind, targetId: item.target_id })
  // a view the favorites list does not have yet (it is still loading) goes last
  return [...keys, ...next.map((targetId): FavoriteKey => ({ kind: 'view', targetId }))]
}
