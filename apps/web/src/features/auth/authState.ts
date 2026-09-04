import type { AuthUserResponse } from '../../api/generated/types.gen'

type QueryStatus = 'pending' | 'error' | 'success'

interface AuthGateInput {
  setupComplete: boolean | undefined
  user: AuthUserResponse | null | undefined
  userStatus: QueryStatus
}

export type AuthGateState = 'loading' | 'setup' | 'login' | 'authenticated'

export function authGateState(input: AuthGateInput): AuthGateState {
  if (input.setupComplete === undefined) return 'loading'
  if (!input.setupComplete) return 'setup'
  if (input.userStatus === 'pending') return 'loading'
  if (!input.user) return 'login'
  return 'authenticated'
}
