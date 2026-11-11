import { useAuthOptions } from '@/features/auth/api'

/** Mail is on for this Orbit server (Admin → Inbound mail). Off, and until the options load: no mail app, menu items or settings. */
export function useMailEnabled(): boolean {
  return useAuthOptions().data?.mail_enabled === true
}
