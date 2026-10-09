/**
 * The browser side of passkeys (WebAuthn), through the native JSON helpers: the server sends `{ publicKey: … }` in the
 * WebAuthn JSON form and takes the credential's `toJSON()` back.
 */

type JsonObject = { [key: string]: unknown }

/** This browser can create and use passkeys with the JSON helpers (missing before Chrome 129, Safari 18.4, Firefox 119). */
export function passkeysSupported(): boolean {
  return typeof PublicKeyCredential !== 'undefined'
    && typeof PublicKeyCredential.parseCreationOptionsFromJSON === 'function'
    && typeof PublicKeyCredential.parseRequestOptionsFromJSON === 'function'
    && typeof navigator !== 'undefined'
    && Boolean(navigator.credentials)
}

/** The person closed the browser prompt, or it timed out: not an error worth showing. */
export function isPasskeyCancel(error: unknown): boolean {
  return error instanceof Error && (error.name === 'NotAllowedError' || error.name === 'AbortError')
}

export class PasskeyUnsupportedError extends Error {
  constructor() {
    super('This browser does not support passkeys.')
    this.name = 'PasskeyUnsupportedError'
  }
}

function publicKeyOf(options: JsonObject): JsonObject {
  const publicKey = options.publicKey
  if (!publicKey || typeof publicKey !== 'object') throw new Error('The passkey options from the server are invalid.')
  return publicKey as JsonObject
}

function credentialJson(credential: Credential | null): JsonObject {
  const json = (credential as PublicKeyCredential | null)?.toJSON?.() as JsonObject | undefined
  if (!json) throw new Error('The browser did not return a passkey.')
  return json
}

/** Registers a new passkey with the server's creation options; returns the credential for the finish call. */
export async function createPasskey(options: JsonObject): Promise<JsonObject> {
  if (!passkeysSupported()) throw new PasskeyUnsupportedError()
  const json = publicKeyOf(options) as unknown as PublicKeyCredentialCreationOptionsJSON
  const publicKey = PublicKeyCredential.parseCreationOptionsFromJSON(json)
  return credentialJson(await navigator.credentials.create({ publicKey }))
}

/** Signs in with a passkey with the server's request options; returns the assertion for the finish call. */
export async function getPasskey(options: JsonObject): Promise<JsonObject> {
  if (!passkeysSupported()) throw new PasskeyUnsupportedError()
  const json = publicKeyOf(options) as unknown as PublicKeyCredentialRequestOptionsJSON
  const publicKey = PublicKeyCredential.parseRequestOptionsFromJSON(json)
  return credentialJson(await navigator.credentials.get({ publicKey }))
}

/** A starting name for a new passkey, from the platform the browser reports. */
export function defaultPasskeyName(userAgent: string = typeof navigator === 'undefined' ? '' : navigator.userAgent): string {
  if (/iPhone/.test(userAgent)) return 'iPhone'
  if (/iPad/.test(userAgent)) return 'iPad'
  if (/Android/.test(userAgent)) return 'Android'
  if (/Mac OS X|Macintosh/.test(userAgent)) return 'Mac'
  if (/Windows/.test(userAgent)) return 'Windows'
  if (/CrOS/.test(userAgent)) return 'Chromebook'
  if (/Linux/.test(userAgent)) return 'Linux'
  return 'This device'
}
