import { useMemo } from 'react'
import { taskFromRecord, type Task, type TaskStatusDef } from '@/features/tasks/api/models'
import { useProjects } from '@/features/tasks/api/projects'
import { useTasks } from '@/features/tasks/api/tasks'
import { pickerCandidates } from '@/features/tasks/relationsLib'
import { useWorkspace } from '@/features/workspaces/workspaceContext'

/** Recently updated tasks: the browse list, and where identifiers match (the server search only knows text). */
const RECENT = { sort: 'updated_at', order: 'desc', limit: 100 } as const

export interface UseTaskCandidatesInput {
  query: string
  excludeIds: readonly string[]
  excludeDuplicates: boolean
  statuses: TaskStatusDef[]
}

/**
 * Searchable task candidates shared by pickers: the task picker dialog and the parent filter picker.
 * Same data pattern as the command palette; mounted only while the picker is open, since the queries start on mount.
 */
export function useTaskCandidates({ query, excludeIds, excludeDuplicates, statuses }: UseTaskCandidatesInput): { candidates: Task[]; loading: boolean } {
  const { workspace } = useWorkspace()
  const projects = useProjects(workspace.id)
  const search = query.trim()
  const recent = useTasks(workspace.id, RECENT)
  // with an empty query this is the same query key as `recent`, so a single request goes out
  const searched = useTasks(workspace.id, { ...RECENT, search: search || undefined })
  const candidates = useMemo(() => {
    const records = [...(searched.data?.pages ?? []), ...(recent.data?.pages ?? [])].flatMap((page) => page.items)
    const tasks = records.map((record) => taskFromRecord(record, projects.data?.find((project) => project.id === record.project_id)))
    return pickerCandidates({ tasks, query: search, excludeIds, excludeDuplicates, statuses })
  }, [excludeDuplicates, excludeIds, projects.data, recent.data, search, searched.data, statuses])
  const loading = recent.isPending || searched.isFetching
  return { candidates, loading }
}
