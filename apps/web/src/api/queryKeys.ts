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
  projects: (workspaceId: string) => [...workspace(workspaceId), 'projects'] as const,
  statuses: (workspaceId: string, projectId: string) =>
    [...workspace(workspaceId), 'projects', projectId, 'statuses'] as const,
  comments: (workspaceId: string, taskId: string) =>
    [...workspace(workspaceId), 'tasks', 'detail', taskId, 'comments'] as const,
  attachments: (workspaceId: string, taskId: string) =>
    [...workspace(workspaceId), 'tasks', 'detail', taskId, 'attachments'] as const,
  commentAttachments: (workspaceId: string, taskId: string, commentId: string) =>
    [...workspace(workspaceId), 'tasks', 'detail', taskId, 'comments', commentId, 'attachments'] as const,
  taskTrash: (workspaceId: string) => [...workspace(workspaceId), 'tasks', 'trash'] as const,
  tasks: {
    all: (workspaceId: string) => [...workspace(workspaceId), 'tasks'] as const,
    list: (workspaceId: string, filters: TaskFilters = {}) =>
      [...workspace(workspaceId), 'tasks', 'list', filters] as const,
    detail: (workspaceId: string, taskId: string) =>
      [...workspace(workspaceId), 'tasks', 'detail', taskId] as const,
  },
}
