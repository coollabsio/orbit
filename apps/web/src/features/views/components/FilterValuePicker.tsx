import { useState, type ComponentProps } from 'react'
import { cn } from 'cn'
import { format, parseISO } from 'date-fns'
import type { DateRange } from 'react-day-picker'
import { Button } from '@/components/ui/button'
import { Calendar } from '@/components/ui/calendar'
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from '@/components/ui/command'
import { Input } from '@/components/ui/input'
import { Separator } from '@/components/ui/separator'
import { UserAvatar } from '@/components/common/UserAvatar'
import { CheckMark } from '@/components/common/CheckMark'
import { ColorDot } from '@/components/common/ColorDot'
import { PriorityIcon } from '@/features/tasks/components/PriorityIcon'
import { TaskStatusIcon } from '@/features/tasks/components/TaskStatusIcon'
import { useTaskCandidates } from '@/features/tasks/useTaskCandidates'
import { MAX_FILTER_VALUES, NO_PARENT, chosenValues, fieldMeta, isDateValue, isFilterNumber, listValue, taskValueLabel, targetOf, unlistedOptions, valueOptions, type FilterOptions, type Glyph } from '../filterFields'
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
  switch (fieldMeta(props.condition, props.options).kind) {
    case 'text':
      return <TextValue {...props} />
    case 'date':
      return <DateValuePicker {...props} />
    case 'task':
      return <TaskValue {...props} />
    case 'number':
      return <NumberValue {...props} />
    case 'boolean':
      return <BooleanValue {...props} />
    case 'missing':
      return <p className="w-60 p-3 text-xs text-muted-foreground">This field was deleted. Remove the filter.</p>
    case 'list':
      return <ListValue {...props} />
  }
}

/** The value's own mark: status ring, priority bars, label/project dot, avatar, or a plain icon. */
export function ValueGlyph({ glyph, size = 14 }: { glyph: Glyph; size?: number }) {
  switch (glyph.kind) {
    case 'status':
      return <TaskStatusIcon status={glyph.status} size={size} />
    case 'priority':
      return <PriorityIcon priority={glyph.priority} size={size} />
    case 'dot':
      return <ColorDot color={glyph.color} className="size-2" />
    case 'member':
      return <UserAvatar user={glyph.member} name={glyph.name} size={size + 2} />
    case 'icon': {
      const Icon = glyph.icon
      return <Icon aria-hidden className="size-3.5 text-muted-foreground" />
    }
  }
}

const VALUES_FULL_TITLE = `Up to ${MAX_FILTER_VALUES} values per filter`

function ListValue({ condition, options, onChange }: FilterValuePickerProps) {
  const selected = chosenValues(condition, options)
  const meta = fieldMeta(condition, options)
  const full = selected.length >= MAX_FILTER_VALUES
  const toggle = (value: string) => {
    if (selected.includes(value)) onChange({ ...condition, value: selected.filter((item) => item !== value) })
    else if (!full) onChange({ ...condition, value: [...selected, value] })
  }
  // chosen values the list lacks (a member picked by ID, a deleted label) come first, so they can be unchecked
  const rows = [...unlistedOptions(condition, options), ...valueOptions(condition, options)]
  return (
    <Command label={`Search ${meta.plural}`} className="w-64">
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
                <CheckMark checked={checked} />
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
    <Command shouldFilter={false} label="Search tasks" className="w-80">
      <CommandInput autoFocus aria-label="Search tasks" placeholder="Search tasks…" value={query} onValueChange={setQuery} />
      <CommandList>
        <CommandGroup>
          {rows.map((row) => {
            const checked = selected.includes(row.value)
            const capped = full && !checked
            return (
              <CommandItem key={row.value} value={row.value} aria-checked={checked} disabled={capped} title={capped ? VALUES_FULL_TITLE : undefined}
                className="data-[disabled=true]:pointer-events-auto" onSelect={() => toggle(row.value)}>
                <CheckMark checked={checked} />
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

/** A date preset row: full-width ghost button; `aria-pressed` marks the current value. */
function PresetButton({ className, ...props }: ComponentProps<typeof Button>) {
  return <Button type="button" variant="ghost" size="sm" className={cn('w-full justify-start text-sm font-normal aria-pressed:font-medium', className)} {...props} />
}

function DateValuePicker({ condition, onChange, onDone }: FilterValuePickerProps) {
  const target = targetOf(condition)
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
          <PresetButton
            key={preset.label}
            aria-pressed={sameValue(preset.value, condition.value)}
            onClick={() => commit({ ...target, operator: condition.operator, value: preset.value })}
          >
            {preset.label}
          </PresetButton>
        ))}
        {condition.field === 'due_date' || condition.field === 'custom_field' ? (
          <>
            <Separator className="my-1" />
            <PresetButton onClick={() => commit({ ...target, operator: 'is_empty' })}>{condition.field === 'due_date' ? 'No due date' : 'No date'}</PresetButton>
            <PresetButton onClick={() => commit({ ...target, operator: 'is_not_empty' })}>{condition.field === 'due_date' ? 'Has a due date' : 'Has a date'}</PresetButton>
          </>
        ) : null}
      </div>
      <div className="border-t">
        {range ? (
          <Calendar
            mode="range"
            selected={draftRange}
            onSelect={(next) => {
              setDraftRange(next)
              if (next?.from && next.to && next.from.getTime() !== next.to.getTime()) {
                commit({ ...target, operator: 'between', value: [toAbsolute(next.from), toAbsolute(next.to)] })
              }
            }}
          />
        ) : (
          <Calendar
            mode="single"
            selected={selectedDay}
            defaultMonth={selectedDay}
            onSelect={(day) => {
              if (day) commit({ ...target, operator: condition.operator, value: toAbsolute(day) })
            }}
          />
        )}
      </div>
    </div>
  )
}

function TextValue({ condition, options, onChange, onDone }: FilterValuePickerProps) {
  const [text, setText] = useState(typeof condition.value === 'string' ? condition.value : '')
  const trimmed = text.trim()
  const prompt = condition.field === 'custom_field' ? `${fieldMeta(condition, options).label} contains` : 'Title or description contains'
  return (
    <form
      className="flex w-72 items-center gap-2 p-2"
      onSubmit={(event) => {
        event.preventDefault()
        if (!trimmed) return
        onChange({ ...targetOf(condition), operator: 'contains', value: trimmed })
        onDone()
      }}
    >
      <Input
        autoFocus
        aria-label={prompt}
        placeholder={`${prompt}…`}
        maxLength={200}
        value={text}
        onChange={(event) => setText(event.target.value)}
        className="h-8 flex-1"
      />
      <Button type="submit" size="sm" disabled={!trimmed}>Apply</Button>
    </form>
  )
}

const numberText = (value: unknown) => (isFilterNumber(value) ? String(value) : '')

/** The number of a number input; null while it is blank or not a number. */
function parseNumber(text: string): number | null {
  const trimmed = text.trim()
  if (!trimmed) return null
  const number = Number(trimmed.replace(/,/g, ''))
  return Number.isFinite(number) ? number : null
}

/** A number field: one number, or the two inclusive bounds of `between`. Enter applies. */
function NumberValue({ condition, options, onChange, onDone }: FilterValuePickerProps) {
  const range = condition.operator === 'between'
  const bounds = Array.isArray(condition.value) ? condition.value : []
  const [first, setFirst] = useState(numberText(range ? bounds[0] : condition.value))
  const [second, setSecond] = useState(numberText(bounds[1]))
  const label = fieldMeta(condition, options).label
  const low = parseNumber(first)
  const high = parseNumber(second)
  const valid = low !== null && (!range || high !== null)
  return (
    <form
      className="flex w-72 items-center gap-2 p-2"
      onSubmit={(event) => {
        event.preventDefault()
        if (low === null) return
        if (!range) onChange({ ...targetOf(condition), operator: condition.operator, value: low })
        else if (high !== null) onChange({ ...targetOf(condition), operator: 'between', value: [Math.min(low, high), Math.max(low, high)] })
        else return
        onDone()
      }}
    >
      <Input autoFocus inputMode="decimal" aria-label={range ? `${label} from` : label} placeholder={range ? 'From' : 'Number'} value={first} onChange={(event) => setFirst(event.target.value)} className="h-8 min-w-0 flex-1" />
      {range ? (
        <>
          <span aria-hidden className="text-xs text-muted-foreground">–</span>
          <Input inputMode="decimal" aria-label={`${label} to`} placeholder="To" value={second} onChange={(event) => setSecond(event.target.value)} className="h-8 min-w-0 flex-1" />
        </>
      ) : null}
      <Button type="submit" size="sm" disabled={!valid}>Apply</Button>
    </form>
  )
}

/** A checkbox field: checked or not checked. */
function BooleanValue({ condition, onChange, onDone }: FilterValuePickerProps) {
  const choices = [{ label: 'Checked', value: true }, { label: 'Not checked', value: false }]
  return (
    <div role="group" aria-label="Checkbox value" className="flex w-44 flex-col gap-px p-1">
      {choices.map((choice) => (
        <PresetButton
          key={choice.label}
          aria-pressed={condition.value === choice.value}
          onClick={() => {
            onChange({ ...targetOf(condition), operator: 'is', value: choice.value })
            onDone()
          }}
        >
          {choice.label}
        </PresetButton>
      ))}
    </div>
  )
}
