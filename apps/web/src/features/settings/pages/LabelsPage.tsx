import { useState, type DragEvent, type FormEvent } from 'react'
import { Link } from 'react-router'
import type { LabelGroupRecord, LabelRecord } from '@/api/generated/types.gen'
import { confirmAction } from '@/components/common/confirmAction'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { SettingsRow } from '@/features/settings/components/SettingsParts'
import { labelGroupConflictCount, useCreateLabel, useDeleteLabel, useLabelGroupMutations, useLabelGroups, useLabels, useUpdateLabel } from '@/features/tasks/api/labels'
import { LABEL_COLORS, LabelColorPicker, LabelPill } from '@/features/tasks/components/TaskLabels'
import { joinConflictFilter } from '@/features/tasks/labelGroups'


function LabelForm({ kind = 'Label', initial, submitLabel, pending, onSubmit, onCancel }: { kind?: 'Label' | 'Group'; initial?: Pick<LabelRecord, 'name' | 'color'>; submitLabel: string; pending: boolean; onSubmit: (name: string, color: string) => Promise<void>; onCancel?: () => void }) {
  const [name, setName] = useState(initial?.name ?? '')
  const [color, setColor] = useState(initial?.color ?? LABEL_COLORS[0]!)
  const submit = async (event: FormEvent) => {
    event.preventDefault()
    const trimmed = name.trim()
    if (!trimmed || pending) return
    try {
      await onSubmit(trimmed, color)
      if (!initial) setName('')
    } catch {
      // The page shows the mutation error.
    }
  }
  return (
    <form className="flex w-full flex-wrap items-center gap-3" onSubmit={(event) => void submit(event)}>
      <Input className="w-full max-w-[260px]" aria-label={`${kind} name`} placeholder={`${kind} name`} maxLength={100} value={name} onChange={(event) => setName(event.target.value)} autoFocus={Boolean(initial)} />
      {/* A color outside the palette (e.g. the GitHub label) shows no selected swatch and is kept unless another is picked. */}
      <LabelColorPicker value={color} onChange={setColor} />
      <div className="flex gap-2">
        <Button type="submit" disabled={!name.trim() || pending}>{submitLabel}</Button>
        {onCancel ? <Button type="button" variant="ghost" onClick={onCancel}>Cancel</Button> : null}
      </div>
    </form>
  )
}

/** A label could not join a group: the tasks that have it and a different label of the group. */
interface JoinConflict {
  label: LabelRecord
  group: LabelGroupRecord
  count: number
}

export function LabelsPage() {
  const { workspace } = useWorkspace()
  const labels = useLabels(workspace.id)
  const groups = useLabelGroups(workspace.id)
  const createLabel = useCreateLabel(workspace.id)
  const updateLabel = useUpdateLabel(workspace.id)
  const deleteLabel = useDeleteLabel(workspace.id)
  const groupMutations = useLabelGroupMutations(workspace.id)
  const [editingId, setEditingId] = useState<string>()
  const [dragId, setDragId] = useState<string | null>(null)
  // the section the dragged label is over: a group id, or 'none' for the labels with no group
  const [overSection, setOverSection] = useState<string | null>(null)
  const [conflict, setConflict] = useState<JoinConflict | null>(null)
  const sorted = [...(labels.data ?? [])].sort((a, b) => a.name.localeCompare(b.name))
  const sections: Array<{ group: LabelGroupRecord | null; labels: LabelRecord[] }> = [
    ...(groups.data ?? []).map((group) => ({ group, labels: sorted.filter((label) => label.group_id === group.id) })),
    { group: null, labels: sorted.filter((label) => !label.group_id) },
  ]

  const remove = async (label: LabelRecord) => {
    if (!await confirmAction({ title: `Delete ${label.name}?`, description: 'The label is removed from all tasks. You cannot undo this.', confirmLabel: 'Delete', danger: true })) return
    deleteLabel.mutate(label)
  }
  const removeGroup = async (group: LabelGroupRecord) => {
    if (!await confirmAction({ title: `Delete the group ${group.name}?`, description: 'The labels of the group stay, with no group.', confirmLabel: 'Delete', danger: true })) return
    groupMutations.remove.mutate(group)
  }
  /** Puts a label in a group (`null`: no group). The server refuses while a task has it and a different label of the group. */
  const moveToGroup = (label: LabelRecord, group: LabelGroupRecord | null) => {
    if ((label.group_id ?? null) === (group?.id ?? null)) return
    setConflict(null)
    updateLabel.mutate({ label, groupId: group?.id ?? null }, {
      onError: (error) => {
        const count = labelGroupConflictCount(error)
        if (count !== null && group) setConflict({ label, group, count })
      },
    })
  }
  const endDrag = () => {
    setDragId(null)
    setOverSection(null)
  }

  return <>
    <SettingsCard title="Create label" description="Labels are shared by all projects in this workspace.">
      <LabelForm submitLabel="Create label" pending={createLabel.isPending} onSubmit={async (name, color) => { await createLabel.mutateAsync({ name, color }) }} />
      {createLabel.isError ? <p className="mt-2 text-xs text-destructive" role="alert">The label could not be created.</p> : null}
    </SettingsCard>
    <SettingsCard title="Create label group" description="A task has only one label of a group, for example one Type or one Size. Drag a label into a group to add it.">
      <LabelForm kind="Group" submitLabel="Create group" pending={groupMutations.create.isPending} onSubmit={async (name, color) => { await groupMutations.create.mutateAsync({ name, color }) }} />
      {groupMutations.create.isError ? <p className="mt-2 text-xs text-destructive" role="alert">The group could not be created. Its name must be different from the other groups.</p> : null}
    </SettingsCard>
    <SettingsCard title="Labels" description="Rename, recolor or delete workspace labels, and sort them into groups." flush>
      <div className="flex flex-col divide-y">
        {labels.isPending ? <SettingsRow>Loading labels…</SettingsRow> : null}
        {labels.isError ? <SettingsRow role="alert">Labels could not be loaded.</SettingsRow> : null}
        {labels.data?.length === 0 ? <SettingsRow>No labels yet.</SettingsRow> : null}
        {conflict ? (
          <SettingsRow className="text-xs text-destructive" role="alert">
            <span className="min-w-0 flex-1">
              {conflict.label.name} cannot join {conflict.group.name}: {conflict.count} {conflict.count === 1 ? 'task has' : 'tasks have'} it and a different {conflict.group.name} label. A task can have only one label of a group. Remove one of the two labels from {conflict.count === 1 ? 'that task' : 'those tasks'}, then try again.
            </span>
            <Link
              className="font-medium underline underline-offset-2"
              to="/tasks"
              state={{ applyFilter: joinConflictFilter(conflict.label.id, sorted.filter((label) => label.group_id === conflict.group.id).map((label) => label.id)) }}
            >
              Show {conflict.count === 1 ? 'the task' : `the ${conflict.count} tasks`}
            </Link>
          </SettingsRow>
        ) : null}
        {sections.map(({ group, labels: members }) => {
          const sectionId = group?.id ?? 'none'
          // the list of labels with no group shows only as a place to drop a label, or when it has labels
          if (!group && members.length === 0 && (groups.data ?? []).length === 0) return null
          return (
            <section
              key={sectionId}
              aria-label={group ? `Group ${group.name}` : 'Labels with no group'}
              data-drop-target={overSection === sectionId || undefined}
              className="flex flex-col divide-y data-[drop-target]:bg-muted/60"
              onDragOver={(event: DragEvent<HTMLElement>) => {
                if (!dragId) return
                event.preventDefault()
                event.dataTransfer.dropEffect = 'move'
                setOverSection(sectionId)
              }}
              onDrop={(event) => {
                event.preventDefault()
                const label = sorted.find((item) => item.id === dragId)
                if (label) moveToGroup(label, group)
                endDrag()
              }}
            >
              {group ? (
                editingId === group.id
                  ? <SettingsRow className="bg-muted/30">
                      <LabelForm kind="Group" initial={group} submitLabel="Save" pending={groupMutations.update.isPending} onCancel={() => setEditingId(undefined)} onSubmit={async (name, color) => { await groupMutations.update.mutateAsync({ group, name, color }); setEditingId(undefined) }} />
                    </SettingsRow>
                  : <SettingsRow className="bg-muted/30 py-2">
                      <div className="flex min-w-0 flex-1 items-center gap-2 text-xs font-medium"><LabelPill label={group} /><span className="text-muted-foreground">{members.length === 1 ? '1 label' : `${members.length} labels`}</span></div>
                      <Button variant="ghost" type="button" aria-label={`Rename group ${group.name}`} onClick={() => { groupMutations.update.reset(); setEditingId(group.id) }}>Rename</Button>
                      <Button variant="ghost" type="button" aria-label={`Delete group ${group.name}`} disabled={groupMutations.remove.isPending} onClick={() => void removeGroup(group)}>Delete</Button>
                    </SettingsRow>
              ) : (groups.data ?? []).length > 0 ? <SettingsRow className="bg-muted/30 py-2 text-xs font-medium text-muted-foreground">No group</SettingsRow> : null}
              {group && members.length === 0 ? <SettingsRow className="text-xs text-muted-foreground">Drag a label here.</SettingsRow> : null}
              {members.map((label) => editingId === label.id
                ? <SettingsRow key={label.id}>
                    <LabelForm initial={label} submitLabel="Save" pending={updateLabel.isPending} onCancel={() => setEditingId(undefined)} onSubmit={async (name, color) => { await updateLabel.mutateAsync({ label, name, color }); setEditingId(undefined) }} />
                  </SettingsRow>
                : <SettingsRow
                    key={label.id}
                    // the row stays mounted while it is dragged: a drag ends when its source leaves the DOM
                    draggable
                    data-dragging={dragId === label.id || undefined}
                    className="data-[dragging]:opacity-40"
                    onDragStart={(event) => {
                      setDragId(label.id)
                      event.dataTransfer.effectAllowed = 'move'
                      event.dataTransfer.setData('text/plain', label.id)
                    }}
                    onDragEnd={endDrag}
                  >
                    <div className="min-w-0 flex-1"><LabelPill label={{ name: label.name, color: label.color }} /></div>
                    {group ? <Button variant="ghost" type="button" aria-label={`Remove ${label.name} from ${group.name}`} disabled={updateLabel.isPending} onClick={() => moveToGroup(label, null)}>Ungroup</Button> : null}
                    <Button variant="ghost" type="button" aria-label={`Edit ${label.name}`} onClick={() => { updateLabel.reset(); setEditingId(label.id) }}>Edit</Button>
                    <Button variant="ghost" type="button" aria-label={`Delete ${label.name}`} disabled={deleteLabel.isPending} onClick={() => void remove(label)}>Delete</Button>
                  </SettingsRow>)}
            </section>
          )
        })}
        {updateLabel.isError && !conflict ? <SettingsRow className="text-xs text-destructive" role="alert">The label could not be saved. It may have been changed by someone else; try again.</SettingsRow> : null}
        {groupMutations.update.isError || groupMutations.remove.isError ? <SettingsRow className="text-xs text-destructive" role="alert">The group could not be saved. It may have been changed by someone else; try again.</SettingsRow> : null}
        {deleteLabel.isError ? <SettingsRow className="text-xs text-destructive" role="alert">The label could not be deleted.</SettingsRow> : null}
      </div>
    </SettingsCard>
  </>
}
