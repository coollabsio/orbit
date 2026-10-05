import type { AuthUserResponse } from '@/api/generated/types.gen'

interface AuthGateInput {
  /** `undefined` until the setup status has loaded once. */
  setupComplete: boolean | undefined
  /** `null` is signed out (a 401); `undefined` until the user has loaded once. */
  user: AuthUserResponse | null | undefined
  /** A first load that does not work. A failed refetch of data the gate already has is not this. */
  failed: boolean
}

export type AuthGateState = 'loading' | 'unavailable' | 'setup' | 'login' | 'authenticated'

export function initialLoginValues(development: boolean) {
  return development
    ? { email: 'test@example.com', password: 'password' }
    : { email: '', password: '' }
}

export const recoveryRequestCopy = {
  title: 'Contact your administrator',
  detail: 'Your installation administrator can create a one-time password recovery link for you.',
}

export function authGateState(input: AuthGateInput): AuthGateState {
  // What the gate knows decides first: with a user from before, a failed refetch keeps the app on screen.
  if (input.setupComplete === false) return 'setup'
  if (input.setupComplete && input.user === null) return 'login'
  if (input.setupComplete && input.user) return 'authenticated'
  return input.failed ? 'unavailable' : 'loading'
}
