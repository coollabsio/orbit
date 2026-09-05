import { Add, Xmark } from 'reicon-react'
import type { LabelRecord } from '../../../api/generated/types.gen'
import { Dropdown } from '../../../components/ui/Dropdown'

export function LabelPill({ label }: { label: LabelRecord }) {
  return <span className="pill"><span className="pill-dot" style={{ background: label.color }} />{label.name}</span>
}

export function TaskLabels({ labelIds, labels, onChange }: { labelIds: string[]; labels: LabelRecord[]; onChange: (ids: string[]) => void }) {
  const selected = labels.filter((label) => labelIds.includes(label.id))
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
        {() => <><div className="popover-heading">Labels</div>{labels.map((label) => {
          const active = labelIds.includes(label.id)
          return <button key={label.id} aria-label={label.name} className="popover-option" data-selected={active || undefined} aria-pressed={active} onClick={() => onChange(active ? labelIds.filter((id) => id !== label.id) : [...labelIds, label.id])}>
            <LabelPill label={label} />
            {active ? <Xmark size={14} className="popover-option-remove" aria-hidden="true" /> : null}
          </button>
        })}</>}
      </Dropdown>
    </div>
  )
}
