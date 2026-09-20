import { confirmAction } from '@/components/common/confirmAction'
import { useMutation, useQueries, useQuery, useQueryClient } from '@tanstack/react-query'
import { apiClient } from '@/api/client'
import { fetchAllPages } from '@/api/pagination'
import {
  createProject,
  createStatus,
  deleteProject,
  deleteStatus,
  listProjects,
  listProjectTrash,
  listStatuses,
  reorderStatuses,
  restoreProject,
  updateProject,
  updateStatus,
} from '@/api/generated/sdk.gen'
import type {
  ProjectBody,
  ProjectRecord,
  ProjectUpdateBody,
  ReorderItem,
  StatusBody,
  StatusRecord,
  StatusUpdateBody,
} from '@/api/generated/types.gen'
import { queryKeys } from '@/api/queryKeys'
import type { StatusCategory, TaskStatusDef } from './models'
import { isTaskVersionConflict } from './conflicts'

function required<T>(data: T | undefined, message: string): T {
  if (data === undefined) throw new Error(message)
  return data
}

export function statusFromRecord(status: StatusRecord): TaskStatusDef {
  const category = ['unstarted', 'started', 'completed', 'cancelled'].includes(status.category)
    ? status.category as StatusCategory
    : 'unstarted'
  return {
    id: status.id,
    projectId: status.project_id,
    name: status.name,
    description: status.description,
    color: status.color,
    category,
    position: status.position,
    version: status.version,
  }
}

export function useProjects(workspaceId: string, enabled = true) {
  return useQuery({
    queryKey: queryKeys.projects(workspaceId),
    enabled,
    queryFn: async () => {
      const page = await fetchAllPages(async (cursor) => {
        const { data } = await listProjects({ client: apiClient, path: { workspace_id: workspaceId }, query: { limit: 100, cursor }, throwOnError: true })
        return required(data, 'Projects response was empty.')
      })
      return page.items
    },
  })
}

export function useProjectStatuses(workspaceId: string, projectId: string | undefined) {
  return useQuery({
    queryKey: queryKeys.statuses(workspaceId, projectId ?? ''),
    enabled: Boolean(projectId),
    queryFn: async () => {
      const page = await fetchAllPages(async (cursor) => {
        const { data } = await listStatuses({ client: apiClient, path: { workspace_id: workspaceId, project_id: projectId! }, query: { limit: 100, cursor }, throwOnError: true })
        return required(data, 'Statuses response was empty.')
      })
      return page.items.map(statusFromRecord)
    },
  })
}

export function useAllStatuses(workspaceId: string, projects: ProjectRecord[]) {
  const results = useQueries({
    queries: projects.map((project) => ({
      queryKey: queryKeys.statuses(workspaceId, project.id),
      queryFn: async () => {
        const page = await fetchAllPages(async (cursor) => {
          const { data } = await listStatuses({ client: apiClient, path: { workspace_id: workspaceId, project_id: project.id }, query: { limit: 100, cursor }, throwOnError: true })
          return required(data, 'Statuses response was empty.')
        })
        return page.items.map(statusFromRecord)
      },
    })),
  })
  return {
    data: results.flatMap((result) => result.data ?? []),
    isPending: results.some((result) => result.isPending),
    isError: results.some((result) => result.isError),
    error: results.find((result) => result.error)?.error ?? null,
  }
}

export function useCreateProject(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (body: ProjectBody) => {
      const { data } = await createProject({ client: apiClient, path: { workspace_id: workspaceId }, body, throwOnError: true })
      return required(data, 'Create project response was empty.')
    },
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.projects(workspaceId) }),
  })
}

export function useUpdateProject(workspaceId: string, projectId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (body: ProjectUpdateBody) => {
      const { data } = await updateProject({ client: apiClient, path: { workspace_id: workspaceId, project_id: projectId }, body, throwOnError: true })
      return required(data, 'Update project response was empty.')
    },
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.projects(workspaceId) }),
  })
}

export function useDeleteProject(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async ({ projectId, version }: { projectId: string; version: number }) => {
      await deleteProject({ client: apiClient, path: { workspace_id: workspaceId, project_id: projectId }, query: { expected_version: version }, throwOnError: true })
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.workspace(workspaceId) })
    },
  })
}

export function useProjectTrash(workspaceId: string) {
  return useQuery({
    queryKey: queryKeys.projectTrash(workspaceId),
    queryFn: async () => {
      const page = await fetchAllPages(async (cursor) => {
        const { data } = await listProjectTrash({ client: apiClient, path: { workspace_id: workspaceId }, query: { limit: 100, cursor }, throwOnError: true })
        return required(data, 'Project trash response was empty.')
      })
      return page.items
    },
  })
}

export function useRestoreProject(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async ({ projectId, version }: { projectId: string; version: number }) => {
      const { data } = await restoreProject({ client: apiClient, path: { workspace_id: workspaceId, project_id: projectId }, body: { expected_version: version }, throwOnError: true })
      return required(data, 'Restore project response was empty.')
    },
    onError: async (error) => {
      if (isTaskVersionConflict(error) && await confirmAction({ title: 'Refresh trash?', description: 'This project now conflicts with an active project key. Refresh trash?', confirmLabel: 'Refresh trash' })) {
        void queryClient.invalidateQueries({ queryKey: queryKeys.projectTrash(workspaceId) })
      }
    },
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.workspace(workspaceId) }),
  })
}

export function useCreateStatus(workspaceId: string, projectId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (body: StatusBody) => {
      const { data } = await createStatus({ client: apiClient, path: { workspace_id: workspaceId, project_id: projectId }, body, throwOnError: true })
      return required(data, 'Create status response was empty.')
    },
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.statuses(workspaceId, projectId) }),
  })
}

export function useUpdateStatus(workspaceId: string, projectId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async ({ statusId, body }: { statusId: string; body: StatusUpdateBody }) => {
      const { data } = await updateStatus({ client: apiClient, path: { workspace_id: workspaceId, project_id: projectId, status_id: statusId }, body, throwOnError: true })
      return required(data, 'Update status response was empty.')
    },
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.statuses(workspaceId, projectId) }),
  })
}

export function useDeleteStatus(workspaceId: string, projectId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async ({ statusId, version }: { statusId: string; version: number }) => {
      await deleteStatus({ client: apiClient, path: { workspace_id: workspaceId, project_id: projectId, status_id: statusId }, query: { expected_version: version }, throwOnError: true })
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.statuses(workspaceId, projectId) })
      void queryClient.invalidateQueries({ queryKey: queryKeys.tasks.all(workspaceId) })
    },
  })
}

export function useReorderStatuses(workspaceId: string, projectId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (items: ReorderItem[]) => {
      const { data } = await reorderStatuses({ client: apiClient, path: { workspace_id: workspaceId, project_id: projectId }, body: { items }, throwOnError: true })
      return required(data, 'Reorder statuses response was empty.')
    },
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.statuses(workspaceId, projectId) }),
  })
}
