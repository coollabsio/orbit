import { useState } from 'react'
import { Add, Xmark } from 'reicon-react'
import type { LabelRecord } from '../../../api/generated/types.gen'
import { Dropdown } from '../../../components/ui/Dropdown'
import { useCreateLabel } from '../api/labels'

const LABEL_COLORS = ['#8b5cf6', '#0ea5e9', '#22c55e', '#eab308', '#f97316', '#ef4444', '#ec4899', '#64748b']

export function LabelPill({ label }: { label: LabelRecord }) {
  return <span className="pill"><span className="pill-dot" style={{ background: label.color }} />{label.name}</span>
}

export function TaskLabels({ workspaceId, labelIds, labels, onChange }: { workspaceId: string; labelIds: string[]; labels: LabelRecord[]; onChange: (ids: string[]) => void }) {
  const selected = labels.filter((label) => labelIds.includes(label.id))
  const createLabel = useCreateLabel(workspaceId)
  const [name, setName] = useState('')
  const [color, setColor] = useState(LABEL_COLORS[0]!)
  const submit = async (event: React.FormEvent) => {
    event.preventDefault()
    const trimmed = name.trim()
    if (!trimmed || createLabel.isPending) return
    const label = await createLabel.mutateAsync({ name: trimmed, color })
    onChange(labelIds.includes(label.id) ? labelIds : [...labelIds, label.id])
    setName('')
  }
  return (
    <div className="tasks-side-pills">
      {selected.map((label) => (
        <span key={label.id} className="pill tasks-label-pill">
          <span className="pill-dot" style={{ background: label.color }} />
          {label.name}
          <button type="button" className="tasks-label-remove" aria-label={`Remove label ${label.name}`} onClick={() => onChange(labelIds.filter((id) => id !== label.id))}>
            <Xmark size={12} />
          </button>
        </span>
      ))}
      <Dropdown trigger={() => <button className="pill tasks-label-add" aria-label="Add label"><Add size={12} />{labelIds.length === 0 ? 'Add label' : null}</button>}>
        {() => (
          <>
            <div className="popover-heading">Labels</div>
            {labels.length === 0 ? <div className="popover-empty">No labels yet.</div> : null}
            {labels.map((label) => {
              const active = labelIds.includes(label.id)
              return (
                <button key={label.id} aria-label={label.name} className="popover-option" data-selected={active || undefined} aria-pressed={active} onClick={() => onChange(active ? labelIds.filter((id) => id !== label.id) : [...labelIds, label.id])}>
                  <LabelPill label={label} />
                  {active ? <Xmark size={14} className="popover-option-remove" aria-hidden="true" /> : null}
                </button>
              )
            })}
            <form className="tasks-label-create" onSubmit={(event) => void submit(event)}>
              <input className="input" aria-label="New label name" placeholder="New label" value={name} onChange={(event) => setName(event.target.value)} />
              <div className="tasks-label-colors">
                {LABEL_COLORS.map((value) => (
                  <button key={value} type="button" className="tasks-label-color" style={{ background: value }} aria-label={`Color ${value}`} aria-pressed={color === value} onClick={() => setColor(value)} />
                ))}
              </div>
              <button type="submit" className="button button-primary" disabled={!name.trim() || createLabel.isPending}>
                {createLabel.isPending ? 'Creating…' : 'Create label'}
              </button>
              {createLabel.isError ? <p className="text-danger text-xs" role="alert">Label could not be created.</p> : null}
            </form>
          </>
        )}
      </Dropdown>
    </div>
  )
}
