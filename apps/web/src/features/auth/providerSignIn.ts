/**
 * The browser side of "Continue with Google / GitHub": a PKCE verifier kept in this tab while the provider's page is
 * open, and the handoff the server puts in the `/login/callback` fragment.
 */
import type { Provider, SignInIntent } from '@orbit/api/generated/types.gen'

export const PROVIDER_NAMES: Record<Provider, string> = { google: 'Google', github: 'GitHub' }

/** What the tab remembers between leaving for the provider and coming back to `/login/callback`. */
export type ProviderFlow = { provider: Provider; intent: SignInIntent; verifier: string }

// One slot is enough: sessionStorage belongs to this tab, and a tab runs one flow at a time.
const FLOW_KEY = 'orbit:provider-flow'

function base64url(bytes: Uint8Array): string {
  let binary = ''
  for (const byte of bytes) binary += String.fromCharCode(byte)
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')
}

/** 32 random bytes: a 43-character verifier (RFC 7636). */
export function createCodeVerifier(): string {
  return base64url(crypto.getRandomValues(new Uint8Array(32)))
}

/** The S256 challenge of a verifier. */
export async function codeChallenge(verifier: string): Promise<string> {
  const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(verifier))
  return base64url(new Uint8Array(digest))
}

export function saveProviderFlow(flow: ProviderFlow) {
  sessionStorage.setItem(FLOW_KEY, JSON.stringify(flow))
}

export function storedProviderFlow(): ProviderFlow | null {
  try {
    const value = JSON.parse(sessionStorage.getItem(FLOW_KEY) ?? 'null') as Partial<ProviderFlow> | null
    if (!value || !(value.provider === 'google' || value.provider === 'github') || typeof value.verifier !== 'string') return null
    if (!(value.intent === 'sign_in' || value.intent === 'connect' || value.intent === 'confirm')) return null
    return { provider: value.provider, intent: value.intent, verifier: value.verifier }
  } catch {
    return null
  }
}

export function clearProviderFlow() {
  sessionStorage.removeItem(FLOW_KEY)
}

/** `#handoff=…&return_to=…` from the server's redirect to `/login/callback`. */
export function parseProviderCallback(hash: string): { handoff: string | null; returnTo: string | null } {
  const params = new URLSearchParams(hash.replace(/^#/, ''))
  return { handoff: params.get('handoff') || null, returnTo: params.get('return_to') }
}
