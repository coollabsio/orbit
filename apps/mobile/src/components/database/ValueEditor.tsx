/**
 * The sheet editor of one value (the web's `CellEditor` at phone size): a field for text, number and link, a checked
 * list for options and people, and the system calendar for dates. Each change saves at once, as on the web.
 */
import { dayOf, editedDate, localDate, localParts } from '@orbit/database/dates'
import { OPTION_COLORS, type CellValue, type Property, type Row, type SelectOption, type StatusOption } from '@orbit/database/types'
import { cellOf, numberEditText, numberFromEditText, options, parseValue } from '@orbit/database/values'
import { randomUUID } from 'expo-crypto'
import { useState } from 'react'
import { Pressable, Switch, View } from 'react-native'

import { parseTime } from '../../data/database'
import { usePeople } from '../../queries'
import { useTheme } from '../../theme'
import { Avatar, Button, Icon, Input, Text } from '../../ui'
import { DayPicker } from '../tasks/DayPicker'
import { SheetList, SheetNote, SheetRow, SheetSearch } from '../tasks/Sheet'
import { ChoiceRow, Pill } from './cells'
import { FilesEditor } from './FilesEditor'

export interface ValueEditorProps {
  property: Property
  row: Row
  onCommit: (value: CellValue | null) => void
  /** Saves a select's whole option list (an option created here); absent when the viewer cannot change properties. */
  onOptions?: (options: SelectOption[]) => Promise<void>
  onDone: () => void
}

export function ValueEditor(props: ValueEditorProps) {
  const { property, row, onCommit, onDone } = props
  const value = cellOf(row, property)
  switch (property.type) {
    case 'text':
      return (
        <TextForm
          initial={value?.kind === 'text' ? value.text : ''}
          label={property.name}
          multiline
          onSave={(text) => {
            const next = text.trim() === '' ? null : ({ kind: 'text', text } as const)
            if ((next?.text ?? '') !== (value?.kind === 'text' ? value.text : '')) onCommit(next)
            onDone()
          }}
        />
      )
    case 'number':
      return (
        <TextForm
          initial={value?.kind === 'number' ? numberEditText(property, value.number) : ''}
          label={property.name}
          keyboard="decimal-pad"
          onSave={(text) => {
            if (text.trim() === '') {
              if (value) onCommit(null)
              return onDone()
            }
            const number = numberFromEditText(property, text)
            if (number === null) return 'Enter a number.'
            if (value?.kind !== 'number' || value.number !== number) onCommit({ kind: 'number', number })
            onDone()
          }}
        />
      )
    case 'url':
      return (
        <TextForm
          initial={value?.kind === 'url' ? value.url : ''}
          label={property.name}
          keyboard="url"
          onSave={(text) => {
            const trimmed = text.trim()
            if (trimmed === '') {
              if (value) onCommit(null)
              return onDone()
            }
            const url = parseValue(property, trimmed)
            if (!url) return 'Enter a link that starts with http:// or https://.'
            if (value?.kind !== 'url' || value.url !== trimmed) onCommit(url)
            onDone()
          }}
        />
      )
    case 'email':
    case 'phone':
      return (
        <TextForm
          initial={value?.kind === 'text' ? value.text : ''}
          label={property.name}
          keyboard={property.type === 'email' ? 'email-address' : 'phone-pad'}
          onSave={(text) => {
            const trimmed = text.trim()
            if (trimmed === '') {
              if (value) onCommit(null)
              return onDone()
            }
            const next = parseValue(property, trimmed)
            if (!next) return property.type === 'email' ? 'Enter an email address.' : 'Enter a phone number (digits, spaces and + - ( ) .).'
            if (value?.kind !== 'text' || value.text !== trimmed) onCommit(next)
            onDone()
          }}
        />
      )
    case 'files':
      return <FilesEditor row={row} value={value} onCommit={onCommit} />
    case 'unique_id':
      return null
    case 'select':
    case 'status':
    case 'multi_select':
      return <ChoiceEditor {...props} />
    case 'date':
      return <DateEditor {...props} value={value?.kind === 'date' ? value : null} />
    case 'person':
      return <PersonEditor ids={value?.kind === 'people' ? value.ids : []} onCommit={onCommit} />
    case 'title':
    case 'checkbox':
    case 'created_time':
    case 'created_by':
    case 'last_edited_time':
    case 'last_edited_by':
      return null
  }
}

/** One field and Save. `onSave` returns an error to show, or nothing when it saved. */
function TextForm({ initial, label, multiline = false, keyboard, onSave }: {
  initial: string
  label: string
  multiline?: boolean
  keyboard?: 'decimal-pad' | 'url' | 'email-address' | 'phone-pad'
  onSave: (text: string) => string | void
}) {
  const [text, setText] = useState(initial)
  const [error, setError] = useState<string | null>(null)
  const save = () => setError(onSave(text) ?? null)
  return (
    <View className="px-4 pb-2 gap-3">
      <Input
        label={label}
        value={text}
        onChangeText={setText}
        multiline={multiline}
        maxLength={multiline ? 10_000 : undefined}
        keyboardType={keyboard}
        autoCapitalize={keyboard ? 'none' : 'sentences'}
        autoCorrect={!keyboard}
        autoFocus
        returnKeyType={multiline ? 'default' : 'done'}
        onSubmitEditing={multiline ? undefined : save}
        error={error ?? undefined}
      />
      <View className="flex-row justify-end">
        <Button label="Save" onPress={save} />
      </View>
    </View>
  )
}

function ChoiceEditor({ property, row, onCommit, onOptions, onDone }: ValueEditorProps) {
  const { colors } = useTheme()
  const [query, setQuery] = useState('')
  const [creating, setCreating] = useState(false)
  const multi = property.type === 'multi_select'
  const value = cellOf(row, property)
  const selected = value?.kind === 'choices' ? value.ids : value?.kind === 'choice' ? [value.id] : []
  const list = options(property)
  const trimmed = query.trim()
  const needle = trimmed.toLowerCase()
  const shown = list.filter((option) => !needle || option.name.toLowerCase().includes(needle))
  const canCreate = onOptions && trimmed !== '' && !list.some((option) => option.name.toLowerCase() === needle)

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
    onDone()
  }
  const create = async () => {
    if (!onOptions || creating) return
    const option = { id: randomUUID(), name: trimmed.slice(0, 200), color: OPTION_COLORS[(list.length % (OPTION_COLORS.length - 1)) + 1]! }
    const status: StatusOption = { ...option, group: 'todo' }
    const next: SelectOption[] = [...list, property.type === 'status' ? status : option]
    setCreating(true)
    try {
      await onOptions(next)
    } catch {
      return
    } finally {
      setCreating(false)
    }
    pick(option.id)
  }

  return (
    <>
      <SheetSearch value={query} onChange={setQuery} placeholder={onOptions ? 'Search or create an option…' : 'Search options…'} />
      <SheetList>
        {shown.map((option) => (
          <ChoiceRow key={option.id} label={option.name} checked={selected.includes(option.id)} onPress={() => pick(option.id)}>
            <Pill color={option.color} label={option.name} dot={property.type === 'status'} />
          </ChoiceRow>
        ))}
        {canCreate ? <SheetRow label={`${creating ? 'Creating' : 'Create'} “${trimmed}”`} leading={<Icon name="plus" size={18} color={colors.mutedForeground} />} onPress={() => void create()} /> : null}
        {shown.length === 0 && !canCreate ? <SheetNote>No options</SheetNote> : null}
      </SheetList>
    </>
  )
}

function PersonEditor({ ids, onCommit }: { ids: string[]; onCommit: (value: CellValue | null) => void }) {
  const people = usePeople()
  const [query, setQuery] = useState('')
  const needle = query.trim().toLowerCase()
  const shown = people.filter((person) => !needle || person.name.toLowerCase().includes(needle))
  const toggle = (id: string) => {
    const next = ids.includes(id) ? ids.filter((other) => other !== id) : [...ids, id]
    onCommit(next.length > 0 ? { kind: 'people', ids: next } : null)
  }
  return (
    <>
      <SheetSearch value={query} onChange={setQuery} placeholder="Search people…" />
      <SheetList>
        {shown.map((person) => (
          <SheetRow key={person.id} label={person.name} leading={<Avatar id={person.id} name={person.name} size={20} />} checked={ids.includes(person.id)} onPress={() => toggle(person.id)} />
        ))}
        {shown.length === 0 ? <SheetNote>No people</SheetNote> : null}
      </SheetList>
    </>
  )
}

type DateField = 'start' | 'end'

function DateEditor({ value, onCommit, onDone }: ValueEditorProps & { value: Extract<CellValue, { kind: 'date' }> | null }) {
  const start = value ? localParts(value.date.start) : null
  const end = value?.date.end ? localParts(value.date.end) : null
  const [hasEnd, setHasEnd] = useState(end !== null)
  const [withTime, setWithTime] = useState(value?.date.start.kind === 'moment')
  const [startTime, setStartTime] = useState(start?.time ?? '09:00')
  const [endTime, setEndTime] = useState(end?.time ?? start?.time ?? '09:00')
  const [picking, setPicking] = useState<DateField | null>(start ? null : 'start')
  const current = { start: start?.day ?? null, end: end?.day ?? null, withTime, startTime, endTime }
  const save = (next: Partial<typeof current>) => {
    const merged = { ...current, ...next }
    if (merged.start) onCommit({ kind: 'date', date: editedDate({ ...merged, start: merged.start, end: hasEnd || next.end ? merged.end : null }) })
  }
  const pickDay = (field: DateField, day: Date) => {
    setPicking(null)
    if (field === 'start') save({ start: dayOf(day) })
    else save({ end: dayOf(day) })
  }
  return (
    <SheetList>
      <DateRow label={hasEnd ? 'Start' : 'Date'} day={current.start} active={picking === 'start'} onPress={() => setPicking(picking === 'start' ? null : 'start')} />
      {picking === 'start' ? <DayPicker value={current.start ? localDate(current.start) : new Date()} onPick={(day) => pickDay('start', day)} onCancel={() => setPicking(null)} /> : null}
      {hasEnd ? <DateRow label="End" day={current.end} active={picking === 'end'} onPress={() => setPicking(picking === 'end' ? null : 'end')} /> : null}
      {hasEnd && picking === 'end' ? <DayPicker value={localDate(current.end ?? current.start ?? dayOf(new Date()))} onPick={(day) => pickDay('end', day)} onCancel={() => setPicking(null)} /> : null}
      <SwitchRow
        label="End date"
        value={hasEnd}
        onChange={(checked) => {
          setHasEnd(checked)
          if (checked) setPicking('end')
          else if (current.start && current.end) save({ end: null })
        }}
      />
      <SwitchRow
        label="Include time"
        value={withTime}
        onChange={(checked) => {
          setWithTime(checked)
          save({ withTime: checked })
        }}
      />
      {withTime && current.start ? (
        <View className="flex-row gap-2 px-4 pb-2">
          <TimeField label="Start time" value={startTime} onChange={setStartTime} onSave={(time) => save({ startTime: time })} />
          {hasEnd && current.end ? <TimeField label="End time" value={endTime} onChange={setEndTime} onSave={(time) => save({ endTime: time })} /> : null}
        </View>
      ) : null}
      {value ? (
        <SheetRow
          label="Clear"
          icon="close"
          onPress={() => {
            onCommit(null)
            onDone()
          }}
        />
      ) : null}
    </SheetList>
  )
}

function DateRow({ label, day, active, onPress }: { label: string; day: string | null; active: boolean; onPress: () => void }) {
  const { colors } = useTheme()
  return (
    <Pressable accessibilityRole="button" accessibilityLabel={`${label}: ${day ?? 'none'}`} onPress={onPress} className="min-h-11 flex-row items-center gap-3 px-4 active:bg-accent">
      <Icon name="calendar" size={18} color={colors.mutedForeground} />
      <Text className="font-noto-medium text-[14px] flex-1">{label}</Text>
      <Text className={`text-[14px] ${active ? 'text-primary' : day ? 'text-foreground' : 'text-muted-foreground'}`}>{day ? localDate(day).toLocaleDateString(undefined, { dateStyle: 'medium' }) : 'Pick a day'}</Text>
    </Pressable>
  )
}

function SwitchRow({ label, value, onChange }: { label: string; value: boolean; onChange: (value: boolean) => void }) {
  const { colors } = useTheme()
  return (
    <View className="min-h-11 flex-row items-center justify-between px-4">
      <Text className="font-noto-medium text-[14px]">{label}</Text>
      <Switch accessibilityLabel={label} value={value} onValueChange={onChange} trackColor={{ true: colors.primary, false: colors.input }} />
    </View>
  )
}

/** `HH:MM`; a time it cannot read goes back to the last saved one. */
function TimeField({ label, value, onChange, onSave }: { label: string; value: string; onChange: (text: string) => void; onSave: (time: string) => void }) {
  const [text, setText] = useState(value)
  const commit = () => {
    const time = parseTime(text)
    if (!time) return setText(value)
    setText(time)
    onChange(time)
    if (time !== value) onSave(time)
  }
  return (
    <View className="flex-1">
      <Input label={label} value={text} onChangeText={setText} onBlur={commit} onSubmitEditing={commit} placeholder="09:00" keyboardType="numbers-and-punctuation" returnKeyType="done" />
    </View>
  )
}
