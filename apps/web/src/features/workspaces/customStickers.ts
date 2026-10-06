import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { apiClient } from '@/api/client'
import { createChatSticker, deleteChatSticker, listChatStickers } from '@/api/generated/sdk.gen'
import type { CustomStickerRecord } from '@/api/generated/types.gen'
import { queryKeys } from '@/api/queryKeys'

/**
 * The stickers of the workspace, like the server stickers of Discord. They live here, beside the custom emoji, because
 * chat sends them and the workspace settings manage them, and both features may read this one.
 */
export function useCustomStickers(workspaceId: string, enabled = true) {
  return useQuery({
    queryKey: queryKeys.customStickers(workspaceId),
    queryFn: async () => {
      const { data } = await listChatStickers({ client: apiClient, path: { workspace_id: workspaceId }, throwOnError: true })
      if (!data) throw new Error('Stickers response was empty.')
      return data
    },
    // The chat socket says when the list changed (`stickers.changed`, `applyChatEvent`): no reason to ask again before.
    staleTime: 60 * 60_000,
    enabled,
  })
}

/** Adds a sticker from an image file. The server checks the name, the type and the size again. */
export function useCreateCustomSticker(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async ({ name, file }: { name: string; file: File }) => {
      const { data } = await createChatSticker({ client: apiClient, path: { workspace_id: workspaceId }, query: { name }, body: { file }, throwOnError: true })
      if (!data) throw new Error('Create sticker response was empty.')
      return data
    },
    onSuccess: () => queryClient.invalidateQueries({ queryKey: queryKeys.customStickers(workspaceId) }),
  })
}

/** Messages that were sent with a deleted sticker say that it was deleted. */
export function useDeleteCustomSticker(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (sticker: CustomStickerRecord) => {
      await deleteChatSticker({ client: apiClient, path: { workspace_id: workspaceId, sticker_id: sticker.id }, throwOnError: true })
    },
    onSuccess: () => queryClient.invalidateQueries({ queryKey: queryKeys.customStickers(workspaceId) }),
  })
}
