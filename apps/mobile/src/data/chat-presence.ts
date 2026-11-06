/**
 * Who the other members see as online, from the live socket, as the web keeps it (`features/realtime/presence.ts`):
 * the `hello` frame lists everybody who does not show as offline, and each `presence` event changes one member.
 * Nobody is in the map while the socket is closed, so everybody shows as offline.
 */
import { useSyncExternalStore } from 'react'

export type ShownPresence = 'online' | 'idle' | 'dnd' | 'offline'

export interface PresenceEntry {
  status: Exclude<ShownPresence, 'offline'>
  emoji: string | null
  text: string | null
  /** RFC 3339. Ends the custom status only; the presence stays. */
  expiresAt: string | null
}

export type PresenceMap = ReadonlyMap<string, PresenceEntry>

interface WirePresence {
  user_id?: unknown
  status?: unknown
  emoji?: unknown
  text?: unknown
  expires_at?: unknown
}

const SHOWN = new Set(['online', 'idle', 'dnd'])
const listeners = new Set<() => void>()
let presence: PresenceMap = new Map()

function commit(next: PresenceMap) {
  presence = next
  for (const listener of listeners) listener()
}

const text = (value: unknown) => (typeof value === 'string' && value !== '' ? value : null)

/** `null` is offline (or garbage). */
function toEntry(wire: WirePresence): PresenceEntry | null {
  if (typeof wire.status !== 'string' || !SHOWN.has(wire.status)) return null
  return { status: wire.status as PresenceEntry['status'], emoji: text(wire.emoji), text: text(wire.text), expiresAt: text(wire.expires_at) }
}

/** Reads a raw frame of `/api/v1/workspaces/{id}/live`; frames without presence change nothing. */
export function readPresenceFrame(raw: unknown) {
  if (typeof raw !== 'string') return
  let frame: { type?: unknown; topic?: unknown; presence?: unknown; event?: unknown }
  try {
    frame = JSON.parse(raw)
  } catch {
    return
  }
  if (frame === null || typeof frame !== 'object') return
  if (frame.type === 'hello' && Array.isArray(frame.presence)) {
    const next = new Map<string, PresenceEntry>()
    for (const wire of frame.presence as WirePresence[]) {
      const entry = wire && typeof wire.user_id === 'string' ? toEntry(wire) : null
      if (entry) next.set(wire.user_id as string, entry)
    }
    return commit(next)
  }
  if (frame.topic !== 'presence' || frame.event === null || typeof frame.event !== 'object') return
  const wire = frame.event as WirePresence & { type?: unknown }
  if (wire.type !== 'presence' || typeof wire.user_id !== 'string') return
  const entry = toEntry(wire)
  if (!entry && !presence.has(wire.user_id)) return
  const next = new Map(presence)
  if (entry) next.set(wire.user_id, entry)
  else next.delete(wire.user_id)
  commit(next)
}

/** The socket closed for good (sign-out, another workspace): nobody is known to be online. */
export function resetPresence() {
  if (presence.size > 0) commit(new Map())
}

const subscribe = (listener: () => void) => {
  listeners.add(listener)
  return () => void listeners.delete(listener)
}
const snapshot = () => presence

export function usePresence(): PresenceMap {
  return useSyncExternalStore(subscribe, snapshot, snapshot)
}

/** What a member shows as, and their custom status while it has not ended. */
export function presenceOf(map: PresenceMap, userId: string | null | undefined, now = Date.now()): { status: ShownPresence; emoji: string | null; text: string | null } {
  const entry = userId ? map.get(userId) : undefined
  if (!entry) return { status: 'offline', emoji: null, text: null }
  const ended = entry.expiresAt !== null && !(Date.parse(entry.expiresAt) > now)
  return { status: entry.status, emoji: ended ? null : entry.emoji, text: ended ? null : entry.text }
}
