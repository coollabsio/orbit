import { formatDuration, parseDuration } from '@orbit/api/duration'
import { useState } from 'react'
import { Pressable, View } from 'react-native'

import type { Person } from '../../data/chat'
import { markdownToText } from '../../data/markdown'
import { dayDate, dayValue, type TimeEntry, type TimeEntryDraft, todayLocal, workedOnLabel } from '../../data/time'
import { useTheme } from '../../theme'
import { Avatar, Button, Icon, IconButton, Input, LabelTip, Text } from '../../ui'
import { Toggle } from '../settings/Shell'
import { DayPicker } from './DayPicker'
import { SheetList, SheetRow } from './Sheet'

const DURATION_HELP = 'Write hours and minutes, such as 1h 30m, 90m or 1.5h. A number alone is minutes. Up to 24h per entry.'
const BILLABLE_HELP = 'Billable time counts toward what you bill a client.'

/**
 * A time entry in the activity feed, as the web card: `Ada logged 1h 30m · Nov 10` and the note. An entry the user
 * may change opens its menu from the more button or a long press.
 */
export function TimeEntryCard({ entry, nameOf, onMenu }: { entry: TimeEntry; nameOf: (userId: string) => string; onMenu: (entry: TimeEntry) => void }) {
  const { colors } = useTheme()
  const name = nameOf(entry.user_id)
  const note = entry.note?.trim() ? markdownToText(entry.note) : ''
  return (
    <Pressable
      accessibilityLabel={`${name} logged ${formatDuration(entry.minutes)} on ${workedOnLabel(entry.worked_on)}`}
      disabled={!entry.can_edit}
      onLongPress={() => onMenu(entry)}
      className="my-3 flex-row gap-2 rounded-lg border border-border bg-card/50 px-3.5 py-2.5 active:bg-muted"
    >
      <Avatar id={entry.user_id} name={name} size={20} />
      <View className="flex-1 min-w-0">
        <View className="min-h-5 flex-row items-center gap-1.5">
          <Text numberOfLines={1} className="text-[13px] leading-[18px] shrink text-muted-foreground">
            <Text className="font-noto-semibold text-[13px] leading-[18px] text-foreground">{name}</Text> logged{' '}
            <Text className="font-noto-medium text-[13px] leading-[18px] text-foreground">{formatDuration(entry.minutes)}</Text> · {workedOnLabel(entry.worked_on)}
          </Text>
          {entry.billable ? <Icon name="card" size={12} color={colors.mutedForeground} /> : null}
        </View>
        {note ? (
          <Text selectable className="text-[13px] leading-[19px] mt-0.5">
            {note}
          </Text>
        ) : null}
      </View>
      {entry.can_edit ? <IconButton icon="more" label="Time entry actions" color={colors.mutedForeground} onPress={() => onMenu(entry)} /> : null}
    </Pressable>
  )
}

/**
 * Log time, or edit an entry: duration, day (today by default), note and billable. `people` is the member list of
 * someone who may log time for others (`time.manage`); without it the entry is the user's own.
 */
export function TimeEntryForm({
  entry,
  userId,
  people,
  saving,
  onSave,
}: {
  entry?: TimeEntry
  userId: string
  people: readonly Person[] | null
  saving: boolean
  onSave: (draft: TimeEntryDraft) => void
}) {
  const [duration, setDuration] = useState(entry ? formatDuration(entry.minutes) : '')
  const [day, setDay] = useState(entry?.worked_on ?? todayLocal())
  const [note, setNote] = useState(entry?.note ?? '')
  const [billable, setBillable] = useState(entry?.billable ?? false)
  const [person, setPerson] = useState(entry?.user_id ?? userId)
  const [picking, setPicking] = useState<'day' | 'person' | null>(null)
  const [tried, setTried] = useState(false)
  const minutes = parseDuration(duration)
  const save = () => {
    setTried(true)
    if (minutes === null) return
    onSave({ minutes, worked_on: day, note, billable, ...(people ? { user_id: person } : {}) })
  }
  const toggle = (what: 'day' | 'person') => setPicking((current) => (current === what ? null : what))
  return (
    <SheetList>
      <View className="px-4 pb-2 gap-3">
        <Input
          label="Duration"
          help={DURATION_HELP}
          value={duration}
          onChangeText={setDuration}
          placeholder="1h 30m"
          autoCapitalize="none"
          autoCorrect={false}
          autoFocus={!entry}
          returnKeyType="done"
          onSubmitEditing={save}
          error={tried && minutes === null ? 'Enter a duration from 1m to 24h, such as 1h 30m.' : undefined}
        />
        <FormRow label="Date" value={workedOnLabel(day)} icon="calendar" open={picking === 'day'} onPress={() => toggle('day')} />
        {picking === 'day' ? (
          <DayPicker
            value={dayDate(day)}
            onPick={(picked) => {
              setDay(dayValue(picked))
              setPicking(null)
            }}
            onCancel={() => setPicking(null)}
          />
        ) : null}
        {people ? <FormRow label="Member" value={people.find((item) => item.id === person)?.name ?? 'Unknown'} icon="user" open={picking === 'person'} onPress={() => toggle('person')} /> : null}
      </View>
      {people && picking === 'person'
        ? people.map((item) => (
            <SheetRow
              key={item.id}
              label={item.name}
              leading={<Avatar id={item.id} name={item.name} size={20} />}
              checked={item.id === person}
              onPress={() => {
                setPerson(item.id)
                setPicking(null)
              }}
            />
          ))
        : null}
      <View className="px-4 pb-2 gap-3">
        <Input label="Note" value={note} onChangeText={setNote} placeholder="What did you work on?" multiline />
        <View className="min-h-11 flex-row items-center justify-between">
          <LabelTip label="Billable" text={BILLABLE_HELP} />
          <Toggle label="Billable" value={billable} onChange={setBillable} />
        </View>
        <View className="flex-row justify-end">
          <Button label={entry ? 'Save' : 'Log time'} loading={saving} disabled={tried && minutes === null} onPress={save} />
        </View>
      </View>
    </SheetList>
  )
}

/** A form row that opens a choice under it: `Date   Nov 10 ⌄`. */
function FormRow({ label, value, icon, open, onPress }: { label: string; value: string; icon: 'calendar' | 'user'; open: boolean; onPress: () => void }) {
  const { colors } = useTheme()
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={`${label}: ${value}`}
      accessibilityState={{ expanded: open }}
      onPress={onPress}
      className="min-h-11 flex-row items-center justify-between gap-3 rounded-lg active:bg-muted"
    >
      <Text className="font-noto-medium text-[14px] leading-[19.25px]">{label}</Text>
      <View className="flex-row items-center gap-1.5 shrink">
        <Icon name={icon} size={14} color={colors.mutedForeground} />
        <Text numberOfLines={1} className="text-[14px] shrink">
          {value}
        </Text>
        <View className={open ? 'rotate-180' : undefined}>
          <Icon name="chevronDown" size={14} color={colors.mutedForeground} />
        </View>
      </View>
    </Pressable>
  )
}
