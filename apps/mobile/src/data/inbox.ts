import type { NotificationRecord } from './orbit'

/** Where a notification leads in the app: its conversation, page or task, as on the web. */
export function notificationTarget(notification: NotificationRecord): string | null {
  if (notification.kind === 'chat_mentioned' && notification.chat_conversation_id) return `/chat/${notification.chat_conversation_id}`
  if (notification.page_id) return `/docs/${notification.page_id}`
  return notification.task_id ? `/tasks/${notification.task_id}` : null
}

/** The Activity "Mentions" tab. */
export const isMention = (notification: NotificationRecord) =>
  ['comment_mentioned', 'description_mentioned', 'page_comment_mentioned', 'page_mentioned', 'chat_mentioned'].includes(notification.kind)

/** The web inbox's wording (`notificationCopy` in `features/inbox/api.ts`). */
export function notificationCopy(notification: NotificationRecord, pageTitle: string | undefined, actorName: string | undefined): { title: string; body: string } {
  const actor = actorName ?? 'Someone'
  const page = pageTitle === undefined ? 'a page' : `“${pageTitle.trim() || 'Untitled'}”`
  switch (notification.kind) {
    case 'chat_mentioned':
      return { title: `${actor} mentioned you in chat`, body: 'Open the conversation to see the message.' }
    case 'page_mentioned':
      return { title: `${actor} mentioned you in ${page}`, body: 'Open the page to see where.' }
    case 'page_comment_mentioned':
      return { title: 'You were mentioned in a comment', body: `Someone mentioned you in a comment on ${page}.` }
    case 'comment_mentioned':
      return { title: 'You were mentioned', body: 'Someone mentioned you in a task comment.' }
    case 'description_mentioned':
      return { title: 'You were mentioned', body: 'Someone mentioned you in a task description.' }
    default:
      return { title: 'You were assigned a task', body: 'A task was assigned to you.' }
  }
}
