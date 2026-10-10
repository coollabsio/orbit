import { useMutation, useQueries, useQuery, useQueryClient } from '@tanstack/react-query'
import { apiClient } from '@/api/client'
import {
  endCycleToday,
  getCycleSettings,
  listCurrentCycles,
  listCycleDays,
  listCycles,
  startCycleToday,
  updateCycle,
  updateCycleSettings,
} from '@/api/generated/sdk.gen'
import type { CycleRecord, CycleSettingsBody, CycleSettingsRecord, CycleUpdateBody, ProjectRecord } from '@/api/generated/types.gen'
import { queryKeys } from '@/api/queryKeys'

export type Cycle = CycleRecord
export type CycleSettings = CycleSettingsRecord

function required<T>(data: T | undefined, message: string): T {
  if (data === undefined) throw new Error(message)
  return data
}

const NO_CYCLES: Cycle[] = []

async function fetchCycles(workspaceId: string, projectId: string): Promise<Cycle[]> {
  const { data } = await listCycles({ client: apiClient, path: { workspace_id: workspaceId, project_id: projectId }, throwOnError: true })
  return required(data, 'Cycles response was empty.').items
}

/** Every cycle of one project with its totals, in time order. */
export function useCycles(workspaceId: string, projectId: string | undefined) {
  return useQuery({
    queryKey: queryKeys.cycles(workspaceId, projectId ?? ''),
    enabled: Boolean(projectId),
    queryFn: () => fetchCycles(workspaceId, projectId ?? ''),
  })
}

/** The cycles of every given project, flattened: for pages that show tasks of more than one project. */
export function useAllCycles(workspaceId: string, projects: ProjectRecord[]): Cycle[] {
  return useQueries({
    queries: projects.map((project) => ({
      queryKey: queryKeys.cycles(workspaceId, project.id),
      queryFn: () => fetchCycles(workspaceId, project.id),
    })),
    combine: (results) => (results.some((result) => result.data && result.data.length > 0) ? results.flatMap((result) => result.data ?? []) : NO_CYCLES),
  })
}

/** The current cycle of each project that has one. */
export function useCurrentCycles(workspaceId: string) {
  return useQuery({
    queryKey: queryKeys.currentCycles(workspaceId),
    queryFn: async () => {
      const { data } = await listCurrentCycles({ client: apiClient, path: { workspace_id: workspaceId }, throwOnError: true })
      return required(data, 'Current cycles response was empty.').items
    },
  })
}

export function useCycleDays(workspaceId: string, cycle: Pick<Cycle, 'id' | 'project_id'> | undefined) {
  return useQuery({
    queryKey: queryKeys.cycleDays(workspaceId, cycle?.project_id ?? '', cycle?.id ?? ''),
    enabled: Boolean(cycle),
    queryFn: async () => {
      const { data } = await listCycleDays({ client: apiClient, path: { workspace_id: workspaceId, project_id: cycle?.project_id ?? '', cycle_id: cycle?.id ?? '' }, throwOnError: true })
      return required(data, 'Cycle days response was empty.').items
    },
  })
}

export function useCycleSettings(workspaceId: string, projectId: string) {
  return useQuery({
    queryKey: queryKeys.cycleSettings(workspaceId, projectId),
    queryFn: async () => {
      const { data } = await getCycleSettings({ client: apiClient, path: { workspace_id: workspaceId, project_id: projectId }, throwOnError: true })
      return required(data, 'Cycle settings response was empty.')
    },
  })
}

/** A change of the settings, a manual action or a rename changes cycles, and can move tasks between them. */
function useInvalidateCycles(workspaceId: string) {
  const queryClient = useQueryClient()
  return () => {
    void queryClient.invalidateQueries({ queryKey: queryKeys.projects(workspaceId) })
    void queryClient.invalidateQueries({ queryKey: queryKeys.currentCycles(workspaceId) })
    void queryClient.invalidateQueries({ queryKey: queryKeys.tasks.all(workspaceId) })
  }
}

export function useUpdateCycleSettings(workspaceId: string, projectId: string) {
  const queryClient = useQueryClient()
  const invalidate = useInvalidateCycles(workspaceId)
  return useMutation({
    mutationFn: async (body: CycleSettingsBody) => {
      const { data } = await updateCycleSettings({ client: apiClient, path: { workspace_id: workspaceId, project_id: projectId }, body, throwOnError: true })
      return required(data, 'Cycle settings response was empty.')
    },
    onSuccess: (record) => {
      queryClient.setQueryData(queryKeys.cycleSettings(workspaceId, projectId), record)
      invalidate()
    },
  })
}

export function useCycleMutations(workspaceId: string, projectId: string) {
  const invalidate = useInvalidateCycles(workspaceId)
  const path = (cycleId: string) => ({ workspace_id: workspaceId, project_id: projectId, cycle_id: cycleId })
  const update = useMutation({
    mutationFn: async ({ cycleId, body }: { cycleId: string; body: CycleUpdateBody }) =>
      required((await updateCycle({ client: apiClient, path: path(cycleId), body, throwOnError: true })).data, 'Cycle response was empty.'),
    onSettled: invalidate,
  })
  const startToday = useMutation({
    mutationFn: async (cycleId: string) => required((await startCycleToday({ client: apiClient, path: path(cycleId), throwOnError: true })).data, 'Cycle response was empty.'),
    onSettled: invalidate,
  })
  const endToday = useMutation({
    mutationFn: async (cycleId: string) => required((await endCycleToday({ client: apiClient, path: path(cycleId), throwOnError: true })).data, 'Cycle response was empty.'),
    onSettled: invalidate,
  })
  return { update, startToday, endToday }
}
