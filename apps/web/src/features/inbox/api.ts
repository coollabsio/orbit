import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { apiClient } from '@/api/client'
import { queryKeys } from '@/api/queryKeys'
import {
  listNotifications,
  patchNotification,
  readAllNotifications,
} from '@/api/generated/sdk.gen'
import type { NotificationPatchBody, NotificationRecord } from '@/api/generated/types.gen'
import { fetchAllPages } from '@/api/pagination'
import { taskPath } from '@/lib/taskLinks'

function required<T>(data: T | undefined, message: string): T {
  if (data === undefined) throw new Error(message)
  return data
}

export type NotificationState = 'inbox' | 'snoozed' | 'archived'

export function useNotifications(workspaceId: string, unread = false, state: NotificationState = 'inbox', enabled = true) {
  return useQuery({
    queryKey: queryKeys.notifications(workspaceId, unread, state),
    enabled,
    queryFn: async () => {
      const page = await fetchAllPages(async (cursor) => {
        const { data } = await listNotifications({
          client: apiClient,
          path: { workspace_id: workspaceId },
          query: { state, unread, cursor, limit: 100 },
          throwOnError: true,
        })
        return required(data, 'Notifications response was empty.')
      })
      return page.items
    },
  })
}

/** Whether the inbox has an unread notification: one row is enough to know. */
export function useHasUnreadNotifications(workspaceId: string) {
  return useQuery({
    // under the list's key, so what refreshes the inbox refreshes this
    queryKey: [...queryKeys.notifications(workspaceId, true), 'any'] as const,
    queryFn: async () => {
      const { data } = await listNotifications({
        client: apiClient,
        path: { workspace_id: workspaceId },
        query: { unread: true, limit: 1 },
        throwOnError: true,
      })
      return required(data, 'Notifications response was empty.').items.length > 0
    },
  })
}

/** Read or unread, snooze (`snoozed_until: null` ends it) and archive, for one notification. */
export function usePatchNotification(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async ({ id, body }: { id: string; body: NotificationPatchBody }) => {
      const { data } = await patchNotification({
        client: apiClient,
        path: { workspace_id: workspaceId, notification_id: id },
        body,
        throwOnError: true,
      })
      return required(data, 'Notification response was empty.')
    },
    onSuccess: () => queryClient.invalidateQueries({ queryKey: [...queryKeys.workspace(workspaceId), 'notifications'] }),
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
    // Stay pending until the unread count refreshes, so the button does not re-enable for a moment.
    onSuccess: () => queryClient.invalidateQueries({ queryKey: queryKeys.workspace(workspaceId) }),
  })
}

/** Where a notification leads: the task, the milestone, the triage queue, the page with the comment thread open, the page at the mention, or the chat message. */
export function notificationTarget(notification: NotificationRecord): string | null {
  if (notification.kind === 'chat_mentioned' && notification.chat_conversation_id) {
    const message = notification.chat_message_id ? `?m=${encodeURIComponent(notification.chat_message_id)}` : ''
    return `/chat/${notification.chat_conversation_id}${message}`
  }
  if (notification.kind === 'page_mentioned' && notification.page_id) {
    const block = notification.page_block_id ? `?block=${encodeURIComponent(notification.page_block_id)}` : ''
    return `/docs/${notification.page_id}${block}`
  }
  if (notification.kind === 'page_comment_mentioned' && notification.page_id) {
    const thread = notification.page_thread_id ? `?thread=${encodeURIComponent(notification.page_thread_id)}` : ''
    return `/docs/${notification.page_id}${thread}`
  }
  if (notification.kind === 'milestone_update_posted') {
    return notification.milestone_id && notification.milestone_project_id
      ? `/tasks/projects/${notification.milestone_project_id}/milestones/${notification.milestone_id}`
      : null
  }
  // a new task in triage opens in the queue, where it is accepted or declined
  if (notification.kind === 'task_triage_new') return '/tasks/triage'
  // the task page replaces the id with the identifier once the task loads
  return notification.task_id ? taskPath({ id: notification.task_id }) : null
}

/** Mentions in task comments, page comments, page bodies and chat. */
export function isMention(notification: NotificationRecord): boolean {
  return ['comment_mentioned', 'task_mentioned', 'page_comment_mentioned', 'page_mentioned', 'chat_mentioned'].includes(notification.kind)
}

/** What happened, as the second line of a task row shows it after the actor. */
const TASK_EVENTS: Record<string, string> = {
  task_assigned: 'assigned you',
  comment_mentioned: 'mentioned you in a comment',
  task_mentioned: 'mentioned you in the description',
  task_commented: 'commented',
  task_status_changed: 'changed the status',
  task_blocked: 'the task is now blocked',
  task_unblocked: 'the task is no longer blocked',
  task_triage_new: 'new in triage',
}

/** The two lines of an inbox row. A task row is the task (identifier and title) and its latest event. */
export function notificationCopy(notification: NotificationRecord, pageTitle?: string, actorName?: string): { title: string; body: string } {
  if (notification.task_id) {
    const title = notification.task_title?.trim() || 'Untitled'
    return {
      title: notification.task_identifier ? `${notification.task_identifier} ${title}` : title,
      // a kind of a newer server
      body: `${actorName ?? (notification.actor_user_id ? 'Someone' : 'GitHub')} · ${TASK_EVENTS[notification.kind] ?? 'updated the task'}`,
    }
  }
  const body = (text: string) => (actorName ? `${actorName} · ${text}` : text)
  if (notification.kind === 'milestone_update_posted') {
    return { title: notification.milestone_name?.trim() || 'Milestone', body: body('posted an update') }
  }
  if (notification.kind === 'chat_mentioned') {
    return { title: `${actorName ?? 'Someone'} mentioned you in chat`, body: body('Open the conversation to see the message.') }
  }
  const page = pageTitle === undefined ? 'a page' : `“${pageTitle.trim() || 'Untitled'}”`
  if (notification.kind === 'page_mentioned') {
    return { title: `${actorName ?? 'Someone'} mentioned you in ${page}`, body: body('Open the page to see where.') }
  }
  return { title: 'You were mentioned in a comment', body: body(`Someone mentioned you in a comment on ${page}.`) }
}

/** The snooze choices at `now`: in an hour, tomorrow at 9:00 and next Monday at 9:00 (local time). */
export function snoozePresets(now: Date): { label: string; until: Date }[] {
  const morning = (daysAhead: number) => new Date(now.getFullYear(), now.getMonth(), now.getDate() + daysAhead, 9)
  return [
    { label: 'In 1 hour', until: new Date(now.getTime() + 60 * 60 * 1000) },
    { label: 'Tomorrow', until: morning(1) },
    // Monday of the next week; on a Monday that is seven days ahead
    { label: 'Next week', until: morning(((8 - now.getDay()) % 7) || 7) },
  ]
}
