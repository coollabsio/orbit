import { useEffect, useState } from 'react'

/** `value`, updated only after it has stayed the same for `delay` ms. */
export function useDebouncedValue<T>(value: T, delay: number): T {
  const [debounced, setDebounced] = useState(value)
  useEffect(() => {
    const timer = setTimeout(() => setDebounced(value), delay)
    return () => clearTimeout(timer)
  }, [value, delay])
  return debounced
}

/** `pending`, but only once it has lasted a short time, so fast saves do not flash a loading state. */
export function useSlowPending(pending: boolean): boolean {
  return useDebouncedValue(pending, 300) && pending
}
