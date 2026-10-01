import { useLayoutEffect, type RefObject } from 'react'

/** Grows a textarea with its text. The container limits the height; past that the textarea scrolls. */
export function useAutosize(ref: RefObject<HTMLTextAreaElement | null>, value: string) {
  useLayoutEffect(() => {
    const element = ref.current
    if (!element) return
    element.style.height = 'auto'
    // `scrollHeight` leaves out the borders that `height` includes with border-box sizing.
    element.style.height = `${element.scrollHeight + element.offsetHeight - element.clientHeight}px`
  }, [ref, value])
}
