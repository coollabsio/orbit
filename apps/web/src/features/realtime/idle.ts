import { useSyncExternalStore } from 'react'

/** No input for this long and the tab counts as idle. */
export const IDLE_AFTER_MS = 10 * 60_000

const INPUT_EVENTS = ['pointerdown', 'pointermove', 'keydown', 'wheel', 'touchstart'] as const

/**
 * Reports `true` when there was no input on the window for `idleMs`, and `false` on the next input. Only a change is
 * reported. An input only notes its time; the one timer looks at that time when it fires, so a stream of
 * `pointermove` events does not set a timer for each of them. Returns the function that stops watching.
 */
export function watchIdle(report: (idle: boolean) => void, idleMs = IDLE_AFTER_MS): () => void {
  let idle = false
  let lastInput = Date.now()
  let timer: ReturnType<typeof setTimeout> | undefined

  const check = () => {
    const rest = lastInput + idleMs - Date.now()
    if (rest > 0) {
      timer = setTimeout(check, rest)
      return
    }
    idle = true
    report(true)
  }
  const onInput = () => {
    lastInput = Date.now()
    if (!idle) return
    idle = false
    report(false)
    timer = setTimeout(check, idleMs)
  }

  timer = setTimeout(check, idleMs)
  for (const name of INPUT_EVENTS) window.addEventListener(name, onInput, { passive: true })
  return () => {
    clearTimeout(timer)
    for (const name of INPUT_EVENTS) window.removeEventListener(name, onInput)
    if (idle) report(false)
  }
}

/** The choices for how long a window may be out of focus before the user counts as not looking at it, in minutes. */
export const AWAY_MINUTES = [1, 2, 3, 5, 10] as const
export const DEFAULT_AWAY_MINUTES = 1
const AWAY_KEY = 'orbit:away-minutes'

/** The stored choice; anything that is not one of the choices becomes the default. */
export function parseAwayMinutes(raw: string | null): number {
  const minutes = Number(raw)
  return (AWAY_MINUTES as readonly number[]).includes(minutes) ? minutes : DEFAULT_AWAY_MINUTES
}

const awayListeners = new Set<() => void>()

/** The setting belongs to the device (it is about this window), so it is in `localStorage`. */
export function getAwayMinutes(): number {
  try {
    return parseAwayMinutes(window.localStorage.getItem(AWAY_KEY))
  } catch {
    return DEFAULT_AWAY_MINUTES
  }
}

export function setAwayMinutes(minutes: number) {
  try {
    window.localStorage.setItem(AWAY_KEY, String(parseAwayMinutes(String(minutes))))
  } catch {
    // Storage is full or blocked: the default stays.
  }
  for (const listener of [...awayListeners]) listener()
}

function subscribeAway(listener: () => void) {
  awayListeners.add(listener)
  // Another tab changed the setting.
  const onStorage = (event: StorageEvent) => {
    if (event.key === AWAY_KEY || event.key === null) listener()
  }
  window.addEventListener('storage', onStorage)
  return () => {
    awayListeners.delete(listener)
    window.removeEventListener('storage', onStorage)
  }
}

export function useAwayMinutes(): [number, (minutes: number) => void] {
  return [useSyncExternalStore(subscribeAway, getAwayMinutes, () => DEFAULT_AWAY_MINUTES), setAwayMinutes]
}

/**
 * Reports `true` when the window has been out of focus (or the tab hidden) for `awayMs`, and `false` when it has the
 * focus again. Only a change is reported. Returns the function that stops watching.
 */
export function watchAway(report: (away: boolean) => void, awayMs = DEFAULT_AWAY_MINUTES * 60_000): () => void {
  let away = false
  let timer: ReturnType<typeof setTimeout> | undefined

  const left = () => {
    if (away || timer !== undefined) return
    timer = setTimeout(() => {
      timer = undefined
      away = true
      report(true)
    }, awayMs)
  }
  const back = () => {
    clearTimeout(timer)
    timer = undefined
    if (!away) return
    away = false
    report(false)
  }
  const onVisibility = () => (document.visibilityState === 'hidden' ? left() : document.hasFocus() ? back() : undefined)

  if (!document.hasFocus()) left()
  window.addEventListener('blur', left)
  window.addEventListener('focus', back)
  document.addEventListener('visibilitychange', onVisibility)
  return () => {
    clearTimeout(timer)
    window.removeEventListener('blur', left)
    window.removeEventListener('focus', back)
    document.removeEventListener('visibilitychange', onVisibility)
    if (away) report(false)
  }
}
