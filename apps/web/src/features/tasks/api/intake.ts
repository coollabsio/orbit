import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { apiClient } from '@/api/client'
import {
  createRecurringTask,
  createTaskFromPayload,
  createTemplate,
  deleteRecurringTask,
  deleteTemplate,
  listRecurringTasks,
  listTemplates,
  triageTask,
  updateRecurringTask,
  updateTemplate,
} from '@/api/generated/sdk.gen'
import type {
  RecurringTaskBody,
  RecurringTaskRecord,
  RecurringTaskUpdateBody,
  TaskPayload,
  TaskQueryBody,
  TemplateBody,
  TemplateRecord,
  TemplateUpdateBody,
  TriageBody,
} from '@/api/generated/types.gen'
import { queryKeys } from '@/api/queryKeys'
import { taskQueryOptions } from '@/features/views/api/taskQuery'

export type Template = TemplateRecord
export type RecurringTask = RecurringTaskRecord
export type { TaskPayload }

function required<T>(data: T | undefined, message: string): T {
  if (data === undefined) throw new Error(message)
  return data
}

/** The triage queue of the workspace, oldest first. One query feeds the sidebar count and the page. */
const TRIAGE_QUERY: TaskQueryBody = {
  filter: { op: 'and', children: [{ field: 'status_category', operator: 'is', value: ['triage'] }] } as unknown as TaskQueryBody['filter'],
  order_by: 'created',
  order_direction: 'asc',
  show_completed: 'all',
}

export function useTriageQueue(workspaceId: string, enabled = true) {
  return useQuery({ ...taskQueryOptions(workspaceId, TRIAGE_QUERY), enabled })
}

export function useTriageTask(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async ({ taskId, body }: { taskId: string; body: TriageBody }) => {
      const { data } = await triageTask({ client: apiClient, path: { workspace_id: workspaceId, task_id: taskId }, body, throwOnError: true })
      return required(data, 'Triage response was empty.')
    },
    onSettled: () => void queryClient.invalidateQueries({ queryKey: queryKeys.tasks.all(workspaceId) }),
  })
}

/** Creates a task and its sub-issues in one request, so a template is never used in part. */
export function useCreateTaskFromPayload(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async ({ projectId, payload }: { projectId: string; payload: TaskPayload }) => {
      const { data } = await createTaskFromPayload({ client: apiClient, path: { workspace_id: workspaceId }, body: { project_id: projectId, payload }, throwOnError: true })
      return required(data, 'Create task response was empty.')
    },
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.tasks.all(workspaceId) }),
  })
}

export function useTemplates(workspaceId: string, projectId: string | undefined) {
  return useQuery({
    queryKey: queryKeys.templates(workspaceId, projectId ?? ''),
    enabled: Boolean(projectId),
    queryFn: async () => {
      const { data } = await listTemplates({ client: apiClient, path: { workspace_id: workspaceId, project_id: projectId ?? '' }, throwOnError: true })
      return required(data, 'Templates response was empty.').items
    },
  })
}

export function useTemplateMutations(workspaceId: string, projectId: string) {
  const queryClient = useQueryClient()
  const path = { workspace_id: workspaceId, project_id: projectId }
  const onSettled = () => void queryClient.invalidateQueries({ queryKey: queryKeys.templates(workspaceId, projectId) })
  const create = useMutation({
    mutationFn: async (body: TemplateBody) => required((await createTemplate({ client: apiClient, path, body, throwOnError: true })).data, 'Create template response was empty.'),
    onSettled,
  })
  const update = useMutation({
    mutationFn: async ({ templateId, body }: { templateId: string; body: TemplateUpdateBody }) =>
      required((await updateTemplate({ client: apiClient, path: { ...path, template_id: templateId }, body, throwOnError: true })).data, 'Update template response was empty.'),
    onSettled,
  })
  const remove = useMutation({
    mutationFn: async (template: Pick<Template, 'id' | 'version'>) => {
      await deleteTemplate({ client: apiClient, path: { ...path, template_id: template.id }, query: { expected_version: template.version }, throwOnError: true })
    },
    onSettled,
  })
  return { create, update, remove }
}

export function useRecurringTasks(workspaceId: string, projectId: string | undefined) {
  return useQuery({
    queryKey: queryKeys.recurringTasks(workspaceId, projectId ?? ''),
    enabled: Boolean(projectId),
    queryFn: async () => {
      const { data } = await listRecurringTasks({ client: apiClient, path: { workspace_id: workspaceId, project_id: projectId ?? '' }, throwOnError: true })
      return required(data, 'Recurring tasks response was empty.').items
    },
  })
}

export function useRecurringTaskMutations(workspaceId: string, projectId: string) {
  const queryClient = useQueryClient()
  const path = { workspace_id: workspaceId, project_id: projectId }
  const onSettled = () => void queryClient.invalidateQueries({ queryKey: queryKeys.recurringTasks(workspaceId, projectId) })
  const create = useMutation({
    mutationFn: async (body: RecurringTaskBody) => required((await createRecurringTask({ client: apiClient, path, body, throwOnError: true })).data, 'Create recurring task response was empty.'),
    onSettled,
  })
  const update = useMutation({
    mutationFn: async ({ recurringId, body }: { recurringId: string; body: RecurringTaskUpdateBody }) =>
      required((await updateRecurringTask({ client: apiClient, path: { ...path, recurring_id: recurringId }, body, throwOnError: true })).data, 'Update recurring task response was empty.'),
    onSettled,
  })
  const remove = useMutation({
    mutationFn: async (routine: Pick<RecurringTask, 'id' | 'version'>) => {
      await deleteRecurringTask({ client: apiClient, path: { ...path, recurring_id: routine.id }, query: { expected_version: routine.version }, throwOnError: true })
    },
    onSettled,
  })
  return { create, update, remove }
}
