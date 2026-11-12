import type { NotificationRecord } from './orbit'
import { pullRoute } from './pull-route'

const IMPORT_NAMES: Record<string, string> = { asana: 'Asana', linear: 'Linear', trello: 'Trello', notion: 'Notion', obsidian: 'Obsidian' }

/** Where a notification leads in the app: its conversation, page, task or import, as on the web. */
export function notificationTarget(notification: NotificationRecord): string | null {
  if (notification.kind.startsWith('github_')) {
    const [owner, repo] = (notification.github_repository ?? '').split('/')
    return owner && repo && notification.github_pull_number ? pullRoute({ owner, repo, number: notification.github_pull_number }) : null
  }
  if (notification.kind === 'import_finished') {
    const source = notification.import_source ?? ''
    return IMPORT_NAMES[source] && notification.import_id ? `/settings/import/${source}/${notification.import_id}` : null
  }
  if (notification.kind === 'chat_mentioned' && notification.chat_conversation_id) return `/chat/${notification.chat_conversation_id}`
  if (notification.page_id) return `/docs/${notification.page_id}`
  return notification.task_id ? `/tasks/${notification.task_id}` : null
}

/** The second line of a notice (the web's `notificationLine`): the Orbit actor and the body, except for imports (the
 *  user's own) and GitHub notices (their title names the GitHub account, which may have no Orbit account). */
export function notificationLine(notification: NotificationRecord, body: string, actorName: string | undefined): string {
  if (!actorName || notification.import_id || notification.kind.startsWith('github_')) return body
  return `${actorName} · ${body}`
}

/** The Activity "Mentions" tab. */
export const isMention = (notification: NotificationRecord) =>
  ['comment_mentioned', 'description_mentioned', 'page_comment_mentioned', 'page_mentioned', 'chat_mentioned'].includes(notification.kind)

/** The web inbox's wording (`notificationCopy` in `features/inbox/api.ts`). */
export function notificationCopy(notification: NotificationRecord, pageTitle: string | undefined, actorName: string | undefined): { title: string; body: string } {
  const actor = actorName ?? 'Someone'
  const page = pageTitle === undefined ? 'a page' : `“${pageTitle.trim() || 'Untitled'}”`
  if (notification.kind.startsWith('github_')) {
    // the GitHub login: who acted may have no Orbit account
    const who = notification.github_actor_login ?? 'Someone'
    const body = `${notification.github_repository ?? ''}#${notification.github_pull_number ?? ''}`
    if (notification.kind === 'github_review_requested') return { title: `${who} asked for your review`, body }
    if (notification.kind === 'github_merged') return { title: `${who} merged your pull request`, body }
    if (notification.github_review_state === 'approved') return { title: `${who} approved your pull request`, body }
    if (notification.github_review_state === 'changes_requested') return { title: `${who} requested changes`, body }
    return { title: `${who} commented on your pull request`, body }
  }
  switch (notification.kind) {
    case 'import_finished': {
      const name = IMPORT_NAMES[notification.import_source ?? ''] ?? 'An'
      return notification.import_status === 'failed'
        ? { title: `${name} import failed`, body: 'Open the import to see what happened.' }
        : { title: `${name} import complete`, body: 'Open the import to see what was imported.' }
    }
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
