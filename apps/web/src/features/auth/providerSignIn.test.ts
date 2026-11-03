import { afterEach, expect, test } from 'bun:test'
import { clearProviderFlow, codeChallenge, createCodeVerifier, parseProviderCallback, saveProviderFlow, storedProviderFlow } from './providerSignIn'

afterEach(() => clearProviderFlow())

test('the S256 challenge matches the RFC 7636 example', async () => {
  expect(await codeChallenge('dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk')).toBe('E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM')
})

test('a verifier is 43 base64url characters and new each time', () => {
  const verifier = createCodeVerifier()
  expect(verifier).toMatch(/^[A-Za-z0-9_-]{43}$/)
  expect(createCodeVerifier()).not.toBe(verifier)
})

test('the flow survives in this tab until it is cleared', () => {
  saveProviderFlow({ provider: 'github', intent: 'connect', verifier: 'v'.repeat(43) })
  expect(storedProviderFlow()).toEqual({ provider: 'github', intent: 'connect', verifier: 'v'.repeat(43) })
  clearProviderFlow()
  expect(storedProviderFlow()).toBe(null)
  sessionStorage.setItem('orbit:provider-flow', '{"provider":"gitlab","intent":"sign_in","verifier":"x"}')
  expect(storedProviderFlow()).toBe(null)
})

test('the callback fragment gives the handoff and the return path', () => {
  expect(parseProviderCallback('#handoff=abc&return_to=%2Foauth%2Fconsent%3Frequest%3Dr')).toEqual({ handoff: 'abc', returnTo: '/oauth/consent?request=r' })
  expect(parseProviderCallback('')).toEqual({ handoff: null, returnTo: null })
})
