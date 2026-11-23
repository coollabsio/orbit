/**
 * Settings → Mail → Sending and Mail preferences (and Admin → Mail's routes) over the generated SDK: send routes,
 * signatures, the undo time, snippets and vacation replies. No React. The keys follow the web's `mailKeys`.
 */
import {
  createInstanceMailProviderSendRoute,
  createInstanceMailSendRoute,
  createMailSnippet,
  createWorkspaceMailProviderSendRoute,
  createWorkspaceMailSendRoute,
  deleteInstanceMailSendRoute,
  deleteMailSignature,
  deleteMailSnippet,
  deleteWorkspaceMailSendRoute,
  getMailboxVacation,
  getMailSettings,
  getMailSignature,
  listInstanceMailSendRoutes,
  listMailSnippets,
  listWorkspaceMailSendRoutes,
  putMailboxVacation,
  putMailSettings,
  putMailSignature,
  setMailSendDefault,
  testInstanceMailSendRoute,
  testWorkspaceMailSendRoute,
  updateInstanceMailSendRoute,
  updateMailSnippet,
  updateWorkspaceMailSendRoute,
} from '@orbit/api/generated/sdk.gen'
import type {
  CreateMailSendRouteBody,
  CreateMailSnippetBody,
  CreateProviderSendRouteBody,
  MailMemberSettings,
  MailSendRouteList,
  MailSendRouteTest,
  MailSendRouteView,
  MailSignature,
  MailSnippet,
  MailVacation,
  PutMailVacationBody,
  UpdateMailSendRouteBody,
  UpdateMailSnippetBody,
} from '@orbit/api/generated/types.gen'

import type { OrbitClient } from './orbit'

export type { CreateMailSendRouteBody, CreateProviderSendRouteBody, MailSendRouteList, MailSendRouteTest, MailSendRouteView, MailSnippet, MailVacation, PutMailVacationBody, UpdateMailSendRouteBody }

/** The root user's routes (`instance`, Admin → Mail) or the workspace's own. */
export type ProviderScope = 'instance' | 'workspace'

export const mailSendingKeys = {
  sendRoutes: (scope: ProviderScope, workspaceId: string) => (scope === 'instance' ? (['admin', 'mail', 'send-routes'] as const) : (['mail', workspaceId, 'send-routes'] as const)),
  signatures: (workspaceId: string) => ['mail', workspaceId, 'signatures'] as const,
  snippets: (workspaceId: string) => ['mail', workspaceId, 'snippets'] as const,
  settingsMe: (workspaceId: string) => ['mail', workspaceId, 'settings-me'] as const,
  vacation: (workspaceId: string, mailboxId: string) => ['mail', workspaceId, 'vacation', mailboxId] as const,
}

const at = (client: OrbitClient, workspaceId: string) => ({ client, path: { workspace_id: workspaceId }, throwOnError: true as const })

// ---- send routes

export async function sendRoutes(client: OrbitClient, scope: ProviderScope, workspaceId: string): Promise<MailSendRouteList> {
  return scope === 'instance' ? (await listInstanceMailSendRoutes({ client, throwOnError: true })).data : (await listWorkspaceMailSendRoutes(at(client, workspaceId))).data
}

/** The answer carries the secrets the server made (`generated`), the only time it shows them. */
export async function createSendRoute(client: OrbitClient, scope: ProviderScope, workspaceId: string, body: CreateMailSendRouteBody): Promise<MailSendRouteView> {
  return scope === 'instance'
    ? (await createInstanceMailSendRoute({ client, body, throwOnError: true })).data
    : (await createWorkspaceMailSendRoute({ ...at(client, workspaceId), body })).data
}

/** Secrets left out of `body.secrets` keep their values; `body.settings` replaces the settings. */
export async function updateSendRoute(client: OrbitClient, scope: ProviderScope, workspaceId: string, routeId: string, body: UpdateMailSendRouteBody): Promise<MailSendRouteView> {
  return scope === 'instance'
    ? (await updateInstanceMailSendRoute({ client, path: { route_id: routeId }, body, throwOnError: true })).data
    : (await updateWorkspaceMailSendRoute({ client, path: { workspace_id: workspaceId, route_id: routeId }, body, throwOnError: true })).data
}

/** Refused while a domain uses the route (`route_in_use`) or while it is the default (`route_is_default`). */
export async function deleteSendRoute(client: OrbitClient, scope: ProviderScope, workspaceId: string, routeId: string): Promise<void> {
  if (scope === 'instance') await deleteInstanceMailSendRoute({ client, path: { route_id: routeId }, throwOnError: true })
  else await deleteWorkspaceMailSendRoute({ client, path: { workspace_id: workspaceId, route_id: routeId }, throwOnError: true })
}

/** **Send test**: a message to `to` through the route, with the result of each step. */
export async function testSendRoute(client: OrbitClient, scope: ProviderScope, workspaceId: string, routeId: string, to: string): Promise<MailSendRouteTest> {
  return scope === 'instance'
    ? (await testInstanceMailSendRoute({ client, path: { route_id: routeId }, body: { to }, throwOnError: true })).data
    : (await testWorkspaceMailSendRoute({ client, path: { workspace_id: workspaceId, route_id: routeId }, body: { to }, throwOnError: true })).data
}

/** **Also send through this provider**: a route with the provider's account and only what it lacks. */
export async function createProviderSendRoute(client: OrbitClient, scope: ProviderScope, workspaceId: string, providerId: string, body: CreateProviderSendRouteBody): Promise<MailSendRouteView> {
  return scope === 'instance'
    ? (await createInstanceMailProviderSendRoute({ client, path: { provider_id: providerId }, body, throwOnError: true })).data
    : (await createWorkspaceMailProviderSendRoute({ client, path: { workspace_id: workspaceId, provider_id: providerId }, body, throwOnError: true })).data
}

/** The instance route that domains without one use; `null` clears it (root user). */
export async function setSendDefault(client: OrbitClient, routeId: string | null): Promise<MailSendRouteList> {
  return (await setMailSendDefault({ client, body: { route_id: routeId }, throwOnError: true })).data
}

// ---- the member's own

/** The signature of each address (`null` body when none). */
export async function signatures(client: OrbitClient, workspaceId: string, addressIds: readonly string[]): Promise<MailSignature[]> {
  return Promise.all(addressIds.map(async (addressId) => (await getMailSignature({ client, path: { workspace_id: workspaceId, address_id: addressId }, throwOnError: true })).data))
}

/** An empty body removes the signature. */
export async function saveSignature(client: OrbitClient, workspaceId: string, addressId: string, bodyHtml: string): Promise<MailSignature> {
  const path = { workspace_id: workspaceId, address_id: addressId }
  if (bodyHtml.trim() === '') {
    await deleteMailSignature({ client, path, throwOnError: true })
    return { address_id: addressId, body_html: null }
  }
  return (await putMailSignature({ client, path, body: { body_html: bodyHtml }, throwOnError: true })).data
}

export async function mailSettings(client: OrbitClient, workspaceId: string): Promise<MailMemberSettings> {
  return (await getMailSettings(at(client, workspaceId))).data
}

/** The undo send delay (0, 5, 10, 20 or 30 s). */
export async function saveMailSettings(client: OrbitClient, workspaceId: string, undoSendSeconds: number): Promise<MailMemberSettings> {
  return (await putMailSettings({ ...at(client, workspaceId), body: { undo_send_seconds: undoSendSeconds } })).data
}

export async function snippets(client: OrbitClient, workspaceId: string): Promise<MailSnippet[]> {
  return (await listMailSnippets(at(client, workspaceId))).data
}

export async function createSnippet(client: OrbitClient, workspaceId: string, body: CreateMailSnippetBody): Promise<MailSnippet> {
  return (await createMailSnippet({ ...at(client, workspaceId), body })).data
}

export async function updateSnippet(client: OrbitClient, workspaceId: string, snippetId: string, body: UpdateMailSnippetBody): Promise<MailSnippet> {
  return (await updateMailSnippet({ client, path: { workspace_id: workspaceId, snippet_id: snippetId }, body, throwOnError: true })).data
}

export async function deleteSnippet(client: OrbitClient, workspaceId: string, snippetId: string): Promise<void> {
  await deleteMailSnippet({ client, path: { workspace_id: workspaceId, snippet_id: snippetId }, throwOnError: true })
}

export async function vacation(client: OrbitClient, workspaceId: string, mailboxId: string): Promise<MailVacation> {
  return (await getMailboxVacation({ client, path: { workspace_id: workspaceId, mailbox_id: mailboxId }, throwOnError: true })).data
}

/** The whole reply: the server replaces every field. */
export async function setVacation(client: OrbitClient, workspaceId: string, mailboxId: string, body: PutMailVacationBody): Promise<MailVacation> {
  return (await putMailboxVacation({ client, path: { workspace_id: workspaceId, mailbox_id: mailboxId }, body, throwOnError: true })).data
}

/** The web's `vacationState`: `on` between the start (inclusive) and the end (exclusive); `scheduled` before the start. */
export function vacationState(saved: Pick<MailVacation, 'enabled' | 'starts_at' | 'ends_at'>, now: Date): 'off' | 'on' | 'scheduled' {
  if (!saved.enabled) return 'off'
  if (saved.ends_at && new Date(saved.ends_at).getTime() <= now.getTime()) return 'off'
  if (saved.starts_at && new Date(saved.starts_at).getTime() > now.getTime()) return 'scheduled'
  return 'on'
}
