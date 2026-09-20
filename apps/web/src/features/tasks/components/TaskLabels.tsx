import { useState } from 'react'
import { Add as Plus, Xmark as X } from 'reicon-react'
import type { LabelRecord } from '@/api/generated/types.gen'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { useCreateLabel } from '@/features/tasks/api/labels'

const LABEL_COLORS = ['#8b5cf6', '#0ea5e9', '#22c55e', '#eab308', '#f97316', '#ef4444', '#ec4899', '#64748b']

const PILL = 'inline-flex h-[22px] items-center gap-1.5 overflow-visible rounded-full border border-border bg-muted px-2.5 text-xs font-medium leading-none whitespace-nowrap text-foreground'
const MENU = 'flex max-h-(--available-height) w-auto min-w-[180px] flex-col gap-px overflow-y-auto p-1'
const OPTION =
  `group flex h-auto w-full min-h-8 items-center justify-start gap-2 rounded-md border-0 px-2 py-1.5 text-left text-sm font-normal whitespace-normal text-foreground transition-colors hover:bg-accent dark:hover:bg-accent [&_svg:not([class*='size-'])]:size-3.5 data-[selected]:bg-accent data-[selected]:font-medium`

export function LabelPill({ label }: { label: LabelRecord }) {
  return <Badge variant="outline" className={PILL}><span className="size-1.5 shrink-0 rounded-full" style={{ background: label.color }} />{label.name}</Badge>
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
        <Badge key={label.id} variant="outline" className={`${PILL} gap-1 pr-1`}>
          <span className="size-1.5 shrink-0 rounded-full" style={{ background: label.color }} />
          {label.name}
          <Button type="button" variant="ghost" size="icon-xs" className="size-4 rounded-full border-0 text-muted-foreground/70 transition-colors hover:bg-accent hover:text-foreground dark:hover:bg-accent" aria-label={`Remove label ${label.name}`} onClick={() => onChange(labelIds.filter((id) => id !== label.id))}>
            <X className="size-3" />
          </Button>
        </Badge>
      ))}
      <Popover>
        <PopoverTrigger render={<Button variant="ghost" className={`${PILL} cursor-pointer gap-1 border-dashed text-muted-foreground/70 transition-colors hover:bg-muted hover:text-foreground dark:hover:bg-muted`} aria-label="Add label"><Plus className="size-3" />{labelIds.length === 0 ? 'Add label' : null}</Button>} />
        <PopoverContent align="start" className={MENU}>
          <div className="px-2 py-1 text-[10px] font-semibold tracking-wide text-muted-foreground/70 uppercase">Labels</div>
          {labels.length === 0 ? <div className="px-2.5 py-2 text-xs text-muted-foreground/70">No labels yet.</div> : null}
          {labels.map((label) => {
            const active = labelIds.includes(label.id)
            return (
              <Button variant="ghost" key={label.id} aria-label={label.name} className={OPTION} data-selected={active || undefined} aria-pressed={active} onClick={() => onChange(active ? labelIds.filter((id) => id !== label.id) : [...labelIds, label.id])}>
                <LabelPill label={label} />
                {active ? <X className="ml-auto size-3.5 shrink-0 text-muted-foreground/70 group-hover:text-foreground" aria-hidden="true" /> : null}
              </Button>
            )
          })}
          <form className="flex flex-col gap-2 border-t border-border p-2" onSubmit={(event) => void submit(event)}>
            <Input aria-label="New label name" placeholder="New label" value={name} onChange={(event) => setName(event.target.value)} />
            <div className="flex flex-wrap gap-1.5">
              {LABEL_COLORS.map((value) => (
                <Button key={value} type="button" size="icon-xs" className="size-[18px] rounded-full border-2 border-transparent p-0 aria-pressed:border-foreground" style={{ background: value }} aria-label={`Color ${value}`} aria-pressed={color === value} onClick={() => setColor(value)} />
              ))}
            </div>
            <Button type="submit" disabled={!name.trim() || createLabel.isPending}>
              {createLabel.isPending ? 'Creating…' : 'Create label'}
            </Button>
            {createLabel.isError ? <p className="text-xs text-destructive" role="alert">Label could not be created.</p> : null}
          </form>
        </PopoverContent>
      </Popover>
    </div>
  )
}
