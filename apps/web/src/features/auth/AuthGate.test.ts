import { describe, expect, test } from 'bun:test'
import { authGateState, initialLoginValues, recoveryRequestCopy } from './authState'

test('login prefills seeded credentials only in development', () => {
  expect(initialLoginValues(true)).toEqual({ email: 'test@example.com', password: 'password' })
  expect(initialLoginValues(false)).toEqual({ email: '', password: '' })
})

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

test('public recovery copy directs users to the installation administrator', () => {
  expect(recoveryRequestCopy).toEqual({
    title: 'Contact your administrator',
    detail: 'Your installation administrator can create a one-time password recovery link for you.',
  })
})
