export type ConsentProject = { id: string; name: string; workspace_id: string; workspace_name: string }
export type OAuthConsent = { client_name: string; redirect_to: string; scope: string; csrf_token: string; projects: ConsentProject[] }

export async function oauthRequest<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(path, { credentials: 'same-origin', ...init })
  if (!response.ok) throw new Error('The connection request could not be completed. Sign in again or restart the connection.')
  if (response.status === 204) return undefined as T
  return response.json() as Promise<T>
}
