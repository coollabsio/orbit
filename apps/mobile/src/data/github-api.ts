/** The member's GitHub connection for PR review (the web's Account settings → GitHub). No React. */
import { deleteGithubConnection, githubConnection, startGithubConnection } from '@orbit/api/generated/sdk.gen'
import type { GithubConnection } from '@orbit/api/generated/types.gen'

import type { OrbitClient } from './orbit'

export const githubKeys = {
  connection: (workspaceId: string) => ['github-connection', workspaceId] as const,
}

export type ConnectResult = 'connected' | 'failed' | 'taken'

export async function connection(client: OrbitClient, workspaceId: string): Promise<GithubConnection> {
  return (await githubConnection({ client, path: { workspace_id: workspaceId }, throwOnError: true })).data
}

/** GitHub's authorize URL; the browser comes back to `returnUrl` with `?github=…`. */
export async function startConnection(client: OrbitClient, workspaceId: string, returnUrl: string): Promise<string> {
  const { data } = await startGithubConnection({ client, path: { workspace_id: workspaceId }, body: { client: 'mobile', return_url: returnUrl }, throwOnError: true })
  return data.authorize_url
}

export async function disconnect(client: OrbitClient, workspaceId: string): Promise<void> {
  await deleteGithubConnection({ client, path: { workspace_id: workspaceId }, throwOnError: true })
}

/** The `github` outcome in the URL the browser returned to, or `null`. */
export function githubConnectResult(url: string): ConnectResult | null {
  // React Native's URLSearchParams cannot read: the one key is read by hand
  const value = /[?&]github=([^&#]+)/.exec(url)?.[1]
  return value === 'connected' || value === 'failed' || value === 'taken' ? value : null
}

export const CONNECT_MESSAGES: Record<ConnectResult, string> = {
  connected: 'GitHub connected',
  failed: 'Could not connect GitHub. Try again.',
  taken: 'This GitHub account is already connected to another Orbit user.',
}
