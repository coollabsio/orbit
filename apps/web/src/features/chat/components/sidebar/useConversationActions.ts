import { toast } from 'sonner'
import { confirmAction } from '@/components/common/confirmAction'
import { useChatContext } from '@/features/chat/api/chatContext'
import { useArchiveChannel, useLeaveChannel, useMarkRead, useSetFavorite, useSetNotify } from '@/features/chat/api/mutations'
import { useConversationState } from '@/features/chat/api/queries'
import type { Conversation, NotifyLevel } from '@/features/chat/api/types'
import { conversationPath } from '@/features/chat/chatRoutes'
import { useMembers } from '@/features/workspaces/api'

/** The current user is a workspace owner or admin: they manage every channel and the shared categories. */
export function useIsChatAdmin(): boolean {
  const { workspaceId, currentUserId } = useChatContext()
  const role = useMembers(workspaceId).data?.find((member) => member.id === currentUserId)?.role
  return role === 'Owner' || role === 'Admin'
}

/**
 * What the row menu and the header menu can do with a conversation, and whether the user may. Write errors show a
 * toast with the action's name; the optimistic change rolls back in the mutation.
 */
export function useConversationActions(conversation: Conversation) {
  const { currentUserId } = useChatContext()
  const admin = useIsChatAdmin()
  const state = useConversationState(conversation.id).data
  const markRead = useMarkRead()
  const setFavorite = useSetFavorite()
  const setNotify = useSetNotify()
  const leaveChannel = useLeaveChannel()
  const archiveChannel = useArchiveChannel()

  const id = conversation.id
  const channel = conversation.kind !== 'dm'
  const canManage = channel && !conversation.archived && (admin || conversation.createdBy === currentUserId)
  const failed = (action: string) => () => void toast.error(`Could not ${action}. Try again.`)

  return {
    state,
    /** Edit the channel. */
    canManage,
    canArchive: canManage && !conversation.isDefault,
    canLeave: channel && conversation.isMember && !conversation.isDefault,
    markRead: () => markRead.mutate(id, { onError: failed('mark as read') }),
    toggleFavorite: () => setFavorite.mutate({ conversationId: id, favorite: !state?.favorite }, { onError: failed(state?.favorite ? 'remove the star' : 'add the star') }),
    setNotify: (notify: NotifyLevel) => setNotify.mutate({ conversationId: id, notify }, { onError: failed('change the notification setting') }),
    copyLink: () => {
      navigator.clipboard
        .writeText(`${window.location.origin}${conversationPath(id)}`)
        .then(() => toast.success('Link copied'))
        .catch(failed('copy the link'))
    },
    leave: async () => {
      // leaving a private channel removes it for the user: they need an invitation to come back
      if (conversation.kind === 'private') {
        const confirmed = await confirmAction({
          title: `Leave ${conversation.name}?`,
          description: 'This is a private channel. To come back, a member has to add you again.',
          confirmLabel: 'Leave channel',
          danger: true,
        })
        if (!confirmed) return
      }
      leaveChannel.mutate(id, { onError: failed('leave the channel') })
    },
    archive: async () => {
      const confirmed = await confirmAction({
        title: `Archive #${conversation.name}?`,
        description: 'The channel becomes read-only and leaves the sidebar for everyone. Its messages stay searchable.',
        confirmLabel: 'Archive channel',
        danger: true,
      })
      if (confirmed) archiveChannel.mutate(id, { onError: failed('archive the channel') })
    },
  }
}

export type ConversationActions = ReturnType<typeof useConversationActions>
