import { useRef, useState, type CSSProperties, type ReactElement } from 'react'
import { useNavigate } from 'react-router'
import { Add as Plus, Setting2 as Settings } from 'reicon-react'
import type { LabelRecord } from '@/api/generated/types.gen'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Command, CommandGroup, CommandInput, CommandItem, CommandList } from '@/components/ui/command'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { useCreateLabel } from '@/features/tasks/api/labels'
import { labelKeyOf, projectLabels } from '@/features/tasks/taskMeta'
import { CheckMark } from '@/components/common/CheckMark'
import { ColorDot } from '@/components/common/ColorDot'
import { Tip } from '@/components/common/Tip'

/** The label palette: soft tones (the status colours' family) that read on light and dark, each with a name to say. */
const LABEL_PALETTE = [
  { name: 'Indigo', color: '#5e6ad2' },
  { name: 'Teal', color: '#26b5ce' },
  { name: 'Green', color: '#4cb782' },
  { name: 'Yellow', color: '#f2c94c' },
  { name: 'Orange', color: '#f2994a' },
  { name: 'Red', color: '#eb5757' },
  { name: 'Pink', color: '#e17bbd' },
  { name: 'Violet', color: '#a78bfa' },
  { name: 'Grey', color: '#8b8f98' },
]

export const LABEL_COLORS = LABEL_PALETTE.map((swatch) => swatch.color)

/** A label as an outline badge with its colour dot; `children` go after the name (e.g. a remove button). */
export function LabelPill({ label, className, children }: { label: Pick<LabelRecord, 'name' | 'color'>; className?: string; children?: React.ReactNode }) {
  return (
    <Badge variant="outline" data-slot="label-pill" className={className}>
      <ColorDot color={label.color} />
      {label.name}
      {children}
    </Badge>
  )
}

/** Arrow keys move through the swatches, as in a native radio group. */
const SWATCH_STEP: Record<string, number> = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }

/**
 * The label palette as a radio group of small swatches. The chosen one wears a ring in its own colour, offset by a
 * transparent gap, so it reads on any surface. One swatch is a Tab stop and the arrow keys move the choice (roving
 * tabindex). A colour outside the palette (e.g. a GitHub label's) checks none; the first swatch then takes the focus.
 */
export function LabelColorPicker({ value, onChange }: { value: string; onChange: (color: string) => void }) {
  const swatches = useRef<Array<HTMLButtonElement | null>>([])
  const checked = LABEL_PALETTE.findIndex((swatch) => swatch.color === value)
  const choose = (index: number) => {
    const next = (index + LABEL_PALETTE.length) % LABEL_PALETTE.length
    onChange(LABEL_PALETTE[next]!.color)
    swatches.current[next]?.focus()
  }
  return (
    <div
      role="radiogroup"
      aria-label="Label color"
      className="flex flex-wrap items-center gap-1.5"
      onKeyDown={(event) => {
        // the arrows, Enter and Space belong to the swatches, not to a command list around them
        if (event.key in SWATCH_STEP || event.key === 'Enter' || event.key === ' ') event.stopPropagation()
        const step = SWATCH_STEP[event.key]
        if (step === undefined) return
        event.preventDefault()
        choose(checked === -1 ? (step > 0 ? 0 : -1) : checked + step)
      }}
    >
      {LABEL_PALETTE.map((swatch, index) => (
        <Tip key={swatch.color} label={swatch.name}>
          <button
            ref={(element) => { swatches.current[index] = element }}
            type="button"
            role="radio"
            aria-checked={index === checked}
            aria-label={swatch.name}
            tabIndex={index === Math.max(checked, 0) ? 0 : -1}
            style={{ '--swatch': swatch.color } as CSSProperties}
            className="size-4 shrink-0 cursor-pointer rounded-full bg-(--swatch) shadow-[inset_0_0_0_1px_rgb(0_0_0/0.18)] outline-offset-2 outline-(--swatch) transition-transform duration-150 ease-[cubic-bezier(0.23,1,0.32,1)] focus-visible:outline-2 active:scale-90 aria-checked:outline-[1.5px] aria-checked:outline-solid motion-safe:hover:scale-115 motion-reduce:transition-none"
            onClick={() => onChange(swatch.color)}
          />
        </Tip>
      ))}
    </div>
  )
}

/** The palette colour the project uses least, so new labels stay apart without a choice. */
function nextLabelColor(labels: LabelRecord[]): string {
  const uses = (color: string) => labels.filter((label) => label.color === color).length
  return LABEL_COLORS.reduce((best, color) => (uses(color) < uses(best) ? color : best))
}

/**
 * Picks a task's labels from its project, like the label filter: a search, then one row per label with a checkbox.
 * The task's labels come first (in the order they had when the picker opened, so rows do not jump while toggling). A
 * name the search does not find becomes a "Create" row below the matches, so Enter picks a match first and creates
 * only when nothing matches; the palette under it is preset to the colour the project uses least. The list is
 * filtered here (by name, never by id) and keeps this order. `editLink` adds "Edit labels", which opens the project
 * settings to rename, recolour or delete (leave it off where leaving the page would lose a draft). Without a project
 * (still loading) nothing can be created.
 */
export function LabelPicker({ workspaceId, projectId, labelIds, labels: allLabels, onChange, trigger, editLink = false }: { workspaceId: string; projectId: string; labelIds: string[]; labels: LabelRecord[]; onChange: (ids: string[]) => void; trigger: ReactElement; editLink?: boolean }) {
  const navigate = useNavigate()
  const createLabel = useCreateLabel(workspaceId)
  const [query, setQuery] = useState('')
  const [pinned, setPinned] = useState<string[]>([])
  const [pickedColor, setPickedColor] = useState<string>()
  const labels = projectLabels(allLabels, projectId).sort((a, b) => Number(pinned.includes(b.id)) - Number(pinned.includes(a.id)) || a.name.localeCompare(b.name))
  const name = query.trim()
  const shown = name ? labels.filter((label) => label.name.toLowerCase().includes(name.toLowerCase())) : labels
  const creatable = name !== '' && projectId !== '' && !labels.some((label) => labelKeyOf(label) === labelKeyOf({ name }))
  const color = pickedColor ?? nextLabelColor(labels)
  // toggles wait for a create: its result is added to the ids of the render that started it
  const toggle = (id: string) => {
    if (!createLabel.isPending) onChange(labelIds.includes(id) ? labelIds.filter((item) => item !== id) : [...labelIds, id])
  }
  const create = async () => {
    if (!creatable || createLabel.isPending) return
    try {
      const label = await createLabel.mutateAsync({ projectId, name, color })
      onChange(labelIds.includes(label.id) ? labelIds : [...labelIds, label.id])
      setQuery('')
      setPickedColor(undefined)
    } catch {
      // the picker shows the mutation error
    }
  }
  return (
    <Popover onOpenChange={(open) => {
      if (open) setPinned(labelIds)
      else { setQuery(''); setPickedColor(undefined); createLabel.reset() }
    }}>
      <PopoverTrigger render={trigger} />
      <PopoverContent align="start" className="w-auto gap-0 p-0">
        <Command label="Change labels" shouldFilter={false} className="w-60">
          <CommandInput autoFocus aria-label="Change labels" placeholder={labels.length > 0 ? 'Change labels…' : 'New label name…'} value={query} onValueChange={setQuery} />
          <CommandList>
            {shown.length > 0 ? (
              <CommandGroup>
                {shown.map((label) => {
                  const checked = labelIds.includes(label.id)
                  return (
                    <CommandItem key={label.id} value={label.id} aria-checked={checked} onSelect={() => toggle(label.id)}>
                      <CheckMark checked={checked} />
                      <ColorDot color={label.color} className="size-2" />
                      <span className="min-w-0 flex-1 truncate">{label.name}</span>
                    </CommandItem>
                  )
                })}
              </CommandGroup>
            ) : null}
            {creatable ? (
              <CommandGroup>
                <CommandItem value="create-label" disabled={createLabel.isPending} onSelect={() => void create()}>
                  <Plus className="size-3.5 text-muted-foreground" />
                  <span className="shrink-0 text-muted-foreground">{createLabel.isPending ? 'Creating' : 'Create'}</span>
                  {/* the label as it will look; it follows the colour picked below */}
                  <LabelPill label={{ name, color }} className="min-w-0 truncate" />
                </CommandItem>
              </CommandGroup>
            ) : null}
          </CommandList>
          {labels.length === 0 && !name ? (
            <p className="px-3 pt-2.5 pb-2 text-xs text-muted-foreground">This project has no labels yet. Type a name to create one.</p>
          ) : null}
          {name && shown.length === 0 && !creatable ? <p className="px-3 py-2.5 text-xs text-muted-foreground">No labels match.</p> : null}
          {creatable ? (
            // preventDefault keeps the focus in the search, so Enter still creates after a click on a colour
            <div className="px-3 pt-1 pb-2.5" onMouseDown={(event) => event.preventDefault()}>
              <LabelColorPicker value={color} onChange={setPickedColor} />
            </div>
          ) : null}
          {createLabel.isError ? <p className="px-3 pb-2 text-xs text-destructive" role="alert">The label could not be created. Try again.</p> : null}
          {editLink ? (
            <div className="-mx-1 -mb-1 mt-1 border-t p-1">
              <Button type="button" variant="ghost" size="sm" className="w-full justify-start font-normal text-muted-foreground" onClick={() => navigate(`/tasks/projects/${projectId}/settings`)}>
                <Settings className="size-3.5" />
                Edit labels
              </Button>
            </div>
          ) : null}
        </Command>
      </PopoverContent>
    </Popover>
  )
}
