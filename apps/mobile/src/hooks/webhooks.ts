/** The Webhooks screen's queries and writes, over `data/webhooks-api.ts`, with the web's refreshes. */
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import * as api from '../data/webhooks-api'
import { useOrbit } from '../orbit-context'

export function useWebhooks(enabled: boolean) {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: api.webhookKeys.list(workspaceId), queryFn: () => api.webhooks(client, workspaceId), enabled })
}

export function useWebhookDeliveries(webhookId: string) {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: api.webhookKeys.deliveries(workspaceId, webhookId), queryFn: () => api.webhookDeliveries(client, workspaceId, webhookId) })
}

/** Every write but rotate refreshes the list (turning a webhook off cancels its pending deliveries, so those refresh too). */
export function useWebhookWrites() {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  const onSuccess = () => void queryClient.invalidateQueries({ queryKey: api.webhookKeys.list(workspaceId) })
  return {
    create: useMutation({ mutationFn: (body: api.WebhookCreateBody) => api.createWebhook(client, workspaceId, body), onSuccess }),
    update: useMutation({ mutationFn: ({ webhookId, body }: { webhookId: string; body: Parameters<typeof api.updateWebhook>[3] }) => api.updateWebhook(client, workspaceId, webhookId, body), onSuccess }),
    remove: useMutation({ mutationFn: (webhookId: string) => api.deleteWebhook(client, workspaceId, webhookId), onSuccess }),
    rotate: useMutation({ mutationFn: (webhookId: string) => api.rotateSecret(client, workspaceId, webhookId) }),
  }
}

export function useRetryDelivery(webhookId: string) {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (deliveryId: string) => api.retryDelivery(client, workspaceId, webhookId, deliveryId),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: api.webhookKeys.deliveries(workspaceId, webhookId) }),
  })
}
