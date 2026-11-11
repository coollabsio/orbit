import { useMemo } from 'react'
import { useLocation, useNavigate } from 'react-router'
import type { NotificationRecord } from '@orbit/api/generated/types.gen'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { useMembers } from '@/features/workspaces/api'
import { usePageTree } from '@/features/docs/api/pages'
import { notificationTarget, useMarkNotificationRead } from '@/features/inbox/api'

/** What a row needs to show and open notifications: the actors, the page titles and the read mutation, loaded once for the list. */
export function useNotificationRows(notifications: NotificationRecord[]) {
  const { workspace } = useWorkspace()
  const navigate = useNavigate()
  const location = useLocation()
  const members = useMembers(workspace.id)
  const pageTree = usePageTree(workspace.id, notifications.some((notification) => notification.page_id))
  const markRead = useMarkNotificationRead(workspace.id)
  // one lookup for each row, not a search through every member and page
  const memberById = useMemo(() => new Map((members.data ?? []).map((member) => [member.id, member])), [members.data])
  const titleById = useMemo(() => new Map((pageTree.data ?? []).map((page) => [page.id, page.title])), [pageTree.data])
  return {
    actor: (notification: NotificationRecord) => memberById.get(notification.actor_user_id),
    pageTitle: (notification: NotificationRecord) => (notification.page_id ? titleById.get(notification.page_id) : undefined),
    markRead: (notification: NotificationRecord) => {
      if (!notification.read_at) markRead.mutate(notification.id)
    },
    /** Marks the notification read and opens what it is about. A task gets a way back to where the user was. */
    open: (notification: NotificationRecord) => {
      if (!notification.read_at) markRead.mutate(notification.id)
      const target = notificationTarget(notification)
      if (!target) return
      if (notification.page_id || notification.chat_conversation_id || notification.import_id) {
        navigate(target)
        return
      }
      const params = new URLSearchParams({ redirect: `${location.pathname}${location.search}` })
      navigate(`${target}?${params}`)
    },
  }
}
