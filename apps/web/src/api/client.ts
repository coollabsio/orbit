import { createOrbitClient, type OrbitClientOptions } from '@orbit/api/client'

export { CONTRACT_ID } from '@orbit/api/generated/contract'

type ApiClientOptions = Pick<OrbitClientOptions, 'fetch' | 'onUnauthorized' | 'onPaymentRequired'>

export const UNAUTHORIZED_EVENT = 'orbit:unauthorized'
/** Orbit Cloud refused a write: the workspace is read-only or at a trial limit; `detail` is the server's message. */
export const PAYMENT_REQUIRED_EVENT = 'orbit:payment-required'

export function createApiClient(options: ApiClientOptions = {}) {
  return createOrbitClient({
    ...options,
    baseUrl: globalThis.location?.origin ?? 'http://localhost',
    credentials: 'include',
  })
}

export const apiClient = createApiClient({
  onUnauthorized: () => window.dispatchEvent(new Event(UNAUTHORIZED_EVENT)),
  onPaymentRequired: (detail) => window.dispatchEvent(new CustomEvent(PAYMENT_REQUIRED_EVENT, { detail })),
})
