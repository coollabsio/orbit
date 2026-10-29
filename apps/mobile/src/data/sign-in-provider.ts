/**
 * Sign-in with Google or GitHub: the server's start → provider → handoff → exchange flow, in the system browser's auth
 * session. No React and no Expo: the screens pass the device parts (`ProviderDevice`, see `src/sign-in-provider.ts`),
 * so tests run the whole flow over a fake fetch and a fake browser.
 */
import { ApiProblem } from '@orbit/api/problem'
import { disconnectConnectedAccount, exchangeProviderSignIn, listConnectedAccounts, startProviderSignIn as startRequest } from '@orbit/api/generated/sdk.gen'
import type { ConnectedAccount, LoginResponse, Provider, SignInIntent, TwoFactorChallenge } from '@orbit/api/generated/types.gen'

import { type OrbitClient, type SignIn, signedIn } from './orbit'

export type { ConnectedAccount, Provider, SignInIntent }

export const PROVIDER_LABEL: Record<Provider, string> = { google: 'Google', github: 'GitHub' }

/** The redirect the server accepts from every build of the app. */
export const NATIVE_REDIRECT = 'orbit://auth/callback'

/**
 * `Linking.createURL('auth/callback')` → the redirect to send. Expo Go answers with its own `exp://host:port/--/…` URL,
 * which a server accepts only when its operator lists it in `ORBIT_DEV_NATIVE_REDIRECTS`; every other build gets the
 * fixed scheme URL, whatever form `createURL` gives it.
 */
export function nativeRedirect(created: string): string {
  return /^exps?:\/\//i.test(created) ? created : NATIVE_REDIRECT
}

export function base64url(bytes: Uint8Array): string {
  let binary = ''
  for (const byte of bytes) binary += String.fromCharCode(byte)
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')
}

export type Sha256 = (data: Uint8Array<ArrayBuffer>) => Promise<ArrayBuffer | Uint8Array>

/** RFC 7636 S256: `base64url(sha256(ascii(verifier)))`. */
export async function codeChallenge(verifier: string, sha256: Sha256): Promise<string> {
  return base64url(new Uint8Array(await sha256(new TextEncoder().encode(verifier))))
}

/** 32 random bytes as base64url: the 43-character verifier the server wants. */
export function codeVerifier(randomBytes: (count: number) => Uint8Array): string {
  return base64url(randomBytes(32))
}

/** The `handoff` of the URL the auth session came back with, or `null` when it is not our redirect or has none. */
export function handoffFrom(url: string, redirect: string): string | null {
  const [base, query = ''] = url.split('#')[0].split('?', 2)
  if (base !== redirect.split('?')[0]) return null
  for (const pair of query.split('&')) {
    const [key, value = ''] = pair.split('=', 2)
    if (key === 'handoff' && value) return decodeURIComponent(value.replace(/\+/g, ' '))
  }
  return null
}

/** What `WebBrowser.openAuthSessionAsync` resolves to: only `success` carries the redirect URL. */
export type AuthSessionResult = { type: 'success'; url: string } | { type: 'cancel' | 'dismiss' | 'opened' | 'locked' }

export interface ProviderDevice {
  randomBytes: (count: number) => Uint8Array
  sha256: Sha256
  redirect: () => string
  openAuthSession: (url: string, redirect: string) => Promise<AuthSessionResult>
}

export type ProviderOutcome = SignIn | { kind: 'connected' } | { kind: 'confirmed' } | { kind: 'cancelled' }

/** The exchange's answer: 200 a session, 202 the two-factor step, 204 a finished connect or confirm. */
export function exchangeOutcome(intent: SignInIntent, status: number, body: LoginResponse | TwoFactorChallenge | undefined): ProviderOutcome {
  if (status === 204) {
    if (intent === 'connect') return { kind: 'connected' }
    if (intent === 'confirm') return { kind: 'confirmed' }
  }
  if (intent !== 'sign_in' || !body) throw new Error('The server answered the sign-in in a way this app does not know. Update the app.')
  return signedIn(body)
}

/**
 * Runs one provider flow. `client` is Origin-less: without a token for `sign_in`, with the session's for `connect` and
 * `confirm`. The session token comes back only in the exchange's body, never in a URL. `password` goes with `connect`
 * from an account that has one.
 */
export async function startProviderSignIn(client: OrbitClient, provider: Provider, intent: SignInIntent, device: ProviderDevice, password?: string): Promise<ProviderOutcome> {
  const verifier = codeVerifier(device.randomBytes)
  const redirect = device.redirect()
  const { data } = await startRequest({
    client,
    path: { provider },
    body: { intent, code_challenge: await codeChallenge(verifier, device.sha256), redirect, password },
    throwOnError: true,
  })
  const session = await device.openAuthSession(data.authorize_url, redirect)
  if (session.type !== 'success') return { kind: 'cancelled' }
  const handoff = handoffFrom(session.url, redirect)
  if (!handoff) throw new Error(`${PROVIDER_LABEL[provider]} did not finish the sign-in. Try again.`)
  const exchanged = await exchangeProviderSignIn({ client, body: { handoff, code_verifier: verifier }, throwOnError: true })
  return exchangeOutcome(intent, exchanged.response.status, exchanged.data as LoginResponse | TwoFactorChallenge | undefined)
}

const MESSAGES: Record<string, string> = {
  provider_not_configured: 'This sign-in provider is not set up on this server.',
  invalid_redirect: "This server does not accept the app's sign-in address. In Expo Go, the server's operator must list it in ORBIT_DEV_NATIVE_REDIRECTS.",
  authentication_busy: 'Too many sign-ins at once. Wait a moment and try again.',
  invalid_handoff: 'The sign-in took too long or was already used. Try again.',
  provider_denied: 'The sign-in was cancelled at the provider.',
  provider_failed: 'The provider could not be reached. Try again.',
  provider_email_unverified: 'The provider has no verified email address for this account. Verify it there, then try again.',
  registration_closed: 'No Orbit account uses this email address, and this server does not let new people sign up.',
  account_suspended: 'This account is suspended.',
  session_mismatch: 'Your session changed during the sign-in. Try again.',
  identity_mismatch: 'Confirm with the account you connected to Orbit.',
  identity_in_use: 'This provider account is already connected to another Orbit account.',
  identity_conflict: 'Another account of this provider is already connected. Disconnect it first.',
  provider_already_connected: 'This provider is already connected.',
  identity_not_found: 'This provider is not connected.',
  last_sign_in_method: 'This is your only way to sign in. Set a password or connect another account first.',
}

/** A readable message for a provider flow's error; other errors keep the server's own detail. */
export function providerErrorMessage(error: unknown): string {
  if (error instanceof ApiProblem) return MESSAGES[error.code] ?? error.detail
  return error instanceof Error ? error.message : String(error)
}

export const isReauthenticationRequired = (error: unknown) => error instanceof ApiProblem && error.status === 401 && error.code === 'reauthentication_required'

/**
 * Runs `action`; when the server wants a fresh sign-in (an account without a password, an older session), runs
 * `confirm` and retries once. `null`: the person cancelled the confirmation.
 */
export async function withReauthentication<T>(action: () => Promise<T>, confirm: () => Promise<boolean>): Promise<T | null> {
  try {
    return await action()
  } catch (error) {
    if (!isReauthenticationRequired(error)) throw error
    if (!(await confirm())) return null
    return action()
  }
}

// ---- connected accounts

export async function connectedAccounts(client: OrbitClient): Promise<ConnectedAccount[]> {
  return (await listConnectedAccounts({ client, throwOnError: true })).data
}

export async function disconnect(client: OrbitClient, provider: Provider): Promise<void> {
  await disconnectConnectedAccount({ client, path: { provider }, throwOnError: true })
}

export interface ProviderRow {
  provider: Provider
  account: ConnectedAccount | null
}

/** The Connected accounts card: every configured provider, and any connected one the admin has since removed. */
export function providerRows(configured: readonly Provider[], connected: readonly ConnectedAccount[]): ProviderRow[] {
  const order: Provider[] = ['google', 'github']
  return order
    .filter((provider) => configured.includes(provider) || connected.some((account) => account.provider === provider))
    .map((provider) => ({ provider, account: connected.find((account) => account.provider === provider) ?? null }))
}

/** A server field worth asking for its sign-in options: a host with a dot or a port, or `localhost`. */
export function looksLikeServer(input: string): boolean {
  const match = /^(?:https?:\/\/)?([^/\s:?#]+)(:\d+)?(?:[/?#]|$)/i.exec(input.trim())
  return Boolean(match && (match[1].includes('.') || match[2] || match[1].toLowerCase() === 'localhost'))
}
