import { useSyncExternalStore } from 'react'
import { localeWeekStart, type Weekday } from '@orbit/database/calendar'

const WEEK_START_KEY = 'orbit:calendar-week-start'
const weekStartListeners = new Set<() => void>()

function storedWeekStart(): Weekday | null {
  try {
    const stored = localStorage.getItem(WEEK_START_KEY)
    const day = Number(stored)
    return stored !== null && Number.isInteger(day) && day >= 0 && day <= 6 ? (day as Weekday) : null
  } catch {
    return null
  }
}

/** The first day of the week calendars show: this browser's choice, else the viewer's locale (spec P3 13.2). */
export function useWeekStart(locale?: string): Weekday {
  const stored = useSyncExternalStore(
    (listener) => {
      weekStartListeners.add(listener)
      return () => weekStartListeners.delete(listener)
    },
    storedWeekStart,
  )
  return stored ?? localeWeekStart(locale)
}

/** Keeps the first day of the week for every calendar in this browser. */
export function setWeekStart(day: Weekday) {
  try {
    localStorage.setItem(WEEK_START_KEY, String(day))
  } catch {
    // private mode: calendars keep the locale's week start
  }
  for (const listener of weekStartListeners) listener()
}
