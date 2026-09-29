import { useState } from 'react'

/**
 * Counts opens. A dialog stays mounted through its exit animation and can be reopened during it, so its form keys on
 * this to start fresh on every open instead of keeping the last open's values.
 */
export function useOpenKey(open: boolean): number {
  const [key, setKey] = useState(0)
  const [wasOpen, setWasOpen] = useState(open)
  if (open !== wasOpen) {
    setWasOpen(open)
    if (open) setKey(key + 1)
  }
  return key
}
