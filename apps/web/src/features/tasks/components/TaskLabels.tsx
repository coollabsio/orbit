import { useState } from 'react'
import { Add as Plus, Xmark as X } from 'reicon-react'
import type { LabelRecord } from '@/api/generated/types.gen'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { useCreateLabel } from '@/features/tasks/api/labels'
import { addLabel, labelDisplayName, labelSections, toggleLabel } from '@/features/tasks/labelGroups'
import { ColorDot } from '@/components/common/ColorDot'
import { ColorSwatch } from '@/components/common/ColorSwatch'

export const LABEL_COLORS = ['#8b5cf6', '#0ea5e9', '#22c55e', '#eab308', '#f97316', '#ef4444', '#ec4899', '#64748b']

/** A label as an outline badge with its colour dot; `children` go after the name (e.g. a remove button). */
export function LabelPill({ label, className, children }: { label: Pick<LabelRecord, 'name' | 'color'> & { group_name?: string | null }; className?: string; children?: React.ReactNode }) {
  return (
    <Badge variant="outline" data-slot="label-pill" className={className}>
      <ColorDot color={label.color} />
      {/* a label of a group reads `Group / Label` */}
      {label.group_name ? <span className="text-muted-foreground">{label.group_name} /</span> : null}
      {label.name}
      {children}
    </Badge>
  )
}

/** The label palette as a row of small swatches. A colour outside it (e.g. a GitHub label's) presses none. */
export function LabelColorPicker({ value, onChange }: { value: string; onChange: (color: string) => void }) {
  return (
    <div role="group" aria-label="Label color" className="flex flex-wrap gap-1.5">
      {LABEL_COLORS.map((color) => (
        <ColorSwatch key={color} size="xs" color={color} aria-label={`Color ${color}`} aria-pressed={value === color} onClick={() => onChange(color)} />
      ))}
    </div>
  )
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
    onChange(labelIds.includes(label.id) ? labelIds : addLabel(labelIds, label.id, [...labels, label]))
    setName('')
  }
  return (
    <div className="flex flex-wrap gap-1">
      {selected.map((label) => (
        <LabelPill key={label.id} label={label} className="pr-0.5">
          <Button type="button" variant="ghost" size="icon-xs" className="size-4 rounded-full text-muted-foreground" aria-label={`Remove label ${labelDisplayName(label)}`} onClick={() => onChange(labelIds.filter((id) => id !== label.id))}>
            <X />
          </Button>
        </LabelPill>
      ))}
      <Popover>
        <PopoverTrigger render={<Button variant="outline" size="xs" className="h-5 rounded-full border-dashed text-muted-foreground" aria-label="Add label"><Plus />{labelIds.length === 0 ? 'Add label' : null}</Button>} />
        <PopoverContent align="start" className="max-h-(--available-height) w-auto min-w-45 gap-px overflow-y-auto overscroll-contain p-1">
          <div className="px-2 py-1 text-[10px] font-semibold tracking-wide text-muted-foreground/70 uppercase">Labels</div>
          {labels.length === 0 ? <div className="px-2.5 py-2 text-xs text-muted-foreground/70">No labels yet.</div> : null}
          {/* one label of a group for each task: a pick in a group replaces the group's other label */}
          {labelSections(labels).map((section) => (
            <div key={section.groupId ?? 'none'} role="group" aria-label={section.name ?? 'Labels with no group'} className="flex flex-col gap-px">
              {section.name ? <div className="px-2 pt-1.5 pb-0.5 text-[11px] font-medium text-muted-foreground">{section.name}</div> : null}
              {section.labels.map((label) => {
                const active = labelIds.includes(label.id)
                return (
                  <Button variant="ghost" key={label.id} aria-label={labelDisplayName(label)} className="group w-full justify-start font-normal data-selected:bg-muted" data-selected={active || undefined} aria-pressed={active} onClick={() => onChange(toggleLabel(labelIds, label.id, labels))}>
                    <LabelPill label={{ name: label.name, color: label.color }} />
                    {active ? <X className="ml-auto size-3.5 shrink-0 text-muted-foreground/70 group-hover:text-foreground" aria-hidden="true" /> : null}
                  </Button>
                )
              })}
            </div>
          ))}
          <form className="flex flex-col gap-2 border-t p-2" onSubmit={(event) => void submit(event)}>
            <Input aria-label="New label name" placeholder="New label" value={name} onChange={(event) => setName(event.target.value)} />
            <LabelColorPicker value={color} onChange={setColor} />
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
