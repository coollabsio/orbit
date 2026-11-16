import type { MailFolder, MailNamePattern } from '@orbit/api/generated/types.gen'

/**
 * Which threads a list shows. Without `folder`: the inbox, or with `labelId` or `starred` the inbox and the archive.
 * Without `mailboxId`: every mailbox the user reads.
 */
export interface ThreadFilter {
  mailboxId?: string
  folder?: MailFolder
  labelId?: string
  starred?: boolean
}

/** The outbound queue views the server lists (`GET mail/outbound?view=`). */
export type OutboundView = 'sent' | 'scheduled' | 'outbox'

/**
 * Deliberately outside the `['workspace', id]` prefix (like `chatKeys`): workspace events invalidate that whole
 * prefix, and mail has its own live topic (`emitMailEvent`), so a task or page change must not read the mail again.
 */
export const mailKeys = {
  all: (workspaceId: string) => ['mail', workspaceId] as const,
  /** The mailboxes and labels the user reads (`GET mail/me`). */
  me: (workspaceId: string) => ['mail', workspaceId, 'me'] as const,
  /** Unread threads of each folder of each mailbox. */
  counts: (workspaceId: string) => ['mail', workspaceId, 'counts'] as const,
  /** Prefix of every thread list: the folder and label lists and the searches. */
  lists: (workspaceId: string) => ['mail', workspaceId, 'list'] as const,
  threads: (workspaceId: string, filter: ThreadFilter) => ['mail', workspaceId, 'list', 'threads', filter] as const,
  search: (workspaceId: string, query: string) => ['mail', workspaceId, 'list', 'search', query] as const,
  /** Prefix of every open thread. */
  threadDetails: (workspaceId: string) => ['mail', workspaceId, 'thread'] as const,
  thread: (workspaceId: string, threadId: string) => ['mail', workspaceId, 'thread', threadId] as const,
  /** The user's drafts (all mailboxes, or one). Not under `list`: they are not thread lists. */
  drafts: (workspaceId: string, mailboxId?: string) => ['mail', workspaceId, 'drafts', mailboxId ?? null] as const,
  /** Prefix of every draft read. */
  draftsAll: (workspaceId: string) => ['mail', workspaceId, 'drafts'] as const,
  draft: (workspaceId: string, draftId: string) => ['mail', workspaceId, 'draft', draftId] as const,
  /** The suppressed recipients of a saved draft, for the recipients it had (`recipients`: sorted, joined). */
  draftCheck: (workspaceId: string, draftId: string, recipients: string) => ['mail', workspaceId, 'draft-check', draftId, recipients] as const,
  /** The addresses the user sends as; of a thread, the one that received it comes first. */
  fromAddresses: (workspaceId: string, threadId?: string) => ['mail', workspaceId, 'from-addresses', threadId ?? null] as const,
  /** Prefix of every outbound view (`sent`, `scheduled`, `outbox`). */
  outboundAll: (workspaceId: string) => ['mail', workspaceId, 'outbound'] as const,
  outbound: (workspaceId: string, view: OutboundView) => ['mail', workspaceId, 'outbound', view] as const,
  contacts: (workspaceId: string, query: string) => ['mail', workspaceId, 'contacts', query] as const,
  signatures: (workspaceId: string) => ['mail', workspaceId, 'signatures'] as const,
  snippets: (workspaceId: string) => ['mail', workspaceId, 'snippets'] as const,
  settingsMe: (workspaceId: string) => ['mail', workspaceId, 'settings-me'] as const,
  vacation: (workspaceId: string, mailboxId: string) => ['mail', workspaceId, 'vacation', mailboxId] as const,
  suppressions: (workspaceId: string) => ['mail', workspaceId, 'suppressions'] as const,
  sendRoutes: (workspaceId: string) => ['mail', workspaceId, 'send-routes'] as const,
  sendPolicy: (workspaceId: string) => ['mail', workspaceId, 'send-policy'] as const,
  domains: (workspaceId: string) => ['mail', workspaceId, 'domains'] as const,
  domain: (workspaceId: string, domainId: string) => ['mail', workspaceId, 'domains', domainId] as const,
  mailboxes: (workspaceId: string) => ['mail', workspaceId, 'mailboxes'] as const,
  /** Under `mailboxes`: a mailbox change makes new proposals. */
  bulkPreview: (workspaceId: string, domainId: string, pattern: MailNamePattern) =>
    ['mail', workspaceId, 'mailboxes', 'bulk-preview', domainId, pattern] as const,
  labels: (workspaceId: string) => ['mail', workspaceId, 'labels'] as const,
  receipts: (workspaceId: string) => ['mail', workspaceId, 'receipts'] as const,
  /** The instance providers and the workspace's own (`GET mail/providers`). */
  providers: (workspaceId: string) => ['mail', workspaceId, 'providers'] as const,
  /**
   * Outside the `all` prefix: a new URL reloads the message frame, so a resync must not fetch one. The URL is valid
   * for 5 minutes and only for opening the document.
   */
  viewUrl: (workspaceId: string, messageId: string, loadImages: boolean) =>
    ['mail-view-url', workspaceId, messageId, { loadImages }] as const,
  /** **Show original**: outside the `all` prefix too, so a resync does not read a large message again. */
  raw: (workspaceId: string, messageId: string) => ['mail-raw', workspaceId, messageId] as const,
}
