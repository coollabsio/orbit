import { useQueries } from '@tanstack/react-query'
import { ApiProblem } from '@/api/problem'
import { taskIdentifier } from '@/features/tasks/api/models'
import { useProjects } from '@/features/tasks/api/projects'
import { taskRecordQuery } from '@/features/tasks/api/tasks'
import { NO_PARENT, listValue, type TaskFilterRef } from './filterFields'
import { isGroup, type FilterGroup } from './viewState'

/** Parent condition values from every branch of the tree, "none" dropped, deduped in first-seen order. */
export function parentFilterIds(filter: FilterGroup): string[] {
  const seen = new Set<string>()
  const ids: string[] = []
  const walk = (group: FilterGroup) => {
    for (const child of group.children) {
      if (isGroup(child)) {
        walk(child)
        continue
      }
      if (child.field !== 'parent') continue
      for (const value of listValue(child)) {
        if (value === NO_PARENT || seen.has(value)) continue
        seen.add(value)
        ids.push(value)
      }
    }
  }
  walk(filter)
  return ids
}

/**
 * The task each Parent filter value refers to, for chip and picker text. `null` means the task was loaded and is
 * deleted or trashed (GET /tasks/{id} 404s for a trashed task); a still-loading or otherwise-failed id is left out,
 * so its chip falls back to a short id until it resolves.
 */
export function useFilterTaskRefs(workspaceId: string, filter: FilterGroup): Record<string, TaskFilterRef | null> {
  const ids = parentFilterIds(filter)
  const projects = useProjects(workspaceId)
  const results = useQueries({ queries: ids.map((id) => ({ ...taskRecordQuery(workspaceId, id), retry: false })) })
  const refs: Record<string, TaskFilterRef | null> = {}
  ids.forEach((id, index) => {
    const result = results[index]
    if (!result) return
    if (result.data) {
      if (result.data.deleted_at) {
        refs[id] = null
        return
      }
      const project = projects.data?.find((item) => item.id === result.data.project_id)
      refs[id] = { identifier: taskIdentifier(result.data.id, project), title: result.data.title }
      return
    }
    if (result.error instanceof ApiProblem && result.error.status === 404) refs[id] = null
  })
  return refs
}
