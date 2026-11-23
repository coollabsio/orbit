/** The swipe actions of the mail list, kept on this device: swipes exist only on a touch screen. */
import { useSyncExternalStore } from 'react'

import { DEFAULT_SWIPES, parseSwipes, type SwipeSettings } from '../data/mail-swipe'
import { storage } from '../storage'

const KEY = 'orbit.mailSwipes'
let swipes = DEFAULT_SWIPES
/** Read once, on the first use; a choice made before the read ends wins. */
let loaded = false
let chosen = false
const listeners = new Set<() => void>()

const subscribe = (listener: () => void) => {
  listeners.add(listener)
  if (!loaded) {
    loaded = true
    void storage.get(KEY).then(
      (raw) => {
        if (chosen) return
        swipes = parseSwipes(raw)
        listeners.forEach((each) => each())
      },
      () => {},
    )
  }
  return () => void listeners.delete(listener)
}

export function useMailSwipes(): SwipeSettings {
  return useSyncExternalStore(subscribe, () => swipes)
}

export function setMailSwipes(next: SwipeSettings) {
  chosen = true
  swipes = next
  listeners.forEach((each) => each())
  void storage.set(KEY, JSON.stringify(next)).catch(() => {})
}
