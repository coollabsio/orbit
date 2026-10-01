import { useCallback, useEffect, useState, useSyncExternalStore } from 'react'

/** The app's phone breakpoint, the same as the `max-[899px]:` classes. */
const PHONE = '(max-width: 899px)'

function subscribePhone(listener: () => void) {
  if (typeof window.matchMedia !== 'function') return () => {}
  const media = window.matchMedia(PHONE)
  media.addEventListener('change', listener)
  return () => media.removeEventListener('change', listener)
}

/** The window is phone-sized: `/chat` is the list, every other chat screen is a full screen, panes are sheets. */
export function useIsPhone(): boolean {
  return useSyncExternalStore(subscribePhone, () => typeof window.matchMedia === 'function' && window.matchMedia(PHONE).matches)
}

/** The live width of an element (`null` until it is measured, before the first paint). Pass the first value as its `ref`. */
export function useElementWidth(): [(node: HTMLElement | null) => (() => void) | undefined, number | null] {
  const [width, setWidth] = useState<number | null>(null)
  const ref = useCallback((node: HTMLElement | null) => {
    if (!node) return undefined
    setWidth(node.getBoundingClientRect().width)
    if (typeof ResizeObserver === 'undefined') return undefined
    const observer = new ResizeObserver(([entry]) => setWidth(entry.contentRect.width))
    observer.observe(node)
    return () => observer.disconnect()
  }, [])
  return [ref, width]
}

/** What the user last used. A right pane fades in only when the pointer opened it; a key opens it at once. */
let lastInput: 'pointer' | 'keyboard' = 'keyboard'

export const openedByPointer = () => lastInput === 'pointer'

/** Mount once in the chat page: keeps `openedByPointer` current. */
export function useInputModality() {
  useEffect(() => {
    const onPointer = () => {
      lastInput = 'pointer'
    }
    const onKey = () => {
      lastInput = 'keyboard'
    }
    window.addEventListener('pointerdown', onPointer, true)
    window.addEventListener('keydown', onKey, true)
    return () => {
      window.removeEventListener('pointerdown', onPointer, true)
      window.removeEventListener('keydown', onKey, true)
    }
  }, [])
}
