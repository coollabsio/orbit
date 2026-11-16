import type { IconComponent } from 'reicon-react'
import { Archive, Clock, DirectboxSend, DirectInbox, Edit, Send, ShieldCross, Trash } from 'reicon-react'
import type { MailAttachmentView, MailFolder, MailPerson, MailThreadSummary } from '@orbit/api/generated/types.gen'
import { ApiProblem } from '@orbit/api/problem'
import type { Attachment } from '@/lib/attachmentLib'
import type { ThreadFilter } from './api/keys'

/** The folders that are not thread lists: they read the drafts or the outbound queue. */
export type SendFolder = 'drafts' | 'sent' | 'scheduled' | 'outbox'
/** Every folder of the rail. */
export type ListFolder = MailFolder | SendFolder

export const FOLDERS: ReadonlyArray<{ folder: ListFolder; label: string; icon: IconComponent }> = [
  { folder: 'inbox', label: 'Inbox', icon: DirectInbox },
  { folder: 'drafts', label: 'Drafts', icon: Edit },
  { folder: 'sent', label: 'Sent', icon: Send },
  { folder: 'scheduled', label: 'Scheduled', icon: Clock },
  { folder: 'outbox', label: 'Outbox', icon: DirectboxSend },
  { folder: 'archive', label: 'Archive', icon: Archive },
  { folder: 'spam', label: 'Spam', icon: ShieldCross },
  { folder: 'trash', label: 'Trash', icon: Trash },
]

/** The folders of the rail after the inbox: the Outbox only while something waits in it or failed (§9.1 U11). */
export const railFolders = (showOutbox: boolean) => FOLDERS.slice(1).filter(({ folder }) => showOutbox || folder !== 'outbox')

const FOLDER_NAMES = new Set<string>(FOLDERS.map(({ folder }) => folder))
const SEND_FOLDERS = new Set<string>(['drafts', 'sent', 'scheduled', 'outbox'])
export const isSendFolder = (folder: ListFolder): folder is SendFolder => SEND_FOLDERS.has(folder)
const isFolder = (value: string | undefined): value is ListFolder => value !== undefined && FOLDER_NAMES.has(value)

export const folderLabel = (folder: ListFolder) => FOLDERS.find((entry) => entry.folder === folder)?.label ?? folder

/** What the thread list shows. A folder without `mailboxId` is the folder of every mailbox the user reads. */
export type MailView =
  | { kind: 'folder'; folder: ListFolder; mailboxId?: string }
  | { kind: 'label'; labelId: string }
  | { kind: 'starred' }
  | { kind: 'search' }

export interface MailRoute {
  view: MailView
  threadId: string | null
}

/**
 * The view and open thread of a path below `/mail` (the route's splat): `` (the inbox of all mailboxes), `archive`,
 * `<mailbox>/<folder>`, `label/<id>`, `starred`, `search` (with `?q=`), each optionally followed by `t/<thread>`.
 * `null` for anything else.
 */
export function parseMailPath(splat: string): MailRoute | null {
  const parts = splat.split('/').filter(Boolean)
  let threadId: string | null = null
  if (parts.length >= 2 && parts[parts.length - 2] === 't') {
    threadId = parts[parts.length - 1]
    parts.length -= 2
  }
  const [first, second] = parts
  let view: MailView | null = null
  if (parts.length === 0) view = { kind: 'folder', folder: 'inbox' }
  else if (parts.length === 1 && isFolder(first)) view = { kind: 'folder', folder: first }
  else if (parts.length === 1 && first === 'starred') view = { kind: 'starred' }
  else if (parts.length === 1 && first === 'search') view = { kind: 'search' }
  else if (parts.length === 2 && first === 'label') view = { kind: 'label', labelId: second }
  else if (parts.length === 2 && isFolder(second)) view = { kind: 'folder', folder: second, mailboxId: first }
  return view ? { view, threadId } : null
}

/** The path of a view, without the search query. */
export function mailViewPath(view: MailView): string {
  switch (view.kind) {
    case 'folder':
      if (view.mailboxId) return `/mail/${encodeURIComponent(view.mailboxId)}/${view.folder}`
      return view.folder === 'inbox' ? '/mail' : `/mail/${view.folder}`
    case 'label':
      return `/mail/label/${encodeURIComponent(view.labelId)}`
    case 'starred':
      return '/mail/starred'
    case 'search':
      return '/mail/search'
  }
}

/** The path of a thread opened from a view. */
export const mailThreadPath = (view: MailView, threadId: string) => `${mailViewPath(view)}/t/${encodeURIComponent(threadId)}`

/** A key per view, so the page state (selection, focus) starts over in another view. */
export const mailViewKey = (view: MailView) => mailViewPath(view)

/** The thread list query of a view; `null` for a search or a drafts/scheduled/outbox folder, which have their own queries. */
export function threadFilterOf(view: MailView): ThreadFilter | null {
  switch (view.kind) {
    case 'folder':
      if (view.folder === 'sent') return view.mailboxId ? { mailboxId: view.mailboxId, sent: true } : { sent: true }
      if (isSendFolder(view.folder)) return null
      return view.mailboxId ? { mailboxId: view.mailboxId, folder: view.folder } : { folder: view.folder }
    case 'label':
      return { labelId: view.labelId }
    case 'starred':
      return { starred: true }
    case 'search':
      return null
  }
}

/** The height limits of a message frame, in pixels. */
export const FRAME_MIN_HEIGHT = 80
export const FRAME_MAX_HEIGHT = 20_000

/**
 * The height a message frame asks for, or `null` when the event is not from that frame or carries no finite number
 * as `orbitMailHeight`. Every page and frame can post to the app window; only the frame's own window counts.
 */
export function frameHeight(event: { source: unknown; data: unknown }, frameWindow: unknown): number | null {
  if (frameWindow == null || event.source !== frameWindow) return null
  const data = event.data
  if (!data || typeof data !== 'object') return null
  const height = (data as Record<string, unknown>).orbitMailHeight
  if (typeof height !== 'number' || !Number.isFinite(height)) return null
  return Math.min(FRAME_MAX_HEIGHT, Math.max(FRAME_MIN_HEIGHT, Math.ceil(height)))
}

export interface SearchSuggestion {
  /** What the list shows. */
  label: string
  /** The whole query after picking it. */
  query: string
}

const OPERATORS = ['from', 'to', 'subject', 'label', 'in', 'is', 'has', 'before', 'after'] as const
const OPERATOR_VALUES: Partial<Record<(typeof OPERATORS)[number], readonly string[]>> = {
  in: FOLDERS.map(({ folder }) => folder),
  is: ['unread', 'starred'],
  has: ['attachment'],
}
const MAX_SUGGESTIONS = 8

const quoted = (value: string) => (/\s/.test(value) ? `"${value}"` : value)

/**
 * Completions for the word being typed (the last one): an operator name (`fr` → `from:`), or a value of the
 * operator (`in:ar` → `in:archive`, `label:` → the label names, `from:` → the known senders). Picking one keeps the
 * rest of the query.
 */
export function searchSuggestions(query: string, known: { labels: readonly string[]; people: readonly string[] }): SearchSuggestion[] {
  // the last word starts after the last space outside quotes
  let start = 0
  let open = false
  for (let index = 0; index < query.length; index += 1) {
    if (query[index] === '"') open = !open
    else if (!open && /\s/.test(query[index])) start = index + 1
  }
  const head = query.slice(0, start)
  const word = query.slice(start)
  if (!word) return []
  const colon = word.indexOf(':')
  if (colon < 0) {
    const typed = word.toLowerCase()
    return OPERATORS.filter((name) => name.startsWith(typed)).map((name) => ({ label: `${name}:`, query: `${head}${name}:` }))
  }
  const operator = word.slice(0, colon).toLowerCase() as (typeof OPERATORS)[number]
  if (!OPERATORS.includes(operator)) return []
  const typed = word.slice(colon + 1).replace(/^"/, '').replace(/"$/, '').toLowerCase()
  const values =
    OPERATOR_VALUES[operator] ?? (operator === 'label' ? known.labels : operator === 'from' || operator === 'to' ? known.people : [])
  return [...new Set(values)]
    .filter((value) => value.toLowerCase().startsWith(typed) && value.toLowerCase() !== typed)
    .slice(0, MAX_SUGGESTIONS)
    .map((value) => ({ label: `${operator}:${quoted(value)}`, query: `${head}${operator}:${quoted(value)} ` }))
}

/** A person's display name, else their address. */
export const personName = (person: MailPerson | null | undefined) => person?.name?.trim() || person?.address || 'Unknown sender'

/** The senders of a list row: first names when there are several. */
export function threadSenders(thread: Pick<MailThreadSummary, 'from'>): string {
  if (thread.from.length <= 1) return personName(thread.from[0])
  return thread.from.map((person) => personName(person).split(/\s+/)[0]).join(', ')
}

/** The Sent list's row: the recipients of the newest message sent ("To: ana@x.test, bob@x.test +3"), else the senders. */
export function sentRecipients(thread: Pick<MailThreadSummary, 'sent_to' | 'sent_to_more' | 'from'>): string {
  if (thread.sent_to.length === 0) return threadSenders(thread)
  return `To: ${thread.sent_to.join(', ')}${thread.sent_to_more > 0 ? ` +${thread.sent_to_more}` : ''}`
}

/** The first line of text, for a collapsed message. */
export const firstLine = (text: string) => text.split('\n').find((line) => line.trim() !== '')?.trim() ?? ''

/** The image types the attachment route shows inline (no SVG). */
const INLINE_IMAGES = new Set(['image/png', 'image/jpeg', 'image/gif', 'image/webp', 'image/avif'])

/** A mail attachment for the shared attachment list: images open in the viewer, everything else downloads. */
export function mailAttachment(workspaceId: string, attachment: MailAttachmentView): Attachment {
  const url = `/api/v1/workspaces/${encodeURIComponent(workspaceId)}/mail/attachments/${encodeURIComponent(attachment.id)}`
  const image = INLINE_IMAGES.has(attachment.content_type)
  return {
    id: attachment.id,
    fileName: attachment.filename?.trim() || 'attachment',
    // a type the route will not show inline must not reach an <img>; a PDF opens in the viewer (pdf.js, no scripts)
    mimeType: image || attachment.content_type === 'application/pdf' ? attachment.content_type : 'application/octet-stream',
    fileSize: attachment.size,
    url: image ? `${url}?inline=1` : url,
  }
}

/** The original message (`.eml`) download. */
export const rawMessageUrl = (workspaceId: string, messageId: string) =>
  `/api/v1/workspaces/${encodeURIComponent(workspaceId)}/mail/messages/${encodeURIComponent(messageId)}/raw`

/** The server's explanation of a failed request, else `fallback`. */
export const problemText = (error: unknown, fallback: string) => (error instanceof ApiProblem && error.detail ? error.detail : fallback)

/** The fields of a draft the user edits. */
const DRAFT_TEXT_FIELDS = ['subject', 'body_html', 'to', 'cc', 'bcc', 'from_address_id', 'mode'] as const
export interface DraftText {
  subject?: string
  body_html?: string
  to?: string[]
  cc?: string[]
  bcc?: string[]
  from_address_id?: string
  mode?: string
}

/**
 * A save came back 409 `draft_conflict`: someone saved a newer version. The server copy replaces the draft, and the
 * text the user had not saved waits in `restore` (the "Restore my text" banner), `null` when it equals the server's.
 */
export function applyDraftConflict<T extends DraftText>(local: DraftText & { version?: number }, server: T): { draft: T; restore: DraftText | null } {
  const restore: DraftText = {}
  let differs = false
  for (const field of DRAFT_TEXT_FIELDS) {
    const mine = local[field]
    if (mine === undefined) continue
    ;(restore as Record<string, unknown>)[field] = mine
    if (JSON.stringify(mine) !== JSON.stringify(server[field])) differs = true
  }
  return { draft: server, restore: differs ? restore : null }
}

/** The current draft a 409 `draft_conflict` carries in `conflict.current`, else `null`. */
export function conflictDraft<T>(error: unknown): T | null {
  if (!(error instanceof ApiProblem) || error.status !== 409 || error.code !== 'draft_conflict') return null
  const current = (error.problem.conflict as { current?: unknown } | null | undefined)?.current
  return current && typeof current === 'object' ? (current as T) : null
}

/**
 * The message a 409 `draft_sending` names: the reply of that thread is held for sending (undo time, scheduled, or a
 * hold), so no draft can open until it is undone. `null` for any other error. Read untyped: the generated `Problem`
 * lacks `outbound_id`.
 */
export function sendingOutboundId(error: unknown): string | null {
  if (!(error instanceof ApiProblem) || error.status !== 409 || error.code !== 'draft_sending') return null
  const id = (error.problem as { outbound_id?: unknown }).outbound_id
  return typeof id === 'string' && id !== '' ? id : null
}
