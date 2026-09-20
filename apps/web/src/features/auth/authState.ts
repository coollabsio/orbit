import type { AuthUserResponse } from '@/api/generated/types.gen'

type QueryStatus = 'pending' | 'error' | 'success'

interface AuthGateInput {
  setupComplete: boolean | undefined
  user: AuthUserResponse | null | undefined
  userStatus: QueryStatus
}

export type AuthGateState = 'loading' | 'setup' | 'login' | 'authenticated'

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
  if (input.setupComplete === undefined) return 'loading'
  if (!input.setupComplete) return 'setup'
  if (input.userStatus === 'pending') return 'loading'
  if (!input.user) return 'login'
  return 'authenticated'
}
