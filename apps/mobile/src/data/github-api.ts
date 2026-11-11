/** The member's GitHub connection for PR review (the web's Account settings → GitHub). No React. */
import { deleteGithubConnection, finishGithubConnection, githubConnection, startGithubConnection } from '@orbit/api/generated/sdk.gen'
import type { GithubConnection } from '@orbit/api/generated/types.gen'

import type { OrbitClient } from './orbit'

export const githubKeys = {
  connection: (workspaceId: string) => ['github-connection', workspaceId] as const,
}

export type ConnectResult = 'connected' | 'failed' | 'taken' | 'mismatch'

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

/** Connects the account as the signed-in member with the one-time token GitHub's callback put in the return URL. */
export async function finishConnection(client: OrbitClient, workspaceId: string, token: string): Promise<void> {
  await finishGithubConnection({ client, path: { workspace_id: workspaceId }, body: { token }, throwOnError: true })
}

export type GithubReturn = { kind: 'finish'; token: string } | { kind: 'failed' }

/** Where GitHub's callback sent the browser back: a finish token, a failure, or `null` (not a GitHub return). */
export function githubReturn(url: string): GithubReturn | null {
  // React Native's URLSearchParams cannot read: the two keys are read by hand
  const github = /[?&]github=([^&#]+)/.exec(url)?.[1]
  if (!github) return null
  const token = /[?&]token=([^&#]+)/.exec(url)?.[1]
  return github === 'finish' && token ? { kind: 'finish', token: decodeURIComponent(token) } : { kind: 'failed' }
}

/** The outcome for a finish problem's `code`. */
export function finishResult(code: string | undefined): ConnectResult {
  if (code === 'github_account_taken') return 'taken'
  if (code === 'github_connect_mismatch') return 'mismatch'
  return 'failed'
}

export const CONNECT_MESSAGES: Record<ConnectResult, string> = {
  connected: 'GitHub connected',
  failed: 'Could not connect GitHub. Try again.',
  taken: 'This GitHub account is already connected to another Orbit user.',
  mismatch: 'Another member started this GitHub connection. Connect from your own Account settings.',
}
