import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { apiClient } from '@/api/client'
import {
  createMilestone,
  createMilestoneDescriptionPage,
  createMilestoneUpdate,
  createProjectOverviewPage,
  deleteMilestone,
  deleteMilestoneUpdate,
  getProject,
  listMilestoneUpdates,
  listWorkspaceMilestones,
  updateMilestone,
  updateMilestoneUpdate,
} from '@/api/generated/sdk.gen'
import type {
  HealthUpdateBody,
  HealthUpdatePatchBody,
  MilestoneBody,
  MilestoneRecord,
  MilestoneUpdateBody,
  MilestoneUpdateRecord,
} from '@/api/generated/types.gen'
import { queryKeys } from '@/api/queryKeys'

export type Milestone = MilestoneRecord
export type MilestoneUpdate = MilestoneUpdateRecord
export type MilestoneStatus = 'planned' | 'in_progress' | 'completed' | 'cancelled'
export type MilestoneHealth = 'on_track' | 'at_risk' | 'off_track'

export const MILESTONE_STATUSES: readonly MilestoneStatus[] = ['planned', 'in_progress', 'completed', 'cancelled']
export const MILESTONE_STATUS_LABEL: Record<MilestoneStatus, string> = {
  planned: 'Planned',
  in_progress: 'In progress',
  completed: 'Completed',
  cancelled: 'Cancelled',
}
export const HEALTH_VALUES: readonly MilestoneHealth[] = ['on_track', 'at_risk', 'off_track']
export const HEALTH_LABEL: Record<MilestoneHealth, string> = {
  on_track: 'On track',
  at_risk: 'At risk',
  off_track: 'Off track',
}

function required<T>(data: T | undefined, message: string): T {
  if (data === undefined) throw new Error(message)
  return data
}

/** Every milestone of the workspace, in project then position order. One query feeds the roadmap,
 * the project pages and the task pickers. */
export function useMilestones(workspaceId: string, enabled = true) {
  return useQuery({
    queryKey: queryKeys.milestones(workspaceId),
    enabled,
    queryFn: async () => {
      const { data } = await listWorkspaceMilestones({ client: apiClient, path: { workspace_id: workspaceId }, throwOnError: true })
      return required(data, 'Milestones response was empty.').items
    },
  })
}

export function useProject(workspaceId: string, projectId: string | undefined) {
  return useQuery({
    queryKey: queryKeys.project(workspaceId, projectId ?? ''),
    enabled: Boolean(projectId),
    queryFn: async () => {
      const { data } = await getProject({ client: apiClient, path: { workspace_id: workspaceId, project_id: projectId ?? '' }, throwOnError: true })
      return required(data, 'Project response was empty.')
    },
  })
}

function useInvalidateMilestones(workspaceId: string) {
  const queryClient = useQueryClient()
  return () => {
    void queryClient.invalidateQueries({ queryKey: queryKeys.milestones(workspaceId) })
    void queryClient.invalidateQueries({ queryKey: queryKeys.tasks.all(workspaceId) })
  }
}

export function useCreateMilestone(workspaceId: string, projectId: string) {
  const invalidate = useInvalidateMilestones(workspaceId)
  return useMutation({
    mutationFn: async (body: MilestoneBody) => {
      const { data } = await createMilestone({ client: apiClient, path: { workspace_id: workspaceId, project_id: projectId }, body, throwOnError: true })
      return required(data, 'Create milestone response was empty.')
    },
    onSuccess: invalidate,
  })
}

export function useUpdateMilestone(workspaceId: string) {
  const queryClient = useQueryClient()
  const invalidate = useInvalidateMilestones(workspaceId)
  return useMutation({
    mutationFn: async ({ milestone, body }: { milestone: Pick<Milestone, 'id' | 'project_id'>; body: MilestoneUpdateBody }) => {
      const { data } = await updateMilestone({ client: apiClient, path: { workspace_id: workspaceId, project_id: milestone.project_id, milestone_id: milestone.id }, body, throwOnError: true })
      return required(data, 'Update milestone response was empty.')
    },
    // Write the record at once: the next change (a second drag on the roadmap) needs its version.
    onSuccess: (record) => {
      queryClient.setQueryData(queryKeys.milestones(workspaceId), (current: Milestone[] | undefined) =>
        current?.map((item) => (item.id === record.id ? record : item)),
      )
      invalidate()
    },
    onError: invalidate,
  })
}

export function useDeleteMilestone(workspaceId: string) {
  const invalidate = useInvalidateMilestones(workspaceId)
  return useMutation({
    mutationFn: async (milestone: Pick<Milestone, 'id' | 'project_id' | 'version'>) => {
      await deleteMilestone({ client: apiClient, path: { workspace_id: workspaceId, project_id: milestone.project_id, milestone_id: milestone.id }, query: { expected_version: milestone.version }, throwOnError: true })
    },
    onSuccess: invalidate,
  })
}

/** Returns the id of the hidden Docs page that holds the project description; creates it when absent. */
export function useEnsureProjectOverviewPage(workspaceId: string, projectId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async () => {
      const { data } = await createProjectOverviewPage({ client: apiClient, path: { workspace_id: workspaceId, project_id: projectId }, throwOnError: true })
      return required(data, 'Overview page response was empty.').page_id
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.projects(workspaceId) })
    },
  })
}

export function useEnsureMilestoneDescriptionPage(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (milestone: Pick<Milestone, 'id' | 'project_id'>) => {
      const { data } = await createMilestoneDescriptionPage({ client: apiClient, path: { workspace_id: workspaceId, project_id: milestone.project_id, milestone_id: milestone.id }, throwOnError: true })
      return required(data, 'Description page response was empty.').page_id
    },
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.milestones(workspaceId) }),
  })
}

export function useMilestoneUpdates(workspaceId: string, milestone: Pick<Milestone, 'id' | 'project_id'> | undefined) {
  return useQuery({
    queryKey: queryKeys.milestoneUpdates(workspaceId, milestone?.id ?? ''),
    enabled: Boolean(milestone),
    queryFn: async () => {
      const { data } = await listMilestoneUpdates({ client: apiClient, path: { workspace_id: workspaceId, project_id: milestone?.project_id ?? '', milestone_id: milestone?.id ?? '' }, throwOnError: true })
      return required(data, 'Milestone updates response was empty.').items
    },
  })
}

/** Post, edit and delete of a health update. Each one changes the health of the milestone. */
export function useMilestoneUpdateMutations(workspaceId: string, milestone: Pick<Milestone, 'id' | 'project_id'>) {
  const queryClient = useQueryClient()
  const path = { workspace_id: workspaceId, project_id: milestone.project_id, milestone_id: milestone.id }
  const onSuccess = () => void queryClient.invalidateQueries({ queryKey: queryKeys.milestones(workspaceId) })
  const post = useMutation({
    mutationFn: async (body: HealthUpdateBody) => {
      const { data } = await createMilestoneUpdate({ client: apiClient, path, body, throwOnError: true })
      return required(data, 'Create update response was empty.')
    },
    onSuccess,
  })
  const edit = useMutation({
    mutationFn: async ({ updateId, body }: { updateId: string; body: HealthUpdatePatchBody }) => {
      const { data } = await updateMilestoneUpdate({ client: apiClient, path: { ...path, update_id: updateId }, body, throwOnError: true })
      return required(data, 'Edit update response was empty.')
    },
    onSuccess,
  })
  const remove = useMutation({
    mutationFn: async (update: Pick<MilestoneUpdate, 'id' | 'version'>) => {
      await deleteMilestoneUpdate({ client: apiClient, path: { ...path, update_id: update.id }, query: { expected_version: update.version }, throwOnError: true })
    },
    onSuccess,
  })
  return { post, edit, remove }
}
