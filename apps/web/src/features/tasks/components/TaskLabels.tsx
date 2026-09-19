import { useState } from 'react'
import { Plus, X } from 'lucide-react'
import type { LabelRecord } from '../../../api/generated/types.gen'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Dropdown } from '../../../components/ui/Dropdown'
import { useCreateLabel } from '../api/labels'

const LABEL_COLORS = ['#8b5cf6', '#0ea5e9', '#22c55e', '#eab308', '#f97316', '#ef4444', '#ec4899', '#64748b']

const PILL = 'inline-flex h-[22px] items-center gap-1.5 rounded-full border border-border bg-muted px-2.5 text-xs font-medium leading-none whitespace-nowrap text-foreground'
const MENU = 'flex min-w-[180px] flex-col gap-px p-1'
const OPTION =
  'group flex w-full min-h-8 items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm text-foreground transition-colors hover:bg-accent data-[selected]:bg-accent data-[selected]:font-medium'

export function LabelPill({ label }: { label: LabelRecord }) {
  return <span className={PILL}><span className="size-1.5 shrink-0 rounded-full" style={{ background: label.color }} />{label.name}</span>
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
    <div className="flex flex-wrap gap-1">
      {selected.map((label) => (
        <span key={label.id} className={`${PILL} gap-1 pr-1`}>
          <span className="size-1.5 shrink-0 rounded-full" style={{ background: label.color }} />
          {label.name}
          <button type="button" className="inline-flex size-4 items-center justify-center rounded-full text-muted-foreground/70 transition-colors hover:bg-accent hover:text-foreground" aria-label={`Remove label ${label.name}`} onClick={() => onChange(labelIds.filter((id) => id !== label.id))}>
            <X className="size-3" />
          </button>
        </span>
      ))}
      <Dropdown trigger={() => <button className={`${PILL} cursor-pointer gap-1 border-dashed text-muted-foreground/70 transition-colors hover:text-foreground`} aria-label="Add label"><Plus className="size-3" />{labelIds.length === 0 ? 'Add label' : null}</button>}>
        {() => (
          <div className={MENU}>
            <div className="px-2 py-1 text-[10px] font-semibold tracking-wide text-muted-foreground/70 uppercase">Labels</div>
            {labels.length === 0 ? <div className="px-2.5 py-2 text-xs text-muted-foreground/70">No labels yet.</div> : null}
            {labels.map((label) => {
              const active = labelIds.includes(label.id)
              return (
                <button key={label.id} aria-label={label.name} className={OPTION} data-selected={active || undefined} aria-pressed={active} onClick={() => onChange(active ? labelIds.filter((id) => id !== label.id) : [...labelIds, label.id])}>
                  <LabelPill label={label} />
                  {active ? <X className="ml-auto size-3.5 shrink-0 text-muted-foreground/70 group-hover:text-foreground" aria-hidden="true" /> : null}
                </button>
              )
            })}
            <form className="flex flex-col gap-2 border-t border-border p-2" onSubmit={(event) => void submit(event)}>
              <Input aria-label="New label name" placeholder="New label" value={name} onChange={(event) => setName(event.target.value)} />
              <div className="flex flex-wrap gap-1.5">
                {LABEL_COLORS.map((value) => (
                  <button key={value} type="button" className="size-[18px] rounded-full border-2 border-transparent p-0 aria-pressed:border-foreground" style={{ background: value }} aria-label={`Color ${value}`} aria-pressed={color === value} onClick={() => setColor(value)} />
                ))}
              </div>
              <Button type="submit" disabled={!name.trim() || createLabel.isPending}>
                {createLabel.isPending ? 'Creating…' : 'Create label'}
              </Button>
              {createLabel.isError ? <p className="text-xs text-destructive" role="alert">Label could not be created.</p> : null}
            </form>
          </div>
        )}
      </Dropdown>
    </div>
  )
}
