import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { apiClient, type createApiClient } from '@/api/client'
import {
  createView,
  deleteView,
  favoriteView,
  getView,
  getViewPreference,
  listViews,
  putViewPreference,
  reorderViewFavorites,
  unfavoriteView,
  updateView,
} from '@/api/generated/sdk.gen'
import type { SavedViewRecord, ViewCreateBody, ViewUpdateBody } from '@/api/generated/types.gen'
import { ApiProblem } from '@/api/problem'
import { queryKeys } from '@/api/queryKeys'
import type { PageKey, ViewState } from '@/features/views/viewState'

type ApiClient = ReturnType<typeof createApiClient>
type ApiViewState = ViewCreateBody['state']

/** `state` uses the hand-written `ViewState` (the generated schema describes the same JSON more loosely). */
export type SavedView = Omit<SavedViewRecord, 'state'> & { state: ViewState | null }
export type ViewCreateInput = Omit<ViewCreateBody, 'state'> & { state: ViewState }
export type ViewUpdateInput = Omit<ViewUpdateBody, 'state'> & { state?: ViewState }

/** The hand-written `ViewState` is the source of truth; the generated schema types describe the same JSON. */
const toApiState = (state: ViewState) => state as unknown as ApiViewState

/** The record's state, or null when missing or unreadable (`state_error`). */
export function viewStateOf(record: { state?: unknown }): ViewState | null {
  return (record.state ?? null) as ViewState | null
}

function required<T>(data: T | undefined, message: string): T {
  if (data === undefined) throw new Error(message)
  return data
}

const isMissing = (error: unknown) => error instanceof ApiProblem && error.status === 404

/** A missing or forbidden view is final: show the 404 page at once instead of retrying. */
function retryUnlessMissing(failureCount: number, error: unknown) {
  return !isMissing(error) && failureCount < 3
}

export function savedViewsQueryOptions(workspaceId: string, client: ApiClient = apiClient) {
  return {
    queryKey: queryKeys.views(workspaceId),
    queryFn: async (): Promise<SavedView[]> => {
      const { data } = await listViews({ client, path: { workspace_id: workspaceId }, throwOnError: true })
      return required(data, 'Views response was empty.') as unknown as SavedView[]
    },
  }
}

export function savedViewQueryOptions(workspaceId: string, viewId: string, client: ApiClient = apiClient) {
  return {
    queryKey: queryKeys.view(workspaceId, viewId),
    queryFn: async (): Promise<SavedView> => {
      const { data } = await getView({ client, path: { workspace_id: workspaceId, view_id: viewId }, throwOnError: true })
      return required(data, 'View response was empty.') as unknown as SavedView
    },
  }
}

export function viewPreferenceQueryOptions(workspaceId: string, pageKey: PageKey, client: ApiClient = apiClient) {
  return {
    queryKey: queryKeys.viewPreference(workspaceId, pageKey),
    queryFn: async (): Promise<ViewState | null> => {
      try {
        const { data } = await getViewPreference({ client, path: { workspace_id: workspaceId, page_key: pageKey }, throwOnError: true })
        return data ? viewStateOf(data) : null
      } catch (error) {
        if (isMissing(error)) return null
        throw error
      }
    },
  }
}

export async function savePreference(workspaceId: string, pageKey: PageKey, state: ViewState, client: ApiClient = apiClient) {
  const { data } = await putViewPreference({
    client,
    path: { workspace_id: workspaceId, page_key: pageKey },
    body: { state: toApiState(state) },
    throwOnError: true,
  })
  return data
}

export function useSavedViews(workspaceId: string) {
  return useQuery(savedViewsQueryOptions(workspaceId))
}

export function useSavedView(workspaceId: string, viewId: string | undefined) {
  return useQuery({ ...savedViewQueryOptions(workspaceId, viewId ?? ''), enabled: Boolean(viewId), retry: retryUnlessMissing })
}

/**
 * `data` is the stored state, or null when the page has none yet (404) or it is unreadable.
 * A 404 is mapped to `null` inside the query function and never retries; any other error
 * (e.g. a transient 500) gets a bounded retry so a one-off failure cannot be mistaken for
 * "no preference" — callers should treat only a successful query as "loaded" (see Task 10).
 */
export function useViewPreference(workspaceId: string, pageKey: PageKey, enabled = true) {
  return useQuery({ ...viewPreferenceQueryOptions(workspaceId, pageKey), enabled, retry: retryUnlessMissing })
}

export function useCreateView(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (input: ViewCreateInput): Promise<SavedView> => {
      const { data } = await createView({
        client: apiClient,
        path: { workspace_id: workspaceId },
        body: { ...input, state: toApiState(input.state) },
        throwOnError: true,
      })
      return required(data, 'Create view response was empty.') as unknown as SavedView
    },
    onSuccess: (view) => {
      queryClient.setQueryData(queryKeys.view(workspaceId, view.id), view)
      void queryClient.invalidateQueries({ queryKey: queryKeys.views(workspaceId), exact: true })
    },
  })
}

export function useUpdateView(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async ({ viewId, body }: { viewId: string; body: ViewUpdateInput }): Promise<SavedView> => {
      const { state, ...fields } = body
      const { data } = await updateView({
        client: apiClient,
        path: { workspace_id: workspaceId, view_id: viewId },
        body: state ? { ...fields, state: toApiState(state) } : fields,
        throwOnError: true,
      })
      return required(data, 'Update view response was empty.') as unknown as SavedView
    },
    onSuccess: (view) => {
      queryClient.setQueryData(queryKeys.view(workspaceId, view.id), view)
      void queryClient.invalidateQueries({ queryKey: queryKeys.views(workspaceId), exact: true })
    },
  })
}

export function useDeleteView(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (viewId: string) => {
      await deleteView({ client: apiClient, path: { workspace_id: workspaceId, view_id: viewId }, throwOnError: true })
      return viewId
    },
    onSuccess: (viewId) => {
      queryClient.removeQueries({ queryKey: queryKeys.view(workspaceId, viewId) })
      void queryClient.invalidateQueries({ queryKey: queryKeys.views(workspaceId), exact: true })
    },
  })
}

type ListSnapshot = { previous: SavedView[] | undefined }
type FavoriteSnapshot = ListSnapshot & { previousView: SavedView | undefined }

export function useSetFavorite(workspaceId: string) {
  const queryClient = useQueryClient()
  const listKey = queryKeys.views(workspaceId)
  return useMutation<void, Error, { viewId: string; favorite: boolean }, FavoriteSnapshot>({
    mutationFn: async ({ viewId, favorite }) => {
      const path = { workspace_id: workspaceId, view_id: viewId }
      if (favorite) await favoriteView({ client: apiClient, path, throwOnError: true })
      else await unfavoriteView({ client: apiClient, path, throwOnError: true })
    },
    onMutate: async ({ viewId, favorite }) => {
      const viewKey = queryKeys.view(workspaceId, viewId)
      await Promise.all([queryClient.cancelQueries({ queryKey: listKey, exact: true }), queryClient.cancelQueries({ queryKey: viewKey })])
      const previous = queryClient.getQueryData<SavedView[]>(listKey)
      const previousView = queryClient.getQueryData<SavedView>(viewKey)
      // like the server, a new favorite goes last (max + 1), so it does not jump in the sidebar when the list refetches
      const positions = (previous ?? []).filter((view) => view.is_favorite && view.id !== viewId).map((view) => view.favorite_position ?? 0)
      const position = favorite ? (positions.length > 0 ? Math.max(...positions) + 1 : 0) : null
      const patch = { is_favorite: favorite, favorite_position: position }
      queryClient.setQueryData<SavedView[]>(listKey, (views) =>
        views?.map((view) => view.id === viewId ? { ...view, ...patch } : view))
      // the open view's header star reads the detail query
      queryClient.setQueryData<SavedView>(viewKey, (view) => view && { ...view, ...patch })
      return { previous, previousView }
    },
    onError: (_error, { viewId }, snapshot) => {
      if (snapshot?.previous) queryClient.setQueryData(listKey, snapshot.previous)
      if (snapshot?.previousView) queryClient.setQueryData(queryKeys.view(workspaceId, viewId), snapshot.previousView)
    },
    // the list and the open view both carry `is_favorite`
    onSettled: () => void queryClient.invalidateQueries({ queryKey: listKey }),
  })
}

export function useReorderFavorites(workspaceId: string) {
  const queryClient = useQueryClient()
  const listKey = queryKeys.views(workspaceId)
  return useMutation<void, Error, string[], ListSnapshot>({
    mutationFn: async (viewIds) => {
      await reorderViewFavorites({ client: apiClient, path: { workspace_id: workspaceId }, body: { view_ids: viewIds }, throwOnError: true })
    },
    onMutate: async (viewIds) => {
      await queryClient.cancelQueries({ queryKey: listKey, exact: true })
      const previous = queryClient.getQueryData<SavedView[]>(listKey)
      queryClient.setQueryData<SavedView[]>(listKey, (views) => views?.map((view) => {
        const position = viewIds.indexOf(view.id)
        return position === -1 ? view : { ...view, favorite_position: position }
      }))
      return { previous }
    },
    onError: (_error, _input, snapshot) => {
      if (snapshot?.previous) queryClient.setQueryData(listKey, snapshot.previous)
    },
    onSettled: () => void queryClient.invalidateQueries({ queryKey: listKey, exact: true }),
  })
}

export function usePutViewPreference(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ pageKey, state }: { pageKey: PageKey; state: ViewState }) => savePreference(workspaceId, pageKey, state),
    onSuccess: (_record, { pageKey, state }) => {
      queryClient.setQueryData(queryKeys.viewPreference(workspaceId, pageKey), state)
    },
  })
}
