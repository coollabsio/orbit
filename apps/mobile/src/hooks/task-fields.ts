/** The field gallery and a project's fields (custom task fields, issue #87), over `data/task-api.ts`. */
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { keys } from '../data/keys'
import * as taskApi from '../data/task-api'
import { useOrbit } from '../orbit-context'

/** Under the `projects` and `task` prefixes, so the workspace events socket refreshes them with the projects and tasks. */
export const fieldKeys = {
  gallery: (workspaceId: string) => [...keys.projects(workspaceId), 'task-fields'] as const,
  project: (workspaceId: string, projectId: string) => [...keys.projects(workspaceId), projectId, 'fields'] as const,
  values: (workspaceId: string, taskId: string) => [...keys.task(workspaceId, taskId), 'fields'] as const,
}

export function useTaskFieldGallery(enabled = true) {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: fieldKeys.gallery(workspaceId), queryFn: () => taskApi.taskFields(client, workspaceId), enabled })
}

/** A gallery change shows in every project's fields and task's values too: every `…/fields` key of the workspace refetches. */
function useRefreshFields() {
  const { workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  return () => {
    void queryClient.invalidateQueries({ queryKey: fieldKeys.gallery(workspaceId) })
    void queryClient.invalidateQueries({ predicate: ({ queryKey }) => (queryKey[0] === 'projects' || queryKey[0] === 'task') && queryKey[1] === workspaceId && queryKey.at(-1) === 'fields' })
  }
}

/** Create, edit and delete of gallery fields. */
export function useTaskFieldWrites() {
  const { client, workspaceId } = useOrbit()
  const refresh = useRefreshFields()
  return {
    create: useMutation({ mutationFn: (body: taskApi.TaskFieldBody) => taskApi.newTaskField(client, workspaceId, body), onSettled: refresh }),
    update: useMutation({
      mutationFn: ({ field, changes }: { field: taskApi.TaskField; changes: Omit<taskApi.TaskFieldUpdateBody, 'expected_version'> }) =>
        taskApi.changeTaskField(client, workspaceId, field.id, { ...changes, expected_version: field.version }),
      onSettled: refresh,
    }),
    remove: useMutation({ mutationFn: (field: taskApi.TaskField) => taskApi.removeTaskField(client, workspaceId, field), onSettled: refresh }),
  }
}

/** Replaces the fields the project shows, in order. */
export function useSetProjectTaskFields(projectId: string) {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  const refresh = useRefreshFields()
  return useMutation({
    mutationFn: (fieldIds: string[]) => taskApi.saveProjectTaskFields(client, workspaceId, projectId, fieldIds),
    onSuccess: (fields) => queryClient.setQueryData(fieldKeys.project(workspaceId, projectId), fields),
    onSettled: refresh,
  })
}
