import { useState, type ReactElement } from 'react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import type { Cycle, Milestone } from '@/features/tasks/api/models'
import { cycleDates, cycleName, cyclePickerGroups, cycleTiming, ESTIMATE_OPTIONS, MAX_ESTIMATE, parseEstimate, targetLabel } from '@/features/tasks/planningLib'

/** A project's milestones in their order, plus "No milestone". */
export function MilestonePicker({ milestones, value, onChange, trigger }: {
  milestones: Milestone[]
  value: string | null
  onChange: (milestoneId: string | null) => void
  trigger: ReactElement
}) {
  return (
    <DropdownMenu modal={false}>
      <DropdownMenuTrigger render={trigger} />
      <DropdownMenuContent align="start" className="w-auto min-w-52">
        <DropdownMenuItem className="data-selected:bg-accent data-selected:font-medium" data-selected={value === null || undefined} onClick={() => onChange(null)}>
          No milestone
        </DropdownMenuItem>
        {milestones.length > 0 ? <DropdownMenuSeparator /> : null}
        {milestones.map((milestone) => (
          <DropdownMenuItem
            key={milestone.id}
            className="data-selected:bg-accent data-selected:font-medium"
            data-selected={milestone.id === value || undefined}
            onClick={() => onChange(milestone.id)}
          >
            <span className="flex-1 truncate">{milestone.name}</span>
            {milestone.target_at ? <span className="text-xs text-muted-foreground tabular-nums">{targetLabel(milestone.target_at)}</span> : null}
          </DropdownMenuItem>
        ))}
        {milestones.length === 0 ? <DropdownMenuItem disabled>No milestones in this project</DropdownMenuItem> : null}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

function CycleItem({ cycle, value, onChange }: { cycle: Cycle; value: string | null; onChange: (cycleId: string) => void }) {
  return (
    <DropdownMenuItem className="data-selected:bg-accent data-selected:font-medium" data-selected={cycle.id === value || undefined} onClick={() => onChange(cycle.id)}>
      <span className="flex-1 truncate">{cycleName(cycle)}</span>
      {cycleTiming(cycle) === 'current' ? <Badge variant="secondary">Current</Badge> : null}
      <span className="text-xs text-muted-foreground tabular-nums">{cycleDates(cycle)}</span>
    </DropdownMenuItem>
  )
}

/** The current and upcoming cycles first; past cycles wait in a submenu. */
export function CyclePicker({ cycles, value, onChange, trigger }: {
  cycles: Cycle[]
  value: string | null
  onChange: (cycleId: string | null) => void
  trigger: ReactElement
}) {
  const { open, past } = cyclePickerGroups(cycles)
  return (
    <DropdownMenu modal={false}>
      <DropdownMenuTrigger render={trigger} />
      <DropdownMenuContent align="start" className="w-auto min-w-64">
        <DropdownMenuItem className="data-selected:bg-accent data-selected:font-medium" data-selected={value === null || undefined} onClick={() => onChange(null)}>
          No cycle
        </DropdownMenuItem>
        {cycles.length > 0 ? <DropdownMenuSeparator /> : null}
        {open.map((cycle) => <CycleItem key={cycle.id} cycle={cycle} value={value} onChange={onChange} />)}
        {past.length > 0 ? (
          <DropdownMenuSub>
            <DropdownMenuSubTrigger>Past cycles</DropdownMenuSubTrigger>
            <DropdownMenuSubContent className="w-auto min-w-64">
              {past.map((cycle) => <CycleItem key={cycle.id} cycle={cycle} value={value} onChange={onChange} />)}
            </DropdownMenuSubContent>
          </DropdownMenuSub>
        ) : null}
        {cycles.length === 0 ? <DropdownMenuItem disabled>No cycles in this project</DropdownMenuItem> : null}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

/** Quick point values, a field for any other whole number, and Clear. Every choice saves and closes. */
export function EstimatePicker({ value, onChange, trigger }: {
  value: number | null
  onChange: (estimate: number | null) => void
  trigger: ReactElement
}) {
  const [open, setOpen] = useState(false)
  const [custom, setCustom] = useState('')
  const parsed = parseEstimate(custom)
  const choose = (estimate: number | null) => {
    if (estimate !== value) onChange(estimate)
    setOpen(false)
    setCustom('')
  }
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger render={trigger} />
      <PopoverContent align="start" className="w-60 gap-2 p-2">
        <div className="flex flex-wrap gap-1" role="group" aria-label="Estimate">
          {ESTIMATE_OPTIONS.map((points) => (
            <Button key={points} type="button" size="sm" variant={points === value ? 'default' : 'outline'} className="min-w-8 tabular-nums" aria-pressed={points === value} onClick={() => choose(points)}>
              {points}
            </Button>
          ))}
        </div>
        <form className="flex gap-1" onSubmit={(event) => {
          event.preventDefault()
          if (parsed !== null) choose(parsed)
        }}>
          <Input className="h-8 min-w-0 flex-1" inputMode="numeric" aria-label="Custom estimate" placeholder={`Points (0–${MAX_ESTIMATE})`} value={custom} onChange={(event) => setCustom(event.target.value)} aria-invalid={custom.trim() !== '' && parsed === null} />
          <Button type="submit" size="sm" disabled={parsed === null}>Set</Button>
        </form>
        {value !== null ? <Button type="button" size="sm" variant="ghost" className="justify-start" onClick={() => choose(null)}>Clear estimate</Button> : null}
      </PopoverContent>
    </Popover>
  )
}
