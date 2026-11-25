import { useState } from 'react'
import { MoreH as MoreHorizontal, Edit as Pencil, Trash as Trash2 } from 'reicon-react'
import type { TaskField } from '@orbit/api/generated/types.gen'
import { ColorDot } from '@/components/common/ColorDot'
import { confirmAction } from '@/components/common/confirmAction'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Tip } from '@/components/common/Tip'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { LoadingText } from '@/components/ui/spinner'
import { SettingsRow } from '@/features/settings/components/SettingsParts'
import { useCreateTaskField, useDeleteTaskField, useTaskFields, useUpdateTaskField } from '@/features/tasks/api/fields'
import { TaskFieldForm } from '@/features/tasks/components/TaskFieldForm'
import { fieldKindLabel } from '@/features/tasks/fieldsLib'
import { useWorkspace } from '@/features/workspaces/workspaceContext'

function projectsText(count: number): string {
  return count === 0 ? 'Not in a project' : count === 1 ? 'In 1 project' : `In ${count} projects`
}

/** The workspace's field gallery: every member creates fields; the creator, owners and admins change them. */
export function TaskFieldsPage() {
  const { workspace } = useWorkspace()
  const fields = useTaskFields(workspace.id)
  const createField = useCreateTaskField(workspace.id)
  const updateField = useUpdateTaskField(workspace.id)
  const deleteField = useDeleteTaskField(workspace.id)
  const [editingId, setEditingId] = useState<string>()
  const list = fields.data ?? []

  const remove = async (field: TaskField) => {
    if (!await confirmAction({ title: `Delete ${field.name}?`, description: `${projectsText(field.project_count)}. Every task loses its value of this field. You cannot undo this.`, confirmLabel: 'Delete', danger: true })) return
    deleteField.mutate(field)
  }

  return (
    <SettingsCard title="Task fields" description="Custom fields that any project of the workspace can add to its tasks, for example a customer, a budget or the time spent. Add a field to a project in its settings, under Fields." flush>
      <div className="flex flex-col divide-y">
        <SettingsRow>
          <TaskFieldForm submitLabel="Create field" pending={createField.isPending} onSubmit={async (body) => { await createField.mutateAsync(body) }} />
        </SettingsRow>
        {createField.isError ? <SettingsRow className="text-xs text-destructive" role="alert">The field could not be created. Its name may already be in use.</SettingsRow> : null}
        {fields.isPending ? <SettingsRow><LoadingText>Loading fields…</LoadingText></SettingsRow> : null}
        {fields.isError ? <SettingsRow role="alert">Fields could not be loaded.</SettingsRow> : null}
        {fields.isSuccess && list.length === 0 ? <SettingsRow>No fields yet.</SettingsRow> : null}
        {list.map((field) => editingId === field.id
          ? <SettingsRow key={field.id}>
              <TaskFieldForm
                initial={field}
                submitLabel="Save"
                pending={updateField.isPending}
                onCancel={() => setEditingId(undefined)}
                onSubmit={async ({ name, options }) => {
                  await updateField.mutateAsync({ fieldId: field.id, body: { name, options, expected_version: field.version } })
                  setEditingId(undefined)
                }}
              />
            </SettingsRow>
          : <SettingsRow key={field.id} className="group/row flex-nowrap">
              <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                <span className="truncate text-sm font-medium text-foreground">
                  {field.name}
                  <span className="font-normal text-muted-foreground"> · {fieldKindLabel(field.kind)}</span>
                </span>
                {field.options.length > 0 ? (
                  <span className="flex min-w-0 flex-wrap items-center gap-x-2.5 gap-y-1 text-xs text-muted-foreground/70">
                    {field.options.map((option) => <span key={option.id} className="inline-flex items-center gap-1"><ColorDot color={option.color} />{option.name}</span>)}
                  </span>
                ) : null}
              </div>
              <span className="shrink-0 text-xs text-muted-foreground">{projectsText(field.project_count)}</span>
              {field.can_manage ? (
                <DropdownMenu>
                  <Tip label="Actions">
                    <DropdownMenuTrigger
                      render={
                        <Button variant="ghost" size="icon-sm" className="size-[30px] rounded-full bg-muted transition-opacity duration-150 ease-out focus-visible:opacity-100 aria-expanded:opacity-100 hover-fine:opacity-0 hover-fine:group-hover/row:opacity-100" aria-label={`${field.name} actions`}>
                          <MoreHorizontal className="size-4" />
                        </Button>
                      }
                    />
                  </Tip>
                  <DropdownMenuContent align="end" className="w-auto min-w-45">
                    <DropdownMenuItem onClick={() => { updateField.reset(); setEditingId(field.id) }}>
                      <Pencil className="size-3.5" />
                      Edit
                    </DropdownMenuItem>
                    {/* data-danger (not variant="destructive"): the menu popup forces destructive items to the accent colour */}
                    <DropdownMenuItem className="text-destructive focus:bg-destructive/10 focus:text-destructive *:[svg]:text-destructive" data-danger="true" disabled={deleteField.isPending} onClick={() => void remove(field)}>
                      <Trash2 className="size-3.5" />
                      Delete
                    </DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>
              ) : null}
            </SettingsRow>)}
        {updateField.isError ? <SettingsRow className="text-xs text-destructive" role="alert">The field could not be saved. Its name may be in use, or someone else changed it; try again.</SettingsRow> : null}
        {deleteField.isError ? <SettingsRow className="text-xs text-destructive" role="alert">The field could not be deleted.</SettingsRow> : null}
      </div>
    </SettingsCard>
  )
}
