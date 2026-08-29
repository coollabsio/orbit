import { useState } from 'react'
import { ChevronLeft, ChevronRight, Clock } from 'reicon-react'
import { Listbox } from './Listbox'

const WEEKDAYS = ['Mo', 'Tu', 'We', 'Th', 'Fr', 'Sa', 'Su']

const HALF_HOURS = Array.from({ length: 48 }, (_, i) => {
  const h = String(Math.floor(i / 2)).padStart(2, '0')
  return `${h}:${i % 2 ? '30' : '00'}`
})

function timeOf(date: Date) {
  return `${String(date.getHours()).padStart(2, '0')}:${String(date.getMinutes()).padStart(2, '0')}`
}

function sameDay(a: Date, b: Date) {
  return a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate()
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

/** Calendar + time picker for a popover (month grid, ‹ › navigation, half-hour time list). */
export function DatePicker({ value, onChange, onClear, onDone }: DatePickerProps) {
  const selected = value ? new Date(value) : null
  const [view, setView] = useState(() => {
    const base = selected ?? new Date()
    return new Date(base.getFullYear(), base.getMonth(), 1)
  })
  // time used for the next picked day when nothing is selected yet
  const [timeDraft, setTimeDraft] = useState('09:00')
  const time = selected ? timeOf(selected) : timeDraft
  const timeOptions = (HALF_HOURS.includes(time) ? HALF_HOURS : [...HALF_HOURS, time].sort()).map((t) => ({ value: t, label: t }))

  const today = new Date()
  const monthLabel = view.toLocaleDateString(undefined, { month: 'long', year: 'numeric' })
  // Monday-first grid, 6 rows
  const offset = (view.getDay() + 6) % 7
  const cells = Array.from({ length: 42 }, (_, i) => new Date(view.getFullYear(), view.getMonth(), i - offset + 1))

  const shiftMonth = (delta: number) => setView(new Date(view.getFullYear(), view.getMonth() + delta, 1))

  const pickDay = (day: Date) => onChange(withTime(day, time).toISOString())

  const pickTime = (next: string) => {
    if (selected) onChange(withTime(selected, next).toISOString())
    else setTimeDraft(next)
  }

  return (
    <div className="datepicker">
      <div className="datepicker-head">
        <button type="button" className="icon-button" aria-label="Previous month" onClick={() => shiftMonth(-1)}>
          <ChevronLeft size={14} />
        </button>
        <span className="datepicker-title">{monthLabel}</span>
        <button type="button" className="icon-button" aria-label="Next month" onClick={() => shiftMonth(1)}>
          <ChevronRight size={14} />
        </button>
      </div>
      <div className="datepicker-grid" role="grid">
        {WEEKDAYS.map((day) => (
          <span key={day} className="datepicker-weekday">
            {day}
          </span>
        ))}
        {cells.map((day) => (
          <button
            key={day.toISOString()}
            type="button"
            className="datepicker-day"
            data-muted={day.getMonth() !== view.getMonth() || undefined}
            data-today={sameDay(day, today) || undefined}
            data-selected={(selected && sameDay(day, selected)) || undefined}
            aria-label={day.toDateString()}
            aria-pressed={(selected && sameDay(day, selected)) || undefined}
            onClick={() => pickDay(day)}
          >
            {day.getDate()}
          </button>
        ))}
      </div>
      <div className="datepicker-time">
        <Clock size={14} />
        <Listbox value={time} options={timeOptions} onChange={pickTime} aria-label="Time" className="datepicker-time-list" />
      </div>
      <div className="datepicker-footer">
        <button type="button" className="button button-ghost" onClick={onClear} disabled={!value}>
          Clear
        </button>
        <button type="button" className="button button-primary" onClick={onDone}>
          Done
        </button>
      </div>
    </div>
  )
}
