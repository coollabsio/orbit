import { useState } from 'react'
import { toast } from 'sonner'
import { MoreH as MoreHorizontal, Edit as Pencil, Plus, Trash as Trash2 } from 'reicon-react'
import type { TaskField } from '@orbit/api/generated/types.gen'
import { OPTIONAL_TASK_PROPERTIES, showsTaskProperty, withTaskProperty } from '@orbit/api/taskProperties'
import { InfoTip } from '@/components/common/InfoTip'
import { ColorDot } from '@/components/common/ColorDot'
import { confirmAction } from '@/components/common/confirmAction'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Tip } from '@/components/common/Tip'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Switch } from '@/components/ui/switch'
import { LoadingText } from '@/components/ui/spinner'
import { SettingsRow } from '@/features/settings/components/SettingsParts'
import { useCreateTaskField, useDeleteTaskField, useTaskFields, useUpdateTaskField } from '@/features/tasks/api/fields'
import { TaskFieldDialog } from '@/features/tasks/components/TaskFieldForm'
import { fieldKindLabel } from '@/features/tasks/fieldsLib'
import { useSetHiddenTaskProperties } from '@/features/workspaces/api'
import { useCan } from '@/features/workspaces/permissions'
import { useWorkspace } from '@/features/workspaces/workspaceContext'

function projectsText(count: number): string {
  return count === 0 ? 'Not in a project' : count === 1 ? 'In 1 project' : `In ${count} projects`
}

/** Milestone, cycle and estimate: owners and admins turn them off for the whole workspace. */
function BuiltInPropertiesCard() {
  const { workspace } = useWorkspace()
  const canChange = useCan('workspace.update')
  const save = useSetHiddenTaskProperties(workspace.id)
  return (
    <SettingsCard title="Built-in properties" description="Turn off a property the workspace does not use: tasks, filters and display options stop showing it. Tasks keep their values, and they show again when you turn it back on. Owners and admins change this.">
      <div className="flex flex-col divide-y divide-border">
        {OPTIONAL_TASK_PROPERTIES.map(({ property, label, help }) => (
          <div key={property} className="flex items-center justify-between gap-4 py-3 first:pt-0 last:pb-0">
            <span className="inline-flex items-center gap-1.5 text-sm font-medium text-foreground">{label} <InfoTip text={help} /></span>
            <Switch
              aria-label={label}
              checked={showsTaskProperty(workspace, property)}
              disabled={!canChange || save.isPending}
              onCheckedChange={(shown: boolean) =>
                save.mutate(withTaskProperty(workspace, property, shown), { onError: () => toast.error('Could not save your choice. Try again.') })
              }
            />
          </div>
        ))}
      </div>
    </SettingsCard>
  )
}

/** The workspace's field gallery: every member creates fields; the creator, owners and admins change them. */
export function TaskFieldsPage() {
  const { workspace } = useWorkspace()
  const fields = useTaskFields(workspace.id)
  const createField = useCreateTaskField(workspace.id)
  const updateField = useUpdateTaskField(workspace.id)
  const deleteField = useDeleteTaskField(workspace.id)
  const [editingId, setEditingId] = useState<string>()
  const [creating, setCreating] = useState(false)
  const list = fields.data ?? []
  const editing = list.find((field) => field.id === editingId)

  const remove = async (field: TaskField) => {
    if (!await confirmAction({ title: `Delete ${field.name}?`, description: `${projectsText(field.project_count)}. Every task loses its value of this field. You cannot undo this.`, confirmLabel: 'Delete', danger: true })) return
    deleteField.mutate(field)
  }

  return (
    <>
      <BuiltInPropertiesCard />
      <SettingsCard
        title="Task fields"
        description="Custom fields that any project of the workspace can add to its tasks, for example a customer, a customer group or a budget. Add a field to a project in its settings, under Fields."
        actions={<Button type="button" onClick={() => { createField.reset(); setCreating(true) }}><Plus className="size-3.5" />New field</Button>}
        flush
      >
        <div className="flex flex-col divide-y">
          {fields.isPending ? <SettingsRow><LoadingText>Loading fields…</LoadingText></SettingsRow> : null}
          {fields.isError ? <SettingsRow role="alert">Fields could not be loaded.</SettingsRow> : null}
          {fields.isSuccess && list.length === 0 ? <SettingsRow>No fields yet.</SettingsRow> : null}
          {list.map((field) => (
              <SettingsRow key={field.id} className="group/row flex-nowrap">
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
              </SettingsRow>
          ))}
          {deleteField.isError ? <SettingsRow className="text-xs text-destructive" role="alert">The field could not be deleted.</SettingsRow> : null}
        </div>
      </SettingsCard>
      {creating ? (
        <TaskFieldDialog
          title="New field"
          gallery={list}
          submitLabel="Create field"
          pending={createField.isPending}
          error={createField.isError ? 'The field could not be created. Its name may already be in use.' : undefined}
          onClose={() => setCreating(false)}
          onSubmit={async (body) => {
            await createField.mutateAsync(body)
            setCreating(false)
          }}
        />
      ) : null}
      {editing ? (
        <TaskFieldDialog
          title={`Edit ${editing.name}`}
          initial={editing}
          submitLabel="Save"
          pending={updateField.isPending}
          error={updateField.isError ? 'The field could not be saved. Its name may be in use, or someone else changed it; try again.' : undefined}
          onClose={() => setEditingId(undefined)}
          onSubmit={async ({ name, options }) => {
            await updateField.mutateAsync({ fieldId: editing.id, body: { name, options, expected_version: editing.version } })
            setEditingId(undefined)
          }}
        />
      ) : null}
    </>
  )
}
