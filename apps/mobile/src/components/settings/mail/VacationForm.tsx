/**
 * The web's `VacationForm.tsx`: a mailbox's vacation reply (the owner's for a personal one, a manager's for a shared
 * one), and `MailBodyField`, the body of a signature, snippet or reply. The app edits mail bodies as plain text, as
 * the composer does (`data/mail-compose.ts`).
 */
import { useRef, useState } from 'react'
import { Pressable, View } from 'react-native'

import { htmlToText, textToHtml } from '../../../data/mail-compose'
import { type MailVacation, vacationState } from '../../../data/mail-sending-api'
import { localInputValue, mailProblem } from '../../../data/mail-settings'
import { useSetVacation, useVacation } from '../../../hooks/mail-sending'
import { useTheme } from '../../../theme'
import { Button, Icon, Input, LabelTip, Text } from '../../../ui'
import { Note, OptionText, Toggle } from '../Shell'
import { VacationTimePicker } from './VacationTimePicker'

/**
 * A mail body as plain text. `initialHtml` is read once: give the field a new `key` to load another body. `onChange`
 * gets the HTML (`''` when blank).
 */
export function MailBodyField({ label, accessibilityLabel, initialHtml, onChange, placeholder }: { label?: string; accessibilityLabel: string; initialHtml: string; onChange: (html: string) => void; placeholder: string }) {
  const [text, setText] = useState(() => htmlToText(initialHtml))
  return (
    <Input
      label={label}
      accessibilityLabel={accessibilityLabel}
      multiline
      placeholder={placeholder}
      value={text}
      onChangeText={(next) => {
        setText(next)
        onChange(textToHtml(next))
      }}
    />
  )
}

/** The HTML a saved body has after a round trip through the plain-text field: unchanged text gives this back. */
export const plainBodyHtml = (html: string) => textToHtml(htmlToText(html))

/** The vacation reply of a mailbox: the owner's for a personal one, a manager's for a shared one. */
export function VacationForm({ mailboxId, mailboxName }: { mailboxId: string; mailboxName: string }) {
  const vacation = useVacation(mailboxId)
  // the web's toast: it outlives the form, which starts again after each save
  const [notice, setNotice] = useState<string | null>(null)
  if (vacation.isPending) return <Note loading>Loading the vacation reply…</Note>
  if (vacation.isError) {
    return (
      <View className="flex-row flex-wrap items-center gap-2">
        <Note error>{mailProblem(vacation.error, 'The vacation reply could not be loaded.')}</Note>
        <Button label="Retry" size="sm" variant="ghost" onPress={() => void vacation.refetch()} />
      </View>
    )
  }
  return (
    <View className="gap-3">
      {/* a fresh form for each saved version */}
      <VacationFields key={JSON.stringify(vacation.data)} mailboxId={mailboxId} mailboxName={mailboxName} saved={vacation.data} onNotice={setNotice} />
      {notice ? <Note>{notice}</Note> : null}
    </View>
  )
}

function VacationFields({ mailboxId, mailboxName, saved, onNotice }: { mailboxId: string; mailboxName: string; saved: MailVacation; onNotice: (notice: string | null) => void }) {
  const save = useSetVacation()
  const [enabled, setEnabled] = useState(saved.enabled)
  const [startsAt, setStartsAt] = useState(() => localInputValue(saved.starts_at, saved.timezone))
  const [endsAt, setEndsAt] = useState(() => localInputValue(saved.ends_at, saved.timezone))
  const [subject, setSubject] = useState(saved.subject)
  const [contactsOnly, setContactsOnly] = useState(saved.contacts_only)
  const body = useRef(saved.body_html)
  const [hasBody, setHasBody] = useState(saved.body_html.trim() !== '')
  const [problem, setProblem] = useState<string | null>(null)
  const state = vacationState(saved, new Date())

  function submit() {
    if (save.isPending) return
    setProblem(null)
    onNotice(null)
    if (enabled && !hasBody) {
      setProblem('Write the reply before you turn it on.')
      return
    }
    save.mutate(
      {
        mailboxId,
        body: { enabled, starts_at: startsAt || null, ends_at: endsAt || null, subject: subject.trim(), body_html: body.current, contacts_only: contactsOnly },
      },
      {
        onSuccess: (next) => onNotice(next.enabled ? 'Vacation reply saved' : 'Vacation reply is off'),
        onError: (error) => setProblem(mailProblem(error, 'The vacation reply could not be saved.')),
      },
    )
  }

  return (
    <View className="gap-4">
      <View className="flex-row items-center gap-3">
        <View className="min-w-0 flex-1 gap-0.5">
          <LabelTip
            label="Vacation reply"
            text={`Orbit answers mail to ${mailboxName} with this reply, at most once a week for each sender. It never answers lists, bulk mail or automatic messages.`}
            className="font-noto-medium text-[13px] text-foreground"
          />
          {state === 'on' ? <Text className="text-[12px] text-muted-foreground">On now</Text> : null}
          {state === 'scheduled' ? <Text className="text-[12px] text-muted-foreground">Starts later</Text> : null}
        </View>
        <Toggle label={`Vacation reply for ${mailboxName}`} value={enabled} onChange={setEnabled} />
      </View>
      <VacationTimeField label="Starts" help={`Empty: now. Times are in ${saved.timezone}.`} empty="Now" value={startsAt} onChange={setStartsAt} />
      <VacationTimeField label="Ends" help={`Empty: until you turn it off. Times are in ${saved.timezone}.`} empty="Until you turn it off" value={endsAt} min={startsAt} onChange={setEndsAt} />
      <Input
        label="Subject"
        help="Empty: “Re:” and the subject of the message."
        maxLength={200}
        autoComplete="off"
        placeholder="Re: the subject of the message"
        value={subject}
        onChangeText={setSubject}
      />
      <MailBodyField
        label="Reply"
        accessibilityLabel={`Vacation reply of ${mailboxName}`}
        placeholder="Write the reply…"
        initialHtml={saved.body_html}
        onChange={(html) => {
          body.current = html
          setHasBody(html.trim() !== '')
        }}
      />
      <View className="flex-row items-center gap-3">
        <OptionText title="Only people we wrote to" help={`Reply only to senders that the members of ${mailboxName} sent mail to before.`} />
        <Toggle label="Reply only to people we wrote to" value={contactsOnly} onChange={setContactsOnly} />
      </View>
      {problem ? <Note error>{problem}</Note> : null}
      <View className="flex-row">
        <Button label={save.isPending ? 'Saving…' : 'Save'} disabled={save.isPending} onPress={submit} />
      </View>
    </View>
  )
}

/** `2026-11-20T09:00` (a time in the mailbox's zone) as a `Date` with that wall clock, for the system picker. */
function wallClock(value: string): Date | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})$/.exec(value)
  if (!match) return null
  const [, year, month, day, hours, minutes] = match.map(Number) as [number, number, number, number, number, number]
  return new Date(year, month - 1, day, hours, minutes)
}

const pad = (value: number) => String(value).padStart(2, '0')
const inputValue = (at: Date) => `${at.getFullYear()}-${pad(at.getMonth() + 1)}-${pad(at.getDate())}T${pad(at.getHours())}:${pad(at.getMinutes())}`

/** The web's `datetime-local` input: the time (or `empty`), Clear, and the system picker under it. */
function VacationTimeField({ label, help, empty, value, min, onChange }: { label: string; help: string; empty: string; value: string; min?: string; onChange: (value: string) => void }) {
  const { colors } = useTheme()
  const [picking, setPicking] = useState(false)
  const minimum = min ? (wallClock(min) ?? undefined) : undefined
  return (
    <View className="gap-2">
      <LabelTip label={label} text={help} />
      <View className="flex-row items-center gap-2">
        <Pressable
          accessibilityRole="button"
          accessibilityLabel={`${label}: ${value ? value.replace('T', ' ') : empty}`}
          onPress={() => setPicking((open) => !open)}
          className="h-8 min-w-0 flex-1 flex-row items-center gap-2 rounded-lg border border-input px-2.5 dark:bg-input/30 active:bg-input/50"
        >
          <Icon name="calendar" size={16} color={colors.mutedForeground} />
          <Text numberOfLines={1} className={`shrink text-[14px] ${value ? 'text-foreground' : 'text-muted-foreground'}`}>
            {value ? value.replace('T', ' ') : empty}
          </Text>
        </Pressable>
        {picking ? <Button label="Done" size="sm" variant="ghost" onPress={() => setPicking(false)} /> : null}
        {value && !picking ? <Button label="Clear" size="sm" variant="ghost" onPress={() => onChange('')} /> : null}
      </View>
      {picking ? (
        <VacationTimePicker
          value={wallClock(value) ?? minimum ?? new Date()}
          minimumDate={minimum}
          onPick={(at) => onChange(inputValue(at))}
          onClose={() => setPicking(false)}
        />
      ) : null}
    </View>
  )
}
