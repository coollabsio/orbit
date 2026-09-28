import { useState, type FormEvent } from 'react'
import type { LabelRecord } from '@/api/generated/types.gen'
import { confirmAction } from '@/components/common/confirmAction'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { useCreateLabel, useDeleteLabel, useLabels, useUpdateLabel } from '@/features/tasks/api/labels'
import { LABEL_COLORS, LabelPill } from '@/features/tasks/components/TaskLabels'

const ROW = 'flex flex-wrap items-center gap-3 px-4 py-3'

function ColorSwatches({ value, onChange }: { value: string; onChange: (color: string) => void }) {
  return (
    <div className="flex flex-wrap gap-1.5">
      {LABEL_COLORS.map((color) => (
        <Button key={color} type="button" size="icon-xs" className="size-[18px] rounded-full border-2 border-transparent p-0 aria-pressed:border-foreground" style={{ background: color }} aria-label={`Color ${color}`} aria-pressed={value === color} onClick={() => onChange(color)} />
      ))}
    </div>
  )
}

function LabelForm({ initial, submitLabel, pending, onSubmit, onCancel }: { initial?: LabelRecord; submitLabel: string; pending: boolean; onSubmit: (name: string, color: string) => Promise<void>; onCancel?: () => void }) {
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
      <Input className="w-full max-w-[260px]" aria-label="Label name" placeholder="Label name" maxLength={100} value={name} onChange={(event) => setName(event.target.value)} autoFocus={Boolean(initial)} />
      {/* A color outside the palette (e.g. the GitHub label) shows no selected swatch and is kept unless another is picked. */}
      <ColorSwatches value={color} onChange={setColor} />
      <div className="flex gap-2">
        <Button type="submit" disabled={!name.trim() || pending}>{submitLabel}</Button>
        {onCancel ? <Button type="button" variant="ghost" onClick={onCancel}>Cancel</Button> : null}
      </div>
    </form>
  )
}

export function LabelsPage() {
  const { workspace } = useWorkspace()
  const labels = useLabels(workspace.id)
  const createLabel = useCreateLabel(workspace.id)
  const updateLabel = useUpdateLabel(workspace.id)
  const deleteLabel = useDeleteLabel(workspace.id)
  const [editingId, setEditingId] = useState<string>()
  const sorted = [...(labels.data ?? [])].sort((a, b) => a.name.localeCompare(b.name))

  const remove = async (label: LabelRecord) => {
    if (!await confirmAction({ title: `Delete ${label.name}?`, description: 'The label is removed from all tasks. You cannot undo this.', confirmLabel: 'Delete', danger: true })) return
    deleteLabel.mutate(label)
  }

  return <>
    <SettingsCard title="Create label" description="Labels are shared by all projects in this workspace.">
      <LabelForm submitLabel="Create label" pending={createLabel.isPending} onSubmit={async (name, color) => { await createLabel.mutateAsync({ name, color }) }} />
      {createLabel.isError ? <p className="mt-2 text-xs text-destructive" role="alert">The label could not be created.</p> : null}
    </SettingsCard>
    <SettingsCard title="Labels" description="Rename, recolor or delete workspace labels." flush>
      <div className="flex flex-col divide-y divide-border">
        {labels.isPending ? <div className={ROW}>Loading labels…</div> : null}
        {labels.isError ? <div className={ROW} role="alert">Labels could not be loaded.</div> : null}
        {labels.data?.length === 0 ? <div className={ROW}>No labels yet.</div> : null}
        {sorted.map((label) => editingId === label.id
          ? <div className={ROW} key={label.id}>
              <LabelForm initial={label} submitLabel="Save" pending={updateLabel.isPending} onCancel={() => setEditingId(undefined)} onSubmit={async (name, color) => { await updateLabel.mutateAsync({ label, name, color }); setEditingId(undefined) }} />
            </div>
          : <div className={ROW} key={label.id}>
              <div className="min-w-0 flex-1"><LabelPill label={label} /></div>
              <Button variant="ghost" type="button" aria-label={`Edit ${label.name}`} onClick={() => { updateLabel.reset(); setEditingId(label.id) }}>Edit</Button>
              <Button variant="ghost" type="button" aria-label={`Delete ${label.name}`} disabled={deleteLabel.isPending} onClick={() => void remove(label)}>Delete</Button>
            </div>)}
        {updateLabel.isError ? <p className={`${ROW} text-xs text-destructive`} role="alert">The label could not be saved. It may have been changed by someone else; try again.</p> : null}
        {deleteLabel.isError ? <p className={`${ROW} text-xs text-destructive`} role="alert">The label could not be deleted.</p> : null}
      </div>
    </SettingsCard>
  </>
}
