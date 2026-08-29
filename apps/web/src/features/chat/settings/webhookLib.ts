import type { Channel, Webhook } from '../../../mock/types'

/** Public URL a sender would POST to (the chat reference: /api/webhooks/:id/:token). */
export function webhookUrl(webhook: Webhook): string {
  return `${window.location.origin}/api/webhooks/${webhook.id}/${webhook.token}`
}

export function channelLabel(channels: Channel[], channelId: string): string {
  const ch = channels.find((c) => c.id === channelId)
  return `${ch?.emoji ? `${ch.emoji} ` : '#'}${ch?.name || 'unknown'}`
}

