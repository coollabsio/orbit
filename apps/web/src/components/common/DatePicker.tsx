import { useState } from 'react'
import type { DateRange } from 'react-day-picker'
import { Clock } from 'reicon-react'
import { Button } from '@/components/ui/button'
import { Calendar } from '@/components/ui/calendar'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'

const HALF_HOURS = Array.from({ length: 48 }, (_, i) => {
  const h = String(Math.floor(i / 2)).padStart(2, '0')
  return `${h}:${i % 2 ? '30' : '00'}`
})

function timeOf(date: Date) {
  return `${String(date.getHours()).padStart(2, '0')}:${String(date.getMinutes()).padStart(2, '0')}`
}

function withTime(day: Date, time: string) {
  const [h, m] = time.split(':').map(Number)
  const next = new Date(day)
  next.setHours(h, m, 0, 0)
  return next
}

function startOfLocalDay(day: Date) {
  const next = new Date(day)
  next.setHours(0, 0, 0, 0)
  return next
}

function currentWeek(): DateRange & { from: Date; to: Date } {
  const today = new Date()
  const mondayOffset = (today.getDay() + 6) % 7
  const from = startOfLocalDay(today)
  from.setDate(from.getDate() - mondayOffset)
  const to = new Date(from)
  to.setDate(to.getDate() + 6)
  return { from, to }
}

interface DatePickerProps {
  /** ISO range start, or null for a single due date. */
  startValue: string | null
  /** ISO range end or single due date. */
  value: string | null
  onClear: () => void
  onDone: (value: { start: string | null; end: string }) => void
}

/** Calendar + time picker that supports one date, a date range, and a current-week shortcut. */
export function DatePicker({ startValue, value, onClear, onDone }: DatePickerProps) {
  const initialEnd = value ? new Date(value) : undefined
  const [range, setRange] = useState<DateRange | undefined>(() => initialEnd
    ? { from: startValue ? new Date(startValue) : initialEnd, to: startValue ? initialEnd : undefined }
    : undefined)
  const [month, setMonth] = useState(() => range?.from ?? new Date())
  const [time, setTime] = useState(() => initialEnd ? timeOf(initialEnd) : '09:00')
  const timeOptions = HALF_HOURS.includes(time) ? HALF_HOURS : [...HALF_HOURS, time].sort()

  const selectThisWeek = () => {
    const week = currentWeek()
    setRange(week)
    setMonth(week.from)
  }
  const save = () => {
    if (!range?.from) return
    const endDay = range.to ?? range.from
    onDone({
      start: range.to ? startOfLocalDay(range.from).toISOString() : null,
      end: withTime(endDay, time).toISOString(),
    })
  }

  return (
    <div className="flex flex-col gap-3 p-3">
      <Button type="button" variant="secondary" className="w-full" onClick={selectThisWeek}>This week</Button>
      <Calendar mode="range" selected={range} onSelect={setRange} month={month} onMonthChange={setMonth} />
      <div className="flex items-center gap-2">
        <Clock className="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
        <Select value={time} onValueChange={(next) => next && setTime(next)}>
          <SelectTrigger aria-label="Time">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {timeOptions.map((option) => <SelectItem key={option} value={option}>{option}</SelectItem>)}
          </SelectContent>
        </Select>
      </div>
      <div className="flex justify-between">
        <Button variant="ghost" onClick={onClear} disabled={!value}>Clear</Button>
        <Button onClick={save} disabled={!range?.from}>Done</Button>
      </div>
    </div>
  )
}
