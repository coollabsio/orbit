import { createDatabaseTemplate, deleteDatabaseTemplate, updateDatabaseTemplate } from '@orbit/api/generated/sdk.gen'
import type { CreateDatabaseTemplateBody, DatabaseDetail, DatabaseTemplate, UpdateDatabaseTemplateBody } from '@orbit/api/generated/types.gen'
import { apiClient } from '@/api/client'
import { useDatabaseMutation } from './api'

function withTemplates(detail: DatabaseDetail, change: (templates: DatabaseTemplate[]) => DatabaseTemplate[]): DatabaseDetail {
  return { ...detail, templates: change(detail.templates ?? []) }
}

/** A new template, empty or copied from a row (`from_row_id`). */
export function useCreateTemplate(workspaceId: string, databaseId: string) {
  return useDatabaseMutation<CreateDatabaseTemplateBody, DatabaseTemplate>(workspaceId, databaseId, {
    request: async (path, body) => {
      const { data } = await createDatabaseTemplate({ client: apiClient, path, body, throwOnError: true })
      if (!data) throw new Error('Create template response was empty.')
      return data
    },
    reconcile: (detail, template) => withTemplates(detail, (templates) => [...templates, template]),
    error: 'Could not add the template.',
  })
}

export type TemplateInput = { templateId: string; body: UpdateDatabaseTemplateBody }

export function useUpdateTemplate(workspaceId: string, databaseId: string) {
  return useDatabaseMutation<TemplateInput, DatabaseTemplate>(workspaceId, databaseId, {
    request: async (path, { templateId, body }) => {
      const { data } = await updateDatabaseTemplate({ client: apiClient, path: { ...path, template_id: templateId }, body, throwOnError: true })
      if (!data) throw new Error('Update template response was empty.')
      return data
    },
    optimistic: (detail, { templateId, body }) =>
      withTemplates(detail, (templates) => templates.map((template) => (template.id === templateId && body.name ? { ...template, name: body.name } : template))),
    refetch: true,
    error: 'Could not change the template.',
  })
}

/** Deletes a template for good; views that used it as their default go back to an empty row (refetched). */
export function useDeleteTemplate(workspaceId: string, databaseId: string) {
  return useDatabaseMutation<string, void>(workspaceId, databaseId, {
    request: async (path, templateId) => {
      await deleteDatabaseTemplate({ client: apiClient, path: { ...path, template_id: templateId }, throwOnError: true })
    },
    optimistic: (detail, templateId) => withTemplates(detail, (templates) => templates.filter((template) => template.id !== templateId)),
    refetch: true,
    error: 'Could not delete the template.',
  })
}
