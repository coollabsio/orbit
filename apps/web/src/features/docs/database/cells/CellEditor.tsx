import { useRef, useState, type KeyboardEvent } from 'react'
import { MoreH, Trash } from 'reicon-react'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Calendar } from '@/components/ui/calendar'
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from '@/components/ui/command'
import { Input } from '@/components/ui/input'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Switch } from '@/components/ui/switch'
import { Textarea } from '@/components/ui/textarea'
import { UserAvatar } from '@/components/common/UserAvatar'
import { cellOf, numberEditText, numberFromEditText, options, parseValue } from '@orbit/database/values'
import { OPTION_COLORS, type CellValue, type DatePoint, type OptionColor, type Property, type Row, type SelectOption } from '@orbit/database/types'
import { useDatabaseEnv } from '../context'
import { OPTION_COLOR_LABELS } from '../propertyMeta'
import { Pill } from './Pill'

export type EditEnd = 'stay' | 'down' | 'next' | 'previous'

export interface CellEditorProps {
  property: Property
  row: Row
  /** The cell element the popover editors open from. */
  anchor: HTMLElement | null
  onCommit: (value: CellValue | null) => void
  onTitle: (title: string) => void
  /** Saves a select's options (create, rename, recolor, delete); absent when the viewer cannot change properties. */
  onOptions?: (options: SelectOption[]) => Promise<void>
  /** Ends the edit: where the active cell goes next. */
  onClose: (end: EditEnd) => void
}

/** The editor of the active cell (spec 7.3): inline for title, number and URL, a popover for the rest. */
export function CellEditor(props: CellEditorProps) {
  const { property, row, onCommit, onTitle, onClose } = props
  const value = cellOf(row, property)
  switch (property.type) {
    case 'title':
      return <InlineEditor initial={row.title} label="Title" onSave={(text) => { if (text !== row.title) onTitle(text) }} onClose={onClose} />
    case 'number':
      return (
        <InlineEditor
          initial={value?.kind === 'number' ? numberEditText(property, value.number) : ''}
          label={property.name}
          align="right"
          onSave={(text) => {
            if (text.trim() === '') return value ? onCommit(null) : undefined
            const number = numberFromEditText(property, text)
            if (number === null) toast.error('Enter a number.')
            else if (value?.kind !== 'number' || value.number !== number) onCommit({ kind: 'number', number })
          }}
          onClose={onClose}
        />
      )
    case 'url':
      return (
        <InlineEditor
          initial={value?.kind === 'url' ? value.url : ''}
          label={property.name}
          onSave={(text) => {
            const trimmed = text.trim()
            if (trimmed === '') return value ? onCommit(null) : undefined
            const url = parseValue(property, trimmed)
            if (!url) toast.error('Enter a link that starts with http:// or https://.')
            else if (value?.kind !== 'url' || value.url !== trimmed) onCommit(url)
          }}
          onClose={onClose}
        />
      )
    case 'text':
      return <TextEditor {...props} initial={value?.kind === 'text' ? value.text : ''} />
    case 'select':
    case 'status':
    case 'multi_select':
      return <ChoiceEditor {...props} />
    case 'date':
      return <DateEditor {...props} value={value?.kind === 'date' ? value : null} />
    case 'person':
      return <PersonEditor {...props} ids={value?.kind === 'people' ? value.ids : []} />
    case 'checkbox':
    case 'created_time':
    case 'created_by':
    case 'last_edited_time':
    case 'last_edited_by':
      return null
  }
}

/** Keys that end an edit: Enter saves and goes down, Tab saves and moves sideways, Esc drops the edit. */
function endKey(event: KeyboardEvent): EditEnd | 'cancel' | null {
  if (event.key === 'Escape') return 'cancel'
  if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing) return 'down'
  if (event.key === 'Tab') return event.shiftKey ? 'previous' : 'next'
  return null
}

function InlineEditor({ initial, label, align, onSave, onClose }: {
  initial: string
  label: string
  align?: 'right'
  onSave: (text: string) => void
  onClose: (end: EditEnd) => void
}) {
  const [text, setText] = useState(initial)
  // Enter and Tab close the editor, and the unmount blurs it: save once.
  const done = useRef(false)
  const finish = (end: EditEnd | 'cancel') => {
    if (done.current) return
    done.current = true
    if (end !== 'cancel') onSave(text)
    onClose(end === 'cancel' ? 'stay' : end)
  }
  return (
    <input
      autoFocus
      aria-label={label}
      data-align={align}
      className="h-full w-full min-w-0 bg-transparent text-sm outline-none data-[align=right]:text-right"
      value={text}
      onChange={(event) => setText(event.target.value)}
      onFocus={(event) => event.target.select()}
      onKeyDown={(event) => {
        event.stopPropagation()
        const end = endKey(event)
        if (!end) return
        event.preventDefault()
        finish(end)
      }}
      onBlur={() => finish('stay')}
    />
  )
}

/** Closes a popover editor: Esc and outside clicks end the edit; `commit` runs first unless it was Esc. */
function usePopoverClose(onClose: (end: EditEnd) => void, commit?: () => void) {
  return (open: boolean, details?: { reason?: string }) => {
    if (open) return
    if (details?.reason !== 'escape-key') commit?.()
    onClose('stay')
  }
}

function TextEditor({ property, anchor, initial, onCommit, onClose }: CellEditorProps & { initial: string }) {
  const [text, setText] = useState(initial)
  const save = () => {
    const next = text.trim() === '' ? null : ({ kind: 'text', text } as const)
    if ((next?.text ?? '') !== initial) onCommit(next)
  }
  const onOpenChange = usePopoverClose(onClose, save)
  return (
    <Popover open onOpenChange={onOpenChange}>
      <PopoverContent anchor={anchor} align="start" side="bottom" sideOffset={-33} className="w-(--anchor-width) min-w-72 gap-0 p-0">
        <Textarea
          autoFocus
          aria-label={property.name}
          className="max-h-80 min-h-20 rounded-lg border-0 focus-visible:ring-0"
          value={text}
          maxLength={10_000}
          onChange={(event) => setText(event.target.value)}
          onKeyDown={(event) => {
            event.stopPropagation()
            const end = endKey(event)
            if (end !== 'down' && end !== 'next' && end !== 'previous') return
            event.preventDefault()
            save()
            onClose(end)
          }}
        />
      </PopoverContent>
    </Popover>
  )
}

function newOptionId(): string {
  return crypto.randomUUID()
}

function ChoiceEditor({ property, row, anchor, onCommit, onOptions, onClose }: CellEditorProps) {
  const [query, setQuery] = useState('')
  const multi = property.type === 'multi_select'
  const value = cellOf(row, property)
  const selected = value?.kind === 'choices' ? value.ids : value?.kind === 'choice' ? [value.id] : []
  const list = options(property)
  const trimmed = query.trim()
  const canCreate = onOptions && trimmed !== '' && !list.some((option) => option.name.toLowerCase() === trimmed.toLowerCase())
  const onOpenChange = usePopoverClose(onClose)

  const pick = (id: string) => {
    if (multi) {
      const ids = selected.includes(id) ? selected.filter((other) => other !== id) : [...selected, id]
      onCommit(ids.length > 0 ? { kind: 'choices', ids } : null)
      setQuery('')
      return
    }
    // A status always shows an option; picking the shown one again keeps it.
    if (selected[0] === id) {
      if (property.type === 'select') onCommit(null)
    } else onCommit({ kind: 'choice', id })
    onClose('stay')
  }
  const create = async () => {
    if (!onOptions) return
    const option = { id: newOptionId(), name: trimmed.slice(0, 200), color: OPTION_COLORS[(list.length % (OPTION_COLORS.length - 1)) + 1] }
    const next = property.type === 'status' ? [...list, { ...option, group: 'todo' as const }] : [...list, option]
    try {
      await onOptions(next)
    } catch {
      return
    }
    pick(option.id)
  }

  return (
    <Popover open onOpenChange={onOpenChange}>
      <PopoverContent anchor={anchor} align="start" className="w-64 gap-0 p-0">
        <Command label={property.name} loop>
          <CommandInput
            autoFocus
            aria-label={`Search ${property.name}`}
            placeholder={onOptions ? 'Search or create an option…' : 'Search options…'}
            value={query}
            onValueChange={setQuery}
            onKeyDown={(event) => {
              event.stopPropagation()
              if (event.key === 'Tab') {
                event.preventDefault()
                onClose(event.shiftKey ? 'previous' : 'next')
              }
            }}
          />
          <CommandList>
            <CommandEmpty className="py-3 text-center text-xs text-muted-foreground">No options</CommandEmpty>
            <CommandGroup>
              {list.map((option) => (
                <CommandItem
                  key={option.id}
                  value={option.id}
                  keywords={[option.name]}
                  data-checked={selected.includes(option.id) || undefined}
                  onSelect={() => pick(option.id)}
                  className="group/option"
                >
                  <Pill color={option.color} dot={property.type === 'status'}>{option.name}</Pill>
                  <span className="flex-1" />
                  {onOptions ? <OptionMenu option={option} list={list} onOptions={onOptions} /> : null}
                </CommandItem>
              ))}
              {canCreate ? (
                <CommandItem value={`create:${trimmed}`} keywords={[trimmed]} forceMount onSelect={() => void create()}>
                  <span className="text-muted-foreground">Create</span>
                  <Pill color="default">{trimmed}</Pill>
                </CommandItem>
              ) : null}
            </CommandGroup>
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  )
}

/** Rename, recolor or delete one option. It saves the property's whole option list. */
function OptionMenu({ option, list, onOptions }: { option: SelectOption; list: readonly SelectOption[]; onOptions: (options: SelectOption[]) => Promise<void> }) {
  const [name, setName] = useState(option.name)
  const save = (change: Partial<SelectOption>) =>
    void onOptions(list.map((other) => (other.id === option.id ? { ...other, ...change } : other))).catch(() => undefined)
  const rename = () => {
    const next = name.trim()
    if (next && next !== option.name) save({ name: next.slice(0, 200) })
  }
  return (
    <Popover onOpenChange={(open) => { if (!open) rename() }}>
      <PopoverTrigger
        render={
          <Button
            type="button"
            variant="ghost"
            size="icon-xs"
            aria-label={`Edit option ${option.name}`}
            className="text-muted-foreground hover-fine:opacity-0 hover-fine:group-hover/option:opacity-100 hover-fine:group-data-selected/option:opacity-100"
            onPointerDown={(event) => event.stopPropagation()}
            onClick={(event) => event.stopPropagation()}
          />
        }
      >
        <MoreH />
      </PopoverTrigger>
      <PopoverContent align="start" side="right" className="w-52 gap-1 p-1.5" onKeyDown={(event) => event.stopPropagation()}>
        <Input
          aria-label="Option name"
          value={name}
          maxLength={200}
          onChange={(event) => setName(event.target.value)}
          onKeyDown={(event) => { if (event.key === 'Enter') rename() }}
          onBlur={rename}
        />
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="justify-start text-destructive hover:text-destructive"
          onClick={() => void onOptions(list.filter((other) => other.id !== option.id)).catch(() => undefined)}
        >
          <Trash />
          Delete
        </Button>
        <div className="px-1.5 pt-1 text-xs text-muted-foreground">Colors</div>
        <div role="radiogroup" aria-label="Option color" className="flex flex-col">
          {OPTION_COLORS.map((color: OptionColor) => (
            <Button
              key={color}
              type="button"
              variant="ghost"
              size="sm"
              role="radio"
              aria-checked={option.color === color}
              className="justify-start gap-2 font-normal aria-checked:bg-muted"
              onClick={() => save({ color })}
            >
              <Pill color={color} className="size-4 px-0" aria-hidden="true">{''}</Pill>
              {OPTION_COLOR_LABELS[color]}
            </Button>
          ))}
        </div>
      </PopoverContent>
    </Popover>
  )
}

function dayOf(date: Date): string {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`
}

function localDate(day: string): Date {
  const [year, month, date] = day.split('-').map(Number)
  return new Date(year, month - 1, date)
}

/** A point as the editor shows it: the viewer's local day and `HH:MM`. */
function localParts(point: DatePoint): { day: string; time: string } {
  if (point.kind === 'day') return { day: point.day, time: '09:00' }
  const date = new Date(point.ms)
  return { day: dayOf(date), time: `${String(date.getHours()).padStart(2, '0')}:${String(date.getMinutes()).padStart(2, '0')}` }
}

function pointOf(day: string, time: string | null): DatePoint {
  if (time === null) return { kind: 'day', day }
  const date = new Date(`${day}T${time}:00`)
  const iso = date.toISOString().replace(/\.\d{3}Z$/, 'Z')
  return { kind: 'moment', iso, ms: date.getTime() }
}

function DateEditor({ anchor, value, onCommit, onClose }: CellEditorProps & { value: Extract<CellValue, { kind: 'date' }> | null }) {
  const start = value ? localParts(value.date.start) : null
  const end = value?.date.end ? localParts(value.date.end) : null
  const [hasEnd, setHasEnd] = useState(end !== null)
  const [withTime, setWithTime] = useState(value?.date.start.kind === 'moment')
  const [startTime, setStartTime] = useState(start?.time ?? '09:00')
  const [endTime, setEndTime] = useState(end?.time ?? start?.time ?? '09:00')
  const [month, setMonth] = useState(() => (start ? localDate(start.day) : new Date()))
  const onOpenChange = usePopoverClose(onClose)

  const save = (next: { start: string; end: string | null; withTime: boolean; startTime: string; endTime: string }) => {
    const startPoint = pointOf(next.start, next.withTime ? next.startTime : null)
    let endPoint = next.end ? pointOf(next.end, next.withTime ? next.endTime : null) : null
    if (endPoint && (endPoint.kind === 'day' ? endPoint.day < next.start : endPoint.ms < (startPoint as { ms: number }).ms)) endPoint = null
    onCommit({ kind: 'date', date: { start: startPoint, end: endPoint } })
  }
  const current = { start: start?.day ?? null, end: end?.day ?? null, withTime, startTime, endTime }

  return (
    <Popover open onOpenChange={onOpenChange}>
      <PopoverContent anchor={anchor} align="start" className="w-auto gap-0 p-0" onKeyDown={(event) => event.stopPropagation()}>
        <div className="flex max-h-(--available-height) flex-col gap-3 overflow-y-auto overscroll-contain p-3">
          {hasEnd ? (
            <Calendar
              mode="range"
              selected={start ? { from: localDate(start.day), to: end ? localDate(end.day) : undefined } : undefined}
              month={month}
              onMonthChange={setMonth}
              onSelect={(range) => {
                if (!range?.from) return
                save({ ...current, start: dayOf(range.from), end: range.to ? dayOf(range.to) : null })
              }}
            />
          ) : (
            <Calendar
              mode="single"
              selected={start ? localDate(start.day) : undefined}
              month={month}
              onMonthChange={setMonth}
              onSelect={(day) => {
                if (day) save({ ...current, start: dayOf(day), end: null })
              }}
            />
          )}
          <label className="flex items-center justify-between gap-3 text-sm">
            End date
            <Switch
              checked={hasEnd}
              onCheckedChange={(checked) => {
                setHasEnd(checked)
                if (!checked && current.start && current.end) save({ ...current, start: current.start, end: null })
              }}
            />
          </label>
          <label className="flex items-center justify-between gap-3 text-sm">
            Include time
            <Switch
              checked={withTime}
              onCheckedChange={(checked) => {
                setWithTime(checked)
                if (current.start) save({ ...current, start: current.start, withTime: checked })
              }}
            />
          </label>
          {withTime && current.start ? (
            <div className="flex items-center gap-2">
              <Input
                type="time"
                aria-label="Start time"
                value={startTime}
                onChange={(event) => setStartTime(event.target.value)}
                onBlur={() => { if (current.start && startTime) save({ ...current, start: current.start }) }}
              />
              {hasEnd && current.end ? (
                <Input
                  type="time"
                  aria-label="End time"
                  value={endTime}
                  onChange={(event) => setEndTime(event.target.value)}
                  onBlur={() => { if (current.start && endTime) save({ ...current, start: current.start }) }}
                />
              ) : null}
            </div>
          ) : null}
          <Button
            type="button"
            variant="ghost"
            size="sm"
            disabled={!value}
            onClick={() => {
              onCommit(null)
              onClose('stay')
            }}
          >
            Clear
          </Button>
        </div>
      </PopoverContent>
    </Popover>
  )
}

function PersonEditor({ property, anchor, ids, onCommit, onClose }: CellEditorProps & { ids: string[] }) {
  const { users } = useDatabaseEnv()
  const onOpenChange = usePopoverClose(onClose)
  const toggle = (id: string) => {
    const next = ids.includes(id) ? ids.filter((other) => other !== id) : [...ids, id]
    onCommit(next.length > 0 ? { kind: 'people', ids: next } : null)
  }
  return (
    <Popover open onOpenChange={onOpenChange}>
      <PopoverContent anchor={anchor} align="start" className="w-64 gap-0 p-0">
        <Command label={property.name} loop>
          <CommandInput autoFocus aria-label={`Search ${property.name}`} placeholder="Search people…" onKeyDown={(event) => event.stopPropagation()} />
          <CommandList>
            <CommandEmpty className="py-3 text-center text-xs text-muted-foreground">No people</CommandEmpty>
            <CommandGroup>
              {[...users.values()].map((user) => (
                <CommandItem key={user.id} value={user.id} keywords={[user.name, user.email]} data-checked={ids.includes(user.id) || undefined} onSelect={() => toggle(user.id)}>
                  <UserAvatar user={user} size={18} />
                  <span className="truncate">{user.name}</span>
                </CommandItem>
              ))}
            </CommandGroup>
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  )
}
