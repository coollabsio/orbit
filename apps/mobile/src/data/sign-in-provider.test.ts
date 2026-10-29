import { createOrbitClient } from '@orbit/api/client'
import { ApiProblem } from '@orbit/api/problem'
import { expect, test } from 'bun:test'

import {
  type AuthSessionResult,
  codeChallenge,
  codeVerifier,
  exchangeOutcome,
  handoffFrom,
  looksLikeServer,
  nativeRedirect,
  type ProviderDevice,
  providerErrorMessage,
  providerRows,
  startProviderSignIn,
  withReauthentication,
} from './sign-in-provider'

const sha256 = async (data: Uint8Array<ArrayBuffer>) => crypto.subtle.digest('SHA-256', data)

const problem = (status: number, code: string, detail = 'Server detail.') =>
  new ApiProblem({ type: 'about:blank', title: 'Error', status, code, detail, instance: '/', request_id: 'r1' })

const user = { id: 'u1', email: 'ada@example.com', display_name: 'Ada', has_password: false, installation_admin: false } as never

test('the S256 challenge of the RFC 7636 example verifier', async () => {
  expect(await codeChallenge('dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk', sha256)).toBe('E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM')
})

test('a verifier is 32 bytes as 43 base64url characters', () => {
  expect(codeVerifier((count) => new Uint8Array(count).fill(0xfb))).toBe('-_v7-_v7-_v7-_v7-_v7-_v7-_v7-_v7-_v7-_v7-_s')
})

test('Expo Go keeps its own redirect; every other build sends the app scheme', () => {
  expect(nativeRedirect('exp://192.168.1.5:8082/--/auth/callback')).toBe('exp://192.168.1.5:8082/--/auth/callback')
  expect(nativeRedirect('orbit:///auth/callback')).toBe('orbit://auth/callback')
  expect(nativeRedirect('orbit://auth/callback')).toBe('orbit://auth/callback')
})

test('the handoff comes only from our redirect', () => {
  expect(handoffFrom('orbit://auth/callback?handoff=a%2Bb_c&return_to=%2Fhome', 'orbit://auth/callback')).toBe('a+b_c')
  expect(handoffFrom('exp://h:8082/--/auth/callback?return_to=x&handoff=xyz#frag', 'exp://h:8082/--/auth/callback')).toBe('xyz')
  expect(handoffFrom('orbit://auth/callback?code=xyz', 'orbit://auth/callback')).toBeNull()
  expect(handoffFrom('orbit://elsewhere?handoff=xyz', 'orbit://auth/callback')).toBeNull()
  expect(handoffFrom('orbit://auth/callback?handoff=', 'orbit://auth/callback')).toBeNull()
})

test('the exchange answer maps to a session, the two-factor step, or a finished connect or confirm', () => {
  expect(exchangeOutcome('sign_in', 200, { session_id: 's1', session_token: 'tok', user } as never)).toEqual({ kind: 'signed_in', token: 'tok', user })
  expect(exchangeOutcome('sign_in', 202, { two_factor_token: 'tf' })).toEqual({ kind: 'two_factor', twoFactorToken: 'tf' })
  expect(exchangeOutcome('connect', 204, undefined)).toEqual({ kind: 'connected' })
  expect(exchangeOutcome('confirm', 204, undefined)).toEqual({ kind: 'confirmed' })
  expect(() => exchangeOutcome('sign_in', 204, undefined)).toThrow('Update the app')
})

/** A tokenless client whose fetch records each request and answers from `replies` in order. */
function server(replies: { status: number; body?: unknown }[]) {
  const sent: { url: string; body: unknown; origin: string | null }[] = []
  const client = createOrbitClient({
    baseUrl: 'https://orbit.test',
    token: () => null,
    fetch: async (request) => {
      sent.push({ url: request.url, body: JSON.parse(await request.text()), origin: request.headers.get('origin') })
      const reply = replies.shift()!
      return reply.body === undefined
        ? new Response(null, { status: reply.status })
        : new Response(JSON.stringify(reply.body), { status: reply.status, headers: { 'content-type': 'application/json' } })
    },
  })
  return { client, sent }
}

function device(result: AuthSessionResult): ProviderDevice & { opened: string[] } {
  const opened: string[] = []
  return {
    opened,
    randomBytes: (count) => new Uint8Array(count).fill(0xfb),
    sha256,
    redirect: () => 'orbit://auth/callback',
    openAuthSession: async (url, redirect) => {
      opened.push(`${url} -> ${redirect}`)
      return result
    },
  }
}

const VERIFIER = '-_v7-_v7-_v7-_v7-_v7-_v7-_v7-_v7-_v7-_v7-_s'

test('a sign-in starts, opens the auth session, and exchanges the handoff for a session token', async () => {
  const { client, sent } = server([
    { status: 200, body: { authorize_url: 'https://accounts.google.com/o/oauth2/auth?x=1' } },
    { status: 200, body: { session_id: 's1', session_token: 'tok', user } },
  ])
  const phone = device({ type: 'success', url: 'orbit://auth/callback?handoff=h1' })
  const outcome = await startProviderSignIn(client, 'google', 'sign_in', phone)
  expect(outcome).toEqual({ kind: 'signed_in', token: 'tok', user })
  expect(phone.opened).toEqual(['https://accounts.google.com/o/oauth2/auth?x=1 -> orbit://auth/callback'])
  expect(sent).toEqual([
    {
      url: 'https://orbit.test/api/v1/auth/providers/google/start',
      body: { intent: 'sign_in', code_challenge: await codeChallenge(VERIFIER, sha256), redirect: 'orbit://auth/callback' },
      origin: null,
    },
    { url: 'https://orbit.test/api/v1/auth/providers/exchange', body: { handoff: 'h1', code_verifier: VERIFIER }, origin: null },
  ])
})

test('a connect sends the current password and is connected on 204', async () => {
  const { client, sent } = server([{ status: 200, body: { authorize_url: 'https://github.com/login/oauth/authorize' } }, { status: 204 }])
  expect(await startProviderSignIn(client, 'github', 'connect', device({ type: 'success', url: 'orbit://auth/callback?handoff=h2' }), 'secret')).toEqual({ kind: 'connected' })
  expect(sent[0]).toEqual({
    url: 'https://orbit.test/api/v1/auth/providers/github/start',
    body: { intent: 'connect', code_challenge: await codeChallenge(VERIFIER, sha256), redirect: 'orbit://auth/callback', password: 'secret' },
    origin: null,
  })
})

test('a wrong password stops a connect before the browser opens', async () => {
  const { client, sent } = server([{ status: 401, body: { type: 'about:blank', title: 'Authentication failed', status: 401, code: 'invalid_credentials', detail: 'The password is incorrect.', instance: '/api/v1/auth/providers/github/start', request_id: 'r1' } }])
  const phone = device({ type: 'success', url: 'orbit://auth/callback?handoff=h3' })
  const error = await startProviderSignIn(client, 'github', 'connect', phone, 'wrong').catch((reason: unknown) => reason)
  expect(providerErrorMessage(error)).toBe('The password is incorrect.')
  expect(phone.opened).toEqual([])
  expect(sent).toHaveLength(1)
})

test('closing the browser is a cancelled sign-in, with no exchange', async () => {
  const { client, sent } = server([{ status: 200, body: { authorize_url: 'https://github.com/login/oauth/authorize' } }])
  expect(await startProviderSignIn(client, 'github', 'sign_in', device({ type: 'cancel' }))).toEqual({ kind: 'cancelled' })
  expect(await startProviderSignIn(server([{ status: 200, body: { authorize_url: 'u' } }]).client, 'github', 'sign_in', device({ type: 'dismiss' }))).toEqual({ kind: 'cancelled' })
  expect(sent.length).toBe(1)
})

test('error codes read as sentences; unknown codes keep the server detail', () => {
  expect(providerErrorMessage(problem(409, 'last_sign_in_method'))).toBe('This is your only way to sign in. Set a password or connect another account first.')
  expect(providerErrorMessage(problem(403, 'registration_closed'))).toBe('No Orbit account uses this email address, and this server does not let new people sign up.')
  expect(providerErrorMessage(problem(401, 'invalid_credentials', 'Wrong email or password.'))).toBe('Wrong email or password.')
  expect(providerErrorMessage(new Error('Network request failed'))).toBe('Network request failed')
})

test('reauthentication_required runs the confirmation and retries once', async () => {
  let calls = 0
  const action = async () => {
    calls += 1
    if (calls === 1) throw problem(401, 'reauthentication_required')
    return 'done'
  }
  expect(await withReauthentication(action, async () => true)).toBe('done')
  expect(calls).toBe(2)

  calls = 0
  expect(await withReauthentication(action, async () => false)).toBeNull()
  expect(calls).toBe(1)

  await expect(withReauthentication(async () => Promise.reject(problem(401, 'password_required')), async () => true)).rejects.toThrow('Server detail.')
})

test('connected accounts list configured providers, plus a connected one no longer configured', () => {
  const github = { provider: 'github' as const, email: 'ada', created_at: '2026-01-01T00:00:00Z' }
  expect(providerRows(['google'], [github])).toEqual([
    { provider: 'google', account: null },
    { provider: 'github', account: github },
  ])
  expect(providerRows([], [])).toEqual([])
})

test('a server field is worth asking once it has a dot, a port, or is localhost', () => {
  expect(['orbit.example.com', 'https://orbit.example.com/', 'http://10.0.0.5:8888', 'localhost', 'devbox:18888'].map(looksLikeServer)).toEqual([true, true, true, true, true])
  expect(['', 'orb', 'https://', 'orbit example.com'].map(looksLikeServer)).toEqual([false, false, false, false])
})
