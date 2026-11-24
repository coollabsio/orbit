/**
 * Mail at phone size, as data (the web's `mailLib.ts` and `api/optimistic.ts`): the folder tabs, which threads each
 * list holds, what a thread change does to a list row, people and text of a message, and the live `mail` events.
 * No React.
 */
import type { MailFolder, MailPerson, MailThreadSummary, UpdateMailThreadBody } from '@orbit/api/generated/types.gen'

import type { QueryKey } from '@tanstack/react-query'

import type { IconName } from '../ui/Icon'
import { mailKeys, type ThreadFilter } from './keys'

export type ThreadChange = UpdateMailThreadBody

/** Where a tab's rows come from: a thread list, the user's drafts, or the outbound queue. */
export type TabSource = { kind: 'threads'; filter: ThreadFilter } | { kind: 'drafts' } | { kind: 'outbound'; view: 'scheduled' | 'outbox' }

/** The tabs of the web's phone layout (`mobileViews`), in order. */
export const MAIL_TABS = ['inbox', 'starred', 'drafts', 'sent', 'scheduled', 'outbox', 'archive', 'spam', 'trash'] as const
export type MailTab = (typeof MAIL_TABS)[number]

interface TabInfo {
  label: string
  icon: IconName
  source: TabSource
  empty: { title: string; description: string }
}

export const TAB_INFO: Record<MailTab, TabInfo> = {
  inbox: { label: 'Inbox', icon: 'directInbox', source: { kind: 'threads', filter: { folder: 'inbox' } }, empty: { title: 'You are all caught up', description: 'New mail shows up here.' } },
  starred: { label: 'Starred', icon: 'star', source: { kind: 'threads', filter: { starred: true } }, empty: { title: 'No starred conversations', description: 'Star a conversation to find it here.' } },
  drafts: { label: 'Drafts', icon: 'edit', source: { kind: 'drafts' }, empty: { title: 'No drafts', description: 'Drafts you start show up here.' } },
  sent: { label: 'Sent', icon: 'send', source: { kind: 'threads', filter: { sent: true } }, empty: { title: 'Nothing sent', description: 'Mail you send shows up here.' } },
  scheduled: { label: 'Scheduled', icon: 'clock', source: { kind: 'outbound', view: 'scheduled' }, empty: { title: 'Nothing scheduled', description: 'Mail you schedule shows up here until it goes.' } },
  outbox: { label: 'Outbox', icon: 'directboxSend', source: { kind: 'outbound', view: 'outbox' }, empty: { title: 'Outbox is empty', description: 'Mail that is waiting to go shows up here.' } },
  archive: { label: 'Archive', icon: 'archive', source: { kind: 'threads', filter: { folder: 'archive' } }, empty: { title: 'Nothing archived', description: 'Archived conversations show up here.' } },
  spam: { label: 'Spam', icon: 'shieldCross', source: { kind: 'threads', filter: { folder: 'spam' } }, empty: { title: 'No spam', description: 'Mail that fails authentication lands here.' } },
  trash: { label: 'Trash', icon: 'trash', source: { kind: 'threads', filter: { folder: 'trash' } }, empty: { title: 'Trash is empty', description: 'Conversations in the trash are deleted for good after 30 days.' } },
}

/** The tabs shown: the Outbox only while something waits in it or failed, or while it is open (web §9.1 U11). */
export const shownTabs = (showOutbox: boolean): MailTab[] => MAIL_TABS.filter((tab) => tab !== 'outbox' || showOutbox)

export const parseTab = (value: unknown): MailTab => (typeof value === 'string' && (MAIL_TABS as readonly string[]).includes(value) ? (value as MailTab) : 'inbox')

/** The folder's label for a thread's chip. */
export const folderLabel = (folder: MailFolder) => TAB_INFO[folder].label

/**
 * Whether a thread belongs in a list (the server's rules in `list_threads`). `unread` is left out, as on the web: a row
 * read while the Unread list shows stays (as read) until the list is read again.
 */
export function fitsFilter(thread: MailThreadSummary, filter: ThreadFilter): boolean {
  if (filter.mailboxId && thread.mailbox_id !== filter.mailboxId) return false
  if (filter.labelId && !thread.labels.includes(filter.labelId)) return false
  if (filter.starred && !thread.starred) return false
  if (filter.sent) return thread.last_sent_at != null && thread.folder !== 'trash'
  if (filter.folder) return thread.folder === filter.folder
  if (filter.labelId || filter.starred) return thread.folder === 'inbox' || thread.folder === 'archive'
  return thread.folder === 'inbox'
}

/** The thread after a change. A restore guesses the inbox; the response says where it went. */
export function changedThread(thread: MailThreadSummary, change: ThreadChange): MailThreadSummary {
  let folder: MailFolder = thread.folder
  if (change.folder) folder = change.folder
  else if (change.restore && thread.folder === 'trash') folder = 'inbox'
  const removed = new Set(change.remove_labels ?? [])
  const labels = thread.labels.filter((id) => !removed.has(id))
  for (const id of change.add_labels ?? []) if (!labels.includes(id)) labels.push(id)
  return { ...thread, folder, labels, unread: change.read == null ? thread.unread : !change.read, starred: change.starred ?? thread.starred }
}

/**
 * The rows of a list after some threads changed: changed rows are replaced, and a row that no longer fits the list
 * leaves it. `filter` is `null` for a search, whose rows stay until it is read again.
 */
export function patchRows(rows: MailThreadSummary[], filter: ThreadFilter | null, replace: (thread: MailThreadSummary) => MailThreadSummary | null): MailThreadSummary[] {
  return rows.flatMap((row) => {
    const next = replace(row)
    if (next === null) return [row]
    return filter && !fitsFilter(next, filter) ? [] : [next]
  })
}

/**
 * Which list the mail screen shows (the web's `threadFilterOf`): a label's threads, or a tab's, kept to one mailbox
 * when `mailboxId` is set. Starred stays across every mailbox, as on the web. `null` for drafts and the outbound queue.
 */
export function listFilter(tab: MailTab, scope: { mailboxId?: string; labelId?: string }): ThreadFilter | null {
  if (scope.labelId) return { labelId: scope.labelId }
  const source = TAB_INFO[tab].source
  if (source.kind !== 'threads') return null
  return scope.mailboxId && tab !== 'starred' ? { mailboxId: scope.mailboxId, ...source.filter } : source.filter
}

/** The filter of a list key (`mailKeys.threads`), `null` for a search. */
export const filterOfKey = (key: QueryKey): ThreadFilter | null => (key[3] === 'threads' ? (key[4] as ThreadFilter) : null)

/** The thread actions of the thread screen, by folder (the web's ThreadView header). */
export function threadActions(folder: MailFolder): { archive: 'archive' | 'unarchive' | null; spam: 'spam' | 'not-spam' | null; trash: 'trash' | 'restore' } {
  return {
    archive: folder === 'archive' ? 'unarchive' : folder === 'inbox' ? 'archive' : null,
    spam: folder === 'trash' ? null : folder === 'spam' ? 'not-spam' : 'spam',
    trash: folder === 'trash' ? 'restore' : 'trash',
  }
}

// ---- people and text

/** A person's display name, else their address. */
/** An outside sender's colour, from the address (the web's `senderColor`). */
export function senderColor(address: string | null | undefined): string {
  let hash = 0
  for (const character of (address ?? '').toLowerCase()) hash = (hash * 31 + character.charCodeAt(0)) >>> 0
  return `oklch(0.68 0.13 ${hash % 360})`
}

export const personName = (person: MailPerson | null | undefined) => person?.name?.trim() || person?.address || 'Unknown sender'

/** The senders of a list row: first names when there are several. */
export function threadSenders(thread: Pick<MailThreadSummary, 'from'>): string {
  if (thread.from.length <= 1) return personName(thread.from[0])
  return thread.from.map((person) => personName(person).split(/\s+/)[0]).join(', ')
}

/** The Sent list's row: the recipients of the newest message sent, else the senders. */
export function sentRecipients(thread: Pick<MailThreadSummary, 'sent_to' | 'sent_to_more' | 'from'>): string {
  if (thread.sent_to.length === 0) return threadSenders(thread)
  return `To: ${thread.sent_to.join(', ')}${thread.sent_to_more > 0 ? ` +${thread.sent_to_more}` : ''}`
}

/** `Name <address>, address` for the To and Cc lines. */
export const peopleLine = (list: MailPerson[]) => list.map((person) => (person.name ? `${person.name} <${person.address}>` : person.address)).join(', ')

/** The first line of text, for a collapsed message. */
export const firstLine = (text: string) => text.split('\n').find((line) => line.trim() !== '')?.trim() ?? ''

/** The domain of an address, lower case. */
export const domainOf = (address: string | null | undefined) => address?.split('@').pop()?.toLowerCase() ?? ''

/**
 * A plain-text body without its trailing quote: the `>` lines at the end and the "… wrote:" line before them. `quote`
 * is `null` when there is none, or when nothing else would stay.
 */
export function splitTextQuote(text: string): { body: string; quote: string | null } {
  const lines = text.split('\n')
  const blank = (line: string) => line.trim() === ''
  let end = lines.length
  while (end > 0 && blank(lines[end - 1]!)) end -= 1
  let start = end
  while (start > 0 && (lines[start - 1]!.trimStart().startsWith('>') || blank(lines[start - 1]!))) start -= 1
  while (start < end && blank(lines[start]!)) start += 1
  if (start === end) return { body: text, quote: null }
  let intro = start
  while (intro > 0 && blank(lines[intro - 1]!)) intro -= 1
  const before = intro > 0 ? lines[intro - 1]!.trim() : ''
  const fold = before.endsWith(':') && before.length < 200 ? intro - 1 : start
  if (!lines.slice(0, fold).some((line) => !blank(line))) return { body: text, quote: null }
  return { body: lines.slice(0, fold).join('\n').trimEnd(), quote: lines.slice(fold, end).join('\n') }
}

/** Threads of 5 or more messages fold their middle into "N more messages": the first stays, and the last two. */
export function foldedRange(count: number): [number, number] | null {
  return count >= 5 ? [1, count - 2] : null
}

/** The download path of a received attachment (bearer-authenticated). */
export const attachmentPath = (workspaceId: string, attachmentId: string) =>
  `/api/v1/workspaces/${encodeURIComponent(workspaceId)}/mail/attachments/${encodeURIComponent(attachmentId)}`

// ---- live events of the `mail` topic

export type MailLiveEvent = { type: 'thread.updated'; mailbox_id: string; thread_id: string } | { type: 'outbound.updated'; mailbox_id: string; outbound_id: string }

export function parseMailEvent(value: unknown): MailLiveEvent | null {
  if (!value || typeof value !== 'object') return null
  const event = value as Record<string, unknown>
  if (typeof event.mailbox_id !== 'string') return null
  if (event.type === 'thread.updated' && typeof event.thread_id === 'string') return { type: event.type, mailbox_id: event.mailbox_id, thread_id: event.thread_id }
  if (event.type === 'outbound.updated' && typeof event.outbound_id === 'string') return { type: event.type, mailbox_id: event.mailbox_id, outbound_id: event.outbound_id }
  return null
}

/** What a mail event makes stale: every list, the counts, the outbound views, the drafts, and the changed thread. */
export function mailEventKeys(value: unknown, workspaceId: string): (readonly unknown[])[] {
  const event = parseMailEvent(value)
  if (!event) return []
  const keys: (readonly unknown[])[] = [mailKeys.lists(workspaceId), mailKeys.counts(workspaceId), mailKeys.outboundAll(workspaceId), mailKeys.drafts(workspaceId)]
  if (event.type === 'thread.updated') keys.push(mailKeys.thread(workspaceId, event.thread_id))
  return keys
}

// ---- web paths

/**
 * The app route of a web `/mail…` path (after `/mail`): `t/<id>` at the end opens the thread, a folder or `starred`
 * opens that tab. A mailbox's folder opens the folder of every mailbox (the phone has no mailbox views), a label the
 * inbox.
 */
export function mailRoute(parts: string[]): string {
  if (parts.length >= 2 && parts[parts.length - 2] === 't') return `/mail/${encodeURIComponent(parts[parts.length - 1]!)}`
  const tab = [...parts].reverse().find((part) => (MAIL_TABS as readonly string[]).includes(part))
  return tab && tab !== 'inbox' ? `/mail?tab=${tab}` : '/mail'
}

// ---- the list's date groups

/** The date group of a row by the local day: Today, Yesterday, This week (last 7 days), else the month. */
export function dateGroup(iso: string, now: Date = new Date()): string {
  const day = (date: Date) => new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime()
  const date = new Date(iso)
  const days = Math.round((day(now) - day(date)) / 86_400_000)
  if (days <= 0) return 'Today'
  if (days === 1) return 'Yesterday'
  if (days < 7) return 'This week'
  return date.toLocaleDateString('en-US', date.getFullYear() === now.getFullYear() ? { month: 'long' } : { month: 'long', year: 'numeric' })
}

export type ListEntry = { kind: 'group'; key: string; label: string } | { kind: 'thread'; key: string; thread: MailThreadSummary }

/** The rows with a header before each new date group (`sentView`: by the time sent). A search keeps its order, ungrouped. */
export function groupThreads(threads: MailThreadSummary[], options: { sentView?: boolean; grouped?: boolean } = {}, now: Date = new Date()): ListEntry[] {
  const entries: ListEntry[] = []
  let current: string | null = null
  for (const thread of threads) {
    if (options.grouped !== false) {
      const label = dateGroup((options.sentView && thread.last_sent_at) || thread.last_message_at, now)
      if (label !== current) entries.push({ kind: 'group', key: `group:${label}`, label })
      current = label
    }
    entries.push({ kind: 'thread', key: thread.id, thread })
  }
  return entries
}
