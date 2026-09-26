import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { apiClient } from '@/api/client'
import { queryKeys } from '@/api/queryKeys'
import {
  listNotifications,
  readAllNotifications,
  readNotification,
} from '@/api/generated/sdk.gen'
import type { NotificationRecord } from '@/api/generated/types.gen'
import { fetchAllPages } from '@/api/pagination'

function required<T>(data: T | undefined, message: string): T {
  if (data === undefined) throw new Error(message)
  return data
}

export function useNotifications(workspaceId: string, unread = false) {
  return useQuery({
    queryKey: queryKeys.notifications(workspaceId, unread),
    queryFn: async () => {
      const page = await fetchAllPages(async (cursor) => {
        const { data } = await listNotifications({
          client: apiClient,
          path: { workspace_id: workspaceId },
          query: { unread, cursor, limit: 100 },
          throwOnError: true,
        })
        return required(data, 'Notifications response was empty.')
      })
      return page.items
    },
  })
}

export function useMarkNotificationRead(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (notificationId: string) => {
      const { data } = await readNotification({
        client: apiClient,
        path: { workspace_id: workspaceId, notification_id: notificationId },
        throwOnError: true,
      })
      return required(data, 'Read notification response was empty.')
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.workspace(workspaceId) })
    },
  })
}

export function useMarkAllNotificationsRead(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async () => {
      const { data } = await readAllNotifications({
        client: apiClient,
        path: { workspace_id: workspaceId },
        throwOnError: true,
      })
      return required(data, 'Read-all response was empty.')
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.workspace(workspaceId) })
    },
  })
}

/** Where a notification leads: the task, the page with the comment thread open, or the page at the mention. */
export function notificationTarget(notification: NotificationRecord): string | null {
  if (notification.kind === 'page_mentioned' && notification.page_id) {
    const block = notification.page_block_id ? `?block=${encodeURIComponent(notification.page_block_id)}` : ''
    return `/docs/${notification.page_id}${block}`
  }
  if (notification.kind === 'page_comment_mentioned' && notification.page_id) {
    const thread = notification.page_thread_id ? `?thread=${encodeURIComponent(notification.page_thread_id)}` : ''
    return `/docs/${notification.page_id}${thread}`
  }
  return notification.task_id ? `/tasks/${notification.task_id}` : null
}

/** Mentions in task comments, page comments and page bodies. */
export function isMention(notification: NotificationRecord): boolean {
  return notification.kind === 'comment_mentioned' || notification.kind === 'page_comment_mentioned' || notification.kind === 'page_mentioned'
}

export function notificationCopy(notification: NotificationRecord, pageTitle?: string, actorName?: string): { title: string; body: string } {
  if (notification.kind === 'page_mentioned') {
    const page = pageTitle === undefined ? 'a page' : `“${pageTitle.trim() || 'Untitled'}”`
    return { title: `${actorName ?? 'Someone'} mentioned you in ${page}`, body: 'Open the page to see where.' }
  }
  if (notification.kind === 'page_comment_mentioned') {
    const page = pageTitle === undefined ? 'a page' : `“${pageTitle.trim() || 'Untitled'}”`
    return { title: 'You were mentioned in a comment', body: `Someone mentioned you in a comment on ${page}.` }
  }
  if (notification.kind === 'comment_mentioned') {
    return { title: 'You were mentioned', body: 'Someone mentioned you in a task comment.' }
  }
  return { title: 'You were assigned a task', body: 'A task was assigned to you.' }
}
