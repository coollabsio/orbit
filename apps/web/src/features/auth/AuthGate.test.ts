import { describe, expect, test } from 'bun:test'
import { firstLoadFailed } from '@/lib/connection'
import { authGateState, initialLoginValues, recoveryRequestCopy } from './authState'

test('login prefills seeded credentials only in development', () => {
  expect(initialLoginValues(true)).toEqual({ email: 'test@example.com', password: 'password' })
  expect(initialLoginValues(false)).toEqual({ email: '', password: '' })
})

describe('AuthGate state', () => {
  test('sends an uninitialized installation to setup', () => {
    expect(authGateState({ setupComplete: false, user: undefined, failed: false })).toBe(
      'setup',
    )
  })

  test('sends an anonymous user to login', () => {
    expect(authGateState({ setupComplete: true, user: null, failed: false })).toBe('login')
  })

  test('admits an authenticated user', () => {
    expect(
      authGateState({
        setupComplete: true,
        user: { id: 'user-one', email: 'owner@orbit.test', display_name: 'Owner', installation_admin: true, status: { presence: 'online' } },
        failed: false,
      }),
    ).toBe('authenticated')
  })

  test('a failed refetch keeps a signed-in user in the app', () => {
    const user = { id: 'user-one', email: 'owner@orbit.test', display_name: 'Owner', installation_admin: true, status: { presence: 'online' as const } }
    // The gate has the user from before; both queries now say error (server restart, wifi rejoin).
    const cached = { data: user, isError: true, error: new TypeError('Failed to fetch'), failureCount: 7, failureReason: null }
    const failed = firstLoadFailed({ ...cached, data: { complete: true } }) || firstLoadFailed(cached)
    expect(failed).toBe(false)
    expect(authGateState({ setupComplete: true, user, failed })).toBe('authenticated')
    // A 5xx is a lost connection too.
    expect(firstLoadFailed({ ...cached, error: { status: 503 } })).toBe(false)
    // A real 401 turns the user into `null` and is no error: that still goes to login.
    expect(authGateState({ setupComplete: true, user: null, failed: false })).toBe('login')
  })

  test('a refetch the server refused shows the unavailable message, even with a user from before', () => {
    const user = { id: 'user-one', email: 'owner@orbit.test', display_name: 'Owner', installation_admin: true, status: { presence: 'online' as const } }
    // An old tab after a deploy: 409 `contract_mismatch`. No banner tells the user, so the gate must.
    const refused = { data: user, isError: true, error: { status: 409 }, failureCount: 1, failureReason: null }
    const ok = { data: { complete: true }, isError: false, error: null, failureCount: 0, failureReason: null }
    expect(firstLoadFailed(refused)).toBe(true)
    expect(authGateState({ setupComplete: true, user, failed: firstLoadFailed(ok) || firstLoadFailed(refused) })).toBe('unavailable')
    expect(authGateState({ setupComplete: true, user, failed: firstLoadFailed({ ...ok, isError: true, error: { status: 409 } }) || firstLoadFailed({ ...refused, isError: false, error: null }) })).toBe('unavailable')
  })

  test('a first load that fails shows the unavailable message, not the app', () => {
    const empty = { data: undefined, isError: false, error: null, failureCount: 0, failureReason: null }
    expect(firstLoadFailed(empty)).toBe(false)
    // One failed try may be a blip; the second says the server is not there.
    expect(firstLoadFailed({ ...empty, failureCount: 1 })).toBe(false)
    expect(firstLoadFailed({ ...empty, failureCount: 2 })).toBe(true)
    expect(firstLoadFailed({ ...empty, isError: true })).toBe(true)
    expect(authGateState({ setupComplete: undefined, user: undefined, failed: true })).toBe('unavailable')
    expect(authGateState({ setupComplete: true, user: undefined, failed: true })).toBe('unavailable')
    expect(authGateState({ setupComplete: true, user: undefined, failed: false })).toBe('loading')
  })
})

test('public recovery copy directs users to the installation administrator', () => {
  expect(recoveryRequestCopy).toEqual({
    title: 'Contact your administrator',
    detail: 'Your installation administrator can create a one-time password recovery link for you.',
  })
})
