/**
 * The contents of the task sheets: each picker renders its rows into the one `Sheet` of its screen. Shared by the
 * task detail page and the New task dialog.
 */
import { type ReactNode, useState } from 'react'
import { Platform, Pressable, View } from 'react-native'

import type { Person } from '../../data/chat'
import type { LabelRecord, StatusRecord, TaskRecord } from '../../data/orbit'
import {
  DEFAULT_DUE_TIME,
  dayAfter,
  dueOn,
  type DueRange,
  dueWeek,
  identifier,
  LABEL_PALETTE,
  labelChoices,
  nextLabelColor,
  PRIORITY_CHOICES,
  PRIORITY_LABEL,
  type Priority,
  projectLabels,
  sourceValue,
  taskCandidates,
  timeOf,
  toggleId,
} from '../../data/tasks'
import { useCreateLabel, useTaskSearch } from '../../hooks/tasks'
import { useLabels, useProjects, useStatuses } from '../../queries'
import { useTheme } from '../../theme'
import { Avatar, Button, Icon, type IconName, Input, PriorityIcon, StatusIcon, Text } from '../../ui'
import { DayPicker } from './DayPicker'
import { SheetList, SheetNote, SheetRow, SheetSearch } from './Sheet'

export function PriorityChoices({ value, onSelect }: { value: string; onSelect: (priority: Priority) => void }) {
  return (
    <SheetList>
      {PRIORITY_CHOICES.map((priority) => (
        <SheetRow key={priority} label={PRIORITY_LABEL[priority]} leading={<PriorityIcon priority={priority} />} checked={priority === value} onPress={() => onSelect(priority)} />
      ))}
    </SheetList>
  )
}

export function StatusChoices({ statuses, value, onSelect }: { statuses: readonly StatusRecord[]; value: string | undefined; onSelect: (statusId: string) => void }) {
  return (
    <SheetList>
      {statuses.map((status) => (
        <SheetRow key={status.id} label={status.name} leading={<StatusIcon status={status} />} checked={status.id === value} onPress={() => onSelect(status.id)} />
      ))}
    </SheetList>
  )
}

/** The workspace members with a check each: every tap changes the assignees and the sheet stays open. */
export function PeopleChoices({ people, value, onChange }: { people: readonly Person[]; value: readonly string[]; onChange: (ids: string[]) => void }) {
  const [query, setQuery] = useState('')
  const needle = query.trim().toLowerCase()
  const shown = people.filter((person) => !needle || person.name.toLowerCase().includes(needle))
  return (
    <>
      <SheetSearch value={query} onChange={setQuery} placeholder="Change assignees…" />
      <SheetList>
        {shown.map((person) => (
          <SheetRow key={person.id} label={person.name} leading={<Avatar id={person.id} name={person.name} size={20} />} checked={value.includes(person.id)} onPress={() => onChange(toggleId(value, person.id))} />
        ))}
        {shown.length === 0 ? <SheetNote>No members match.</SheetNote> : null}
      </SheetList>
    </>
  )
}

/**
 * The project's labels with a check each, like the web's label picker: the labels the task had when the sheet
 * opened come first, a name no label has becomes a "Create" row with the palette under it (preset to the colour
 * the project uses least), and the new label is added at once.
 */
export function LabelChoices({ projectId, value, onChange }: { projectId: string; value: readonly string[]; onChange: (ids: string[]) => void }) {
  const { colors } = useTheme()
  const labels = useLabels()
  const create = useCreateLabel()
  const [pinned] = useState(value)
  const [query, setQuery] = useState('')
  const [picked, setPicked] = useState<string>()
  const all = labels.data ?? []
  const { rows, create: name } = labelChoices(all, projectId, query, pinned)
  const color = picked ?? nextLabelColor(projectLabels(all, projectId))
  const own = projectLabels(all, projectId)
  const toggle = (id: string) => {
    if (!create.isPending) onChange(toggleId(value, id))
  }
  const add = () => {
    if (!name || create.isPending) return
    create.mutate(
      { projectId, name, color },
      {
        onSuccess: (label) => {
          onChange(value.includes(label.id) ? [...value] : [...value, label.id])
          setQuery('')
          setPicked(undefined)
        },
      },
    )
  }
  return (
    <>
      <SheetSearch value={query} onChange={setQuery} placeholder={own.length > 0 ? 'Change labels…' : 'New label name…'} />
      <SheetList>
        {rows.map((label) => (
          <SheetRow key={label.id} label={label.name} leading={<Dot color={label.color} />} checked={value.includes(label.id)} onPress={() => toggle(label.id)} />
        ))}
        {name ? (
          <>
            <SheetRow label={`${create.isPending ? 'Creating' : 'Create'} “${name}”`} leading={<Icon name="plus" size={18} color={colors.mutedForeground} />} onPress={add} />
            <Swatches value={color} onChange={setPicked} />
          </>
        ) : null}
        {own.length === 0 && !query.trim() ? <SheetNote>This project has no labels yet. Type a name to create one.</SheetNote> : null}
        {query.trim() && rows.length === 0 && !name ? <SheetNote>No labels match.</SheetNote> : null}
        {create.isError ? <SheetNote tone="destructive">The label could not be created. Try again.</SheetNote> : null}
      </SheetList>
    </>
  )
}

/** The label palette as round swatches; the chosen one wears a ring in its own colour. */
function Swatches({ value, onChange }: { value: string; onChange: (color: string) => void }) {
  return (
    <View accessibilityRole="radiogroup" accessibilityLabel="Label color" className="flex-row flex-wrap gap-2.5 px-4 pt-1 pb-3">
      {LABEL_PALETTE.map((swatch) => {
        const checked = swatch.color === value
        return (
          <Pressable
            key={swatch.color}
            accessibilityRole="radio"
            accessibilityLabel={swatch.name}
            accessibilityState={{ checked }}
            hitSlop={4}
            onPress={() => onChange(swatch.color)}
            className="size-6.5 rounded-full items-center justify-center border-[1.5px]" style={{ borderColor: checked ? swatch.color : 'transparent' }}
          >
            <View className="size-4.5 rounded-full" style={{ backgroundColor: swatch.color }} />
          </Pressable>
        )
      })}
    </View>
  )
}

/**
 * Another task to pick (parent, relation, duplicate target): recently updated tasks, or the server search as the
 * user types. `excludeDuplicates` leaves out tasks that are duplicates themselves.
 */
export function TaskChoices({ excludeIds, excludeDuplicates = false, onSelect }: { excludeIds: readonly string[]; excludeDuplicates?: boolean; onSelect: (task: TaskRecord) => void }) {
  const [query, setQuery] = useState('')
  const search = useTaskSearch(query, true)
  const projects = useProjects()
  const statuses = useStatuses()
  const duplicateStatusIds = excludeDuplicates ? (statuses.data ?? []).filter((status) => status.category === 'duplicate').map((status) => status.id) : undefined
  const candidates = taskCandidates(search.data ?? [], projects.data ?? [], { query, excludeIds, duplicateStatusIds })
  return (
    <>
      <SheetSearch value={query} onChange={setQuery} placeholder="Search tasks…" autoFocus={Platform.OS !== 'web'} />
      <SheetList>
        {candidates.map((task) => (
          <SheetRow
            key={task.id}
            label={task.title || 'Untitled'}
            subtitle={`${identifier(task, projects.data ?? [])} · ${projects.data?.find((project) => project.id === task.project_id)?.name ?? ''}`}
            leading={<StatusIcon status={statuses.data?.find((status) => status.id === task.status_id)} />}
            onPress={() => onSelect(task)}
          />
        ))}
        {candidates.length === 0 ? <SheetNote>{search.isFetching || search.isPending ? 'Searching…' : search.isError ? 'Could not search the tasks.' : 'No matching tasks'}</SheetNote> : null}
      </SheetList>
    </>
  )
}

/**
 * The due date at phone size: Today, Tomorrow, the web's This week / Next week shortcuts, a day from the system
 * calendar, and No due date. A pick keeps the time the task had (else 09:00) and closes the sheet.
 */
export function DueDateChoices({ dueAt, onChange }: { dueAt: string | null | undefined; onChange: (due: DueRange | null) => void }) {
  const [picking, setPicking] = useState(false)
  const time = dueAt ? timeOf(new Date(dueAt)) : DEFAULT_DUE_TIME
  const now = new Date()
  const rows: { key: string; label: string; icon: IconName; due: () => DueRange }[] = [
    { key: 'today', label: 'Today', icon: 'calendar', due: () => dueOn(dayAfter(now, 0), time) },
    { key: 'tomorrow', label: 'Tomorrow', icon: 'calendar', due: () => dueOn(dayAfter(now, 1), time) },
    { key: 'week', label: 'This week', icon: 'calendarTick', due: () => dueWeek(0, time, now) },
    { key: 'next', label: 'Next week', icon: 'calendarTick', due: () => dueWeek(1, time, now) },
  ]
  return (
    <SheetList>
      {rows.map((row) => (
        <SheetRow key={row.key} label={row.label} icon={row.icon} onPress={() => onChange(row.due())} />
      ))}
      <SheetRow label="Pick a date…" icon="calendar" checked={picking} onPress={() => setPicking((value) => !value)} />
      {picking ? <DayPicker value={dueAt ? new Date(dueAt) : now} onPick={(day) => onChange(dueOn(day, time))} onCancel={() => setPicking(false)} /> : null}
      {dueAt ? <SheetRow label="No due date" icon="close" onPress={() => onChange(null)} /> : null}
    </SheetList>
  )
}

/** The source URL form of the web's "Add source" popover: Save sets it, an empty field (or Remove) clears it. */
export function SourceForm({ value, onSave }: { value: string | null | undefined; onSave: (url: string | null) => void }) {
  const [draft, setDraft] = useState(value ?? '')
  return (
    <View className="px-4 pb-2 gap-3">
      <Input
        label="Source URL"
        value={draft}
        onChangeText={setDraft}
        placeholder="https://..."
        keyboardType="url"
        autoCapitalize="none"
        autoCorrect={false}
        autoFocus
        returnKeyType="done"
        onSubmitEditing={() => onSave(sourceValue(draft))}
      />
      <View className="flex-row justify-end gap-2">
        {value ? <Button label="Remove" variant="ghost" onPress={() => onSave(null)} /> : null}
        <Button label="Save" onPress={() => onSave(sourceValue(draft))} />
      </View>
    </View>
  )
}

/** Rows that each run an action (the web's dropdown menus). */
export function MenuChoices<K extends string>({ items, onSelect }: { items: readonly { key: K; label: string; icon?: IconName }[]; onSelect: (key: K) => void }) {
  return (
    <SheetList>
      {items.map((item) => (
        <SheetRow key={item.key} label={item.label} icon={item.icon} onPress={() => onSelect(item.key)} />
      ))}
    </SheetList>
  )
}

export function Dot({ color, size = 8 }: { color: string; size?: number }) {
  return <View className="rounded-full" style={{ width: size, height: size, backgroundColor: color }} />
}

/** A label: the round outline pill with its colour dot, as on the web. */
export function LabelPill({ label, children }: { label: Pick<LabelRecord, 'name' | 'color'>; children?: ReactNode }) {
  return (
    <View className="h-5 flex-row items-center gap-1 px-2 rounded-full border border-input bg-input/30">
      <Dot color={label.color} size={6} />
      <Text numberOfLines={1} className="font-noto-medium text-[12px] leading-[16px]">
        {label.name}
      </Text>
      {children}
    </View>
  )
}
