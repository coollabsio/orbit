import { useCallback, useEffect, useRef, useState } from 'react'

/** What a hover preview shows and where: `rect` is the hovered piece of text (one line of it). */
export interface HoverPreview<T> {
  key: string
  value: T
  rect: DOMRect
}

/** The pointer rests this long on a reference before its preview opens. */
const OPEN_DELAY = 350

/**
 * A preview that opens when the pointer rests on something, and closes at once when it leaves. Moving from one
 * reference to the next while a preview is open swaps it without the wait, as tooltips do.
 */
export function useHoverPreview<T>() {
  const [shown, setShown] = useState<HoverPreview<T> | null>(null)
  const shownKey = useRef<string | null>(null)
  const pendingKey = useRef<string | null>(null)
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)

  const hide = useCallback(() => {
    clearTimeout(timer.current)
    pendingKey.current = null
    if (shownKey.current === null) return
    shownKey.current = null
    setShown(null)
  }, [])

  const show = useCallback((key: string, value: T, rect: DOMRect) => {
    if (shownKey.current === key || pendingKey.current === key) return
    clearTimeout(timer.current)
    pendingKey.current = key
    timer.current = setTimeout(() => {
      pendingKey.current = null
      shownKey.current = key
      setShown({ key, value, rect })
    }, shownKey.current === null ? OPEN_DELAY : 0)
  }, [])

  useEffect(() => () => clearTimeout(timer.current), [])

  return { shown, show, hide }
}

/** The rect of `element` (one per line it wraps over) that holds the point, if any. */
export function rectAt(element: Element, x: number, y: number): DOMRect | null {
  for (const rect of element.getClientRects()) {
    if (x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom) return rect
  }
  return null
}
