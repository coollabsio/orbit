type TaskFilters = Readonly<Record<string, string | number | boolean | undefined>>

const workspace = (workspaceId: string) => ['workspace', workspaceId] as const

export const queryKeys = {
  setup: ['setup-status'] as const,
  currentUser: ['current-user'] as const,
  sessions: ['sessions'] as const,
  workspace,
  workspaces: ['workspaces'] as const,
  members: (workspaceId: string) => [...workspace(workspaceId), 'members'] as const,
  invitations: (workspaceId: string) => [...workspace(workspaceId), 'invitations'] as const,
  apiTokens: (workspaceId: string) => [...workspace(workspaceId), 'api-tokens'] as const,
  projects: (workspaceId: string) => [...workspace(workspaceId), 'projects'] as const,
  statuses: (workspaceId: string, projectId: string) =>
    [...workspace(workspaceId), 'projects', projectId, 'statuses'] as const,
  comments: (workspaceId: string, taskId: string) =>
    [...workspace(workspaceId), 'tasks', 'detail', taskId, 'comments'] as const,
  taskActivity: (workspaceId: string, taskId: string) =>
    [...workspace(workspaceId), 'tasks', 'detail', taskId, 'activity'] as const,
  attachments: (workspaceId: string, taskId: string) =>
    [...workspace(workspaceId), 'tasks', 'detail', taskId, 'attachments'] as const,
  taskRelations: (workspaceId: string, taskId: string) =>
    [...workspace(workspaceId), 'tasks', 'detail', taskId, 'relations'] as const,
  labels: (workspaceId: string) => [...workspace(workspaceId), 'labels'] as const,
  commentAttachments: (workspaceId: string, taskId: string, commentId: string) =>
    [...workspace(workspaceId), 'tasks', 'detail', taskId, 'comments', commentId, 'attachments'] as const,
  taskTrash: (workspaceId: string) => [...workspace(workspaceId), 'tasks', 'trash'] as const,
  projectTrash: (workspaceId: string) => [...workspace(workspaceId), 'projects', 'trash'] as const,
  workspaceTrash: (workspaceId: string) => [...workspace(workspaceId), 'trash'] as const,
  audit: (workspaceId: string) => [...workspace(workspaceId), 'audit'] as const,
  notifications: (workspaceId: string, unread?: boolean) =>
    [...workspace(workspaceId), 'notifications', { unread }] as const,
  /** Saved views list; also the prefix of every `view` key (use `exact: true` to target only the list). */
  views: (workspaceId: string) => [...workspace(workspaceId), 'views'] as const,
  view: (workspaceId: string, viewId: string) => [...workspace(workspaceId), 'views', 'detail', viewId] as const,
  viewPreference: (workspaceId: string, pageKey: string) =>
    [...workspace(workspaceId), 'view-preferences', pageKey] as const,
  /** Under `tasks.all`, so every task mutation's existing invalidation and optimistic patch reaches it. */
  taskQuery: (workspaceId: string, body: object) => [...workspace(workspaceId), 'tasks', 'query', body] as const,
  tasks: {
    all: (workspaceId: string) => [...workspace(workspaceId), 'tasks'] as const,
    list: (workspaceId: string, filters: TaskFilters = {}) =>
      [...workspace(workspaceId), 'tasks', 'list', filters] as const,
    detail: (workspaceId: string, taskId: string) =>
      [...workspace(workspaceId), 'tasks', 'detail', taskId] as const,
  },
}
