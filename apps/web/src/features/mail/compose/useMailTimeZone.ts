import { useCurrentUser } from '@/features/auth/api'
import { currentTimeZone } from '@/features/profile/profileLib'

const knownZone = (zone: string | null | undefined) => {
  if (!zone) return null
  try {
    new Intl.DateTimeFormat('en-US', { timeZone: zone })
    return zone
  } catch {
    return null
  }
}

/** The user's time zone (profile, else this browser's): send later times are read in it. */
export function useMailTimeZone(): string {
  const profile = useCurrentUser().data?.timezone
  return knownZone(profile) ?? knownZone(currentTimeZone()) ?? 'UTC'
}
