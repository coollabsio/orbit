import { useState } from 'react'
import type { DateRange } from 'react-day-picker'
import { Clock } from 'reicon-react'
import { Button } from '@/components/ui/button'
import { Calendar } from '@/components/ui/calendar'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { dueRangeOf, isSameDay, sameDue, timeOf, weekFromNow, type DueRange } from '@/lib/dueRange'

export type { DueRange } from '@/lib/dueRange'

const HALF_HOURS = Array.from({ length: 48 }, (_, i) => {
  const h = String(Math.floor(i / 2)).padStart(2, '0')
  return `${h}:${i % 2 ? '30' : '00'}`
})

interface DatePickerProps {
  /** ISO range start, or null for a single due date. */
  startValue: string | null
  /** ISO range end or single due date. */
  value: string | null
  /** Enables Clear; defaults to whether `value` is set. */
  clearable?: boolean
  onClear: () => void
  /** Every pick commits: a day, a range's second day, a time, or a week shortcut. */
  onChange: (value: DueRange) => void
}

/**
 * Calendar + time picker that supports one date, a date range, and this/next-week shortcuts. There is no Done step:
 * each change calls `onChange` at once and the picker stays open, so a first click sets a single date, a second one
 * extends it to a range and a third starts over.
 */
export function DatePicker({ startValue, value, clearable = !!value, onClear, onChange }: DatePickerProps) {
  const initialEnd = value ? new Date(value) : undefined
  const [range, setRange] = useState<DateRange | undefined>(() => initialEnd
    ? { from: startValue ? new Date(startValue) : initialEnd, to: startValue ? initialEnd : undefined }
    : undefined)
  const [month, setMonth] = useState(() => range?.from ?? new Date())
  const [time, setTime] = useState(() => initialEnd ? timeOf(initialEnd) : '09:00')
  // the value last sent (or loaded), so re-picking the same day sends nothing
  const [committed, setCommitted] = useState<DueRange | null>(() => value ? { start: startValue, end: value } : null)
  const timeOptions = HALF_HOURS.includes(time) ? HALF_HOURS : [...HALF_HOURS, time].sort()

  const commit = (nextRange: DateRange | undefined, nextTime: string) => {
    // a one-day range is kept open-ended, so the next click extends it instead of starting over
    const normalized = nextRange?.from && nextRange.to && isSameDay(nextRange.from, nextRange.to) ? { from: nextRange.from, to: undefined } : nextRange
    setRange(normalized)
    const due = dueRangeOf(normalized, nextTime)
    if (!due || sameDue(due, committed)) return
    setCommitted(due)
    onChange(due)
  }
  const selectWeek = (weeksAhead: number) => {
    const week = weekFromNow(weeksAhead)
    setMonth(week.from)
    commit(week, time)
  }
  const clear = () => {
    setRange(undefined)
    setCommitted(null)
    onClear()
  }

  return (
    <div className="flex max-h-(--available-height) flex-col gap-3 overflow-y-auto overscroll-contain p-3">
      <div className="grid grid-cols-2 gap-2">
        <Button type="button" variant="secondary" onClick={() => selectWeek(0)}>This week</Button>
        <Button type="button" variant="secondary" onClick={() => selectWeek(1)}>Next week</Button>
      </div>
      {/* resetOnSelect: a click on a full range starts a new single date; clicking the selected day again (undefined) keeps it */}
      <Calendar
        mode="range"
        resetOnSelect
        selected={range}
        onSelect={(next) => { if (next) commit(next, time) }}
        month={month}
        onMonthChange={setMonth}
      />
      <div className="flex items-center gap-2">
        <Clock className="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
        <Select
          value={time}
          onValueChange={(next) => {
            if (!next) return
            setTime(next)
            commit(range, next)
          }}
        >
          <SelectTrigger aria-label="Time" className="flex-1">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {timeOptions.map((option) => <SelectItem key={option} value={option}>{option}</SelectItem>)}
          </SelectContent>
        </Select>
        <Button variant="ghost" onClick={clear} disabled={!clearable && !committed}>Clear</Button>
      </div>
    </div>
  )
}
