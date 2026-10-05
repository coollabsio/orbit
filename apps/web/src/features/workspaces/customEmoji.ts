import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { apiClient } from '@/api/client'
import { createChatEmoji, deleteChatEmoji, listChatEmoji } from '@/api/generated/sdk.gen'
import type { CustomEmojiRecord } from '@/api/generated/types.gen'
import { queryKeys } from '@/api/queryKeys'

const byName = (emoji: CustomEmojiRecord[]): ReadonlyMap<string, CustomEmojiRecord> => new Map(emoji.map((entry) => [entry.name, entry]))

function customEmojiQuery(workspaceId: string) {
  return {
    queryKey: queryKeys.customEmoji(workspaceId),
    queryFn: async () => {
      const { data } = await listChatEmoji({ client: apiClient, path: { workspace_id: workspaceId }, throwOnError: true })
      if (!data) throw new Error('Custom emoji response was empty.')
      return data
    },
    // The chat socket says when the list changed (`emoji.changed`, `applyChatEvent`): no reason to ask again before.
    staleTime: 60 * 60_000,
  }
}

/**
 * The custom emoji of the workspace, like the server emoji of Discord. They live here, with the workspace, because
 * chat shows them and the workspace settings manage them, and both features may read this one.
 */
export function useCustomEmoji(workspaceId: string, enabled = true) {
  return useQuery({ ...customEmojiQuery(workspaceId), enabled })
}

/** The same list by name, for `CustomEmojiContext`. The map changes only when the list does. */
export function useCustomEmojiByName(workspaceId: string, enabled = true) {
  return useQuery({ ...customEmojiQuery(workspaceId), enabled, select: byName })
}

/** Adds an emoji from an image file. The server checks the name, the type and the size again. */
export function useCreateCustomEmoji(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async ({ name, file }: { name: string; file: File }) => {
      const { data } = await createChatEmoji({ client: apiClient, path: { workspace_id: workspaceId }, query: { name }, body: { file }, throwOnError: true })
      if (!data) throw new Error('Create emoji response was empty.')
      return data
    },
    onSuccess: () => queryClient.invalidateQueries({ queryKey: queryKeys.customEmoji(workspaceId) }),
  })
}

/** Messages and reactions that name a deleted emoji show `:name:` as text again. */
export function useDeleteCustomEmoji(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (emoji: CustomEmojiRecord) => {
      await deleteChatEmoji({ client: apiClient, path: { workspace_id: workspaceId, emoji_id: emoji.id }, throwOnError: true })
    },
    onSuccess: () => queryClient.invalidateQueries({ queryKey: queryKeys.customEmoji(workspaceId) }),
  })
}
