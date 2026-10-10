import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { apiClient, type createApiClient } from '@/api/client'
import { createLabel, createLabelGroup, deleteLabel, deleteLabelGroup, listLabelGroups, listLabels, updateLabel, updateLabelGroup } from '@/api/generated/sdk.gen'
import type { LabelBody, LabelGroupRecord, LabelRecord } from '@/api/generated/types.gen'
import { ApiProblem } from '@/api/problem'
import { queryKeys } from '@/api/queryKeys'

type ApiClient = ReturnType<typeof createApiClient>

export function labelsQueryOptions(workspaceId: string, client: ApiClient = apiClient) {
  return {
    queryKey: queryKeys.labels(workspaceId),
    queryFn: async () => {
      const labels = []
      let cursor: string | undefined
      do {
        const { data } = await listLabels({
          client,
          path: { workspace_id: workspaceId },
          query: { limit: 100, cursor },
          throwOnError: true,
        })
        if (!data) throw new Error('Labels response was empty.')
        labels.push(...data.items)
        cursor = data.next_cursor ?? undefined
      } while (cursor)
      return labels
    },
  }
}

export function useLabels(workspaceId: string) {
  return useQuery(labelsQueryOptions(workspaceId))
}

export function useCreateLabel(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (body: LabelBody) => {
      const { data } = await createLabel({
        client: apiClient,
        path: { workspace_id: workspaceId },
        body,
        throwOnError: true,
      })
      if (!data) throw new Error('Create label response was empty.')
      return data
    },
    onSuccess: (label: LabelRecord) => {
      queryClient.setQueryData(queryKeys.labels(workspaceId), (current: LabelRecord[] | undefined) =>
        current ? [...current, label] : [label],
      )
      void queryClient.invalidateQueries({ queryKey: queryKeys.labels(workspaceId) })
    },
  })
}

export function useUpdateLabel(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    // `groupId`: absent leaves the group as it is, null takes the label out of its group
    mutationFn: async ({ label, name = label.name, color = label.color, groupId }: { label: LabelRecord; name?: string; color?: string; groupId?: string | null }) => {
      const { data } = await updateLabel({
        client: apiClient,
        path: { workspace_id: workspaceId, label_id: label.id },
        body: { name, color, expected_version: label.version, ...(groupId === undefined ? {} : { group_id: groupId }) },
        throwOnError: true,
      })
      if (!data) throw new Error('Update label response was empty.')
      return data
    },
    onSettled: () => void queryClient.invalidateQueries({ queryKey: queryKeys.labels(workspaceId) }),
  })
}

export function useDeleteLabel(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (label: LabelRecord) => deleteLabel({
      client: apiClient,
      path: { workspace_id: workspaceId, label_id: label.id },
      query: { expected_version: label.version },
      throwOnError: true,
    }),
    onSettled: () => void queryClient.invalidateQueries({ queryKey: queryKeys.labels(workspaceId) }),
  })
}

/** The number of tasks that stop a label from joining a group (409 `label_group_conflict`), else null. */
export function labelGroupConflictCount(error: unknown): number | null {
  if (!(error instanceof ApiProblem) || error.code !== 'label_group_conflict') return null
  return (error.problem as { conflict?: { count?: number } }).conflict?.count ?? null
}

export function useLabelGroups(workspaceId: string) {
  return useQuery({
    queryKey: queryKeys.labelGroups(workspaceId),
    queryFn: async (): Promise<LabelGroupRecord[]> => {
      const { data } = await listLabelGroups({ client: apiClient, path: { workspace_id: workspaceId }, throwOnError: true })
      if (!data?.items) throw new Error('Label groups response was empty.')
      return data.items
    },
  })
}

/** Create, rename and delete label groups. Each change also refreshes the labels: they carry the group's name. */
export function useLabelGroupMutations(workspaceId: string) {
  const queryClient = useQueryClient()
  const path = { workspace_id: workspaceId }
  // the labels key is the prefix of the label groups key
  const onSettled = () => void queryClient.invalidateQueries({ queryKey: queryKeys.labels(workspaceId) })
  const create = useMutation({
    mutationFn: async (body: { name: string; color: string }) => {
      const { data } = await createLabelGroup({ client: apiClient, path, body, throwOnError: true })
      if (!data) throw new Error('Create label group response was empty.')
      return data
    },
    onSettled,
  })
  const update = useMutation({
    mutationFn: async ({ group, name, color = group.color }: { group: LabelGroupRecord; name: string; color?: string }) => {
      const { data } = await updateLabelGroup({ client: apiClient, path: { ...path, group_id: group.id }, body: { name, color, expected_version: group.version }, throwOnError: true })
      if (!data) throw new Error('Update label group response was empty.')
      return data
    },
    onSettled,
  })
  const remove = useMutation({
    mutationFn: (group: LabelGroupRecord) => deleteLabelGroup({ client: apiClient, path: { ...path, group_id: group.id }, query: { expected_version: group.version }, throwOnError: true }),
    onSettled,
  })
  return { create, update, remove }
}
