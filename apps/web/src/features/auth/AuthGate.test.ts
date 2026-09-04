import { describe, expect, test } from 'bun:test'
import { authGateState } from './authState'

describe('AuthGate state', () => {
  test('sends an uninitialized installation to setup', () => {
    expect(authGateState({ setupComplete: false, user: undefined, userStatus: 'pending' })).toBe(
      'setup',
    )
  })

  test('sends an anonymous user to login', () => {
    expect(authGateState({ setupComplete: true, user: null, userStatus: 'success' })).toBe('login')
  })

  test('admits an authenticated user', () => {
    expect(
      authGateState({
        setupComplete: true,
        user: { id: 'user-one', email: 'owner@orbit.test', display_name: 'Owner' },
        userStatus: 'success',
      }),
    ).toBe('authenticated')
  })
})
