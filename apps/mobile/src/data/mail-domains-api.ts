/** Settings → Mail → Domains and Providers over the generated SDK, with the web's query keys. No React. */
import {
  checkMailDomain,
  createInstanceMailProvider,
  createMailDomain,
  createWorkspaceMailProvider,
  deleteInstanceMailProvider,
  deleteMailDomain,
  deleteWorkspaceMailProvider,
  getMailDomain,
  listInstanceMailProviders,
  listInstanceMailSendRoutes,
  listMailboxes,
  listMailDomains,
  listWorkspaceMailProviders,
  listWorkspaceMailSendRoutes,
  rotateMailDomainDkim,
  setMailDomainCatchAll,
  setMailDomainEnabled,
  setMailDomainRoute,
  setMailDomainSendRoute,
  updateInstanceMailProvider,
  updateWorkspaceMailProvider,
} from '@orbit/api/generated/sdk.gen'
import type { CreateMailProviderBody, MailDomain, MailMailbox, MailProviderList, MailProviderView, MailSendRouteList, UpdateMailProviderBody } from '@orbit/api/generated/types.gen'

import type { OrbitClient } from './orbit'

export type { CreateMailProviderBody, MailDomain, MailMailbox, MailProviderList, MailProviderView, MailSendRouteList, UpdateMailProviderBody }

/** `instance`: Admin → Mail (root user); `workspace`: Settings → Mail. */
export type ProviderScope = 'instance' | 'workspace'

/** The web's `mailKeys` and `queryKeys.adminMail*` that these pages read and refresh (same arrays, so both refresh each other). */
export const mailDomainKeys = {
  all: (workspaceId: string) => ['mail', workspaceId] as const,
  domains: (workspaceId: string) => ['mail', workspaceId, 'domains'] as const,
  domain: (workspaceId: string, domainId: string) => ['mail', workspaceId, 'domains', domainId] as const,
  mailboxes: (workspaceId: string) => ['mail', workspaceId, 'mailboxes'] as const,
  providers: (workspaceId: string) => ['mail', workspaceId, 'providers'] as const,
  sendRoutes: (workspaceId: string) => ['mail', workspaceId, 'send-routes'] as const,
  /** Prefix of every From picker: it says whether an address can send. */
  fromAddresses: (workspaceId: string) => ['mail', workspaceId, 'from-addresses'] as const,
  adminProviders: ['admin', 'mail', 'providers'] as const,
  adminSendRoutes: ['admin', 'mail', 'send-routes'] as const,
}

const ws = (workspaceId: string) => ({ workspace_id: workspaceId })
const dom = (workspaceId: string, domainId: string) => ({ workspace_id: workspaceId, domain_id: domainId })

export async function domains(client: OrbitClient, workspaceId: string): Promise<MailDomain[]> {
  return (await listMailDomains({ client, path: ws(workspaceId), throwOnError: true })).data
}

export async function domain(client: OrbitClient, workspaceId: string, domainId: string): Promise<MailDomain> {
  return (await getMailDomain({ client, path: dom(workspaceId, domainId), throwOnError: true })).data
}

/** Managers see every mailbox; members their own. */
export async function mailboxes(client: OrbitClient, workspaceId: string): Promise<MailMailbox[]> {
  return (await listMailboxes({ client, path: ws(workspaceId), throwOnError: true })).data
}

/** `providerId`: receive through that provider; `null`: this server's SMTP listener. */
export async function createDomain(client: OrbitClient, workspaceId: string, name: string, providerId: string | null): Promise<MailDomain> {
  return (await createMailDomain({ client, path: ws(workspaceId), body: { name, provider_id: providerId }, throwOnError: true })).data
}

/** Queues a DNS check (202); the domain shows the result once the job ran. */
export async function checkDomain(client: OrbitClient, workspaceId: string, domainId: string): Promise<void> {
  await checkMailDomain({ client, path: dom(workspaceId, domainId), throwOnError: true })
}

export async function deleteDomain(client: OrbitClient, workspaceId: string, domainId: string): Promise<void> {
  await deleteMailDomain({ client, path: dom(workspaceId, domainId), throwOnError: true })
}

/** The mailbox that gets mail for unknown addresses; `null` refuses it. */
export async function setCatchAll(client: OrbitClient, workspaceId: string, domainId: string, mailboxId: string | null): Promise<MailDomain> {
  return (await setMailDomainCatchAll({ client, path: dom(workspaceId, domainId), body: { mailbox_id: mailboxId }, throwOnError: true })).data
}

export async function setDomainRoute(client: OrbitClient, workspaceId: string, domainId: string, providerId: string | null): Promise<MailDomain> {
  return (await setMailDomainRoute({ client, path: dom(workspaceId, domainId), body: { provider_id: providerId }, throwOnError: true })).data
}

/** `routeId: null`: the server's default route. */
export async function setDomainSendRoute(client: OrbitClient, workspaceId: string, domainId: string, routeId: string | null): Promise<MailDomain> {
  return (await setMailDomainSendRoute({ client, path: dom(workspaceId, domainId), body: { route_id: routeId }, throwOnError: true })).data
}

export async function setDomainEnabled(client: OrbitClient, workspaceId: string, domainId: string, enabled: boolean): Promise<MailDomain> {
  return (await setMailDomainEnabled({ client, path: dom(workspaceId, domainId), body: { enabled }, throwOnError: true })).data
}

/** Starts a DKIM key rotation; the domain comes back with the new record to publish. */
export async function rotateDkim(client: OrbitClient, workspaceId: string, domainId: string): Promise<MailDomain> {
  return (await rotateMailDomainDkim({ client, path: dom(workspaceId, domainId), throwOnError: true })).data
}

export async function sendRoutes(client: OrbitClient, workspaceId: string, scope: ProviderScope): Promise<MailSendRouteList> {
  return scope === 'instance'
    ? (await listInstanceMailSendRoutes({ client, throwOnError: true })).data
    : (await listWorkspaceMailSendRoutes({ client, path: ws(workspaceId), throwOnError: true })).data
}

export async function providers(client: OrbitClient, workspaceId: string, scope: ProviderScope): Promise<MailProviderList> {
  return scope === 'instance'
    ? (await listInstanceMailProviders({ client, throwOnError: true })).data
    : (await listWorkspaceMailProviders({ client, path: ws(workspaceId), throwOnError: true })).data
}

/** The created provider's `generated_secrets` and full webhook URL are in this answer only. */
export async function createProvider(client: OrbitClient, workspaceId: string, scope: ProviderScope, body: CreateMailProviderBody): Promise<MailProviderView> {
  return scope === 'instance'
    ? (await createInstanceMailProvider({ client, body, throwOnError: true })).data
    : (await createWorkspaceMailProvider({ client, path: ws(workspaceId), body, throwOnError: true })).data
}

export async function updateProvider(client: OrbitClient, workspaceId: string, scope: ProviderScope, providerId: string, body: UpdateMailProviderBody): Promise<MailProviderView> {
  return scope === 'instance'
    ? (await updateInstanceMailProvider({ client, path: { provider_id: providerId }, body, throwOnError: true })).data
    : (await updateWorkspaceMailProvider({ client, path: { workspace_id: workspaceId, provider_id: providerId }, body, throwOnError: true })).data
}

export async function deleteProvider(client: OrbitClient, workspaceId: string, scope: ProviderScope, providerId: string): Promise<void> {
  if (scope === 'instance') await deleteInstanceMailProvider({ client, path: { provider_id: providerId }, throwOnError: true })
  else await deleteWorkspaceMailProvider({ client, path: { workspace_id: workspaceId, provider_id: providerId }, throwOnError: true })
}
