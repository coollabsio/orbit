import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { apiClient } from '@/api/client'
import { queryKeys } from '@/api/queryKeys'
import { createWebhook, deleteWebhook, listWebhookDeliveries, listWebhooks, retryWebhookDelivery, rotateWebhookSecret, updateWebhook } from '@orbit/api/generated/sdk.gen'
import type { WebhookCreateBody, WebhookUpdateBody } from '@orbit/api/generated/types.gen'

export function useWebhooks(workspaceId: string, enabled = true) {
  return useQuery({
    queryKey: queryKeys.webhooks(workspaceId),
    enabled,
    queryFn: async () => {
      const { data } = await listWebhooks({ client: apiClient, path: { workspace_id: workspaceId }, throwOnError: true })
      if (!data) throw new Error('Webhook response was empty.')
      return data
    },
  })
}

export function useWebhookDeliveries(workspaceId: string, webhookId: string) {
  return useQuery({
    queryKey: queryKeys.webhookDeliveries(workspaceId, webhookId),
    queryFn: async () => {
      const { data } = await listWebhookDeliveries({ client: apiClient, path: { workspace_id: workspaceId, webhook_id: webhookId }, throwOnError: true })
      if (!data) throw new Error('Delivery response was empty.')
      return data
    },
  })
}

export function useCreateWebhook(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (body: WebhookCreateBody) => {
      const { data } = await createWebhook({ client: apiClient, path: { workspace_id: workspaceId }, body, throwOnError: true })
      if (!data) throw new Error('Webhook response was empty.')
      return data
    },
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.webhooks(workspaceId) }),
  })
}

export function useUpdateWebhook(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async ({ webhookId, body }: { webhookId: string; body: WebhookUpdateBody }) => {
      const { data } = await updateWebhook({ client: apiClient, path: { workspace_id: workspaceId, webhook_id: webhookId }, body, throwOnError: true })
      if (!data) throw new Error('Webhook response was empty.')
      return data
    },
    // turning a webhook off cancels its pending deliveries, so those refresh too
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.webhooks(workspaceId) }),
  })
}

export function useDeleteWebhook(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (webhookId: string) => deleteWebhook({ client: apiClient, path: { workspace_id: workspaceId, webhook_id: webhookId }, throwOnError: true }),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.webhooks(workspaceId) }),
  })
}

export function useRotateWebhookSecret(workspaceId: string) {
  return useMutation({
    mutationFn: async (webhookId: string) => {
      const { data } = await rotateWebhookSecret({ client: apiClient, path: { workspace_id: workspaceId, webhook_id: webhookId }, throwOnError: true })
      if (!data) throw new Error('Secret response was empty.')
      return data.signing_secret
    },
  })
}

export function useRetryWebhookDelivery(workspaceId: string, webhookId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (deliveryId: string) => retryWebhookDelivery({ client: apiClient, path: { workspace_id: workspaceId, webhook_id: webhookId, delivery_id: deliveryId }, throwOnError: true }),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.webhookDeliveries(workspaceId, webhookId) }),
  })
}
