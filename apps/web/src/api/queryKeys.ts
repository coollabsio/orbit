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
  teamspaces: (workspaceId: string) => [...workspace(workspaceId), 'teamspaces'] as const,
  pages: {
    all: (workspaceId: string) => [...workspace(workspaceId), 'pages'] as const,
    tree: (workspaceId: string) => [...workspace(workspaceId), 'pages', 'tree'] as const,
    detail: (workspaceId: string, pageId: string) => [...workspace(workspaceId), 'pages', 'detail', pageId] as const,
    trash: (workspaceId: string) => [...workspace(workspaceId), 'pages', 'trash'] as const,
    search: (workspaceId: string, query: string) => [...workspace(workspaceId), 'pages', 'search', query] as const,
    favorites: (workspaceId: string) => [...workspace(workspaceId), 'pages', 'favorites'] as const,
    versions: (workspaceId: string, pageId: string) => [...workspace(workspaceId), 'pages', 'versions', pageId] as const,
    version: (workspaceId: string, pageId: string, versionId: string) =>
      [...workspace(workspaceId), 'pages', 'versions', pageId, versionId] as const,
  },
  /**
   * Deliberately outside the workspace prefix: realtime events invalidate that whole prefix, and an import creates
   * thousands of pages. The import views poll these keys instead of refetching a (large) scan tree on every event.
   */
  notionImports: {
    all: (workspaceId: string) => ['notion-imports', workspaceId] as const,
    list: (workspaceId: string) => ['notion-imports', workspaceId, 'list'] as const,
    detail: (workspaceId: string, importId: string) => ['notion-imports', workspaceId, 'detail', importId] as const,
    tree: (workspaceId: string, importId: string) => ['notion-imports', workspaceId, 'tree', importId] as const,
  },
  tasks: {
    all: (workspaceId: string) => [...workspace(workspaceId), 'tasks'] as const,
    list: (workspaceId: string, filters: TaskFilters = {}) =>
      [...workspace(workspaceId), 'tasks', 'list', filters] as const,
    detail: (workspaceId: string, taskId: string) =>
      [...workspace(workspaceId), 'tasks', 'detail', taskId] as const,
  },
}
