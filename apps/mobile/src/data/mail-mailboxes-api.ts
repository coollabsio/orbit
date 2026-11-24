/**
 * Settings → Mail → Mailboxes, Mail labels, Suppressions and Failed messages over the generated SDK (the web's
 * `features/mail/api` calls for those pages). No React.
 */
import {
  addMailboxAlias,
  addMailboxMember,
  createMailbox as createMailboxRequest,
  createMailLabel as createLabelRequest,
  createPersonalMailboxes,
  deleteMailbox as deleteMailboxRequest,
  deleteMailLabel as deleteLabelRequest,
  deleteMailSuppression,
  listFailedMailReceipts,
  listMailboxes,
  listMailDomains,
  listMailLabels,
  listMailSuppressions,
  previewPersonalMailboxes,
  removeMailboxAddress,
  removeMailboxMember,
  retryMailReceipt,
  setMailboxStatus,
  updateMailbox,
  updateMailboxAddress,
  updateMailLabel as updateLabelRequest,
} from '@orbit/api/generated/sdk.gen'
import type {
  CreateMailboxBody,
  CreateMailLabelBody,
  CreatePersonalMailboxesBody,
  MailboxProposal,
  MailboxStatus,
  MailDomain,
  MailFailedReceipt,
  MailFailedReceipts,
  MailLabel,
  MailMailbox,
  MailNamePattern,
  MailRejectedDelivery,
  MailRejectReason,
  MailSuppression,
  MailSuppressionPage,
  PersonalMailboxesResult,
  UpdateMailLabelBody,
} from '@orbit/api/generated/types.gen'

import type { OrbitClient } from './orbit'

export type { MailboxProposal, MailDomain, MailFailedReceipt, MailLabel, MailMailbox, MailNamePattern, MailRejectedDelivery, MailSuppression }

/** The web's `mailKeys` for these pages; all sit under `['mail', workspaceId]`, as the mail app's keys do. */
export const mailSettingsKeys = {
  domains: (workspaceId: string) => ['mail', workspaceId, 'domains'] as const,
  mailboxes: (workspaceId: string) => ['mail', workspaceId, 'mailboxes'] as const,
  /** Under `mailboxes`: a mailbox change makes new proposals. */
  bulkPreview: (workspaceId: string, domainId: string, pattern: MailNamePattern) => ['mail', workspaceId, 'mailboxes', 'bulk-preview', domainId, pattern] as const,
  labels: (workspaceId: string) => ['mail', workspaceId, 'labels'] as const,
  suppressions: (workspaceId: string) => ['mail', workspaceId, 'suppressions'] as const,
  receipts: (workspaceId: string) => ['mail', workspaceId, 'receipts'] as const,
}

const at = (client: OrbitClient, workspaceId: string) => ({ client, path: { workspace_id: workspaceId }, throwOnError: true as const })
const inMailbox = (client: OrbitClient, workspaceId: string, mailboxId: string) => ({ client, path: { workspace_id: workspaceId, mailbox_id: mailboxId }, throwOnError: true as const })

// ---- domains and mailboxes

export async function domains(client: OrbitClient, workspaceId: string): Promise<MailDomain[]> {
  return (await listMailDomains(at(client, workspaceId))).data
}

export async function mailboxes(client: OrbitClient, workspaceId: string): Promise<MailMailbox[]> {
  return (await listMailboxes(at(client, workspaceId))).data
}

export async function createMailbox(client: OrbitClient, workspaceId: string, body: CreateMailboxBody): Promise<MailMailbox> {
  return (await createMailboxRequest({ ...at(client, workspaceId), body })).data
}

export async function renameMailbox(client: OrbitClient, workspaceId: string, mailboxId: string, name: string): Promise<MailMailbox> {
  return (await updateMailbox({ ...inMailbox(client, workspaceId, mailboxId), body: { name } })).data
}

export async function deleteMailbox(client: OrbitClient, workspaceId: string, mailboxId: string): Promise<void> {
  await deleteMailboxRequest(inMailbox(client, workspaceId, mailboxId))
}

/** Suspend (refuses new mail, threads stay readable) or resume. */
export async function setStatus(client: OrbitClient, workspaceId: string, mailboxId: string, status: MailboxStatus): Promise<MailMailbox> {
  return (await setMailboxStatus({ ...inMailbox(client, workspaceId, mailboxId), body: { status } })).data
}

export async function addMember(client: OrbitClient, workspaceId: string, mailboxId: string, membershipId: string): Promise<MailMailbox> {
  return (await addMailboxMember({ ...inMailbox(client, workspaceId, mailboxId), body: { membership_id: membershipId } })).data
}

export async function removeMember(client: OrbitClient, workspaceId: string, mailboxId: string, membershipId: string): Promise<MailMailbox> {
  return (await removeMailboxMember({ client, path: { workspace_id: workspaceId, mailbox_id: mailboxId, membership_id: membershipId }, throwOnError: true })).data
}

export async function addAlias(client: OrbitClient, workspaceId: string, mailboxId: string, domainId: string, localPart: string, senderName: string | null): Promise<MailMailbox> {
  return (await addMailboxAlias({ ...inMailbox(client, workspaceId, mailboxId), body: { domain_id: domainId, local_part: localPart, sender_name: senderName } })).data
}

export async function removeAddress(client: OrbitClient, workspaceId: string, mailboxId: string, addressId: string): Promise<MailMailbox> {
  return (await removeMailboxAddress({ client, path: { workspace_id: workspaceId, mailbox_id: mailboxId, address_id: addressId }, throwOnError: true })).data
}

/** Sets the sender name of one address; `null` goes back to the mailbox name. */
export async function setSenderName(client: OrbitClient, workspaceId: string, mailboxId: string, addressId: string, senderName: string | null): Promise<MailMailbox> {
  return (await updateMailboxAddress({ client, path: { workspace_id: workspaceId, mailbox_id: mailboxId, address_id: addressId }, body: { sender_name: senderName }, throwOnError: true })).data
}

/** The proposed address of each member without a personal mailbox, with the conflicts marked. */
export async function bulkPreview(client: OrbitClient, workspaceId: string, domainId: string, pattern: MailNamePattern): Promise<MailboxProposal[]> {
  return (await previewPersonalMailboxes({ ...at(client, workspaceId), body: { domain_id: domainId, pattern } })).data
}

/** Creates the personal mailboxes, with the addresses the admin edited in `overrides`. */
export async function bulkCreate(client: OrbitClient, workspaceId: string, body: CreatePersonalMailboxesBody): Promise<PersonalMailboxesResult> {
  return (await createPersonalMailboxes({ ...at(client, workspaceId), body })).data
}

// ---- labels

export async function labels(client: OrbitClient, workspaceId: string): Promise<MailLabel[]> {
  return (await listMailLabels(at(client, workspaceId))).data
}

export async function createLabel(client: OrbitClient, workspaceId: string, body: CreateMailLabelBody): Promise<MailLabel> {
  return (await createLabelRequest({ ...at(client, workspaceId), body })).data
}

export async function updateLabel(client: OrbitClient, workspaceId: string, labelId: string, body: UpdateMailLabelBody): Promise<MailLabel> {
  return (await updateLabelRequest({ client, path: { workspace_id: workspaceId, label_id: labelId }, body, throwOnError: true })).data
}

export async function deleteLabel(client: OrbitClient, workspaceId: string, labelId: string): Promise<void> {
  await deleteLabelRequest({ client, path: { workspace_id: workspaceId, label_id: labelId }, throwOnError: true })
}

// ---- suppressions and failed messages

export async function suppressionPage(client: OrbitClient, workspaceId: string, q: string | undefined, cursor: string | undefined): Promise<MailSuppressionPage> {
  return (await listMailSuppressions({ ...at(client, workspaceId), query: { q, cursor } })).data
}

export async function removeSuppression(client: OrbitClient, workspaceId: string, address: string): Promise<void> {
  await deleteMailSuppression({ client, path: { workspace_id: workspaceId, address }, throwOnError: true })
}

/** Failed and stuck receipts (`count` of all, `receipts` the newest 100) and the deliveries no mailbox took. */
export async function failedReceipts(client: OrbitClient, workspaceId: string): Promise<MailFailedReceipts> {
  return (await listFailedMailReceipts(at(client, workspaceId))).data
}

/** Files a failed or stuck message again. */
export async function retryReceipt(client: OrbitClient, workspaceId: string, receiptId: string): Promise<void> {
  await retryMailReceipt({ client, path: { workspace_id: workspaceId, receipt_id: receiptId }, throwOnError: true })
}

// ---- the web pages' copy

export const SUPPRESSION_REASONS: Record<string, string> = { hard_bounce: 'Bounced', complaint: 'Complained', manual: 'Added by hand' }

export const REJECT_REASONS: Record<MailRejectReason, string> = {
  unknown_address: 'No mailbox has this address',
  wrong_route: 'This domain does not receive through this provider',
  domain_unavailable: 'The domain is suspended or disabled',
  mailbox_suspended: 'The mailbox is suspended',
}

export const NAME_PATTERNS: readonly { value: MailNamePattern; label: string }[] = [
  { value: 'first', label: 'first@ (ann@)' },
  { value: 'first_dot_last', label: 'first.last@ (ann.lee@)' },
  { value: 'flast', label: 'flast@ (alee@)' },
]

/** What the server keeps of a local part: lower-case letters, digits, dots and hyphens, no dot at an end or twice. */
const LOCAL_PART = /^[a-z0-9-]+(\.[a-z0-9-]+)*$/

export interface ProposalRow extends MailboxProposal {
  localPart: string
  edited: boolean
  problem: 'Not a valid address' | 'Used above' | 'Taken' | null
}

/**
 * The web dialog's rows: the admin's edits over the proposals, each marked like the server marks it (the first member
 * with an address gets it, a later one conflicts; `refused` holds what the server refused at the last confirm).
 */
export function proposalRows(proposals: readonly MailboxProposal[], edits: Record<string, string>, refused: Record<string, string>): ProposalRow[] {
  const seen = new Set<string>()
  return proposals.map((proposal) => {
    const edit = edits[proposal.membership_id]
    const localPart = edit ?? proposal.local_part
    const edited = edit !== undefined && edit !== proposal.local_part
    const repeated = seen.has(localPart)
    seen.add(localPart)
    const problem = !LOCAL_PART.test(localPart)
      ? 'Not a valid address'
      : repeated
        ? 'Used above'
        : refused[proposal.membership_id] === localPart || (!edited && proposal.conflict)
          ? 'Taken'
          : null
    return { ...proposal, localPart, edited, problem }
  })
}
