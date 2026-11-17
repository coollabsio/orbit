// Delivery of sent mail in plain words (§9.1 U10): the status of each recipient, the summary of a message, why a
// recipient bounced, why an Outbox message waits, and whether a vacation reply is on.
import type { MailboxKind, MailOutboundRecipient, MailVacation } from '@orbit/api/generated/types.gen'
import { ApiProblem } from '@orbit/api/problem'

export type DeliveryLabel = 'Sending' | 'Delivered' | 'Delayed' | 'Bounced' | 'Suppressed' | 'Partly delivered'

/** What a recipient shows. */
export type RecipientState = 'sending' | 'delivered' | 'delayed' | 'bounced' | 'suppressed' | 'cancelled'

export type DeliveryRecipient = Pick<MailOutboundRecipient, 'address' | 'status' | 'delayed'>

/**
 * A recipient's state: queued, sending and deferred are "sending" until the server says the first attempt is more
 * than 4 hours ago (`delayed`). "Delivered" means the next server accepted it.
 */
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

/**
 * The status line of a sent message, and the addresses that bounced. A delay shows first (the sender may want to
 * act), then a recipient still on its way; when every recipient is final: all delivered, some delivered, else
 * bounced (or only suppressed). Cancelled recipients never go and do not count.
 */
export function deliverySummary(recipients: readonly DeliveryRecipient[]): { label: DeliveryLabel; bounced: string[] } {
  const states: RecipientState[] = recipients.map(recipientState).filter((state) => state !== 'cancelled')
  const bounced = recipients.filter((recipient) => recipientState(recipient) === 'bounced').map((recipient) => recipient.address)
  const has = (state: RecipientState) => states.includes(state)
  let label: DeliveryLabel
  if (states.length === 0 || (!has('delayed') && has('sending'))) label = 'Sending'
  else if (has('delayed')) label = 'Delayed'
  else if (states.every((state) => state === 'delivered')) label = 'Delivered'
  else if (has('delivered')) label = 'Partly delivered'
  else if (has('bounced')) label = 'Bounced'
  else label = 'Suppressed'
  return { label, bounced }
}

const BOUNCE_REASONS: Record<string, string> = {
  hard: 'The address does not exist or refused the message',
  gave_up: 'Gave up after 5 days',
  route: 'The sending route has a problem',
  unreadable: 'Orbit could not read the sent message',
}

/** Why a recipient bounced, by `bounce_kind`. Orbit's own refusal (`rejected`) is its text in `last_response`. */
export function bounceReason(kind: string | null | undefined, lastResponse: string | null | undefined): string {
  if (kind === 'rejected') return lastResponse?.trim() || 'Orbit refused to send it'
  return (kind && BOUNCE_REASONS[kind]) || 'The message was not delivered'
}

export const SUPPRESSED_REASON = 'Not sent: the address bounced or complained before, so it is on the suppression list'

/**
 * Why a message waits in the Outbox (`hold_reason`): every reason the server writes. `limit`: delivery waits for the
 * send limit. The readiness reasons (`domain_unverified` … `mailbox_suspended`) hold a message whose domain or mailbox
 * stopped being ready before its time; it goes once they are ready again.
 */
export const HOLD_REASONS: Record<string, { text: string; problem: boolean }> = {
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

export interface OutboxItemStatus {
  text: string
  /** It will not go by itself: the row shows the reason as a warning. */
  problem: boolean
  /** Still held: Cancel brings the draft back. */
  canCancel: boolean
}

/** The status of an Outbox row: why it waits, or when it goes. */
export function outboxStatus(
  item: { status: string; hold_reason?: string | null; release_at: string },
  now: Date,
): OutboxItemStatus {
  const canCancel = item.status === 'held'
  if (item.hold_reason) {
    const reason = HOLD_REASONS[item.hold_reason] ?? { text: 'Waiting: something went wrong', problem: true }
    return { ...reason, canCancel }
  }
  const seconds = Math.ceil((new Date(item.release_at).getTime() - now.getTime()) / 1000)
  if (!canCancel || seconds <= 0) return { text: 'Sending', problem: false, canCancel }
  const text = seconds < 60 ? `Sending in ${seconds} s` : `Sending in ${Math.ceil(seconds / 60)} min`
  return { text, problem: false, canCancel }
}

/** `on` between the start (inclusive) and the end (exclusive); `scheduled` before the start. */
export function vacationState(vacation: Pick<MailVacation, 'enabled' | 'starts_at' | 'ends_at'>, now: Date): 'off' | 'on' | 'scheduled' {
  if (!vacation.enabled) return 'off'
  if (vacation.ends_at && new Date(vacation.ends_at).getTime() <= now.getTime()) return 'off'
  if (vacation.starts_at && new Date(vacation.starts_at).getTime() > now.getTime()) return 'scheduled'
  return 'on'
}

/** Why **Retry** failed: a purged recipient (after 90 days) is too old; else the server's text. */
export function retryErrorText(error: unknown): string {
  if (error instanceof ApiProblem && error.code === 'recipient_expired') return 'This message is too old to retry'
  return error instanceof ApiProblem && error.detail ? error.detail : 'The message could not be sent again.'
}

/** The tag after a recipient's address: Bcc recipients are marked (only the sender sees them). */
export const recipientTag = (kind: string): 'Bcc' | null => (kind === 'bcc' ? 'Bcc' : null)

/**
 * Who may change a mailbox's vacation reply (the server's rule): a personal mailbox only its owner, a shared one a
 * member with `mail.manage`.
 */
export function mayChangeVacation(kind: MailboxKind, viewer: { owner: boolean; canManage: boolean }): boolean {
  return kind === 'personal' ? viewer.owner : viewer.canManage
}

/**
 * Whether a reply found held (409 `draft_sending`, at `since`) still is, from the Outbox and Scheduled lists:
 * `unknown` until every list was read after `since`, then `held` while one lists it as `held`, else `gone` (sent or
 * undone).
 */
export function heldReplyState(
  outboundId: string,
  since: number,
  lists: ReadonlyArray<{ items: ReadonlyArray<{ id: string; status: string }>; updatedAt: number }>,
): 'held' | 'gone' | 'unknown' {
  if (lists.some((list) => list.updatedAt < since)) return 'unknown'
  return lists.some((list) => list.items.some((item) => item.id === outboundId && item.status === 'held')) ? 'held' : 'gone'
}
