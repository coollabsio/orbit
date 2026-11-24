/**
 * Every mail call of the app, as plain functions over the generated SDK. No React. Business rules stay on the server:
 * it fills a reply's recipients and subject, quotes the parent when it sends, holds a sent message for the undo time,
 * and sanitizes every body.
 */
import {
  cancelMailOutbound,
  checkMailDraft,
  createMailDraft,
  createMailViewUrl,
  deleteMailDraft,
  getMailCounts,
  getMailDraft,
  getMailMe,
  getMailSendPolicy,
  getMailThread,
  listFromAddresses,
  listMailDrafts,
  listMailOutbound,
  listMailThreads,
  retryMailRecipient,
  searchMail,
  searchMailContacts,
  sendMailDraft,
  setMailSenderPref,
  updateMailDraft,
  updateMailThread,
} from '@orbit/api/generated/sdk.gen'
import type {
  CreateMailDraftBody,
  MailContact,
  MailCounts,
  MailDraft,
  MailFromAddress,
  MailMe,
  MailOutbound,
  MailOutboundItem,
  MailSendPolicy,
  MailSuppressedRecipient,
  MailThreadDetail,
  MailThreadPage,
  MailThreadSummary,
  UpdateMailDraftBody,
} from '@orbit/api/generated/types.gen'

import type { ThreadFilter } from './keys'
import type { ThreadChange } from './mail'
import { absoluteViewUrl } from './mail-frame'
import type { OrbitClient } from './orbit'

export type { MailContact, MailCounts, MailDraft, MailFromAddress, MailMe, MailOutbound, MailOutboundItem, MailThreadDetail, MailThreadPage, MailThreadSummary }

const at = (client: OrbitClient, workspaceId: string) => ({ client, path: { workspace_id: workspaceId }, throwOnError: true as const })

export async function mailMe(client: OrbitClient, workspaceId: string): Promise<MailMe> {
  return (await getMailMe(at(client, workspaceId))).data
}

export async function mailCounts(client: OrbitClient, workspaceId: string): Promise<MailCounts> {
  return (await getMailCounts(at(client, workspaceId))).data
}

/** One page of a folder list, newest first; `cursor` of the answer reads the next. */
export async function threadPage(client: OrbitClient, workspaceId: string, filter: ThreadFilter, cursor?: string): Promise<MailThreadPage> {
  const query = { mailbox_id: filter.mailboxId, label_id: filter.labelId, folder: filter.folder, starred: filter.starred || undefined, sent: filter.sent || undefined, unread: filter.unread || undefined, cursor }
  return (await listMailThreads({ ...at(client, workspaceId), query })).data
}

/** One page of a search (the operators of the server's `parse_query`). */
export async function searchPage(client: OrbitClient, workspaceId: string, q: string, cursor?: string): Promise<MailThreadPage> {
  return (await searchMail({ ...at(client, workspaceId), query: { q, cursor } })).data
}

export async function thread(client: OrbitClient, workspaceId: string, threadId: string): Promise<MailThreadDetail> {
  return (await getMailThread({ client, path: { workspace_id: workspaceId, thread_id: threadId }, throwOnError: true })).data
}

/** Read, star, folder (or `restore` out of the trash) and labels; the answer is the thread as it is now. */
export async function updateThread(client: OrbitClient, workspaceId: string, threadId: string, change: ThreadChange): Promise<MailThreadSummary> {
  return (await updateMailThread({ client, path: { workspace_id: workspaceId, thread_id: threadId }, body: change, throwOnError: true })).data
}

export async function setSenderImages(client: OrbitClient, workspaceId: string, address: string): Promise<void> {
  await setMailSenderPref({ ...at(client, workspaceId), body: { address, load_images: true } })
}

/** The message's sandboxed view document: its HTML and the CSP the server sends with it. The URL's token is its key. */
export async function viewDocument(client: OrbitClient, server: string, workspaceId: string, messageId: string, loadImages: boolean): Promise<{ html: string; policy: string }> {
  const { data } = await createMailViewUrl({ client, path: { workspace_id: workspaceId, message_id: messageId }, body: { load_images: loadImages }, throwOnError: true })
  // no session header: the token in the URL is the document's only key, and the content origin may be another host
  const response = await fetch(absoluteViewUrl(data.url, server), { credentials: 'omit' })
  if (!response.ok) throw new Error(`The message could not be loaded (${response.status}).`)
  const policy = response.headers.get('content-security-policy')
  if (!policy) throw new Error('The server sent the message without its security policy.')
  return { html: await response.text(), policy }
}

// ---- drafts and sending

export async function drafts(client: OrbitClient, workspaceId: string): Promise<MailDraft[]> {
  return (await listMailDrafts({ ...at(client, workspaceId), query: {} })).data
}

export async function draft(client: OrbitClient, workspaceId: string, draftId: string): Promise<MailDraft> {
  return (await getMailDraft({ client, path: { workspace_id: workspaceId, draft_id: draftId }, throwOnError: true })).data
}

/** A new draft, or a thread's reply (one per member and thread: the server answers with the one there is). */
export async function createDraft(client: OrbitClient, workspaceId: string, body: CreateMailDraftBody): Promise<MailDraft> {
  return (await createMailDraft({ ...at(client, workspaceId), body })).data
}

export async function saveDraft(client: OrbitClient, workspaceId: string, draftId: string, body: UpdateMailDraftBody): Promise<MailDraft> {
  return (await updateMailDraft({ client, path: { workspace_id: workspaceId, draft_id: draftId }, body, throwOnError: true })).data
}

export async function removeDraft(client: OrbitClient, workspaceId: string, draftId: string): Promise<void> {
  await deleteMailDraft({ client, path: { workspace_id: workspaceId, draft_id: draftId }, throwOnError: true })
}

/** Recipients of the saved draft on the suppression list. */
export async function draftSuppressed(client: OrbitClient, workspaceId: string, draftId: string): Promise<MailSuppressedRecipient[]> {
  return (await checkMailDraft({ client, path: { workspace_id: workspaceId, draft_id: draftId }, throwOnError: true })).data.suppressed
}

/** Sends after the undo time; the server holds it until `release_at`. */
export async function sendDraft(client: OrbitClient, workspaceId: string, draftId: string): Promise<MailOutbound> {
  return (await sendMailDraft({ client, path: { workspace_id: workspaceId, draft_id: draftId }, body: {}, throwOnError: true })).data
}

/** **Undo** / **Cancel send**: stops a held message and answers its draft (409 `already_sent` after its release). */
export async function cancelOutbound(client: OrbitClient, workspaceId: string, outboundId: string): Promise<MailDraft> {
  return (await cancelMailOutbound({ client, path: { workspace_id: workspaceId, outbound_id: outboundId }, throwOnError: true })).data
}

export async function outbound(client: OrbitClient, workspaceId: string, view: 'scheduled' | 'outbox'): Promise<MailOutboundItem[]> {
  return (await listMailOutbound({ ...at(client, workspaceId), query: { view } })).data
}

export async function retryRecipient(client: OrbitClient, workspaceId: string, outboundId: string, recipientId: string): Promise<void> {
  await retryMailRecipient({ client, path: { workspace_id: workspaceId, outbound_id: outboundId, recipient_id: recipientId }, throwOnError: true })
}

/** The addresses the user can send as; with a thread, the one that received it first. */
export async function fromAddresses(client: OrbitClient, workspaceId: string, threadId?: string): Promise<MailFromAddress[]> {
  return (await listFromAddresses({ ...at(client, workspaceId), query: { thread_id: threadId } })).data
}

/** People the user mailed or got mail from. */
export async function contacts(client: OrbitClient, workspaceId: string, q: string): Promise<MailContact[]> {
  return (await searchMailContacts({ ...at(client, workspaceId), query: { q } })).data
}

/** The workspace's send limits and pause (owners and admins). */
export async function sendPolicy(client: OrbitClient, workspaceId: string): Promise<MailSendPolicy> {
  return (await getMailSendPolicy(at(client, workspaceId))).data
}
