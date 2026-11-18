import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { apiClient } from '@/api/client'
import { queryKeys } from '@/api/queryKeys'
import { blockChatUser, deleteReportedChatMessage, listChatBlocks, listChatReports, reportChatMessage, unblockChatUser } from '@orbit/api/generated/sdk.gen'
import type { BlockedUserRecord, ChatReportReason } from '@orbit/api/generated/types.gen'

/** The people the user blocked belong to the account, not to a workspace: outside the `['workspace', id]` prefix. */
export const chatBlocksKey = ['chat-blocks'] as const

const chatReportsKey = (workspaceId: string) => [...queryKeys.workspace(workspaceId), 'chat-reports'] as const

const NO_BLOCKS: BlockedUserRecord[] = []
const NO_IDS: ReadonlySet<string> = new Set()

export function useChatBlocks(enabled = true) {
  return useQuery({
    queryKey: chatBlocksKey,
    queryFn: async () => {
      const { data } = await listChatBlocks({ client: apiClient, throwOnError: true })
      return data ?? NO_BLOCKS
    },
    staleTime: 5 * 60_000,
    enabled,
  })
}

/** The ids of the people the user blocked. Empty while the list loads or when it fails: then nothing is hidden. */
export function useBlockedIds(enabled = true): ReadonlySet<string> {
  const { data } = useChatBlocks(enabled)
  return data ? new Set(data.map((block) => block.user_id)) : NO_IDS
}

/** Blocks (`blocked: true`) or unblocks a person. Doing it twice changes nothing on the server. */
export function useSetBlocked() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async ({ userId, blocked }: { userId: string; blocked: boolean }) => {
      const options = { client: apiClient, path: { user_id: userId }, throwOnError: true } as const
      await (blocked ? blockChatUser(options) : unblockChatUser(options))
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: chatBlocksKey }),
  })
}

export const REPORT_NOTE_MAX = 500

export const REPORT_REASONS: Record<ChatReportReason, string> = {
  spam: 'Spam',
  harassment: 'Harassment',
  inappropriate: 'Inappropriate content',
  other: 'Other',
}

export function useReportMessage(workspaceId: string) {
  return useMutation({
    mutationFn: async ({ messageId, reason, note }: { messageId: string; reason: ChatReportReason; note: string }) => {
      const { data } = await reportChatMessage({
        client: apiClient,
        path: { workspace_id: workspaceId, message_id: messageId },
        body: { reason, ...(note.trim() ? { note: note.trim() } : {}) },
        throwOnError: true,
      })
      return data
    },
  })
}

/** The reports of the workspace, newest first. Only for members who can manage chat (`chat.manage`). */
export function useChatReports(workspaceId: string, enabled: boolean) {
  return useQuery({
    queryKey: chatReportsKey(workspaceId),
    queryFn: async () => {
      const { data } = await listChatReports({ client: apiClient, path: { workspace_id: workspaceId }, throwOnError: true })
      return data ?? []
    },
    enabled,
  })
}

/** Deletes the message of a report, also in a direct message that the manager cannot read. */
export function useDeleteReportedMessage(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (reportId: string) => {
      await deleteReportedChatMessage({ client: apiClient, path: { workspace_id: workspaceId, report_id: reportId }, throwOnError: true })
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: chatReportsKey(workspaceId) }),
  })
}
