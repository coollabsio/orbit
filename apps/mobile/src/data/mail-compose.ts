/**
 * The compose screen's rules, without React. The app edits plain text: `textToHtml` makes the `body_html` a draft
 * saves (the server sanitizes it, and adds the quote of a reply or the forwarded message when it sends), and
 * `htmlToText` reads a draft written elsewhere back as text. Recipients, the pre-send checks and the signature follow
 * the web's `compose/composeLib.ts`.
 */
import type { MailDraft, MailFromAddress, MailSuppressedRecipient } from '@orbit/api/generated/types.gen'

const escapeHtml = (text: string) => text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;')

/** Plain text as mail HTML: a paragraph for each block between blank lines, a `<br>` for each line break inside one. */
export function textToHtml(text: string): string {
  const blocks = text.replace(/\r\n?/g, '\n').split(/\n[ \t]*\n/)
  return blocks
    .map((block) => block.replace(/^\n+|\n+$/g, ''))
    .filter((block) => block.trim() !== '')
    .map((block) => `<p>${block.split('\n').map(escapeHtml).join('<br>')}</p>`)
    .join('')
}

const ENTITIES: Record<string, string> = { amp: '&', lt: '<', gt: '>', quot: '"', apos: "'", nbsp: ' ' }

function decodeEntities(text: string): string {
  return text.replace(/&(#x[0-9a-f]+|#\d+|[a-z]+);/gi, (whole, name: string) => {
    if (name[0] === '#') {
      const code = name[1]?.toLowerCase() === 'x' ? parseInt(name.slice(2), 16) : parseInt(name.slice(1), 10)
      return Number.isFinite(code) && code > 0 && code <= 0x10ffff ? String.fromCodePoint(code) : whole
    }
    return ENTITIES[name.toLowerCase()] ?? whole
  })
}

const BLOCKS = 'p|div|h[1-6]|ul|ol|blockquote|pre|table|tr'

/** Mail HTML as the text the app edits: blocks are separated by a blank line, `<br>` is a line break, tags go. */
export function htmlToText(html: string): string {
  return decodeEntities(
    html
      .replace(/<(script|style)[^>]*>[\s\S]*?<\/\1>/gi, '')
      .replace(/<br\s*\/?>/gi, '\n')
      .replace(/<li[^>]*>/gi, '\n• ')
      .replace(/<\/li>/gi, '')
      .replace(new RegExp(`</?(${BLOCKS})(\\s[^>]*)?>`, 'gi'), '\n\n')
      .replace(/<[^>]*>/g, ''),
  )
    .replace(/[ \t]+\n/g, '\n')
    .replace(/\n{3,}/g, '\n\n')
    .replace(/^\s+|\s+$/g, '')
}

// ---- recipients

const lower = (address: string) => address.trim().toLowerCase()
const domainOf = (address: string) => lower(address).split('@').at(-1) ?? ''

/**
 * Typed or pasted text as recipients: commas, semicolons and line breaks separate them (not inside quotes or
 * brackets), `Name <address>` keeps the address, and spaces separate bare addresses.
 */
export function splitRecipients(text: string): string[] {
  const pieces: string[] = []
  let current = ''
  let quoted = false
  let bracketed = false
  for (const char of text) {
    if (char === '"') quoted = !quoted
    else if (char === '<') bracketed = true
    else if (char === '>') bracketed = false
    if (!quoted && !bracketed && /[,;\n\r\t]/.test(char)) {
      pieces.push(current)
      current = ''
    } else {
      current += char
    }
  }
  pieces.push(current)
  return pieces.flatMap((piece) => {
    const bracket = /<([^>]*)>/.exec(piece)
    if (bracket) return [bracket[1]!.trim()].filter(Boolean)
    const words = piece.trim().split(/\s+/).filter(Boolean)
    const addresses = words.filter((word) => word.includes('@'))
    if (addresses.length > 0) return addresses
    const rest = piece.trim().replace(/^"|"$/g, '').trim()
    return rest ? [rest] : []
  })
}

/** A recipient list with the typed text added (split like a paste, no duplicates). */
export function commitTyped(addresses: readonly string[], typed: string): string[] {
  const next = [...addresses]
  for (const entry of splitRecipients(typed)) if (!next.some((existing) => lower(existing) === lower(entry))) next.push(entry)
  return next
}

const LOCAL_PART = /^[A-Za-z0-9.!#$&'*+/=?^_`{|}~-]+$/
const DOMAIN = /^(?=.{1,253}$)([A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?\.)+[A-Za-z]{2,63}$/

/** An address the server accepts: no name, quotes, routing (`%`, `!`) or spaces; a domain with a dot. */
export function isValidAddress(address: string): boolean {
  if (address.length > 254) return false
  const parts = address.split('@')
  if (parts.length !== 2) return false
  const [local, domain] = parts as [string, string]
  return local.length > 0 && local.length <= 64 && LOCAL_PART.test(local) && !/[%!]/.test(local) && DOMAIN.test(domain)
}

/** "To ana@x.test +2", for a draft row. */
export function recipientSummary(to: readonly string[], cc: readonly string[], bcc: readonly string[]): string {
  const [label, list] = to.length > 0 ? ['To', to] : cc.length > 0 ? ['Cc', cc] : bcc.length > 0 ? ['Bcc', bcc] : [null, []]
  if (!label || list[0] === undefined) return 'No recipients'
  const others = to.length + cc.length + bcc.length - 1
  return `${label} ${list[0]}${others > 0 ? ` +${others}` : ''}`
}

// ---- pre-send checks (web §9.1 U5: the first Send shows them, the next one sends anyway)

export interface PreSendInput {
  text: string
  subject: string
  fileCount: number
  mode: string
  to: readonly string[]
  cc: readonly string[]
  bcc: readonly string[]
  /** Everyone on the thread so far; `null` for a new message. */
  threadAddresses: readonly string[] | null
  workspaceDomains: readonly string[]
  suppressed: readonly MailSuppressedRecipient[]
}

const ATTACHMENT_WORD = /\battach(ed|ment|ments)?\b/i
const PLACEHOLDER = /\{[^}]+\}/
export const REPLY_ALL_LIMIT = 10
const SUPPRESSION_REASONS: Record<string, string> = { hard_bounce: 'the address does not take mail', complaint: 'marked the mail as spam', manual: 'added by an admin' }

const people = (addresses: readonly string[]) => (addresses.length === 1 ? addresses[0] : `${addresses.length} people`)

/** The warnings shown above Send, in the web's order. */
export function preSendWarnings(input: PreSendInput): string[] {
  const warnings: string[] = []
  const seen = new Set<string>()
  const recipients = [...input.to, ...input.cc, ...input.bcc].filter((address) => !seen.has(lower(address)) && Boolean(seen.add(lower(address))))
  if (ATTACHMENT_WORD.test(input.text) && input.fileCount === 0 && input.mode !== 'forward_attached') warnings.push('You mention an attachment, but no file is attached.')
  if (input.subject.trim() === '') warnings.push('The subject is empty.')
  const placeholder = PLACEHOLDER.exec(input.text)?.[0] ?? PLACEHOLDER.exec(input.subject)?.[0]
  if (placeholder) warnings.push(`${placeholder} is still a placeholder.`)
  if (input.threadAddresses && input.threadAddresses.length > 0) {
    const domains = new Set(input.workspaceDomains.map((domain) => domain.toLowerCase()))
    const inside = (address: string) => domains.has(domainOf(address))
    const onThread = new Set(input.threadAddresses.map(lower))
    if (input.threadAddresses.every(inside)) {
      const outside = recipients.filter((address) => !inside(address) && !onThread.has(lower(address)))
      if (outside.length > 0) warnings.push(`${people(outside)} ${outside.length === 1 ? 'is' : 'are'} outside the workspace. This thread had only workspace addresses.`)
    }
  }
  if (input.mode === 'reply_all' && recipients.length > REPLY_ALL_LIMIT) warnings.push(`Reply all goes to ${recipients.length} people.`)
  const entries = new Map(input.suppressed.map((entry) => [lower(entry.address), entry]))
  const suppressed = recipients.filter((address) => entries.has(lower(address)))
  if (suppressed.length === 1) {
    const reason = SUPPRESSION_REASONS[entries.get(lower(suppressed[0]!))!.reason] ?? 'it bounced before'
    warnings.push(`${suppressed[0]}: ${reason}. Orbit will skip it.`)
  } else if (suppressed.length > 1) {
    warnings.push(`${suppressed.length} recipients are on the suppression list. Orbit will skip them.`)
  }
  return warnings
}

/** Why Send is off for an address that is not ready (`MailFromAddress.not_ready_reason`). */
export function notReadyText(reason: string | null | undefined): string {
  switch (reason) {
    case 'domain_unverified':
      return 'The domain of this address is not verified yet.'
    case 'inbound_not_live':
      return 'Mail for this domain does not arrive at Orbit yet.'
    case 'dkim_inactive':
      return 'The DKIM record of this domain is not active yet.'
    case 'no_route':
      return 'This domain has no send route.'
    case 'mailbox_suspended':
      return 'This mailbox is suspended.'
    default:
      return 'This address cannot send yet.'
  }
}

// ---- the editor's state

/** What the compose screen edits. `body` is plain text. */
export interface ComposeFields {
  fromId: string
  to: string[]
  cc: string[]
  bcc: string[]
  subject: string
  body: string
}

const signatureText = (address: MailFromAddress | undefined) => (address?.signature_html?.trim() ? htmlToText(address.signature_html) : '')

/**
 * The fields of a draft as the screen opens it. A draft with no text yet starts with the signature of its address
 * below two empty lines, as the web puts it in.
 */
export function initialFields(draft: MailDraft, addresses: readonly MailFromAddress[]): ComposeFields {
  const saved = serverFields(draft)
  const signature = signatureText(addresses.find((address) => address.address_id === draft.from_address_id))
  return { ...saved, body: saved.body || (signature ? `\n\n${signature}` : '') }
}

/** The fields as the server has them (what a save is compared with). */
export function serverFields(draft: MailDraft): ComposeFields {
  return { fromId: draft.from_address_id, to: draft.to, cc: draft.cc, bcc: draft.bcc, subject: draft.subject, body: htmlToText(draft.body_html) }
}

/** The body after the From changed: an unchanged signature at the end follows the address (web §9.1 U7). */
export function swapSignature(body: string, before: MailFromAddress | undefined, after: MailFromAddress | undefined): string {
  const old = signatureText(before)
  const next = signatureText(after)
  const trimmed = body.replace(/\s+$/, '')
  if (old && trimmed.endsWith(old)) return next ? `${trimmed.slice(0, -old.length)}${next}` : trimmed.slice(0, -old.length).replace(/\s+$/, '')
  if (!old && next && trimmed === '') return `\n\n${next}`
  return body
}

/**
 * The change to save, against what the server has: only the fields that differ. The body goes only when its text
 * changed, so a draft written elsewhere keeps its formatting until it is edited here.
 */
export function draftChange(opened: ComposeFields, now: ComposeFields): { from_address_id?: string; to?: string[]; cc?: string[]; bcc?: string[]; subject?: string; body_html?: string } {
  const change: ReturnType<typeof draftChange> = {}
  const same = (a: readonly string[], b: readonly string[]) => a.length === b.length && a.every((value, index) => value === b[index])
  if (now.fromId !== opened.fromId) change.from_address_id = now.fromId
  if (!same(now.to, opened.to)) change.to = now.to
  if (!same(now.cc, opened.cc)) change.cc = now.cc
  if (!same(now.bcc, opened.bcc)) change.bcc = now.bcc
  if (now.subject !== opened.subject) change.subject = now.subject
  if (now.body !== opened.body) change.body_html = textToHtml(now.body)
  return change
}

/** A draft made just now (never saved since), left as it opened (`opened`: `initialFields`): it goes when the screen closes, as on the web. */
export function isUntouched(draft: Pick<MailDraft, 'version' | 'created_at' | 'updated_at'>, opened: ComposeFields, now: ComposeFields): boolean {
  return draft.version === 1 && draft.created_at === draft.updated_at && Object.keys(draftChange(opened, now)).length === 0
}
