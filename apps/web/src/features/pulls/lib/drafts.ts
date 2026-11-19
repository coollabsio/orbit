import { useEffect, useRef, useState } from 'react'

export { draftKey, replyDraftKey } from '@orbit/api/github'

function read(key: string): string {
  try { return localStorage.getItem(key) ?? '' } catch { return '' }
}

/** A comment draft that outlives the page: written 300 ms after typing stops, removed by `clear`. */
export function useDraft(key: string): [string, (value: string) => void, () => void] {
  const [value, setValue] = useState(() => read(key))
  // The same component can be reused for another line (the diff reuses annotation slots): read that line's draft.
  const [shownKey, setShownKey] = useState(key)
  if (shownKey !== key) {
    setShownKey(key)
    setValue(read(key))
  }
  const timer = useRef<number | undefined>(undefined)
  const pending = useRef<string | null>(null)
  const flush = () => {
    window.clearTimeout(timer.current)
    if (pending.current === null) return
    try { if (pending.current) localStorage.setItem(key, pending.current); else localStorage.removeItem(key) } catch { /* private mode: the draft lasts this page */ }
    pending.current = null
  }
  // write what is left when the composer closes
  useEffect(() => flush, [key]) // eslint-disable-line react-hooks/exhaustive-deps
  const change = (next: string) => {
    setValue(next)
    pending.current = next
    window.clearTimeout(timer.current)
    timer.current = window.setTimeout(flush, 300)
  }
  const clear = () => {
    window.clearTimeout(timer.current)
    pending.current = null
    setValue('')
    try { localStorage.removeItem(key) } catch { /* nothing kept */ }
  }
  return [value, change, clear]
}
