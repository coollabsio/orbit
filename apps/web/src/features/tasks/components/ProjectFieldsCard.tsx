import { useState } from 'react'
import { Link } from 'react-router'
import { ArrowDown, ArrowUp, Xmark as X, Plus } from 'reicon-react'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Tip } from '@/components/common/Tip'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { LoadingText } from '@/components/ui/spinner'
import { SettingsRow } from '@/features/settings/components/SettingsParts'
import { useCreateTaskField, useProjectTaskFields, useSetProjectTaskFields, useTaskFields } from '@/features/tasks/api/fields'
import { TaskFieldForm } from '@/features/tasks/components/TaskFieldForm'
import { fieldKindLabel } from '@/features/tasks/fieldsLib'
import { useWorkspace } from '@/features/workspaces/workspaceContext'

/** The gallery fields this project's tasks show, in order: add one (or a new one), reorder, remove. */
export function ProjectFieldsCard({ projectId, readOnly }: { projectId: string; readOnly: boolean }) {
  const { workspace } = useWorkspace()
  const shown = useProjectTaskFields(workspace.id, projectId)
  const gallery = useTaskFields(workspace.id)
  const setFields = useSetProjectTaskFields(workspace.id, projectId)
  const createField = useCreateTaskField(workspace.id)
  const [creating, setCreating] = useState(false)
  const list = shown.data ?? []
  const ids = list.map((field) => field.id)
  const addable = (gallery.data ?? []).filter((field) => !ids.includes(field.id))

  const save = (next: string[]) => setFields.mutate(next)
  const move = (index: number, by: -1 | 1) => {
    const next = [...ids]
    const [moved] = next.splice(index, 1)
    next.splice(index + by, 0, moved!)
    save(next)
  }

  return (
    <SettingsCard
      title="Fields"
      description="Custom fields that this project's tasks show, in this order. The fields come from the workspace's field gallery, so other projects can use the same field. A task keeps its values when a field is removed here, and shows them again when the field comes back."
      actions={!readOnly ? (
        <DropdownMenu>
          <DropdownMenuTrigger render={<Button variant="outline" disabled={setFields.isPending}><Plus className="size-3.5" />Add field</Button>} />
          <DropdownMenuContent align="end" className="w-auto min-w-52">
            {addable.map((field) => (
              <DropdownMenuItem key={field.id} onClick={() => save([...ids, field.id])}>
                <span className="truncate">{field.name}</span>
                <span className="ml-auto pl-3 text-xs text-muted-foreground">{fieldKindLabel(field.kind)}</span>
              </DropdownMenuItem>
            ))}
            {addable.length > 0 ? <DropdownMenuSeparator /> : null}
            <DropdownMenuItem onClick={() => { createField.reset(); setCreating(true) }}>
              <Plus className="size-3.5" />
              New field…
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      ) : undefined}
      flush
    >
      <div className="flex flex-col divide-y">
        {creating ? (
          <SettingsRow>
            <TaskFieldForm
              gallery={gallery.data}
              submitLabel="Create and add"
              pending={createField.isPending || setFields.isPending}
              onCancel={() => setCreating(false)}
              onSubmit={async (body) => {
                const field = await createField.mutateAsync(body)
                await setFields.mutateAsync([...ids, field.id])
                setCreating(false)
              }}
            />
          </SettingsRow>
        ) : null}
        {createField.isError ? <SettingsRow className="text-xs text-destructive" role="alert">The field could not be created. Its name may already be in use.</SettingsRow> : null}
        {shown.isPending ? <SettingsRow><LoadingText>Loading fields…</LoadingText></SettingsRow> : null}
        {shown.isError ? <SettingsRow role="alert">Fields could not be loaded.</SettingsRow> : null}
        {shown.isSuccess && list.length === 0 ? (
          <SettingsRow className="text-sm text-muted-foreground">
            No fields yet. Add one from the workspace's <Link className="text-foreground underline" to="/settings/task-fields">field gallery</Link>.
          </SettingsRow>
        ) : null}
        {list.map((field, index) => (
          <SettingsRow key={field.id} className="group/row flex-nowrap">
            <span className="min-w-0 flex-1 truncate text-sm font-medium text-foreground">
              {field.name}
              <span className="font-normal text-muted-foreground"> · {fieldKindLabel(field.kind)}</span>
            </span>
            {!readOnly ? (
              <div className="flex shrink-0 gap-1">
                <Tip label="Move up"><Button variant="ghost" size="icon-sm" aria-label={`Move ${field.name} up`} disabled={index === 0 || setFields.isPending} onClick={() => move(index, -1)}><ArrowUp className="size-3.5" /></Button></Tip>
                <Tip label="Move down"><Button variant="ghost" size="icon-sm" aria-label={`Move ${field.name} down`} disabled={index === list.length - 1 || setFields.isPending} onClick={() => move(index, 1)}><ArrowDown className="size-3.5" /></Button></Tip>
                <Tip label="Remove from project"><Button variant="ghost" size="icon-sm" aria-label={`Remove ${field.name}`} disabled={setFields.isPending} onClick={() => save(ids.filter((id) => id !== field.id))}><X className="size-3.5" /></Button></Tip>
              </div>
            ) : null}
          </SettingsRow>
        ))}
        {setFields.isError ? <SettingsRow className="text-xs text-destructive" role="alert">The project's fields could not be saved.</SettingsRow> : null}
      </div>
    </SettingsCard>
  )
}
