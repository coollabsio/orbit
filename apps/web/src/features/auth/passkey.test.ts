import { afterEach, expect, test } from 'bun:test'
import { createPasskey, defaultPasskeyName, getPasskey, isPasskeyCancel, PasskeyUnsupportedError, passkeysSupported } from './passkey'

const scope = globalThis as { PublicKeyCredential?: unknown }
const originalCredential = scope.PublicKeyCredential
const originalCredentials = Object.getOwnPropertyDescriptor(navigator, 'credentials')

afterEach(() => {
  scope.PublicKeyCredential = originalCredential
  if (originalCredentials) Object.defineProperty(navigator, 'credentials', originalCredentials)
  else delete (navigator as { credentials?: unknown }).credentials
})

function stubBrowser() {
  const calls: { kind: string; options: unknown }[] = []
  scope.PublicKeyCredential = {
    parseCreationOptionsFromJSON: (json: unknown) => ({ parsed: 'create', json }),
    parseRequestOptionsFromJSON: (json: unknown) => ({ parsed: 'get', json }),
  }
  Object.defineProperty(navigator, 'credentials', {
    configurable: true,
    value: {
      create: async (options: unknown) => { calls.push({ kind: 'create', options }); return { toJSON: () => ({ id: 'new' }) } },
      get: async (options: unknown) => { calls.push({ kind: 'get', options }); return { toJSON: () => ({ id: 'existing' }) } },
    },
  })
  return calls
}

test('passkeys count as unsupported without the JSON helpers', async () => {
  scope.PublicKeyCredential = function PublicKeyCredential() {}
  expect(passkeysSupported()).toBe(false)
  await expect(createPasskey({ publicKey: {} })).rejects.toBeInstanceOf(PasskeyUnsupportedError)
})

test('create and get pass the server options through the JSON helpers and return toJSON()', async () => {
  const calls = stubBrowser()
  expect(passkeysSupported()).toBe(true)
  expect(await createPasskey({ publicKey: { challenge: 'a' } })).toEqual({ id: 'new' })
  expect(await getPasskey({ publicKey: { challenge: 'b' } })).toEqual({ id: 'existing' })
  expect(calls).toEqual([
    { kind: 'create', options: { publicKey: { parsed: 'create', json: { challenge: 'a' } } } },
    { kind: 'get', options: { publicKey: { parsed: 'get', json: { challenge: 'b' } } } },
  ])
})

test('a closed browser prompt is a cancel, not a failure', () => {
  expect(isPasskeyCancel(new DOMException('closed', 'NotAllowedError'))).toBe(true)
  expect(isPasskeyCancel(new Error('boom'))).toBe(false)
})

test('the default passkey name follows the platform', () => {
  expect(defaultPasskeyName('Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)')).toBe('Mac')
  expect(defaultPasskeyName('Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X)')).toBe('iPhone')
  expect(defaultPasskeyName('curl/8')).toBe('This device')
})
