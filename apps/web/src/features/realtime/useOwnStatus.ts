import { useEffect, useState } from 'react'
import type { Presence, UserStatus } from '@/api/generated/types.gen'
import { useCurrentUser } from '@/features/auth/api'
import { customStatusOf } from './presence'

export interface OwnStatus {
  /** What the user chose. `invisible` shows as such to the user only. */
  presence: Presence
  emoji: string | null
  text: string | null
  /** Set while the custom status has an end that has not come. */
  expiresAt: string | null
}

/** The chosen status without a custom status that has ended: what to show, and what a new request may send. */
export function currentStatus(status: UserStatus | undefined, now = Date.now()): OwnStatus {
  const expiresAt = status?.expires_at ?? null
  const custom = customStatusOf({ emoji: status?.emoji, text: status?.text, expiresAt }, now)
  const set = custom.emoji !== null || custom.text !== null
  return { presence: status?.presence ?? 'online', ...custom, expiresAt: set ? expiresAt : null }
}

/**
 * The signed-in user's own status, from the current user and never from the presence map (there an invisible user is
 * offline). The custom status goes away when its time has come.
 */
export function useOwnStatus(): OwnStatus {
  const status = useCurrentUser().data?.status
  const expiresAt = status?.expires_at ?? null
  // The clock of this hook moves when the custom status ends, so the component renders again at that moment.
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    if (expiresAt === null) return
    const delay = Math.max(0, Date.parse(expiresAt) - Date.now())
    if (!(delay <= 2 ** 31 - 1)) return
    const timer = setTimeout(() => setNow(Date.now()), delay + 50)
    return () => clearTimeout(timer)
  }, [expiresAt])
  return currentStatus(status, now)
}
