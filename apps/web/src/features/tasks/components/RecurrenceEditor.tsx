import { useState } from 'react'
import type { TaskRecurrence, TaskRecurrenceBase, TaskRecurrenceFrequency } from '@orbit/api/generated/types.gen'
import {
  DEFAULT_RECURRENCE,
  MAX_RECURRENCE_INTERVAL,
  normalizeRecurrence,
  RECURRENCE_FREQUENCIES,
  RECURRENCE_WEEKDAYS,
  recurrenceUnit,
} from '@orbit/api/recurrence'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { InfoTip } from '@/components/common/InfoTip'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'

const BASE_HELP =
  'Due date: a fixed calendar, such as every Monday. Completing late skips the missed dates. Completion date: the next due date counts from the day you complete the task, such as 3 days after.'

/**
 * The repeat rule of a task, edited as a draft: Save sends it, Stop repeating removes it. Weekdays show only for a
 * weekly rule counted from the due date, the one rule they apply to.
 */
export function RecurrenceEditor({ value, onSave, onStop }: {
  value: TaskRecurrence | null | undefined
  onSave: (rule: TaskRecurrence) => void
  onStop: () => void
}) {
  const [draft, setDraft] = useState<TaskRecurrence>(value ?? DEFAULT_RECURRENCE)
  const [intervalText, setIntervalText] = useState(String(draft.interval))
  const update = (changes: Partial<TaskRecurrence>) => setDraft((current) => normalizeRecurrence({ ...current, ...changes }))
  const frequencies = RECURRENCE_FREQUENCIES.map((frequency) => ({ value: frequency, label: recurrenceUnit(frequency, draft.interval) }))
  const showsWeekdays = draft.frequency === 'weekly' && draft.base === 'due'
  return (
    <form
      className="flex w-72 flex-col gap-3"
      onSubmit={(event) => {
        event.preventDefault()
        onSave(draft)
      }}
    >
      <div className="flex items-center gap-2 text-sm">
        <span>Every</span>
        <Input
          aria-label="Interval"
          type="number"
          inputMode="numeric"
          min={1}
          max={MAX_RECURRENCE_INTERVAL}
          className="w-16"
          value={intervalText}
          onChange={(event) => {
            setIntervalText(event.target.value)
            const count = Number(event.target.value)
            if (Number.isFinite(count) && count >= 1) update({ interval: count })
          }}
          onBlur={() => setIntervalText(String(draft.interval))}
        />
        <Select items={frequencies} value={draft.frequency} onValueChange={(next) => next && update({ frequency: next as TaskRecurrenceFrequency })}>
          <SelectTrigger aria-label="Frequency" className="flex-1">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {frequencies.map((option) => (
              <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      {showsWeekdays ? (
        <div className="flex flex-col gap-1.5">
          <span className="text-xs font-medium text-muted-foreground">On</span>
          <ToggleGroup
            aria-label="Weekdays"
            variant="outline"
            size="sm"
            spacing={0}
            multiple
            value={draft.weekdays}
            onValueChange={(days: string[]) => update({ weekdays: days as TaskRecurrence['weekdays'] })}
          >
            {RECURRENCE_WEEKDAYS.map((day) => (
              <ToggleGroupItem key={day.value} value={day.value} aria-label={day.label} className="w-8 px-0 aria-pressed:bg-primary aria-pressed:text-primary-foreground aria-pressed:hover:bg-primary/80">
                {day.letter}
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
        </div>
      ) : null}
      <div className="flex flex-col gap-1.5">
        <span className="inline-flex items-center gap-1 text-xs font-medium text-muted-foreground">
          Next date from <InfoTip text={BASE_HELP} size={12} />
        </span>
        <ToggleGroup
          aria-label="Next date from"
          variant="outline"
          size="sm"
          spacing={0}
          value={[draft.base]}
          onValueChange={(next: string[]) => {
            const base = next.at(-1) as TaskRecurrenceBase | undefined
            if (base) update({ base })
          }}
        >
          <ToggleGroupItem value="due" className="aria-pressed:bg-primary aria-pressed:text-primary-foreground aria-pressed:hover:bg-primary/80">Due date</ToggleGroupItem>
          <ToggleGroupItem value="completion" className="aria-pressed:bg-primary aria-pressed:text-primary-foreground aria-pressed:hover:bg-primary/80">Completion date</ToggleGroupItem>
        </ToggleGroup>
      </div>
      <div className="flex items-center justify-between gap-2">
        {value ? (
          <Button type="button" variant="ghost" size="sm" className="-ml-2 text-destructive hover:text-destructive" onClick={onStop}>
            Stop repeating
          </Button>
        ) : <span />}
        <Button type="submit" size="sm">{value ? 'Save' : 'Repeat'}</Button>
      </div>
    </form>
  )
}
