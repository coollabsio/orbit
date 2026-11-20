/**
 * Delivery of sent mail in plain words (the web's `mailStatus.ts`, §9.1 U10): each recipient's state, the summary line
 * of a sent message, why a recipient bounced, and why an Outbox message waits. No React.
 */
import type { MailOutboundRecipient } from '@orbit/api/generated/types.gen'

export type DeliveryLabel = 'Sending' | 'Delivered' | 'Delayed' | 'Bounced' | 'Suppressed' | 'Partly delivered'
export type RecipientState = 'sending' | 'delivered' | 'delayed' | 'bounced' | 'suppressed' | 'cancelled'

type DeliveryRecipient = Pick<MailOutboundRecipient, 'address' | 'status' | 'delayed'>

export function recipientState(recipient: DeliveryRecipient): RecipientState {
  switch (recipient.status) {
    case 'delivered':
    case 'bounced':
    case 'suppressed':
    case 'cancelled':
      return recipient.status
    case 'deferred':
      return recipient.delayed ? 'delayed' : 'sending'
    default:
      return 'sending'
  }
}

export const RECIPIENT_LABEL: Record<RecipientState, string> = {
  sending: 'Sending',
  delivered: 'Delivered',
  delayed: 'Delayed',
  bounced: 'Bounced',
  suppressed: 'Suppressed',
  cancelled: 'Not sent',
}

/** The status line of a sent message: a delay first, then a recipient on its way, then delivered, partly, bounced. */
export function deliverySummary(recipients: readonly DeliveryRecipient[]): { label: DeliveryLabel; state: RecipientState; failed: number } {
  const states: RecipientState[] = recipients.map(recipientState).filter((state) => state !== 'cancelled')
  const has = (state: RecipientState) => states.includes(state)
  let label: DeliveryLabel
  if (states.length === 0 || (!has('delayed') && has('sending'))) label = 'Sending'
  else if (has('delayed')) label = 'Delayed'
  else if (states.every((state) => state === 'delivered')) label = 'Delivered'
  else if (has('delivered')) label = 'Partly delivered'
  else if (has('bounced')) label = 'Bounced'
  else label = 'Suppressed'
  const state: RecipientState = label === 'Partly delivered' ? 'bounced' : (label.toLowerCase() as RecipientState)
  return { label, state, failed: states.filter((entry) => entry === 'bounced' || entry === 'suppressed').length }
}

const BOUNCE_REASONS: Record<string, string> = {
  hard: 'The address does not exist or refused the message',
  gave_up: 'Gave up after 5 days',
  route: 'The sending route has a problem',
  unreadable: 'Orbit could not read the sent message',
}

/** Why a recipient did not get the message, or `null` when it did (or still may). */
export function recipientProblem(recipient: Pick<MailOutboundRecipient, 'status' | 'delayed' | 'bounce_kind' | 'last_response'>): string | null {
  const state = recipientState({ ...recipient, address: '' })
  if (state === 'suppressed') return 'Not sent: the address bounced or complained before, so it is on the suppression list'
  if (state !== 'bounced') return null
  if (recipient.bounce_kind === 'rejected') return recipient.last_response?.trim() || 'Orbit refused to send it'
  return (recipient.bounce_kind && BOUNCE_REASONS[recipient.bounce_kind]) || 'The message was not delivered'
}

const HOLD_REASONS: Record<string, { text: string; problem: boolean }> = {
  rate_limit: { text: 'Over the send limit: it goes when the limit allows', problem: false },
  limit: { text: 'Over the send limit: it goes when the limit allows', problem: false },
  paused: { text: 'Sending is paused for this workspace', problem: true },
  dkim_inactive: { text: 'Waiting for the domain’s DKIM record to be active', problem: true },
  domain_unverified: { text: 'The domain is not verified. It goes once it is', problem: true },
  inbound_not_live: { text: 'The domain’s MX record is not right. It goes once it is', problem: true },
  no_route: { text: 'The domain has no send route. Ask an admin', problem: true },
  mailbox_suspended: { text: 'The mailbox is suspended. It goes once it is active again', problem: true },
  file_missing: { text: 'A file of the message is gone. Undo it and attach the file again', problem: true },
  link_origin_missing: { text: 'Orbit has no public address for file links. Ask an admin', problem: true },
  link_forwarded: { text: 'A forwarded file cannot be sent as a link', problem: true },
  link_inline: { text: 'An image in the text cannot be sent as a link. Undo it and attach it instead', problem: true },
  send_as_denied: { text: 'You can no longer send from this address', problem: true },
  invalid_address: { text: 'An address is not valid. Undo it and fix the address', problem: true },
  no_recipients: { text: 'It has no recipients. Undo it and add one', problem: true },
  draft_missing: { text: 'Its draft is gone, so it was not sent', problem: true },
  build_failed: { text: 'The message could not be built', problem: true },
  decrypt_failed: { text: 'A file or the original message could not be read', problem: true },
  storage_quota: { text: 'The workspace storage is full', problem: true },
  release_failed: { text: 'It could not be prepared. Orbit tries again', problem: true },
  release_error: { text: 'Something went wrong while preparing it', problem: true },
}

/** An Outbox or Scheduled row: why it waits or when it goes, and whether Cancel send still works. */
export function outboxStatus(item: { status: string; hold_reason?: string | null; release_at: string }, now: Date): { text: string; problem: boolean; canCancel: boolean } {
  const canCancel = item.status === 'held'
  if (item.hold_reason) return { ...(HOLD_REASONS[item.hold_reason] ?? { text: 'Waiting: something went wrong', problem: true }), canCancel }
  const seconds = Math.ceil((new Date(item.release_at).getTime() - now.getTime()) / 1000)
  if (!canCancel || seconds <= 0) return { text: 'Sending', problem: false, canCancel }
  return { text: seconds < 60 ? `Sending in ${seconds} s` : `Sending in ${Math.ceil(seconds / 60)} min`, problem: false, canCancel }
}
