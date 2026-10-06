import type { AuthUserResponse } from '@/api/generated/types.gen'

interface AuthGateInput {
  /** `undefined` until the setup status has loaded once. */
  setupComplete: boolean | undefined
  /** `null` is signed out (a 401); `undefined` until the user has loaded once. */
  user: AuthUserResponse | null | undefined
  /** A load that does not work (`firstLoadFailed`). A refetch that lost the connection, with data from before, is not this. */
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
  // A refetch the server refused (a 409 after a deploy) is `failed` even with a user from before; a lost connection is
  // not, so then what the gate knows keeps the app on screen.
  if (input.failed) return 'unavailable'
  if (input.setupComplete === false) return 'setup'
  if (input.setupComplete && input.user === null) return 'login'
  if (input.setupComplete && input.user) return 'authenticated'
  return 'loading'
}
