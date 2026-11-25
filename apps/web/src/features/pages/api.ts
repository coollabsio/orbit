import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { apiClient } from '@/api/client'
import { queryKeys } from '@/api/queryKeys'
import { createCustomPage, deleteCustomPage, listCustomPages, updateCustomPage } from '@orbit/api/generated/sdk.gen'
import type { CreateCustomPageBody, CustomPage, CustomPageUpdateBody } from '@orbit/api/generated/types.gen'
import { ApiProblem } from '@orbit/api/problem'

/** The workspace's custom pages that the user sees, whether they may manage them, and the origins Orbit may frame. */
export function useCustomPages(workspaceId: string, enabled = true) {
  return useQuery({
    queryKey: queryKeys.customPages(workspaceId),
    enabled: enabled && workspaceId !== '',
    queryFn: async () => {
      const { data } = await listCustomPages({ client: apiClient, path: { workspace_id: workspaceId }, throwOnError: true })
      if (!data) throw new Error('Custom pages response was empty.')
      return data
    },
  })
}

export function useSaveCustomPage(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    // an unchanged URL is not sent: the server checks a sent URL's origin, and a page whose origin left the list keeps its URL
    mutationFn: async (input: { page?: CustomPage; body: CreateCustomPageBody }): Promise<CustomPage> => {
      const { data } = input.page
        ? await updateCustomPage({ client: apiClient, path: { workspace_id: workspaceId, page_id: input.page.id }, body: { ...input.body, url: input.body.url === input.page.url ? undefined : input.body.url, expected_version: input.page.version } satisfies CustomPageUpdateBody, throwOnError: true })
        : await createCustomPage({ client: apiClient, path: { workspace_id: workspaceId }, body: input.body, throwOnError: true })
      if (!data) throw new Error('Custom page response was empty.')
      return data
    },
    onSettled: () => void queryClient.invalidateQueries({ queryKey: queryKeys.customPages(workspaceId) }),
  })
}

export function useDeleteCustomPage(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (page: CustomPage) => deleteCustomPage({ client: apiClient, path: { workspace_id: workspaceId, page_id: page.id }, query: { expected_version: page.version }, throwOnError: true }),
    onSettled: () => void queryClient.invalidateQueries({ queryKey: queryKeys.customPages(workspaceId) }),
  })
}

/** A user-facing sentence for a failed save. */
export function customPageError(error: unknown): string {
  if (error instanceof ApiProblem) {
    if (error.code === 'custom_page_origin_not_allowed') return 'This site is not on the list of allowed origins. Ask the administrator of this Orbit to add it in Admin → General.'
    if (error.code === 'custom_page_slug_taken') return 'Another page already uses this address.'
    if (error.code === 'validation_failed') return error.detail === 'url' ? 'Enter an https URL.' : error.detail === 'slug' ? 'Use lowercase letters, digits and single hyphens for the address.' : 'Check the fields.'
    if (error.status === 403) return 'Only workspace owners and admins can manage custom pages.'
    if (error.status === 409) return 'This page changed. Try again.'
  }
  return 'Could not save the page.'
}
