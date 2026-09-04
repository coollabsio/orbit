type TaskFilters = Readonly<Record<string, string | number | boolean | undefined>>

const workspace = (workspaceId: string) => ['workspace', workspaceId] as const

export const queryKeys = {
  workspace,
  workspaces: ['workspaces'] as const,
  members: (workspaceId: string) => [...workspace(workspaceId), 'members'] as const,
  projects: (workspaceId: string) => [...workspace(workspaceId), 'projects'] as const,
  tasks: {
    all: (workspaceId: string) => [...workspace(workspaceId), 'tasks'] as const,
    list: (workspaceId: string, filters: TaskFilters = {}) =>
      [...workspace(workspaceId), 'tasks', 'list', filters] as const,
    detail: (workspaceId: string, taskId: string) =>
      [...workspace(workspaceId), 'tasks', 'detail', taskId] as const,
  },
}
