import { useState } from 'react'
import { ZOOM_PRESETS, clampZoom } from './timelineLib'

const KEY = 'orbit:timeline_zoom'

/** Pixels per day, remembered across sessions. */
export function useTimelineZoom(): [number, (px: number) => void] {
  const [px, setPx] = useState(() => {
    const saved = Number(localStorage.getItem(KEY))
    return saved > 0 ? clampZoom(saved) : ZOOM_PRESETS.month
  })
  const update = (next: number) => {
    const value = clampZoom(next)
    localStorage.setItem(KEY, String(value))
    setPx(value)
  }
  return [px, update]
}
