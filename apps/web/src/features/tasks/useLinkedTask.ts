import { ApiProblem } from '@orbit/api/problem'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { taskFromRecord } from './api/models'
import { useProjectStatuses, useProjects } from './api/projects'
import { useTask, useTaskRouteId } from './api/tasks'

/** The server says there is no such task for this reader (a failed request is not that). */
function notFound(error: unknown): boolean {
  return error instanceof ApiProblem && (error.status === 404 || error.status === 403)
}

/**
 * The task a link or a typed ID names (its id or identifier), as the reader's own requests return it. `linked` is null
 * while it loads, when the reader cannot see it, when it is in the trash and when the request failed; `missing` (no
 * such task for the reader) and `failed` (no answer) tell those from loading.
 */
export function useLinkedTaskLookup(taskRef: string) {
  const { workspace } = useWorkspace()
  const { taskId, error } = useTaskRouteId(workspace.id, taskRef)
  const query = useTask(workspace.id, taskId)
  const record = query.data
  const projects = useProjects(workspace.id, record !== undefined).data
  const statuses = useProjectStatuses(workspace.id, record?.project_id).data ?? []
  const missing = notFound(error) || notFound(query.error) || Boolean(record?.deleted_at)
  const failed = !missing && (Boolean(error) || query.isError)
  if (!record || record.deleted_at) return { linked: null, missing, failed }
  const project = projects?.find((candidate) => candidate.id === record.project_id)
  const task = taskFromRecord(record, project)
  return { linked: { task, project, projects: projects ?? [], statuses, status: statuses.find((candidate) => candidate.id === task.statusId) }, missing, failed }
}

/** `useLinkedTaskLookup` without the difference between loading and missing: null until the task is there. */
export function useLinkedTask(taskRef: string) {
  return useLinkedTaskLookup(taskRef).linked
}
