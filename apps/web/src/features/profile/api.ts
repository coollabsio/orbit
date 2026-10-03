import { queryOptions, type QueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'
import { apiClient } from '@/api/client'
import { getMemberProfile, putMemberNote } from '@/api/generated/sdk.gen'
import type { MemberProfile } from '@/api/generated/types.gen'
import { ApiProblem } from '@/api/problem'
import { queryKeys } from '@/api/queryKeys'
import { noteToSave } from './profileLib'

/**
 * What the profile popover shows of a member beside the members list. `null` for somebody who is not a member of the
 * workspace (any more). One factory for the prefetch of a trigger and for the popover, so the popover opens with data.
 */
export function memberProfileQueryOptions(workspaceId: string, userId: string) {
  return queryOptions({
    queryKey: queryKeys.memberProfile(workspaceId, userId),
    queryFn: async (): Promise<MemberProfile | null> => {
      try {
        const { data } = await getMemberProfile({ client: apiClient, path: { workspace_id: workspaceId, user_id: userId }, throwOnError: true })
        return data ?? null
      } catch (error) {
        if (error instanceof ApiProblem && error.status === 404) return null
        throw error
      }
    },
  })
}

async function sendNote(workspaceId: string, userId: string, body: string) {
  await putMemberNote({ client: apiClient, path: { workspace_id: workspaceId, user_id: userId }, body: { body }, throwOnError: true })
}

/**
 * Saves the private note about a member when it differs from the saved one. The cache has the new note at once; a
 * refused request puts the previous note back and says so. Returns whether a request was sent.
 */
export async function saveMemberNote(queryClient: QueryClient, workspaceId: string, userId: string, draft: string, send = sendNote): Promise<boolean> {
  const key = memberProfileQueryOptions(workspaceId, userId).queryKey
  const previous = queryClient.getQueryData(key)
  if (!previous) return false
  const body = noteToSave(previous.note, draft)
  if (body === null) return false
  const optimistic = body || null
  // an answer that is on its way has the old note
  void queryClient.cancelQueries({ queryKey: key })
  const next: MemberProfile = { ...previous, note: optimistic }
  queryClient.setQueryData(key, next)
  try {
    await send(workspaceId, userId, body)
  } catch {
    // a newer save may have written the cache since; only this save's own note goes back
    queryClient.setQueryData(key, (current): MemberProfile | null | undefined => (current && current.note === optimistic ? { ...current, note: previous.note } : current))
    toast.error('Could not save your note. Try again.')
  }
  return true
}
