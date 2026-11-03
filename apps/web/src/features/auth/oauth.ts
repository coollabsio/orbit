export type ConsentProject = { id: string; name: string; workspace_id: string; workspace_name: string }
export type OAuthConsent = { client_name: string; scope: string; csrf_token: string; projects: ConsentProject[] }

export async function oauthRequest<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(path, { credentials: 'same-origin', ...init })
  if (!response.ok) throw new Error('The connection request could not be completed. Sign in again or restart the connection.')
  if (response.status === 204) return undefined as T
  return response.json() as Promise<T>
}

// Keep login redirects confined to the one OAuth consent route. Never accept
// external URLs, protocol-relative paths, or client callback URLs here.
export function oauthLoginDestination(value: string | null): string {
  if (!value?.startsWith('/oauth/consent?')) return '/'
  const url = new URL(value, 'https://orbit.invalid')
  const request = url.searchParams.get('request')
  if (url.origin !== 'https://orbit.invalid' || url.pathname !== '/oauth/consent' || !request || !/^[A-Za-z0-9_-]{43}$/.test(request)) return '/'
  return `/oauth/consent?request=${request}`
}
