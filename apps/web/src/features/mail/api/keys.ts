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
