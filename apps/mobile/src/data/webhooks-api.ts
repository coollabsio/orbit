/** The workspace's task webhooks (the web's Settings → Webhooks) over the generated SDK, with the web's labels and messages. No React. */
import { createWebhook as createRequest, deleteWebhook as deleteRequest, listWebhookDeliveries, listWebhooks, retryWebhookDelivery, rotateWebhookSecret, updateWebhook as updateRequest } from '@orbit/api/generated/sdk.gen'
import type { WebhookCreateBody, WebhookCreated, WebhookDelivery, WebhookEventType, WebhookSubscription, WebhookUpdateBody } from '@orbit/api/generated/types.gen'
import { ApiProblem } from '@orbit/api/problem'

import type { OrbitClient } from './orbit'

export type { WebhookCreateBody, WebhookDelivery, WebhookEventType, WebhookSubscription }

export const webhookKeys = {
  list: (workspaceId: string) => ['webhooks', workspaceId] as const,
  deliveries: (workspaceId: string, webhookId: string) => ['webhooks', workspaceId, webhookId, 'deliveries'] as const,
}

export async function webhooks(client: OrbitClient, workspaceId: string): Promise<WebhookSubscription[]> {
  return (await listWebhooks({ client, path: { workspace_id: workspaceId }, throwOnError: true })).data
}

export async function webhookDeliveries(client: OrbitClient, workspaceId: string, webhookId: string): Promise<WebhookDelivery[]> {
  return (await listWebhookDeliveries({ client, path: { workspace_id: workspaceId, webhook_id: webhookId }, throwOnError: true })).data
}

/** The answer carries the signing secret, the only time the server shows it. */
export async function createWebhook(client: OrbitClient, workspaceId: string, body: WebhookCreateBody): Promise<WebhookCreated> {
  return (await createRequest({ client, path: { workspace_id: workspaceId }, body, throwOnError: true })).data
}

export async function updateWebhook(client: OrbitClient, workspaceId: string, webhookId: string, body: WebhookUpdateBody): Promise<WebhookSubscription> {
  return (await updateRequest({ client, path: { workspace_id: workspaceId, webhook_id: webhookId }, body, throwOnError: true })).data
}

export async function deleteWebhook(client: OrbitClient, workspaceId: string, webhookId: string): Promise<void> {
  await deleteRequest({ client, path: { workspace_id: workspaceId, webhook_id: webhookId }, throwOnError: true })
}

/** The new signing secret; the old one stops working at once. */
export async function rotateSecret(client: OrbitClient, workspaceId: string, webhookId: string): Promise<string> {
  return (await rotateWebhookSecret({ client, path: { workspace_id: workspaceId, webhook_id: webhookId }, throwOnError: true })).data.signing_secret
}

export async function retryDelivery(client: OrbitClient, workspaceId: string, webhookId: string, deliveryId: string): Promise<void> {
  await retryWebhookDelivery({ client, path: { workspace_id: workspaceId, webhook_id: webhookId, delivery_id: deliveryId }, throwOnError: true })
}

export const EVENT_TYPES: readonly { value: WebhookEventType; label: string }[] = [
  { value: 'task.created', label: 'Task created' },
  { value: 'task.updated', label: 'Task updated' },
  { value: 'task.deleted', label: 'Task moved to the trash' },
]

/** A `validation_failed` answer names the field in its detail. */
const FIELD_MESSAGES: Record<string, string> = {
  name: 'Enter a name of 1 to 100 characters.',
  url: 'Enter a public https URL.',
  event_types: 'Select at least one event.',
  project_ids: 'Select at least one project.',
}

/** The web's `failure`: the field's message, the server's text when the app key is missing, else `fallback`. */
export function webhookFailure(error: unknown, fallback: string): string {
  if (!(error instanceof ApiProblem)) return fallback
  if (error.code === 'validation_failed') return FIELD_MESSAGES[error.detail] ?? fallback
  if (error.code === 'app_key_missing') return error.detail
  return fallback
}

export type DeliveryTone = 'secondary' | 'outline' | 'destructive'

export const DELIVERY_STATES: Record<string, { label: string; tone: DeliveryTone }> = {
  pending: { label: 'Pending', tone: 'secondary' },
  succeeded: { label: 'Delivered', tone: 'outline' },
  failed: { label: 'Failed', tone: 'destructive' },
  cancelled: { label: 'Cancelled', tone: 'secondary' },
}

export const deliveryState = (delivery: WebhookDelivery) => DELIVERY_STATES[delivery.state] ?? { label: delivery.state, tone: 'secondary' as const }

/** The last answer of the endpoint: its HTTP status, else why the attempt failed. */
export const deliveryAnswer = (delivery: WebhookDelivery) => (delivery.last_status != null ? `HTTP ${delivery.last_status}` : (delivery.last_error ?? '—'))

export const mayRetry = (delivery: WebhookDelivery) => delivery.state === 'failed' || delivery.state === 'cancelled'

/** The row's last line: the events, then the projects by name. */
export function webhookSummary(webhook: WebhookSubscription, projectNames: ReadonlyMap<string, string>): string {
  const projects = webhook.project_ids.map((id) => projectNames.get(id) ?? 'Unavailable project').join(', ') || 'No projects'
  return `${webhook.event_types.join(', ')} · ${projects}`
}
