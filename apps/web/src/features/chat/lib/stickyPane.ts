import { useSyncExternalStore } from 'react'
import type { ChatPane } from '../chatRoutes'

/**
 * The right pane the user left open (members, pins, files or threads). It stays open across conversations and
 * reloads until the user closes it. A thread or a search shows in its place and gives the pane back when it closes.
 */
const KEY = 'orbit.chat.pane'
const PANES: readonly string[] = ['members', 'pins', 'files', 'threads']
const listeners = new Set<() => void>()

export function getStickyPane(): ChatPane | null {
  try {
    const value = globalThis.localStorage?.getItem(KEY)
    return value && PANES.includes(value) ? (value as ChatPane) : null
  } catch {
    return null
  }
}

export function setStickyPane(pane: ChatPane | null) {
  try {
    if (pane) globalThis.localStorage?.setItem(KEY, pane)
    else globalThis.localStorage?.removeItem(KEY)
  } catch {
    // Not being able to remember the pane is harmless.
  }
  for (const listener of [...listeners]) listener()
}

function subscribe(listener: () => void) {
  listeners.add(listener)
  return () => listeners.delete(listener)
}

export function useStickyPane(): ChatPane | null {
  return useSyncExternalStore(subscribe, getStickyPane)
}
