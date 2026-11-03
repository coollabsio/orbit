import { useLayoutEffect, type RefObject } from 'react'
import { useLocation } from 'react-router'

const positions = new Map<string, { top: number; left: number }>()

/**
 * Remembers where a scroller was on this page (path and query) and scrolls back there when the page shows again,
 * for example back on a list after a task or another app was open. `active` is false while the scroller does not
 * show its content yet (loading), so the position is restored once it does. Returns the saved top offset: a
 * TanStack virtualizer needs it as `initialOffset`, as it scrolls to that offset when it attaches.
 */
export function useScrollMemory(ref: RefObject<HTMLElement | null>, name: string, active = true) {
  const { pathname, search } = useLocation()
  const key = `${name}:${pathname}${search}`
  useLayoutEffect(() => {
    const element = ref.current
    if (!element || !active) return
    const saved = positions.get(key)
    if (saved) {
      element.scrollTop = saved.top
      element.scrollLeft = saved.left
    }
    const save = () => positions.set(key, { top: element.scrollTop, left: element.scrollLeft })
    element.addEventListener('scroll', save, { passive: true })
    return () => element.removeEventListener('scroll', save)
  }, [active, key, ref])
  return () => positions.get(key)?.top ?? 0
}
