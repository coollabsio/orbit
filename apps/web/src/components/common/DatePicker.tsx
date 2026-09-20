import { useState } from 'react'
import { Clock } from 'lucide-react'
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

interface DatePickerProps {
  /** ISO string or null. */
  value: string | null
  onChange: (iso: string) => void
  onClear: () => void
  onDone: () => void
}

/** Calendar + time picker for a popover (shadcn Calendar month grid + half-hour time list). */
export function DatePicker({ value, onChange, onClear, onDone }: DatePickerProps) {
  const selected = value ? new Date(value) : undefined
  const [month, setMonth] = useState(() => selected ?? new Date())
  // time used for the next picked day when nothing is selected yet
  const [timeDraft, setTimeDraft] = useState('09:00')
  const time = selected ? timeOf(selected) : timeDraft
  const timeOptions = HALF_HOURS.includes(time) ? HALF_HOURS : [...HALF_HOURS, time].sort()

  const pickDay = (day: Date | undefined) => {
    if (day) onChange(withTime(day, time).toISOString())
  }
  const pickTime = (next: string | null) => {
    if (!next) return
    if (selected) onChange(withTime(selected, next).toISOString())
    else setTimeDraft(next)
  }

  return (
    <div className="flex flex-col gap-3 p-3">
      <Calendar mode="single" selected={selected} onSelect={pickDay} month={month} onMonthChange={setMonth} />
      <div className="flex items-center gap-2">
        <Clock className="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
        <Select value={time} onValueChange={pickTime}>
          <SelectTrigger
            aria-label="Time"
            className="w-full rounded-md bg-background px-3 font-normal shadow-xs hover:bg-muted data-[size=default]:h-9 dark:bg-background dark:hover:bg-muted"
          >
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {timeOptions.map((option) => (
              <SelectItem key={option} value={option}>
                {option}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      <div className="flex justify-between">
        <Button variant="ghost" onClick={onClear} disabled={!value}>
          Clear
        </Button>
        <Button onClick={onDone}>Done</Button>
      </div>
    </div>
  )
}
