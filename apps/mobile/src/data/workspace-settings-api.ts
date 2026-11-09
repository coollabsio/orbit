/** The workspace's GitHub App, API tokens and exports (the web's GitHub, API tokens and Export pages). No React. */
import { createApiToken, createWorkspaceExport, githubWorkspaceSettings, listApiTokens, listWorkspaceExports, revokeApiToken, startGithubManifest } from '@orbit/api/generated/sdk.gen'
import type { ApiTokenRecord, ApiTokenScope, GithubManifestStart, GithubWorkspaceSettings, IssuedApiToken, WorkspaceExport } from '@orbit/api/generated/types.gen'

import type { OrbitClient } from './orbit'

export const workspaceSettingsKeys = {
  github: (workspaceId: string) => ['github-workspace', workspaceId] as const,
  apiTokens: (workspaceId: string) => ['api-tokens', workspaceId] as const,
  exports: (workspaceId: string) => ['workspace-exports', workspaceId] as const,
}

export async function githubSettings(client: OrbitClient, workspaceId: string): Promise<GithubWorkspaceSettings> {
  return (await githubWorkspaceSettings({ client, path: { workspace_id: workspaceId }, throwOnError: true })).data
}

/** The GitHub form to post for a new App; a blank `organization` registers a personal App. */
export async function githubManifest(client: OrbitClient, workspaceId: string, organization: string): Promise<GithubManifestStart> {
  return (await startGithubManifest({ client, path: { workspace_id: workspaceId }, body: { organization: organization || null }, throwOnError: true })).data
}

export async function apiTokens(client: OrbitClient, workspaceId: string): Promise<ApiTokenRecord[]> {
  return (await listApiTokens({ client, path: { workspace_id: workspaceId }, throwOnError: true })).data
}

export interface NewApiToken {
  name: string
  projectIds: string[]
  allProjects: boolean
  scopes: ApiTokenScope[]
  serviceAccount: boolean
  expiresInDays: number | null
}

export async function issueApiToken(client: OrbitClient, workspaceId: string, token: NewApiToken): Promise<IssuedApiToken> {
  const body = { name: token.name, project_ids: token.projectIds, all_projects: token.allProjects, scopes: token.scopes, service_account: token.serviceAccount, expires_in_days: token.expiresInDays }
  return (await createApiToken({ client, path: { workspace_id: workspaceId }, body, throwOnError: true })).data
}

export async function revokeToken(client: OrbitClient, workspaceId: string, tokenId: string): Promise<void> {
  await revokeApiToken({ client, path: { workspace_id: workspaceId, token_id: tokenId }, throwOnError: true })
}

export const EXPIRATION_OPTIONS = [
  { value: 'never', label: 'Never expires' },
  { value: '7', label: '7 days' },
  { value: '30', label: '30 days' },
  { value: '90', label: '90 days' },
  { value: '365', label: '1 year' },
] as const

export type ExpirationValue = (typeof EXPIRATION_OPTIONS)[number]['value']

export const isExpired = (expiresAt: string | null | undefined, now = Date.now()) => expiresAt != null && new Date(expiresAt).getTime() <= now

/** A row of the access picker; a level without a scope is unavailable, except `read` with no `read` scope, which is always granted. */
export interface ScopeResource {
  name: string
  hint: string
  read?: ApiTokenScope
  write?: ApiTokenScope
  disabled?: boolean
}

export type AccessLevel = 'none' | 'read' | 'write'

/** The token form's resources; a service account has task access only. */
export function tokenResources(serviceAccount: boolean): ScopeResource[] {
  return [
    { name: 'Tasks', hint: 'In the selected projects. Write includes delete.', read: 'tasks:read', write: 'tasks:write' },
    { name: 'Docs', hint: 'Including your private docs', read: 'docs:read', write: 'docs:write', disabled: serviceAccount },
    { name: 'Chat', hint: 'Including your private channels and DMs. MCP only.', read: 'chat:read', disabled: serviceAccount },
  ]
}

export function accessLevel(resource: ScopeResource, scopes: readonly ApiTokenScope[]): AccessLevel {
  if (resource.write && scopes.includes(resource.write)) return 'write'
  return !resource.read || scopes.includes(resource.read) ? 'read' : 'none'
}

export function withAccessLevel(resource: ScopeResource, scopes: readonly ApiTokenScope[], level: AccessLevel): ApiTokenScope[] {
  return [
    ...scopes.filter((scope) => scope !== resource.read && scope !== resource.write),
    ...(level !== 'none' && resource.read ? [resource.read] : []),
    ...(level === 'write' && resource.write ? [resource.write] : []),
  ]
}

/** Whether `level` can be picked for `resource` (the web's disabled toggle items). */
export function levelEnabled(resource: ScopeResource, level: AccessLevel): boolean {
  if (resource.disabled) return level === 'none'
  if (level === 'none') return Boolean(resource.read)
  if (level === 'write') return Boolean(resource.write)
  return true
}

/** A token row's second line: owner, prefix, projects, expiry and age. */
export function tokenSummary(token: ApiTokenRecord, projectNames: ReadonlyMap<string, string>, created: string, expiry: string): string {
  const owner = token.service_account_name ? `Service account: ${token.service_account_name}` : 'Personal token'
  const projects = token.all_projects ? 'All projects · ' : token.project_ids.length ? `${token.project_ids.map((id) => projectNames.get(id) ?? 'Unavailable project').join(', ')} · ` : ''
  return `${owner} · ${token.token_prefix}… · ${projects}${expiry} · created ${created}`
}

export async function workspaceExports(client: OrbitClient, workspaceId: string): Promise<WorkspaceExport[]> {
  return (await listWorkspaceExports({ client, path: { workspace_id: workspaceId }, throwOnError: true })).data.items
}

export async function startWorkspaceExport(client: OrbitClient, workspaceId: string): Promise<WorkspaceExport> {
  return (await createWorkspaceExport({ client, path: { workspace_id: workspaceId }, throwOnError: true })).data
}

export const isActiveExport = (item: WorkspaceExport) => item.status === 'queued' || item.status === 'running'

export const exportDownloadPath = (workspaceId: string, exportId: string) => `/api/v1/workspaces/${encodeURIComponent(workspaceId)}/exports/${encodeURIComponent(exportId)}/download`

/** The web's `formatSize`: `512 B`, `1.5 KB`, `2.0 MB`. */
export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`
}
