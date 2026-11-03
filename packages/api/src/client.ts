import { createClient } from './generated/client/index'
import { CONTRACT_ID } from './generated/contract'
import { isPaymentRequired, parseProblem } from './problem'

export interface OrbitClientOptions {
  baseUrl: string
  credentials?: RequestCredentials
  /** Read on every request, so a token that changes after sign-in is picked up. */
  token?: () => string | null | undefined
  fetch?: (request: Request) => Promise<Response>
  onUnauthorized?: () => void
  /** Orbit Cloud refused a write: the workspace is read-only or at a trial limit. */
  onPaymentRequired?: (detail: string) => void
}

export function createOrbitClient(options: OrbitClientOptions) {
  const client = createClient({
    baseUrl: options.baseUrl,
    credentials: options.credentials,
    fetch: options.fetch as typeof globalThis.fetch,
    headers: { 'X-Orbit-Contract': CONTRACT_ID },
  })

  const { token } = options
  if (token) {
    client.interceptors.request.use((request) => {
      const value = token()
      if (value) request.headers.set('Authorization', `Bearer ${value}`)
      return request
    })
  }

  client.interceptors.response.use(async (response) => {
    if (response.ok) return response
    const problem = await parseProblem(response)
    if (problem.status === 401 && problem.code === 'authentication_required') {
      options.onUnauthorized?.()
    }
    if (isPaymentRequired(problem)) options.onPaymentRequired?.(problem.detail)
    throw problem
  })
  return client
}
