/**
 * Every TanStack Query key of the app. The resource comes first and the workspace second, so a prefix such as
 * `['task', workspaceId]` reaches every task detail of one workspace.
 */
export const keys = {
  workspaces: ['workspaces'] as const,
  tasks: (workspaceId: string) => ['tasks', workspaceId] as const,
  /** A server view (`mine`, `my_week` …) or a Home card query, under the `tasks` prefix so task changes reach it. */
  taskView: (workspaceId: string, view: string) => ['tasks', workspaceId, view] as const,
  taskActivity: (workspaceId: string, taskId: string) => ['task-activity', workspaceId, taskId] as const,
  task: (workspaceId: string, taskId: string) => ['task', workspaceId, taskId] as const,
  comments: (workspaceId: string, taskId: string) => ['comments', workspaceId, taskId] as const,
  projects: (workspaceId: string) => ['projects', workspaceId] as const,
  /** The member's own sidebar order of the projects. */
  projectOrder: (workspaceId: string) => ['project-order', workspaceId] as const,
  statuses: (workspaceId: string) => ['statuses', workspaceId] as const,
  members: (workspaceId: string) => ['members', workspaceId] as const,
  projectMembers: (workspaceId: string, projectId: string) => ['project-members', workspaceId, projectId] as const,
  labels: (workspaceId: string) => ['labels', workspaceId] as const,
  milestones: (workspaceId: string, projectId: string) => ['milestones', workspaceId, projectId] as const,
  cycles: (workspaceId: string, projectId: string) => ['cycles', workspaceId, projectId] as const,
  conversations: (workspaceId: string) => ['conversations', workspaceId] as const,
  chatCategories: (workspaceId: string) => ['chat-categories', workspaceId] as const,
  chatStates: (workspaceId: string) => ['chat-states', workspaceId] as const,
  linkPreview: (workspaceId: string, url: string) => ['chat-link-preview', workspaceId, url] as const,
  messages: (workspaceId: string, conversationId: string) => ['messages', workspaceId, conversationId] as const,
  notifications: (workspaceId: string) => ['notifications', workspaceId] as const,
  unreadNotifications: (workspaceId: string) => ['notifications', workspaceId, 'unread'] as const,
  pages: (workspaceId: string) => ['pages', workspaceId] as const,
  page: (workspaceId: string, pageId: string) => ['page', workspaceId, pageId] as const,
  teamspaces: (workspaceId: string) => ['teamspaces', workspaceId] as const,
  /** A linked view's detail sits under the database's own key, so refreshing the database refreshes it too. */
  database: (workspaceId: string, databaseId: string, linkId = '') => (linkId ? ['database', workspaceId, databaseId, linkId] : ['database', workspaceId, databaseId]),
}

/** Which threads a list shows (the web's `ThreadFilter`): without `folder`, the inbox; `starred` the inbox and archive; `sent` every folder but the trash. */
export interface ThreadFilter {
  folder?: 'inbox' | 'archive' | 'spam' | 'trash'
  starred?: boolean
  sent?: boolean
  unread?: boolean
}

/**
 * Mail's keys, as on the web: outside the `[resource, workspaceId]` prefixes the workspace events socket refreshes,
 * because mail has its own live topic. Every list (folders and searches) sits under `lists`.
 */
export const mailKeys = {
  all: (workspaceId: string) => ['mail', workspaceId] as const,
  me: (workspaceId: string) => ['mail', workspaceId, 'me'] as const,
  counts: (workspaceId: string) => ['mail', workspaceId, 'counts'] as const,
  lists: (workspaceId: string) => ['mail', workspaceId, 'list'] as const,
  threads: (workspaceId: string, filter: ThreadFilter) => ['mail', workspaceId, 'list', 'threads', filter] as const,
  search: (workspaceId: string, query: string) => ['mail', workspaceId, 'list', 'search', query] as const,
  thread: (workspaceId: string, threadId: string) => ['mail', workspaceId, 'thread', threadId] as const,
  drafts: (workspaceId: string) => ['mail', workspaceId, 'drafts'] as const,
  draft: (workspaceId: string, draftId: string) => ['mail', workspaceId, 'draft', draftId] as const,
  outbound: (workspaceId: string, view: 'scheduled' | 'outbox') => ['mail', workspaceId, 'outbound', view] as const,
  outboundAll: (workspaceId: string) => ['mail', workspaceId, 'outbound'] as const,
  fromAddresses: (workspaceId: string, threadId?: string) => ['mail', workspaceId, 'from-addresses', threadId ?? null] as const,
  contacts: (workspaceId: string, query: string) => ['mail', workspaceId, 'contacts', query] as const,
  sendPolicy: (workspaceId: string) => ['mail', workspaceId, 'send-policy'] as const,
  /** Outside `all`: a resync must not fetch a new document for a message on screen. */
  view: (workspaceId: string, messageId: string, loadImages: boolean) => ['mail-view', workspaceId, messageId, loadImages ? 'images' : 'blocked'] as const,
}

export type QueryKey = readonly string[]

/**
 * What the workspace events socket refreshes: everything but chat, which has its own live socket. Members and
 * notifications are in it, as on the web, where the event refreshes the whole workspace.
 */
export function workspaceDataKeys(workspaceId: string): QueryKey[] {
  return (['tasks', 'task', 'comments', 'task-activity', 'projects', 'statuses', 'labels', 'milestones', 'cycles', 'members', 'notifications', 'pages', 'page', 'teamspaces', 'database'] as const).map(
    (resource) => [resource, workspaceId],
  )
}

/** What a chat resync refetches. */
export function chatKeys(workspaceId: string): QueryKey[] {
  return [keys.conversations(workspaceId), keys.chatCategories(workspaceId), keys.chatStates(workspaceId), ['messages', workspaceId]]
}
