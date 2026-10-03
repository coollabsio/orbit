import { useSyncExternalStore } from 'react'

/** What the other members see of somebody who does not show as offline. */
export type PresenceStatus = 'online' | 'idle' | 'dnd'

export interface PresenceEntry {
  status: PresenceStatus
  emoji: string | null
  text: string | null
  /** RFC 3339. Ends the custom status (emoji and text) only; the presence stays. */
  expiresAt: string | null
}

/** Everybody who does not show as offline. An invisible member is not in it, also not in their own map. */
export type PresenceMap = ReadonlyMap<string, PresenceEntry>

export interface CustomStatus {
  emoji: string | null
  text: string | null
}

export interface MemberPresence extends CustomStatus {
  status: PresenceStatus | 'offline'
}

const OFFLINE: MemberPresence = { status: 'offline', emoji: null, text: null }
const NO_CUSTOM_STATUS: CustomStatus = { emoji: null, text: null }
/** The longest delay a timer takes; a later expiry is looked at again then. */
const MAX_DELAY = 2 ** 31 - 1

const listeners = new Set<() => void>()
/** A new map for each change, so `useSyncExternalStore` sees it. */
let presence: PresenceMap = new Map()
let expiryTimer: ReturnType<typeof setTimeout> | undefined

const expiryOf = (expiresAt: string | null) => (expiresAt === null ? Number.POSITIVE_INFINITY : Date.parse(expiresAt))

/** The custom status while it has not expired; an unreadable expiry counts as passed. */
export function customStatusOf(custom: { emoji?: string | null; text?: string | null; expiresAt: string | null }, now = Date.now()): CustomStatus {
  if (!(expiryOf(custom.expiresAt) > now)) return NO_CUSTOM_STATUS
  return { emoji: custom.emoji || null, text: custom.text || null }
}

/** What a member shows as: `offline` for somebody who is not in the map. */
export function presenceOf(map: PresenceMap, userId: string | null | undefined): MemberPresence {
  const entry = userId ? map.get(userId) : undefined
  return entry ? { status: entry.status, emoji: entry.emoji, text: entry.text } : OFFLINE
}

function withoutExpired(entry: PresenceEntry, now: number): PresenceEntry {
  if (entry.expiresAt === null || expiryOf(entry.expiresAt) > now) return entry
  return { status: entry.status, emoji: null, text: null, expiresAt: null }
}

/** Publishes the map and sets a timer for the custom status that ends next, so it goes away without a signal. */
function commit(next: Map<string, PresenceEntry>) {
  const now = Date.now()
  let soonest = Number.POSITIVE_INFINITY
  for (const [userId, entry] of next) {
    const current = withoutExpired(entry, now)
    if (current !== entry) next.set(userId, current)
    else soonest = Math.min(soonest, expiryOf(entry.expiresAt))
  }
  presence = next
  clearTimeout(expiryTimer)
  expiryTimer = Number.isFinite(soonest) ? setTimeout(() => commit(new Map(presence)), Math.min(soonest - now, MAX_DELAY)) : undefined
  for (const listener of listeners) listener()
}

/** One member changed; `null` is offline. */
export function setPresence(userId: string, entry: PresenceEntry | null) {
  if (!entry && !presence.has(userId)) return
  const next = new Map(presence)
  if (entry) next.set(userId, entry)
  else next.delete(userId)
  commit(next)
}

/** The whole list, from the live socket's first frame. */
export function replacePresence(entries: PresenceMap) {
  commit(new Map(entries))
}

export function resetPresence() {
  commit(new Map())
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener)
  return () => listeners.delete(listener)
}

const snapshot = () => presence

/** Every member who does not show as offline. Custom statuses that have expired are already taken out. */
export function usePresence(): PresenceMap {
  return useSyncExternalStore(subscribe, snapshot, snapshot)
}

/** One member's status (`offline` when absent) and their custom status. */
export function usePresenceOf(userId: string | null | undefined): MemberPresence {
  return presenceOf(usePresence(), userId)
}
