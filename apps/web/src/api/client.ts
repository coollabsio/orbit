export { CONTRACT_ID } from './generated/contract'
import { CONTRACT_ID } from './generated/contract'
import { createClient } from './generated/client'
import { parseProblem } from './problem'

interface ApiClientOptions {
  fetch?: (request: Request) => Promise<Response>
  onUnauthorized?: () => void
}

export function createApiClient(options: ApiClientOptions = {}) {
  const client = createClient({
    baseUrl: globalThis.location?.origin ?? 'http://localhost',
    credentials: 'include',
    fetch: options.fetch as typeof globalThis.fetch,
    headers: { 'X-Orbit-Contract': CONTRACT_ID },
  })

  client.interceptors.response.use(async (response) => {
    if (response.ok) return response
    const problem = await parseProblem(response)
    if (problem.status === 401) options.onUnauthorized?.()
    throw problem
  })
  return client
}

export const apiClient = createApiClient({
  onUnauthorized: () => window.dispatchEvent(new Event('orbit:unauthorized')),
})
