import { confirmAction } from '@/components/common/confirmAction'
import { useMemo } from 'react'
import { queryOptions, useMutation, useQueries, useQuery, useQueryClient } from '@tanstack/react-query'
import { apiClient } from '@/api/client'
import { fetchAllPages } from '@/api/pagination'
import {
  addProjectMember,
  createProject,
  createStatus,
  deleteProject,
  deleteStatus,
  listProjectMembers,
  listProjects,
  listProjectTrash,
  listTrashedProjectTasks,
  getTrashedProject,
  getProjectOrder,
  putProjectOrder,
  listStatuses,
  purgeProject,
  removeProjectMember,
  reorderStatuses,
  restoreProject,
  updateProject,
  updateStatus,
} from '@orbit/api/generated/sdk.gen'
import type {
  ProjectRecord,
  ProjectUpdateBody,
  ReorderItem,
  StatusBody,
  StatusRecord,
  StatusUpdateBody,
} from '@orbit/api/generated/types.gen'
import { queryKeys } from '@/api/queryKeys'
import type { StatusCategory, TaskStatusDef } from './models'
import { isTaskVersionConflict } from './conflicts'
import { projectDraft } from './projectDraft'
import { loadFailed } from '@/lib/connection'

function required<T>(data: T | undefined, message: string): T {
  if (data === undefined) throw new Error(message)
  return data
}

export function statusFromRecord(status: StatusRecord): TaskStatusDef {
  const category = ['unstarted', 'started', 'completed', 'cancelled', 'duplicate'].includes(status.category)
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

async function fetchProjects(workspaceId: string): Promise<ProjectRecord[]> {
  const page = await fetchAllPages(async (cursor) => {
    const { data } = await listProjects({ client: apiClient, path: { workspace_id: workspaceId }, query: { limit: 100, cursor }, throwOnError: true })
    return required(data, 'Projects response was empty.')
  })
  return page.items
}

/** The workspace's projects; shared by `useProjects` and code that reads them outside React (`fetchQuery`). */
export function projectsQuery(workspaceId: string) {
  return queryOptions({ queryKey: queryKeys.projects(workspaceId), queryFn: () => fetchProjects(workspaceId) })
}

/** The ids of the member's own project order; projects not listed follow by name (see `orderProjects`). */
export function useProjectOrder(workspaceId: string, enabled = true) {
  return useQuery({
    queryKey: queryKeys.projectOrder(workspaceId),
    enabled,
    queryFn: async () => {
      const { data } = await getProjectOrder({ client: apiClient, path: { workspace_id: workspaceId }, throwOnError: true })
      return required(data, 'Project order response was empty.').project_ids
    },
  })
}

/** Saves the member's project order; the sidebar shows it at once and rolls back on error. */
export function useSetProjectOrder(workspaceId: string) {
  const queryClient = useQueryClient()
  const key = queryKeys.projectOrder(workspaceId)
  return useMutation<void, Error, string[], { previous?: string[] }>({
    mutationFn: async (projectIds) => {
      await putProjectOrder({ client: apiClient, path: { workspace_id: workspaceId }, body: { project_ids: projectIds }, throwOnError: true })
    },
    onMutate: async (projectIds) => {
      await queryClient.cancelQueries({ queryKey: key, exact: true })
      const previous = queryClient.getQueryData<string[]>(key)
      queryClient.setQueryData(key, projectIds)
      return { previous }
    },
    onError: (_error, _ids, snapshot) => queryClient.setQueryData(key, snapshot?.previous),
    onSettled: () => void queryClient.invalidateQueries({ queryKey: key, exact: true }),
  })
}

/** A project's statuses; shared by the status hooks and code that reads them outside React (`fetchQuery`). */
export function statusesQuery(workspaceId: string, projectId: string) {
  return queryOptions({
    queryKey: queryKeys.statuses(workspaceId, projectId),
    queryFn: async () => {
      const page = await fetchAllPages(async (cursor) => {
        const { data } = await listStatuses({ client: apiClient, path: { workspace_id: workspaceId, project_id: projectId }, query: { limit: 100, cursor }, throwOnError: true })
        return required(data, 'Statuses response was empty.')
      })
      return page.items.map(statusFromRecord)
    },
  })
}

export function useProjects(workspaceId: string, enabled = true) {
  return useQuery({ ...projectsQuery(workspaceId), enabled })
}

export function useProjectStatuses(workspaceId: string, projectId: string | undefined) {
  return useQuery({ ...statusesQuery(workspaceId, projectId ?? ''), enabled: Boolean(projectId) })
}

export function useAllStatuses(workspaceId: string, projects: ProjectRecord[]) {
  const results = useQueries({
    queries: projects.map((project) => statusesQuery(workspaceId, project.id)),
  })
  return {
    data: results.flatMap((result) => result.data ?? []),
    isPending: results.some((result) => result.isPending),
    // A failed refetch of statuses that are already here is not an error for the page.
    isError: results.some(loadFailed),
    error: results.find((result) => result.error)?.error ?? null,
  }
}

export function useCreateProject(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async ({ name, isPrivate }: { name: string; isPrivate: boolean }) => {
      // A fresh list, so the key skips the keys of projects that other members made since the last load.
      const existing = await fetchProjects(workspaceId)
      const body = { ...projectDraft(name, existing.map((project) => project.key)), private: isPrivate }
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
    // Write the returned record into the list cache synchronously: callers that key a follow-up
    // mutation off `project.version` (e.g. the sub-issue automation switches) must see the bumped
    // version immediately, not after the async refetch below lands.
    onSuccess: (record: ProjectRecord) => {
      queryClient.setQueryData(queryKeys.projects(workspaceId), (current: ProjectRecord[] | undefined) =>
        current?.map((project) => (project.id === record.id ? record : project)),
      )
      void queryClient.invalidateQueries({ queryKey: queryKeys.projects(workspaceId) })
    },
  })
}

/** The people listed on a private project. Owners and admins see the project without being listed. */
export function useProjectMembers(workspaceId: string, projectId: string, enabled: boolean) {
  return useQuery({
    queryKey: queryKeys.projectMembers(workspaceId, projectId),
    enabled,
    queryFn: async () => {
      const { data } = await listProjectMembers({ client: apiClient, path: { workspace_id: workspaceId, project_id: projectId }, throwOnError: true })
      return required(data, 'Project members response was empty.')
    },
  })
}

/**
 * The people who see `project`, for assignee and mention pickers: everyone for a workspace project; for a private one,
 * its listed members plus owners and admins (the server refuses anyone else).
 */
export function useProjectAudience<T extends { id: string; role: string }>(workspaceId: string, project: ProjectRecord | undefined, people: T[]): T[] {
  const isPrivate = project?.private === true
  const members = useProjectMembers(workspaceId, project?.id ?? '', isPrivate).data
  return useMemo(() => {
    if (!isPrivate) return people
    const listed = new Set(members?.map((member) => member.user_id))
    return people.filter((person) => person.role === 'Owner' || person.role === 'Admin' || listed.has(person.id))
  }, [isPrivate, members, people])
}

export function useAddProjectMembers(workspaceId: string, projectId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (userIds: string[]) => {
      await Promise.all(userIds.map((userId) => addProjectMember({ client: apiClient, path: { workspace_id: workspaceId, project_id: projectId }, body: { user_id: userId }, throwOnError: true })))
    },
    onSettled: () => void queryClient.invalidateQueries({ queryKey: queryKeys.projectMembers(workspaceId, projectId) }),
  })
}

/** Removing yourself can hide the project from you, so the project list is refetched too. */
export function useRemoveProjectMember(workspaceId: string, projectId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (userId: string) => {
      await removeProjectMember({ client: apiClient, path: { workspace_id: workspaceId, project_id: projectId, user_id: userId }, throwOnError: true })
    },
    onSettled: () => void queryClient.invalidateQueries({ queryKey: queryKeys.projects(workspaceId) }),
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

/** A project in the trash with its statuses, for its read-only preview. */
export function useTrashedProject(workspaceId: string, projectId: string) {
  return useQuery({
    queryKey: [...queryKeys.projectTrash(workspaceId), 'preview', projectId],
    queryFn: async () => {
      const { data } = await getTrashedProject({ client: apiClient, path: { workspace_id: workspaceId, project_id: projectId }, throwOnError: true })
      return required(data, 'Trashed project response was empty.')
    },
    retry: false,
  })
}

/** Every task of a trashed project, by number. */
export function useTrashedProjectTasks(workspaceId: string, projectId: string) {
  return useQuery({
    queryKey: [...queryKeys.projectTrash(workspaceId), 'preview', projectId, 'tasks'],
    queryFn: async () => {
      const page = await fetchAllPages(async (cursor) => {
        const { data } = await listTrashedProjectTasks({ client: apiClient, path: { workspace_id: workspaceId, project_id: projectId }, query: { limit: 100, cursor }, throwOnError: true })
        return required(data, 'Trashed project tasks response was empty.')
      })
      return page.items
    },
    retry: false,
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

/** Deletes a trashed project forever with all its tasks. */
export function usePurgeProject(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (projectId: string) => {
      await purgeProject({ client: apiClient, path: { workspace_id: workspaceId, project_id: projectId }, throwOnError: true })
    },
    onSettled: () => void queryClient.invalidateQueries({ queryKey: queryKeys.workspace(workspaceId) }),
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
