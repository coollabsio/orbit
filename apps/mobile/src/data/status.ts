/**
 * The signed-in user's own status, as the web reads and writes it (`features/realtime/useOwnStatus.ts`,
 * `StatusMenuItems.tsx`, `CustomStatusDialog.tsx`). No React.
 */
import type { Presence, UserStatus } from '@orbit/api/generated/types.gen'

export type { Presence, UserStatus }

export interface OwnStatus {
  /** What the user chose. `invisible` shows as such to the user only. */
  presence: Presence
  emoji: string | null
  text: string | null
  /** Set while the custom status has an end that has not come. */
  expiresAt: string | null
}

export const PRESENCE_OPTIONS: readonly { value: Presence; label: string; description?: string }[] = [
  { value: 'online', label: 'Online' },
  { value: 'idle', label: 'Idle' },
  { value: 'dnd', label: 'Do not disturb', description: 'No notifications' },
  { value: 'invisible', label: 'Invisible', description: 'You appear offline' },
]

/** An unreadable expiry counts as passed; no expiry never passes. */
const expiryOf = (expiresAt: string | null) => (expiresAt === null ? Number.POSITIVE_INFINITY : Date.parse(expiresAt))

/** The chosen status without a custom status that has ended: what to show, and what a new request may send. */
export function currentStatus(status: UserStatus | undefined, now = Date.now()): OwnStatus {
  const expiresAt = status?.expires_at ?? null
  const live = expiryOf(expiresAt) > now
  const emoji = live ? status?.emoji || null : null
  const text = live ? status?.text || null : null
  const set = emoji !== null || text !== null
  return { presence: status?.presence ?? 'online', emoji, text, expiresAt: set ? expiresAt : null }
}

export const hasCustomStatus = (status: OwnStatus) => status.emoji !== null || status.text !== null

/** Another presence, keeping the custom status. */
export const withPresence = (status: OwnStatus, presence: Presence): UserStatus => ({ presence, emoji: status.emoji, text: status.text, expires_at: status.expiresAt })

/** The presence without a custom status. */
export const withoutCustomStatus = (status: OwnStatus): UserStatus => ({ presence: status.presence, emoji: null, text: null, expires_at: null })

export type ClearAfter = 'keep' | 'never' | '30m' | '1h' | '4h' | 'today'

const CLEAR_AFTER_OPTIONS: readonly { value: ClearAfter; label: string }[] = [
  { value: 'never', label: "Don't clear" },
  { value: '30m', label: '30 minutes' },
  { value: '1h', label: '1 hour' },
  { value: '4h', label: '4 hours' },
  { value: 'today', label: 'Today' },
]

const MINUTES: Partial<Record<ClearAfter, number>> = { '30m': 30, '1h': 60, '4h': 240 }

/** The "Clear after" choices; a status that already has an end offers to keep it first (the default). */
export function clearAfterOptions(expiresAt: string | null, formatDate: (iso: string) => string): { value: ClearAfter; label: string }[] {
  return expiresAt ? [{ value: 'keep', label: `Keep (${formatDate(expiresAt)})` }, ...CLEAR_AFTER_OPTIONS] : [...CLEAR_AFTER_OPTIONS]
}

/** When the custom status ends; "Today" is the end of the local day. `keep` leaves the end that is set. */
export function clearAfterTime(choice: ClearAfter, current: string | null, now = new Date()): string | null {
  if (choice === 'keep') return current
  if (choice === 'never') return null
  if (choice === 'today') return new Date(now.getFullYear(), now.getMonth(), now.getDate(), 23, 59, 59, 999).toISOString()
  return new Date(now.getTime() + (MINUTES[choice] ?? 0) * 60_000).toISOString()
}

/** What Save sends: the trimmed text, and no end for an empty custom status. The presence stays. */
export function customStatusBody(status: OwnStatus, emoji: string | null, text: string, clearAfter: ClearAfter, now = new Date()): UserStatus {
  const trimmed = text.trim() || null
  const empty = emoji === null && trimmed === null
  return { presence: status.presence, emoji, text: trimmed, expires_at: empty ? null : clearAfterTime(clearAfter, status.expiresAt, now) }
}

/** Save needs a custom status to set, or one to clear. */
export const canSaveCustomStatus = (status: OwnStatus, emoji: string | null, text: string) => hasCustomStatus(status) || emoji !== null || text.trim() !== ''
