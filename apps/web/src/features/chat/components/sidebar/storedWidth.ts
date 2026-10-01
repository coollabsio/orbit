import { useState } from 'react'

export interface WidthRange {
  min: number
  max: number
  initial: number
}

export const clampWidth = (width: number, range: WidthRange) => Math.round(Math.min(range.max, Math.max(range.min, width)))

/** A pane width the user can change; it is remembered in `localStorage` under `key`. */
export function useStoredWidth(key: string, range: WidthRange): [number, (width: number) => void] {
  const [width, setWidth] = useState(() => {
    const stored = Number(window.localStorage.getItem(key))
    return Number.isFinite(stored) && stored > 0 ? clampWidth(stored, range) : range.initial
  })
  const resize = (next: number) => {
    const clamped = clampWidth(next, range)
    setWidth(clamped)
    window.localStorage.setItem(key, String(clamped))
  }
  return [width, resize]
}
