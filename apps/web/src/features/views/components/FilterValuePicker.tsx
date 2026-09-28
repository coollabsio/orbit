import { useState } from 'react'
import { format, parseISO } from 'date-fns'
import type { DateRange } from 'react-day-picker'
import { Check } from 'reicon-react'
import { Button } from '@/components/ui/button'
import { Calendar } from '@/components/ui/calendar'
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from '@/components/ui/command'
import { Input } from '@/components/ui/input'
import { UserAvatar } from '@/components/common/UserAvatar'
import { PriorityIcon } from '@/features/tasks/components/PriorityIcon'
import { TaskStatusIcon } from '@/features/tasks/components/TaskStatusIcon'
import { useTaskCandidates } from '@/features/tasks/useTaskCandidates'
import { FIELD_META, MAX_FILTER_VALUES, NO_PARENT, isDateValue, listValue, taskValueLabel, unlistedOptions, valueOptions, type FilterOptions, type Glyph } from '../filterFields'
import type { Condition, DateValue } from '../viewState'

export interface FilterValuePickerProps {
  condition: Condition
  options: FilterOptions
  /** Called on every edit; the caller applies it once `isCompleteCondition` holds. */
  onChange: (next: Condition) => void
  /** Single-choice pickers (dates, text) call this right after committing. */
  onDone: () => void
}

export function FilterValuePicker(props: FilterValuePickerProps) {
  const kind = FIELD_META[props.condition.field].kind
  if (kind === 'text') return <TextValue {...props} />
  if (kind === 'date') return <DateValuePicker {...props} />
  if (kind === 'task') return <TaskValue {...props} />
  return <ListValue {...props} />
}

/** The value's own mark: status ring, priority bars, label/project dot, avatar, or a plain icon. */
export function ValueGlyph({ glyph, size = 14 }: { glyph: Glyph; size?: number }) {
  switch (glyph.kind) {
    case 'status':
      return <TaskStatusIcon status={glyph.status} size={size} />
    case 'priority':
      return <PriorityIcon priority={glyph.priority} size={size} />
    case 'dot':
      return <span className="size-2 shrink-0 rounded-full" style={{ backgroundColor: glyph.color }} />
    case 'member':
      return <UserAvatar user={glyph.member} name={glyph.name} size={size + 2} />
    case 'icon': {
      const Icon = glyph.icon
      return <Icon aria-hidden className="size-3.5 text-muted-foreground" />
    }
  }
}

const VALUES_FULL_TITLE = `Up to ${MAX_FILTER_VALUES} values per filter`

const CHECK = 'flex size-3.5 shrink-0 items-center justify-center rounded-[4px] border border-input text-primary-foreground transition-colors duration-150 data-checked:border-primary data-checked:bg-primary'

function ListValue({ condition, options, onChange }: FilterValuePickerProps) {
  const selected = listValue(condition)
  const meta = FIELD_META[condition.field]
  const full = selected.length >= MAX_FILTER_VALUES
  const toggle = (value: string) => {
    if (selected.includes(value)) onChange({ ...condition, value: selected.filter((item) => item !== value) })
    else if (!full) onChange({ ...condition, value: [...selected, value] })
  }
  // chosen values the list lacks (a member picked by ID, a deleted label) come first, so they can be unchecked
  const rows = [...unlistedOptions(condition, options), ...valueOptions(condition.field, options)]
  return (
    <Command label={`Search ${meta.plural}`} className="w-64 rounded-lg! bg-transparent">
      <CommandInput autoFocus aria-label={`Search ${meta.plural}`} placeholder={`Search ${meta.plural}…`} />
      <CommandList>
        <CommandEmpty className="py-4 text-xs text-muted-foreground">No {meta.plural} match</CommandEmpty>
        <CommandGroup>
          {rows.map((option) => {
            const checked = selected.includes(option.value)
            const capped = full && !checked
            return (
              <CommandItem
                key={option.value}
                value={option.value}
                keywords={[option.label]}
                aria-checked={checked}
                disabled={capped}
                title={capped ? VALUES_FULL_TITLE : undefined}
                // keep hover on a capped row so its title explains why
                className="data-[disabled=true]:pointer-events-auto"
                onSelect={() => toggle(option.value)}
              >
                <span aria-hidden="true" data-checked={checked || undefined} className={CHECK}>
                  {checked ? <Check className="size-2.5" /> : null}
                </span>
                <span aria-hidden="true" className="flex size-4 shrink-0 items-center justify-center">
                  <ValueGlyph glyph={option.glyph} />
                </span>
                <span className="min-w-0 flex-1 truncate">{option.label}</span>
              </CommandItem>
            )
          })}
        </CommandGroup>
      </CommandList>
    </Command>
  )
}

/** Parent filter: "No parent", then chosen tasks the search does not list (another page, a deleted task), then search results. */
function TaskValue({ condition, options, onChange }: FilterValuePickerProps) {
  const [query, setQuery] = useState('')
  const selected = listValue(condition)
  const full = selected.length >= MAX_FILTER_VALUES
  const { candidates, loading } = useTaskCandidates({ query, excludeIds: [], excludeDuplicates: false, statuses: options.statuses })
  const toggle = (value: string) => {
    if (selected.includes(value)) onChange({ ...condition, value: selected.filter((item) => item !== value) })
    else if (!full) onChange({ ...condition, value: [...selected, value] })
  }
  const listed = new Set(candidates.map((task) => task.id))
  const rows = [
    { value: NO_PARENT, label: 'No parent', identifier: null as string | null },
    ...selected.filter((value) => value !== NO_PARENT && !listed.has(value)).map((value) => ({ value, label: taskValueLabel(value, options), identifier: null })),
    ...candidates.map((task) => ({ value: task.id, label: task.title || 'Untitled', identifier: task.identifier })),
  ]
  return (
    <Command shouldFilter={false} label="Search tasks" className="w-80 rounded-lg! bg-transparent">
      <CommandInput autoFocus aria-label="Search tasks" placeholder="Search tasks…" value={query} onValueChange={setQuery} />
      <CommandList>
        <CommandGroup>
          {rows.map((row) => {
            const checked = selected.includes(row.value)
            const capped = full && !checked
            return (
              <CommandItem key={row.value} value={row.value} aria-checked={checked} disabled={capped} title={capped ? VALUES_FULL_TITLE : undefined}
                className="data-[disabled=true]:pointer-events-auto" onSelect={() => toggle(row.value)}>
                <span aria-hidden="true" data-checked={checked || undefined} className={CHECK}>{checked ? <Check className="size-2.5" /> : null}</span>
                {row.identifier ? <span className="w-[72px] shrink-0 text-xs text-muted-foreground/70 tabular-nums">{row.identifier}</span> : null}
                <span className="min-w-0 flex-1 truncate">{row.label}</span>
              </CommandItem>
            )
          })}
        </CommandGroup>
        {loading ? <div role="status" className="px-2 py-2 text-xs text-muted-foreground">Searching…</div> : null}
      </CommandList>
    </Command>
  )
}

const PRESET =
  'flex h-7 w-full items-center rounded-md px-2 text-left text-sm text-foreground outline-none hover-fine:hover:bg-muted focus-visible:bg-muted aria-pressed:font-medium'

const SINGLE_PRESETS: Array<{ label: string; value: DateValue }> = [
  { label: 'Today', value: { relative: 'today' } },
  { label: 'Yesterday', value: { relative: 'today', offset_days: -1 } },
  { label: 'Tomorrow', value: { relative: 'today', offset_days: 1 } },
  { label: 'Start of week', value: { relative: 'start_of_week' } },
  { label: 'End of week', value: { relative: 'end_of_week' } },
  { label: '7 days ago', value: { relative: 'today', offset_days: -7 } },
  { label: 'In 7 days', value: { relative: 'today', offset_days: 7 } },
]

const RANGE_PRESETS: Array<{ label: string; value: [DateValue, DateValue] }> = [
  { label: 'This week', value: [{ relative: 'start_of_week' }, { relative: 'end_of_week' }] },
  { label: 'Next week', value: [{ relative: 'start_of_week', offset_days: 7 }, { relative: 'end_of_week', offset_days: 7 }] },
  { label: 'Next 7 days', value: [{ relative: 'today' }, { relative: 'today', offset_days: 6 }] },
  { label: 'Last 7 days', value: [{ relative: 'today', offset_days: -6 }, { relative: 'today' }] },
]

const toAbsolute = (day: Date): DateValue => ({ absolute: format(day, 'yyyy-MM-dd') })
const sameValue = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b)

function DateValuePicker({ condition, onChange, onDone }: FilterValuePickerProps) {
  const range = condition.operator === 'between'
  const [draftRange, setDraftRange] = useState<DateRange | undefined>(undefined)
  const commit = (next: Condition) => {
    onChange(next)
    onDone()
  }
  const presets = range ? RANGE_PRESETS : SINGLE_PRESETS
  const selectedDay = !range && isDateValue(condition.value) && 'absolute' in condition.value ? parseISO(condition.value.absolute) : undefined
  return (
    <div className="flex w-[17.5rem] flex-col">
      <div role="group" aria-label="Date presets" className="flex flex-col gap-px p-1">
        {presets.map((preset) => (
          <button
            key={preset.label}
            type="button"
            className={PRESET}
            aria-pressed={sameValue(preset.value, condition.value)}
            onClick={() => commit({ field: condition.field, operator: condition.operator, value: preset.value })}
          >
            {preset.label}
          </button>
        ))}
        {condition.field === 'due_date' ? (
          <>
            <div className="my-1 h-px bg-border" />
            <button type="button" className={PRESET} onClick={() => commit({ field: 'due_date', operator: 'is_empty' })}>No due date</button>
            <button type="button" className={PRESET} onClick={() => commit({ field: 'due_date', operator: 'is_not_empty' })}>Has a due date</button>
          </>
        ) : null}
      </div>
      <div className="border-t border-border">
        {range ? (
          <Calendar
            mode="range"
            selected={draftRange}
            onSelect={(next) => {
              setDraftRange(next)
              if (next?.from && next.to && next.from.getTime() !== next.to.getTime()) {
                commit({ field: condition.field, operator: 'between', value: [toAbsolute(next.from), toAbsolute(next.to)] })
              }
            }}
          />
        ) : (
          <Calendar
            mode="single"
            selected={selectedDay}
            defaultMonth={selectedDay}
            onSelect={(day) => {
              if (day) commit({ field: condition.field, operator: condition.operator, value: toAbsolute(day) })
            }}
          />
        )}
      </div>
    </div>
  )
}

function TextValue({ condition, onChange, onDone }: FilterValuePickerProps) {
  const [text, setText] = useState(typeof condition.value === 'string' ? condition.value : '')
  const trimmed = text.trim()
  return (
    <form
      className="flex w-72 items-center gap-2 p-2"
      onSubmit={(event) => {
        event.preventDefault()
        if (!trimmed) return
        onChange({ field: condition.field, operator: 'contains', value: trimmed })
        onDone()
      }}
    >
      <Input
        autoFocus
        aria-label="Title or description contains"
        placeholder="Title or description contains…"
        maxLength={200}
        value={text}
        onChange={(event) => setText(event.target.value)}
        className="h-8 flex-1"
      />
      <Button type="submit" size="sm" disabled={!trimmed}>Apply</Button>
    </form>
  )
}
