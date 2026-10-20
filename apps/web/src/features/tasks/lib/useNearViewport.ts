import { useCallback, useRef, useState } from 'react'

/**
 * True once the element has come within `margin` of the screen, and true from then on. Long comment threads mount
 * their editors only near the screen (each one is a full editor instance).
 */
export function useNearViewport<T extends Element>(margin = '600px'): [(element: T | null) => void, boolean] {
  const [near, setNear] = useState(() => typeof IntersectionObserver === 'undefined')
  const observer = useRef<IntersectionObserver | null>(null)
  const ref = useCallback(
    (element: T | null) => {
      observer.current?.disconnect()
      observer.current = null
      if (!element || near) return
      observer.current = new IntersectionObserver(
        (entries) => {
          if (!entries.some((entry) => entry.isIntersecting)) return
          setNear(true)
          observer.current?.disconnect()
        },
        { rootMargin: margin },
      )
      observer.current.observe(element)
    },
    [margin, near],
  )
  return [ref, near]
}
